use crate::http::{HttpTransport, bounded_body, bounded_error_text, parse_retry_after};
use luminus_core::{
    model::{Capability, ModelId, ModelInfo, ProviderId},
    protocol::{CanonicalRequest, CanonicalResponse, ContentPart, FinishReason, ResponseId, Usage},
    provider::{ProviderAdapter, ProviderContext, ProviderError, ProviderErrorCategory},
};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER};
use serde::{Deserialize, Serialize};

pub struct BlackboxConfig {
    pub base_url: String,
    api_key: String,
}

impl BlackboxConfig {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }
}
impl std::fmt::Debug for BlackboxConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlackboxConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

pub struct BlackboxProvider {
    config: BlackboxConfig,
    transport: HttpTransport,
    id: ProviderId,
}
impl BlackboxProvider {
    pub fn new(config: BlackboxConfig) -> Result<Self, ProviderError> {
        Ok(Self {
            config,
            transport: HttpTransport::new()?,
            id: ProviderId("blackbox".into()),
        })
    }
    fn request(&self, input: &CanonicalRequest) -> Result<UpstreamRequest, ProviderError> {
        if input.stream {
            return Err(ProviderError::new(
                ProviderErrorCategory::UnsupportedCapability,
                "Blackbox streaming is not implemented in R4",
                false,
            ));
        }
        let mut messages = Vec::new();
        for message in &input.messages {
            let role = match message.role {
                luminus_core::protocol::MessageRole::System => "system",
                luminus_core::protocol::MessageRole::Developer => "developer",
                luminus_core::protocol::MessageRole::User => "user",
                luminus_core::protocol::MessageRole::Assistant => "assistant",
                luminus_core::protocol::MessageRole::Tool => "tool",
            };
            let mut text = String::new();
            for part in &message.content {
                match part {
                    ContentPart::Text { text: value } => text.push_str(value),
                    ContentPart::Image { .. } => {
                        return Err(ProviderError::new(
                            ProviderErrorCategory::UnsupportedCapability,
                            "Blackbox image content is not supported by this proof of concept",
                            false,
                        ));
                    }
                    ContentPart::ToolCall { call } => {
                        text.push_str(&serde_json::to_string(call).unwrap_or_default());
                    }
                    ContentPart::ToolResult { content, .. } => {
                        for item in content {
                            if let ContentPart::Text { text: value } = item {
                                text.push_str(value);
                            }
                        }
                    }
                    ContentPart::Reasoning { text: value } => text.push_str(value),
                }
            }
            messages.push(UpstreamMessage {
                role: role.into(),
                content: text,
            });
        }
        Ok(UpstreamRequest {
            model: input.model.0.clone(),
            messages,
            temperature: input.temperature,
            top_p: input.top_p,
            max_tokens: input.max_output_tokens,
            tools: if input.tools.is_empty() {
                None
            } else {
                Some(
                    input
                        .tools
                        .iter()
                        .map(|tool| UpstreamTool {
                            r#type: "function".into(),
                            function: tool.clone(),
                        })
                        .collect(),
                )
            },
        })
    }
}

impl ProviderAdapter for BlackboxProvider {
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }
    fn models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: ModelId("bb/claude-sonnet-4.6".into()),
            provider: self.id.clone(),
            capabilities: vec![Capability::Chat, Capability::Tools],
        }]
    }
    fn execute<'a>(
        &'a self,
        request: &'a CanonicalRequest,
        _context: &'a ProviderContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<CanonicalResponse, ProviderError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let payload = self.request(request)?;
            let response = self
                .transport
                .send(
                    self.transport
                        .post(format!(
                            "{}/chat/completions",
                            self.config.base_url.trim_end_matches('/')
                        ))
                        .header(AUTHORIZATION, format!("Bearer {}", self.config.api_key))
                        .header(CONTENT_TYPE, "application/json")
                        .json(&payload)
                        .send()
                        .await,
                )
                .await?;
            let status = response.status();
            let cooldown = parse_retry_after(
                response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok()),
            );
            let raw_body = bounded_body(response).await?;
            if !status.is_success() {
                let detail = bounded_error_text(&raw_body);
                let mut error = HttpTransport::status_error(status, cooldown);
                if !detail.is_empty() {
                    error.message = format!("{}: {}", error.message, detail);
                }
                return Err(error);
            }
            let body: UpstreamResponse = serde_json::from_slice(&raw_body).map_err(|_| {
                ProviderError::new(
                    ProviderErrorCategory::ProviderFailure,
                    "Invalid Blackbox response JSON",
                    false,
                )
            })?;
            let choice = body.choices.into_iter().next().ok_or_else(|| {
                ProviderError::new(
                    ProviderErrorCategory::ProviderFailure,
                    "Blackbox response contained no choices",
                    false,
                )
            })?;
            Ok(CanonicalResponse {
                id: ResponseId(body.id),
                model: ModelId(body.model.unwrap_or_else(|| request.model.0.clone())),
                content: vec![ContentPart::text(
                    choice.message.content.unwrap_or_default(),
                )],
                finish_reason: match choice.finish_reason.as_deref() {
                    Some("length") => FinishReason::Length,
                    Some("tool_calls") => FinishReason::ToolCalls,
                    Some("content_filter") => FinishReason::ContentFilter,
                    _ => FinishReason::Stop,
                },
                usage: body
                    .usage
                    .map(|u| Usage {
                        input_tokens: u.prompt_tokens,
                        output_tokens: u.completion_tokens,
                        total_tokens: u.total_tokens,
                        ..Usage::default()
                    })
                    .unwrap_or_default(),
                provider_metadata: None,
            })
        })
    }
}

#[derive(Serialize)]
struct UpstreamRequest {
    model: String,
    messages: Vec<UpstreamMessage>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    max_tokens: Option<u32>,
    tools: Option<Vec<UpstreamTool>>,
}
#[derive(Serialize)]
struct UpstreamMessage {
    role: String,
    content: String,
}
#[derive(Serialize)]
struct UpstreamTool {
    r#type: String,
    function: luminus_core::protocol::ToolDefinition,
}
#[derive(Deserialize)]
struct UpstreamResponse {
    id: String,
    model: Option<String>,
    choices: Vec<UpstreamChoice>,
    usage: Option<UpstreamUsage>,
}
#[derive(Deserialize)]
struct UpstreamChoice {
    message: UpstreamAssistant,
    finish_reason: Option<String>,
}
#[derive(Deserialize)]
struct UpstreamAssistant {
    content: Option<String>,
}
#[derive(Deserialize)]
struct UpstreamUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_debug_redacts_key() {
        let text = format!("{:?}", BlackboxConfig::new("http://localhost", "secret"));
        assert!(!text.contains("secret"));
        assert!(text.contains("REDACTED"));
    }
}
