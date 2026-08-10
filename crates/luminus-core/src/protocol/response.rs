use serde::{Deserialize, Serialize};

use super::{ContentPart, Usage};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseId(pub String);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Error,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalResponse {
    pub id: ResponseId,
    pub model: crate::model::ModelId,
    pub content: Vec<ContentPart>,
    pub finish_reason: FinishReason,
    pub usage: Usage,
    pub provider_metadata: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_round_trips() {
        let response = CanonicalResponse {
            id: ResponseId("resp-1".into()),
            model: crate::model::ModelId("demo".into()),
            content: vec![ContentPart::text("done")],
            finish_reason: FinishReason::Stop,
            usage: Usage {
                input_tokens: 2,
                output_tokens: 3,
                total_tokens: 5,
                ..Usage::default()
            },
            provider_metadata: None,
        };
        let json = serde_json::to_string(&response).expect("serializes");
        let decoded: CanonicalResponse = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(decoded, response);
    }
}
