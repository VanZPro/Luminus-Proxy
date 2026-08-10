use std::sync::Arc;

use luminus_core::{
    model::{Capability, ModelId, ProviderId},
    protocol::{CanonicalRequest, CanonicalResponse, ContentPart},
    provider::ProviderContext,
};

use crate::{ProviderRegistry, RouterError};

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
        model::{ModelInfo, ProviderId},
        protocol::{CanonicalMessage, ContentPart, ResponseId, Usage},
        provider::{ProviderAdapter, ProviderError, ProviderErrorCategory},
    };

    struct FakeProvider {
        id: ProviderId,
        capabilities: Vec<Capability>,
        fail: bool,
    }

    impl ProviderAdapter for FakeProvider {
        fn provider_id(&self) -> &ProviderId {
            &self.id
        }
        fn models(&self) -> Vec<ModelInfo> {
            vec![ModelInfo {
                id: ModelId("fake-model".into()),
                provider: self.id.clone(),
                capabilities: self.capabilities.clone(),
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
                if self.fail {
                    return Err(ProviderError::new(
                        ProviderErrorCategory::UpstreamUnavailable,
                        "fake failure",
                        true,
                    ));
                }
                Ok(CanonicalResponse {
                    id: ResponseId("fake-response".into()),
                    model: ModelId("fake-model".into()),
                    content: vec![ContentPart::text("fake response")],
                    finish_reason: luminus_core::protocol::FinishReason::Stop,
                    usage: Usage::default(),
                    provider_metadata: None,
                })
            })
        }
    }

    fn request() -> CanonicalRequest {
        CanonicalRequest {
            model: ModelId("fake-model".into()),
            messages: vec![CanonicalMessage::text(
                luminus_core::protocol::MessageRole::User,
                "hello",
            )],
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

    fn router(provider: FakeProvider) -> Router {
        let id = provider.id.clone();
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(provider));
        Router::new(Arc::new(registry), Some(id))
    }

    #[test]
    fn registry_registers_and_retrieves_provider() {
        let id = ProviderId("fake".into());
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(FakeProvider {
            id: id.clone(),
            capabilities: vec![Capability::Chat],
            fail: false,
        }));
        assert!(registry.get(&id).is_some());
        assert_eq!(registry.list(), vec![id]);
    }

    #[test]
    fn unknown_provider_is_reported() {
        let router = Router::new(
            Arc::new(ProviderRegistry::new()),
            Some(ProviderId("missing".into())),
        );
        assert!(matches!(
            router.resolve(&request()),
            Err(RouterError::ProviderNotFound)
        ));
    }

    #[test]
    fn required_capabilities_are_derived() {
        let mut request = request();
        request.tools.push(luminus_core::protocol::ToolDefinition {
            name: "tool".into(),
            description: None,
            parameters: serde_json::json!({}),
        });
        request.stream = true;
        assert_eq!(
            required_capabilities(&request),
            vec![Capability::Chat, Capability::Tools, Capability::Streaming]
        );
        request.messages[0].content.push(ContentPart::Image {
            image: luminus_core::protocol::ImageContent {
                media_type: "image/png".into(),
                uri: "data:".into(),
            },
        });
        assert!(required_capabilities(&request).contains(&Capability::Vision));
    }

    #[tokio::test]
    async fn missing_capability_is_rejected_before_execution() {
        let router = router(FakeProvider {
            id: ProviderId("fake".into()),
            capabilities: vec![Capability::Chat],
            fail: false,
        });
        let mut request = request();
        request.tools.push(luminus_core::protocol::ToolDefinition {
            name: "tool".into(),
            description: None,
            parameters: serde_json::json!({}),
        });
        assert!(matches!(
            router.resolve(&request),
            Err(RouterError::UnsupportedCapability)
        ));
    }

    #[tokio::test]
    async fn execution_and_provider_error_are_preserved() {
        let provider = FakeProvider {
            id: ProviderId("fake".into()),
            capabilities: vec![Capability::Chat],
            fail: false,
        };
        let router = router(provider);
        let context = ProviderContext::new(
            "test",
            ProviderId("fake".into()),
            ModelId("fake-model".into()),
        );
        assert_eq!(
            router.execute(&request(), &context).await.unwrap().id.0,
            "fake-response"
        );
        let failing = super::Router::new(
            Arc::new({
                let mut registry = ProviderRegistry::new();
                registry.register(Arc::new(FakeProvider {
                    id: ProviderId("fake".into()),
                    capabilities: vec![Capability::Chat],
                    fail: true,
                }));
                registry
            }),
            Some(ProviderId("fake".into())),
        );
        assert!(matches!(
            failing.execute(&request(), &context).await,
            Err(RouterError::ProviderExecution(_))
        ));
    }
}
