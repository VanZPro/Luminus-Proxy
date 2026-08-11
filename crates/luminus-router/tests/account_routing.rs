use luminus_core::{
    model::{AccountDescriptor, AccountId, Capability, ModelId, ModelInfo, ProviderId},
    protocol::{
        CanonicalMessage, CanonicalRequest, CanonicalResponse, ContentPart, FinishReason,
        MessageRole, ResponseId, Usage,
    },
    provider::{ProviderAdapter, ProviderContext, ProviderError, ProviderErrorCategory},
};
use luminus_router::{
    AccountPool, ProviderAccount, ProviderRegistry, RouteCandidate, RoutePlan, Router, RouterError,
    RoutingPolicy,
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Observation {
    account: Option<AccountId>,
    request_id: String,
    provider: ProviderId,
    model: ModelId,
}
struct Scripted {
    id: ProviderId,
    outcome: Option<ProviderErrorCategory>,
    observations: Arc<Mutex<Vec<Observation>>>,
}
impl ProviderAdapter for Scripted {
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
        context: &'a ProviderContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<CanonicalResponse, ProviderError>> + Send + 'a>,
    > {
        Box::pin(async move {
            self.observations.lock().unwrap().push(Observation {
                account: context.account_id.clone(),
                request_id: context.request_id.clone(),
                provider: context.provider_id.clone(),
                model: context.model_id.clone(),
            });
            match self.outcome {
                Some(category) => Err(ProviderError::new(
                    category,
                    "scripted",
                    category != ProviderErrorCategory::InvalidRequest
                        && category != ProviderErrorCategory::Authentication,
                )),
                None => Ok(CanonicalResponse {
                    id: ResponseId("ok".into()),
                    model: ModelId("m".into()),
                    content: vec![ContentPart::text("ok")],
                    finish_reason: FinishReason::Stop,
                    usage: Usage::default(),
                    provider_metadata: None,
                }),
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
fn setup(
    outcomes: &[Option<ProviderErrorCategory>],
) -> (Router, Arc<Mutex<Vec<Observation>>>, ProviderId) {
    let observations = Arc::new(Mutex::new(Vec::new()));
    let provider = ProviderId::from("fake");
    let mut pool = AccountPool::new();
    for (n, outcome) in outcomes.iter().enumerate() {
        let id = AccountId::from(format!("a{}", n + 1));
        pool.register(ProviderAccount {
            descriptor: AccountDescriptor {
                id,
                provider: provider.clone(),
                enabled: true,
            },
            adapter: Arc::new(Scripted {
                id: provider.clone(),
                outcome: *outcome,
                observations: observations.clone(),
            }),
        })
        .unwrap();
    }
    (
        Router::new(Arc::new(ProviderRegistry::new()), None).with_accounts(Arc::new(pool)),
        observations,
        provider,
    )
}
fn plan(provider: ProviderId, max: usize) -> RoutePlan {
    RoutePlan {
        candidates: vec![RouteCandidate {
            provider,
            model: ModelId("m".into()),
            account: None,
        }],
        policy: RoutingPolicy::new(max, true).unwrap(),
    }
}

#[tokio::test]
async fn retryable_errors_fallback_and_context_is_preserved() {
    for category in [
        ProviderErrorCategory::QuotaExceeded,
        ProviderErrorCategory::Timeout,
    ] {
        let (router, seen, provider) = setup(&[Some(category), None]);
        let execution = router
            .execute_plan(
                &request(),
                &plan(provider.clone(), 2),
                &ProviderContext::new("logical", provider.clone(), ModelId("m".into())),
            )
            .await
            .unwrap();
        assert_eq!(
            execution
                .attempts
                .iter()
                .map(|a| a.account().unwrap().0.as_str())
                .collect::<Vec<_>>(),
            ["a1", "a2"]
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].request_id, seen[1].request_id);
        assert_eq!(seen[0].request_id, "logical");
        assert_eq!(seen[0].account, Some(AccountId::from("a1")));
        assert_eq!(seen[1].account, Some(AccountId::from("a2")));
        assert!(
            seen.iter()
                .all(|x| x.provider == provider && x.model == ModelId("m".into()))
        );
    }
}

#[tokio::test]
async fn invalid_request_and_authentication_stop_without_fallback() {
    for category in [
        ProviderErrorCategory::InvalidRequest,
        ProviderErrorCategory::Authentication,
    ] {
        let (router, seen, provider) = setup(&[Some(category), None]);
        assert!(matches!(
            router
                .execute_plan(
                    &request(),
                    &plan(provider, 2),
                    &ProviderContext::new("id", ProviderId::from("fake"), ModelId("m".into()))
                )
                .await,
            Err(RouterError::ProviderExecution(_))
        ));
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn max_attempts_bounds_expanded_accounts_and_deduplicates() {
    let (router, seen, provider) = setup(&[
        Some(ProviderErrorCategory::Timeout),
        Some(ProviderErrorCategory::Timeout),
        None,
    ]);
    let mut route = plan(provider.clone(), 2);
    route.candidates.push(route.candidates[0].clone());
    assert!(
        router
            .execute_plan(
                &request(),
                &route,
                &ProviderContext::new("id", provider, ModelId("m".into()))
            )
            .await
            .is_err()
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].account, Some(AccountId::from("a1")));
    assert_eq!(seen[1].account, Some(AccountId::from("a2")));
}

#[tokio::test]
async fn all_retryable_accounts_fail_deterministically() {
    let (router, seen, provider) = setup(&[
        Some(ProviderErrorCategory::Timeout),
        Some(ProviderErrorCategory::RateLimit),
    ]);
    assert!(matches!(
        router
            .execute_plan(
                &request(),
                &plan(provider.clone(), 2),
                &ProviderContext::new("id", provider, ModelId("m".into()))
            )
            .await,
        Err(RouterError::ProviderExecution(_))
    ));
    assert_eq!(seen.lock().unwrap().len(), 2);
}
