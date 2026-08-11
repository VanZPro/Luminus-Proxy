use std::sync::Arc;

use luminus_core::{
    model::{Capability, ModelId, ProviderId},
    protocol::{CanonicalRequest, CanonicalResponse, ContentPart},
    provider::ProviderContext,
};

use crate::{ProviderRegistry, RouterError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteCandidate {
    pub provider: ProviderId,
    pub model: ModelId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingPolicy {
    pub max_attempts: usize,
    pub fallback_on_retryable: bool,
}

impl RoutingPolicy {
    pub fn new(max_attempts: usize, fallback_on_retryable: bool) -> Result<Self, RouterError> {
        if max_attempts == 0 {
            return Err(RouterError::InvalidPolicy);
        }
        Ok(Self {
            max_attempts,
            fallback_on_retryable,
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
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteAttempt {
    pub target: RouteCandidate,
    pub outcome: RouteAttemptOutcome,
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
}

#[derive(Clone)]
pub struct Router {
    registry: Arc<ProviderRegistry>,
    default_provider: Option<ProviderId>,
}

impl Router {
    pub fn new(registry: Arc<ProviderRegistry>, default_provider: Option<ProviderId>) -> Self {
        Self {
            registry,
            default_provider,
        }
    }

    pub fn resolve(&self, request: &CanonicalRequest) -> Result<RouteTarget, RouterError> {
        let provider_id = self
            .default_provider
            .clone()
            .ok_or(RouterError::NoEligibleProvider)?;
        let provider = self
            .registry
            .get(&provider_id)
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
        for candidate in plan.candidates.iter().take(plan.policy.max_attempts) {
            let Some(provider) = self.registry.get(&candidate.provider) else {
                continue;
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
            match provider.execute(request, context).await {
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
                    let retryable = error.retryable;
                    let category = error.category;
                    attempts.push(RouteAttempt {
                        target: candidate.clone(),
                        outcome: RouteAttemptOutcome::Failed {
                            category,
                            retryable,
                        },
                    });
                    if !retryable || !plan.policy.fallback_on_retryable {
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
                },
                RouteCandidate {
                    provider: ProviderId("b".into()),
                    model: ModelId("m".into()),
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
