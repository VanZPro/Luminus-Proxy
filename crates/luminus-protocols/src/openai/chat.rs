use luminus_core::protocol::{
    ContentPart, FinishReason, MessageRole, ToolCall, ToolDefinition, Usage,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub max_completion_tokens: Option<u32>,
    pub stop: Option<Stop>,
    #[serde(default)]
    pub stream: bool,
    pub tools: Option<Vec<ChatTool>>,
    pub tool_choice: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role")]
pub enum ChatMessage {
    System {
        content: MessageContent,
    },
    Developer {
        content: MessageContent,
    },
    User {
        content: MessageContent,
    },
    Assistant {
        content: Option<MessageContent>,
        tool_calls: Option<Vec<ChatToolCall>>,
    },
    Tool {
        tool_call_id: String,
        content: MessageContent,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPartDto>),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentPartDto {
    pub r#type: String,
    pub text: Option<String>,
    pub image_url: Option<ImageUrl>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageUrl {
    pub url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatTool {
    pub r#type: String,
    pub function: FunctionDefinition,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatToolCall {
    pub id: String,
    pub r#type: String,
    pub function: FunctionCall,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Stop {
    One(String),
    Many(Vec<String>),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Choice {
    pub index: u32,
    pub message: ResponseMessage,
    pub finish_reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponseMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ChatToolCall>>,
}

pub fn text_content(c: &MessageContent) -> Option<String> {
    match c {
        MessageContent::Text(s) => Some(s.clone()),
        MessageContent::Parts(p) => {
            let s = p
                .iter()
                .filter_map(|x| x.text.clone())
                .collect::<Vec<_>>()
                .join("");
            if s.is_empty() { None } else { Some(s) }
        }
    }
}
pub fn canonical_content(c: &MessageContent) -> Vec<ContentPart> {
    match c {
        MessageContent::Text(s) => vec![ContentPart::text(s)],
        MessageContent::Parts(p) => p
            .iter()
            .filter_map(|x| match x.r#type.as_str() {
                "text" => x.text.clone().map(ContentPart::text),
                _ => None,
            })
            .collect(),
    }
}
pub fn role_name(r: MessageRole) -> &'static str {
    match r {
        MessageRole::System => "system",
        MessageRole::Developer => "developer",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}
pub fn finish_name(r: FinishReason) -> String {
    match r {
        FinishReason::Stop => "stop",
        FinishReason::Length => "length",
        FinishReason::ToolCalls => "tool_calls",
        FinishReason::ContentFilter => "content_filter",
        FinishReason::Error | FinishReason::Other => "stop",
    }
    .into()
}
pub fn usage(u: Usage) -> Option<Usage> {
    Some(u)
}
pub fn tool_def(t: &ChatTool) -> ToolDefinition {
    ToolDefinition {
        name: t.function.name.clone(),
        description: t.function.description.clone(),
        parameters: t.function.parameters.clone(),
    }
}
pub fn tool_call(t: &ChatToolCall) -> Result<ToolCall, crate::ProtocolError> {
    Ok(ToolCall {
        id: t.id.clone(),
        name: t.function.name.clone(),
        arguments: serde_json::from_str(&t.function.arguments)
            .map_err(|_| crate::ProtocolError::InvalidToolArguments(t.id.clone()))?,
    })
}
