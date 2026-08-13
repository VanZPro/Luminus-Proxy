use std::{collections::HashSet, sync::Arc};

use luminus_core::{
    model::{AccountId, Capability, ModelId, ProviderId},
    protocol::{CanonicalRequest, CanonicalResponse, ContentPart},
    provider::ProviderContext,
};

use crate::{
    AccountHealthStore, AccountPool, AccountSelectionStrategy, AccountSelector, Clock,
    CooldownPolicy, ProviderRegistry, RouterError, SystemClock,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteCandidate {
    pub provider: ProviderId,
    pub model: ModelId,
    pub account: Option<AccountId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingPolicy {
    pub max_attempts: usize,
    pub fallback_enabled: bool,
}

impl RoutingPolicy {
    pub fn new(max_attempts: usize, fallback_enabled: bool) -> Result<Self, RouterError> {
        if max_attempts == 0 {
            return Err(RouterError::InvalidPolicy);
        }
        Ok(Self {
            max_attempts,
            fallback_enabled,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePlan {
    pub candidates: Vec<RouteCandidate>,
    pub policy: RoutingPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteAttemptOutcome {
    Success,
    Failed {
        category: luminus_core::provider::ProviderErrorCategory,
        retryable: bool,
        fallback_allowed: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteAttempt {
    pub target: RouteCandidate,
    pub outcome: RouteAttemptOutcome,
}

impl RouteAttempt {
    pub fn account(&self) -> Option<&AccountId> {
        self.target.account.as_ref()
    }
}

#[derive(Debug)]
pub struct RouteExecution {
    pub response: CanonicalResponse,
    pub selected_target: RouteCandidate,
    pub attempts: Vec<RouteAttempt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteTarget {
    pub provider: ProviderId,
    pub model: ModelId,
    pub account: Option<AccountId>,
}

#[derive(Clone)]
pub struct Router {
    registry: Arc<ProviderRegistry>,
    accounts: Arc<AccountPool>,
    default_provider: Option<ProviderId>,
    health: AccountHealthStore,
    clock: Arc<dyn Clock>,
    cooldown_policy: CooldownPolicy,
    selector: AccountSelector,
}

impl Router {
    pub fn new(registry: Arc<ProviderRegistry>, default_provider: Option<ProviderId>) -> Self {
        Self {
            registry,
            accounts: Arc::new(AccountPool::new()),
            default_provider,
            health: AccountHealthStore::new(),
            clock: Arc::new(SystemClock),
            cooldown_policy: CooldownPolicy::new(),
            selector: AccountSelector::new(AccountSelectionStrategy::FirstEligible),
        }
    }

    pub fn with_accounts(mut self, accounts: Arc<AccountPool>) -> Self {
        self.accounts = accounts;
        self
    }

    pub fn with_health(mut self, health: AccountHealthStore, clock: Arc<dyn Clock>) -> Self {
        self.health = health;
        self.clock = clock;
        self
    }

    pub fn with_account_selection(mut self, strategy: AccountSelectionStrategy) -> Self {
        self.selector = AccountSelector::new(strategy);
        self
    }

    pub fn health(&self) -> &AccountHealthStore {
        &self.health
    }

    pub fn runtime_invariants(&self) -> (usize, bool, AccountSelectionStrategy, bool) {
        (2, true, self.selector.strategy(), self.health.is_empty())
    }

    pub fn resolve(&self, request: &CanonicalRequest) -> Result<RouteTarget, RouterError> {
        let provider_id = self
            .default_provider
            .clone()
            .ok_or(RouterError::NoEligibleProvider)?;
        let provider = self
            .registry
            .get(&provider_id)
            .or_else(|| {
                self.accounts
                    .eligible_for_provider(&provider_id, &request.model)
                    .into_iter()
                    .next()
                    .map(|account| account.adapter.clone())
            })
            .ok_or(RouterError::ProviderNotFound)?;
        let model = provider
            .models()
            .into_iter()
            .find(|model| model.id == request.model && model.provider == provider_id)
            .ok_or(RouterError::ModelNotFound)?;
        if required_capabilities(request)
            .iter()
            .any(|capability| !model.capabilities.contains(capability))
        {
            return Err(RouterError::UnsupportedCapability);
        }
        Ok(RouteTarget {
            provider: provider_id,
            model: model.id,
            account: None,
        })
    }

    pub async fn execute(
        &self,
        request: &CanonicalRequest,
        context: &ProviderContext,
    ) -> Result<CanonicalResponse, RouterError> {
        let target = self.resolve(request)?;
        let provider = self
            .registry
            .get(&target.provider)
            .ok_or(RouterError::ProviderNotFound)?;
        Ok(provider.execute(request, context).await?)
    }

    pub async fn execute_plan(
        &self,
        request: &CanonicalRequest,
        plan: &RoutePlan,
        context: &ProviderContext,
    ) -> Result<RouteExecution, RouterError> {
        let mut attempts = Vec::new();
        let mut expanded = Vec::new();
        for candidate in &plan.candidates {
            if candidate.account.is_some() {
                expanded.push(candidate.clone());
            } else {
                let accounts = self
                    .accounts
                    .eligible_for_provider(&candidate.provider, &candidate.model);
                if !accounts.is_empty() {
                    let eligible_ids = accounts
                        .into_iter()
                        .filter(|account| {
                            self.health
                                .is_eligible(&account.descriptor.id, self.clock.now())
                        })
                        .map(|account| account.descriptor.id.clone())
                        .collect();
                    let ordered_ids = self.selector.select(
                        &candidate.provider,
                        eligible_ids,
                        self.accounts.ordered_ids_for_provider(&candidate.provider),
                    );
                    expanded.extend(ordered_ids.into_iter().map(|account_id| RouteCandidate {
                        provider: candidate.provider.clone(),
                        model: candidate.model.clone(),
                        account: Some(account_id),
                    }));
                } else if self.registry.get(&candidate.provider).is_some() {
                    expanded.push(candidate.clone());
                }
            }
        }
        let mut visited: HashSet<(String, String, Option<AccountId>)> = HashSet::new();
        for candidate in &expanded {
            if attempts.len() >= plan.policy.max_attempts {
                break;
            }
            if !visited.insert((
                candidate.provider.0.clone(),
                candidate.model.0.clone(),
                candidate.account.clone(),
            )) {
                continue;
            }
            let (provider, account_id) = if let Some(account_id) = &candidate.account {
                let Some(account) = self.accounts.get(account_id) else {
                    continue;
                };
                if !account.descriptor.enabled || account.descriptor.provider != candidate.provider
                {
                    continue;
                }
                if !self.health.is_eligible(account_id, self.clock.now()) {
                    continue;
                }
                (account.adapter.clone(), Some(account_id.clone()))
            } else {
                let Some(provider) = self.registry.get(&candidate.provider) else {
                    continue;
                };
                (provider, None)
            };
            let Some(model) = provider
                .models()
                .into_iter()
                .find(|model| model.id == candidate.model && model.provider == candidate.provider)
            else {
                continue;
            };
            if required_capabilities(request)
                .iter()
                .any(|capability| !model.capabilities.contains(capability))
            {
                continue;
            }
            let mut account_context = context.clone();
            account_context.account_id = account_id.clone();
            match provider.execute(request, &account_context).await {
                Ok(response) => {
                    attempts.push(RouteAttempt {
                        target: candidate.clone(),
                        outcome: RouteAttemptOutcome::Success,
                    });
                    return Ok(RouteExecution {
                        response,
                        selected_target: candidate.clone(),
                        attempts,
                    });
                }
                Err(error) => {
                    if let Some(account_id) = &account_id {
                        self.health.record(
                            account_id,
                            &error,
                            self.clock.now(),
                            &self.cooldown_policy,
                        );
                    }
                    let retryable = error.retryable;
                    let fallback_allowed = error.fallback_allowed();
                    let category = error.category;
                    attempts.push(RouteAttempt {
                        target: candidate.clone(),
                        outcome: RouteAttemptOutcome::Failed {
                            category,
                            retryable,
                            fallback_allowed,
                        },
                    });
                    if !fallback_allowed || !plan.policy.fallback_enabled {
                        return Err(RouterError::ProviderExecution(error));
                    }
                }
            }
        }
        if attempts.is_empty() {
            return Err(RouterError::NoEligibleProvider);
        }
        Err(RouterError::ProviderExecution(
            luminus_core::provider::ProviderError::new(
                luminus_core::provider::ProviderErrorCategory::UpstreamUnavailable,
                "all route candidates failed",
                false,
            ),
        ))
    }
}

pub fn required_capabilities(request: &CanonicalRequest) -> Vec<Capability> {
    let mut result = vec![Capability::Chat];
    if !request.tools.is_empty()
        || request.messages.iter().any(|message| {
            message.content.iter().any(|part| {
                matches!(
                    part,
                    ContentPart::ToolCall { .. } | ContentPart::ToolResult { .. }
                )
            })
        })
    {
        result.push(Capability::Tools);
    }
    if request.messages.iter().any(|message| {
        message
            .content
            .iter()
            .any(|part| matches!(part, ContentPart::Image { .. }))
    }) {
        result.push(Capability::Vision);
    }
    if request.messages.iter().any(|message| {
        message
            .content
            .iter()
            .any(|part| matches!(part, ContentPart::Reasoning { .. }))
    }) || request
        .reasoning
        .as_ref()
        .is_some_and(|reasoning| reasoning.enabled)
    {
        result.push(Capability::Reasoning);
    }
    if request.stream {
        result.push(Capability::Streaming);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use luminus_core::{
        model::ModelInfo,
        protocol::{CanonicalMessage, FinishReason, MessageRole, ResponseId, Usage},
        provider::{ProviderAdapter, ProviderError, ProviderErrorCategory},
    };
    use std::sync::{Arc, Mutex};

    struct FakeProvider {
        id: ProviderId,
        outcome: Option<ProviderErrorCategory>,
        calls: Arc<Mutex<Vec<ProviderId>>>,
    }
    impl ProviderAdapter for FakeProvider {
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
            Box<
                dyn std::future::Future<Output = Result<CanonicalResponse, ProviderError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                self.calls.lock().unwrap().push(self.id.clone());
                if let Some(category) = self.outcome {
                    return Err(ProviderError::new(
                        category,
                        "fake",
                        category != ProviderErrorCategory::InvalidRequest,
                    ));
                }
                Ok(CanonicalResponse {
                    id: ResponseId("ok".into()),
                    model: ModelId("m".into()),
                    content: vec![ContentPart::text("ok")],
                    finish_reason: FinishReason::Stop,
                    usage: Usage::default(),
                    provider_metadata: None,
                })
            })
        }
    }
    fn request() -> CanonicalRequest {
        CanonicalRequest {
            model: ModelId("m".into()),
            messages: vec![CanonicalMessage::text(MessageRole::User, "hi")],
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
    fn plan(ids: &[&str], max: usize) -> (Router, RoutePlan, Arc<Mutex<Vec<ProviderId>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut registry = ProviderRegistry::new();
        let mut candidates = Vec::new();
        for id in ids {
            let provider = ProviderId((*id).into());
            registry.register(Arc::new(FakeProvider {
                id: provider.clone(),
                outcome: if *id == "a" {
                    Some(ProviderErrorCategory::UpstreamUnavailable)
                } else {
                    None
                },
                calls: calls.clone(),
            }));
            candidates.push(RouteCandidate {
                provider,
                model: ModelId("m".into()),
                account: None,
            });
        }
        (
            Router::new(Arc::new(registry), None),
            RoutePlan {
                candidates,
                policy: RoutingPolicy::new(max, true).unwrap(),
            },
            calls,
        )
    }

    #[test]
    fn zero_attempt_policy_is_rejected() {
        assert!(RoutingPolicy::new(0, true).is_err());
    }

    #[tokio::test]
    async fn retryable_failure_falls_back_in_order() {
        let (router, plan, calls) = plan(&["a", "b"], 2);
        let context = ProviderContext::new("test", ProviderId("a".into()), ModelId("m".into()));
        let result = router
            .execute_plan(&request(), &plan, &context)
            .await
            .unwrap();
        assert_eq!(result.attempts.len(), 2);
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .map(|id| id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[tokio::test]
    async fn explicit_account_candidate_uses_account_adapter_and_history() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let provider_id = ProviderId::from("fake");
        let account_id = AccountId::from("account-a");
        let adapter = Arc::new(FakeProvider {
            id: provider_id.clone(),
            outcome: None,
            calls: calls.clone(),
        });
        let mut accounts = AccountPool::new();
        accounts
            .register(crate::ProviderAccount {
                descriptor: luminus_core::model::AccountDescriptor {
                    id: account_id.clone(),
                    provider: provider_id.clone(),
                    enabled: true,
                },
                adapter,
            })
            .unwrap();
        let router =
            Router::new(Arc::new(ProviderRegistry::new()), None).with_accounts(Arc::new(accounts));
        let plan = RoutePlan {
            candidates: vec![RouteCandidate {
                provider: provider_id.clone(),
                model: ModelId("m".into()),
                account: Some(account_id.clone()),
            }],
            policy: RoutingPolicy::new(1, true).unwrap(),
        };
        let context = ProviderContext::new("test", provider_id, ModelId("m".into()));
        let execution = router
            .execute_plan(&request(), &plan, &context)
            .await
            .unwrap();
        assert_eq!(execution.attempts[0].account(), Some(&account_id));
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn provider_level_candidate_expands_accounts_in_order() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let provider_id = ProviderId::from("fake");
        let mut pool = AccountPool::new();
        for id in ["a1", "a2"] {
            pool.register(crate::ProviderAccount {
                descriptor: luminus_core::model::AccountDescriptor {
                    id: AccountId::from(id),
                    provider: provider_id.clone(),
                    enabled: true,
                },
                adapter: Arc::new(FakeProvider {
                    id: provider_id.clone(),
                    outcome: None,
                    calls: calls.clone(),
                }),
            })
            .unwrap();
        }
        let router =
            Router::new(Arc::new(ProviderRegistry::new()), None).with_accounts(Arc::new(pool));
        let plan = RoutePlan {
            candidates: vec![RouteCandidate {
                provider: provider_id.clone(),
                model: ModelId("m".into()),
                account: None,
            }],
            policy: RoutingPolicy::new(2, true).unwrap(),
        };
        let result = router
            .execute_plan(
                &request(),
                &plan,
                &ProviderContext::new("id", provider_id, ModelId("m".into())),
            )
            .await
            .unwrap();
        assert_eq!(result.attempts[0].account(), Some(&AccountId::from("a1")));
    }

    #[tokio::test]
    async fn non_retryable_error_stops() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(FakeProvider {
            id: ProviderId("a".into()),
            outcome: Some(ProviderErrorCategory::InvalidRequest),
            calls: calls.clone(),
        }));
        registry.register(Arc::new(FakeProvider {
            id: ProviderId("b".into()),
            outcome: None,
            calls: calls.clone(),
        }));
        let router = Router::new(Arc::new(registry), None);
        let plan = RoutePlan {
            candidates: vec![
                RouteCandidate {
                    provider: ProviderId("a".into()),
                    model: ModelId("m".into()),
                    account: None,
                },
                RouteCandidate {
                    provider: ProviderId("b".into()),
                    model: ModelId("m".into()),
                    account: None,
                },
            ],
            policy: RoutingPolicy::new(2, true).unwrap(),
        };
        let context = ProviderContext::new("test", ProviderId("a".into()), ModelId("m".into()));
        assert!(
            router
                .execute_plan(&request(), &plan, &context)
                .await
                .is_err()
        );
        assert_eq!(calls.lock().unwrap().len(), 1);
    }
}
