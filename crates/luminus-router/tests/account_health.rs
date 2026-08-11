use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use luminus_core::{
    model::{AccountDescriptor, AccountId, Capability, ModelId, ModelInfo, ProviderId},
    protocol::{
        CanonicalMessage, CanonicalRequest, CanonicalResponse, ContentPart, FinishReason,
        MessageRole, ResponseId, Usage,
    },
    provider::{ProviderAdapter, ProviderContext, ProviderError, ProviderErrorCategory},
};
use luminus_router::{
    AccountHealthStore, AccountPool, Clock, ProviderAccount, ProviderRegistry, RouteCandidate,
    RoutePlan, Router, RoutingPolicy,
};

struct ManualClock(Mutex<Instant>);
impl ManualClock {
    fn new() -> Self {
        Self(Mutex::new(Instant::now()))
    }
    fn advance(&self, duration: Duration) {
        *self.0.lock().unwrap() += duration;
    }
}
impl Clock for ManualClock {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }
}

struct ScriptedProvider {
    id: ProviderId,
    account: AccountId,
    calls: Arc<Mutex<Vec<AccountId>>>,
}
impl ProviderAdapter for ScriptedProvider {
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }
    fn models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: ModelId("m".into()),
            provider: self.id.clone(),
            capabilities: vec![Capability::Chat],
        }]
    }
    fn execute<'a>(
        &'a self,
        _request: &'a CanonicalRequest,
        _context: &'a ProviderContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<CanonicalResponse, ProviderError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let mut calls = self.calls.lock().unwrap();
            calls.push(self.account.clone());
            if self.account == AccountId::from("a")
                && calls.iter().filter(|id| **id == self.account).count() == 1
            {
                Err(ProviderError::new(
                    ProviderErrorCategory::RateLimit,
                    "limited",
                    true,
                ))
            } else {
                Ok(CanonicalResponse {
                    id: ResponseId("ok".into()),
                    model: ModelId("m".into()),
                    content: vec![ContentPart::text("ok")],
                    finish_reason: FinishReason::Stop,
                    usage: Usage::default(),
                    provider_metadata: None,
                })
            }
        })
    }
}

fn request() -> CanonicalRequest {
    CanonicalRequest {
        model: ModelId("m".into()),
        messages: vec![CanonicalMessage::text(MessageRole::User, "x")],
        tools: vec![],
        tool_choice: None,
        temperature: None,
        top_p: None,
        max_output_tokens: None,
        stop: None,
        stream: false,
        reasoning: None,
        metadata: None,
    }
}
fn plan(provider: ProviderId) -> RoutePlan {
    RoutePlan {
        candidates: vec![RouteCandidate {
            provider,
            model: ModelId("m".into()),
            account: None,
        }],
        policy: RoutingPolicy::new(2, true).unwrap(),
    }
}

#[tokio::test]
async fn cooldown_changes_router_selection_across_logical_requests() {
    let provider = ProviderId::from("fake");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut pool = AccountPool::new();
    for id in ["a", "b"] {
        pool.register(ProviderAccount {
            descriptor: AccountDescriptor {
                id: AccountId::from(id),
                provider: provider.clone(),
                enabled: true,
            },
            adapter: Arc::new(ScriptedProvider {
                id: provider.clone(),
                account: AccountId::from(id),
                calls: calls.clone(),
            }),
        })
        .unwrap();
    }
    let clock = Arc::new(ManualClock::new());
    let health = AccountHealthStore::new();
    let router = Router::new(Arc::new(ProviderRegistry::new()), None)
        .with_accounts(Arc::new(pool))
        .with_health(health.clone(), clock.clone());

    let first = router
        .execute_plan(
            &request(),
            &plan(provider.clone()),
            &ProviderContext::new("r1", provider.clone(), ModelId("m".into())),
        )
        .await
        .unwrap();
    assert_eq!(
        first
            .attempts
            .iter()
            .map(|a| a.account().unwrap().0.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[AccountId::from("a"), AccountId::from("b")]
    );
    assert!(!health.is_eligible(&AccountId::from("a"), clock.now()));

    let second = router
        .execute_plan(
            &request(),
            &plan(provider.clone()),
            &ProviderContext::new("r2", provider.clone(), ModelId("m".into())),
        )
        .await
        .unwrap();
    assert_eq!(
        second
            .attempts
            .iter()
            .map(|a| a.account().unwrap().0.as_str())
            .collect::<Vec<_>>(),
        ["b"]
    );
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[
            AccountId::from("a"),
            AccountId::from("b"),
            AccountId::from("b")
        ]
    );

    clock.advance(Duration::from_secs(31));
    let third = router
        .execute_plan(
            &request(),
            &plan(provider.clone()),
            &ProviderContext::new("r3", provider, ModelId("m".into())),
        )
        .await
        .unwrap();
    assert_eq!(
        third
            .attempts
            .iter()
            .map(|a| a.account().unwrap().0.as_str())
            .collect::<Vec<_>>(),
        ["a"]
    );
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        &[
            AccountId::from("a"),
            AccountId::from("b"),
            AccountId::from("b"),
            AccountId::from("a")
        ]
    );
}

#[tokio::test]
async fn explicit_cooling_account_is_not_executed() {
    let provider = ProviderId::from("fake");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut pool = AccountPool::new();
    pool.register(ProviderAccount {
        descriptor: AccountDescriptor {
            id: AccountId::from("a"),
            provider: provider.clone(),
            enabled: true,
        },
        adapter: Arc::new(ScriptedProvider {
            id: provider.clone(),
            account: AccountId::from("a"),
            calls: calls.clone(),
        }),
    })
    .unwrap();
    let clock = Arc::new(ManualClock::new());
    let health = AccountHealthStore::new();
    health.mark_cooldown(
        AccountId::from("a"),
        ProviderErrorCategory::RateLimit,
        clock.now(),
        Duration::from_secs(30),
    );
    let router = Router::new(Arc::new(ProviderRegistry::new()), None)
        .with_accounts(Arc::new(pool))
        .with_health(health, clock);
    let route = RoutePlan {
        candidates: vec![RouteCandidate {
            provider,
            model: ModelId("m".into()),
            account: Some(AccountId::from("a")),
        }],
        policy: RoutingPolicy::new(1, true).unwrap(),
    };
    assert!(
        router
            .execute_plan(
                &request(),
                &route,
                &ProviderContext::new("r", ProviderId::from("fake"), ModelId("m".into()))
            )
            .await
            .is_err()
    );
    assert!(calls.lock().unwrap().is_empty());
}
