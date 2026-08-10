use super::ContentPart;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageRole {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalMessage {
    pub role: MessageRole,
    pub content: Vec<ContentPart>,
    pub name: Option<String>,
}

impl CanonicalMessage {
    pub fn text(role: MessageRole, text: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentPart::text(text)],
            name: None,
        }
    }
}
