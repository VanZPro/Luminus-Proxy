use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{CanonicalMessage, ToolDefinition};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReasoningConfig {
    pub enabled: bool,
    pub effort: Option<ReasoningEffort>,
    pub budget_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToolChoice {
    Auto,
    Required,
    None,
    Specific { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StopSequence {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalRequest {
    pub model: crate::model::ModelId,
    pub messages: Vec<CanonicalMessage>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: Option<ToolChoice>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_output_tokens: Option<u32>,
    pub stop: Option<StopSequence>,
    pub stream: bool,
    pub reasoning: Option<ReasoningConfig>,
    pub metadata: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ContentPart, MessageRole};

    #[test]
    fn request_round_trips_with_multiple_roles() {
        let request = CanonicalRequest {
            model: crate::model::ModelId("demo".into()),
            messages: vec![
                CanonicalMessage::text(MessageRole::System, "system"),
                CanonicalMessage::text(MessageRole::Developer, "developer"),
                CanonicalMessage {
                    role: MessageRole::User,
                    content: vec![ContentPart::text("user")],
                    name: None,
                },
                CanonicalMessage::text(MessageRole::Assistant, "assistant"),
                CanonicalMessage::text(MessageRole::Tool, "tool"),
            ],
            tools: Vec::new(),
            tool_choice: None,
            temperature: Some(0.2),
            top_p: None,
            max_output_tokens: Some(128),
            stop: None,
            stream: true,
            reasoning: None,
            metadata: None,
        };
        let encoded = serde_json::to_string(&request).expect("serializes");
        let decoded: CanonicalRequest = serde_json::from_str(&encoded).expect("deserializes");
        assert_eq!(decoded, request);
    }
}
