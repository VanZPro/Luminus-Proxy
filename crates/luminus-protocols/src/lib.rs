pub mod anthropic;
pub mod error;
pub mod openai;

pub use error::ProtocolError;

#[cfg(test)]
mod tests {
    use super::anthropic::messages::{
        ContentBlock as AnthropicContent, Message as AnthropicMessage, MessagesRequest,
    };
    use super::openai::chat::{ChatMessage, ChatRequest, MessageContent};
    use luminus_core::protocol::{CanonicalRequest, MessageRole};

    #[test]
    fn canonical_layer_bridges_compatible_text_semantics() {
        let openai = ChatRequest {
            model: "shared-model".into(),
            messages: vec![
                ChatMessage::System {
                    content: MessageContent::Text("Be concise".into()),
                },
                ChatMessage::User {
                    content: MessageContent::Text("Hello".into()),
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            max_completion_tokens: None,
            stop: None,
            stream: false,
            tools: None,
            tool_choice: None,
        };
        let canonical = CanonicalRequest::try_from(openai).unwrap();
        assert_eq!(canonical.messages[0].role, MessageRole::System);
        assert_eq!(canonical.messages[1].role, MessageRole::User);

        let anthropic = MessagesRequest {
            model: canonical.model.0,
            max_tokens: 64,
            system: Some("Be concise".into()),
            messages: vec![AnthropicMessage {
                role: "user".into(),
                content: vec![AnthropicContent::Text {
                    text: "Hello".into(),
                }],
            }],
            temperature: None,
            top_p: None,
            stop_sequences: None,
            stream: false,
            tools: None,
            tool_choice: None,
        };
        let anthropic_canonical = CanonicalRequest::try_from(anthropic).unwrap();
        assert_eq!(
            anthropic_canonical.messages[1].content,
            canonical.messages[1].content
        );
    }
}
