use super::chat::*;
use crate::ProtocolError;
use luminus_core::model::ModelId;
use luminus_core::protocol::{
    CanonicalMessage, CanonicalRequest, CanonicalResponse, ContentPart, MessageRole, StopSequence,
    ToolChoice,
};

impl TryFrom<ChatRequest> for CanonicalRequest {
    type Error = ProtocolError;
    fn try_from(value: ChatRequest) -> Result<Self, Self::Error> {
        let messages = value
            .messages
            .into_iter()
            .map(|m| match m {
                ChatMessage::System { content } => Ok(CanonicalMessage::text(
                    MessageRole::System,
                    text_content(&content)
                        .ok_or_else(|| ProtocolError::InvalidContent("system".into()))?,
                )),
                ChatMessage::Developer { content } => Ok(CanonicalMessage::text(
                    MessageRole::Developer,
                    text_content(&content)
                        .ok_or_else(|| ProtocolError::InvalidContent("developer".into()))?,
                )),
                ChatMessage::User { content } => Ok(CanonicalMessage {
                    role: MessageRole::User,
                    content: canonical_content(&content),
                    name: None,
                }),
                ChatMessage::Assistant {
                    content,
                    tool_calls,
                } => {
                    let mut parts = content.as_ref().map(canonical_content).unwrap_or_default();
                    if let Some(calls) = tool_calls {
                        for call in calls {
                            parts.push(ContentPart::ToolCall {
                                call: tool_call(&call)?,
                            });
                        }
                    }
                    Ok(CanonicalMessage {
                        role: MessageRole::Assistant,
                        content: parts,
                        name: None,
                    })
                }
                ChatMessage::Tool {
                    tool_call_id,
                    content,
                } => Ok(CanonicalMessage {
                    role: MessageRole::Tool,
                    content: vec![ContentPart::ToolResult {
                        tool_call_id,
                        content: canonical_content(&content),
                    }],
                    name: None,
                }),
            })
            .collect::<Result<Vec<_>, ProtocolError>>()?;
        let stop = value.stop.map(|s| match s {
            Stop::One(x) => StopSequence::Single(x),
            Stop::Many(x) => StopSequence::Multiple(x),
        });
        Ok(CanonicalRequest {
            model: ModelId(value.model),
            messages,
            tools: value
                .tools
                .unwrap_or_default()
                .iter()
                .map(tool_def)
                .collect(),
            tool_choice: value.tool_choice.map(|_| ToolChoice::Auto),
            temperature: value.temperature,
            top_p: value.top_p,
            max_output_tokens: value.max_completion_tokens.or(value.max_tokens),
            stop,
            stream: value.stream,
            reasoning: None,
            metadata: None,
        })
    }
}

impl TryFrom<CanonicalResponse> for ChatResponse {
    type Error = ProtocolError;
    fn try_from(value: CanonicalResponse) -> Result<Self, Self::Error> {
        let mut content = String::new();
        let mut calls = Vec::new();
        for part in value.content {
            match part {
                ContentPart::Text { text } => content.push_str(&text),
                ContentPart::ToolCall { call } => calls.push(ChatToolCall {
                    id: call.id,
                    r#type: "function".into(),
                    function: FunctionCall {
                        name: call.name,
                        arguments: serde_json::to_string(&call.arguments).map_err(|_| {
                            ProtocolError::InvalidResponseShape("tool arguments".into())
                        })?,
                    },
                }),
                _ => {}
            }
        }
        Ok(ChatResponse {
            id: value.id.0,
            object: "chat.completion".into(),
            model: value.model.0,
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".into(),
                    content: if content.is_empty() {
                        None
                    } else {
                        Some(content)
                    },
                    tool_calls: if calls.is_empty() { None } else { Some(calls) },
                },
                finish_reason: finish_name(value.finish_reason),
            }],
            usage: Some(value.usage),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_tool_arguments_fail() {
        let call = ChatToolCall {
            id: "x".into(),
            r#type: "function".into(),
            function: FunctionCall {
                name: "f".into(),
                arguments: "{".into(),
            },
        };
        assert!(tool_call(&call).is_err());
    }
}
