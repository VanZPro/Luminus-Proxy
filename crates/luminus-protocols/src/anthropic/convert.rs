use super::messages::*;
use crate::ProtocolError;
use luminus_core::model::ModelId;
use luminus_core::protocol::{
    CanonicalMessage, CanonicalRequest, CanonicalResponse, ContentPart, FinishReason, MessageRole,
    ToolCall, ToolChoice as CToolChoice, ToolDefinition,
};

impl TryFrom<MessagesRequest> for CanonicalRequest {
    type Error = ProtocolError;
    fn try_from(value: MessagesRequest) -> Result<Self, Self::Error> {
        let mut messages = Vec::new();
        if let Some(system) = value.system {
            messages.push(CanonicalMessage::text(MessageRole::System, system));
        }
        for message in value.messages {
            let role = match message.role.as_str() {
                "user" => MessageRole::User,
                "assistant" => MessageRole::Assistant,
                other => return Err(ProtocolError::InvalidRole(other.into())),
            };
            let content = message
                .content
                .into_iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => Ok(ContentPart::text(text)),
                    ContentBlock::Thinking { thinking } => {
                        Ok(ContentPart::Reasoning { text: thinking })
                    }
                    ContentBlock::ToolUse { id, name, input } => Ok(ContentPart::ToolCall {
                        call: ToolCall {
                            id,
                            name,
                            arguments: input,
                        },
                    }),
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                    } => Ok(ContentPart::ToolResult {
                        tool_call_id: tool_use_id,
                        content: content
                            .into_iter()
                            .filter_map(|b| {
                                if let ContentBlock::Text { text } = b {
                                    Some(ContentPart::text(text))
                                } else {
                                    None
                                }
                            })
                            .collect(),
                    }),
                })
                .collect::<Result<Vec<_>, ProtocolError>>()?;
            messages.push(CanonicalMessage {
                role,
                content,
                name: None,
            });
        }
        Ok(CanonicalRequest {
            model: ModelId(value.model),
            messages,
            tools: value
                .tools
                .unwrap_or_default()
                .into_iter()
                .map(|t| ToolDefinition {
                    name: t.name,
                    description: t.description,
                    parameters: t.input_schema,
                })
                .collect(),
            tool_choice: value.tool_choice.map(|c| match c {
                ToolChoice::Auto => CToolChoice::Auto,
                ToolChoice::Any => CToolChoice::Required,
                ToolChoice::Tool { name } => CToolChoice::Specific { name },
            }),
            temperature: value.temperature,
            top_p: value.top_p,
            max_output_tokens: Some(value.max_tokens),
            stop: value
                .stop_sequences
                .map(luminus_core::protocol::StopSequence::Multiple),
            stream: value.stream,
            reasoning: None,
            metadata: None,
        })
    }
}

impl TryFrom<CanonicalResponse> for MessagesResponse {
    type Error = ProtocolError;
    fn try_from(value: CanonicalResponse) -> Result<Self, Self::Error> {
        let content = value
            .content
            .into_iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(ContentBlock::Text { text }),
                ContentPart::Reasoning { text } => Some(ContentBlock::Thinking { thinking: text }),
                ContentPart::ToolCall { call } => Some(ContentBlock::ToolUse {
                    id: call.id,
                    name: call.name,
                    input: call.arguments,
                }),
                _ => None,
            })
            .collect();
        let stop_reason = Some(
            match value.finish_reason {
                FinishReason::Stop => "end_turn",
                FinishReason::ToolCalls => "tool_use",
                FinishReason::Length => "max_tokens",
                FinishReason::ContentFilter | FinishReason::Error | FinishReason::Other => {
                    "end_turn"
                }
            }
            .into(),
        );
        Ok(MessagesResponse {
            id: value.id.0,
            r#type: "message".into(),
            role: "assistant".into(),
            content,
            model: value.model.0,
            stop_reason,
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: value.usage.input_tokens,
                output_tokens: value.usage.output_tokens,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_and_tool_use_convert() {
        let request = MessagesRequest {
            model: "m".into(),
            max_tokens: 10,
            system: Some("s".into()),
            messages: vec![Message {
                role: "user".into(),
                content: vec![
                    ContentBlock::Text { text: "u".into() },
                    ContentBlock::ToolUse {
                        id: "t".into(),
                        name: "f".into(),
                        input: serde_json::json!({"x":1}),
                    },
                ],
            }],
            temperature: None,
            top_p: None,
            stop_sequences: None,
            stream: true,
            tools: None,
            tool_choice: None,
        };
        let c = CanonicalRequest::try_from(request).unwrap();
        assert_eq!(c.messages.len(), 2);
        assert!(c.stream);
    }
}
