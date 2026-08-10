use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageContent {
    pub media_type: String,
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ContentPart {
    Text {
        text: String,
    },
    Image {
        image: ImageContent,
    },
    ToolCall {
        call: crate::protocol::ToolCall,
    },
    ToolResult {
        tool_call_id: String,
        content: Vec<ContentPart>,
    },
    Reasoning {
        text: String,
    },
}

impl ContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_content_round_trips() {
        let content = vec![
            ContentPart::text("hello"),
            ContentPart::Reasoning {
                text: "think".into(),
            },
        ];
        let json = serde_json::to_string(&content).expect("serializes");
        let decoded: Vec<ContentPart> = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(decoded, content);
    }
}
