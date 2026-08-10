use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_definition_and_call_preserve_json_arguments() {
        let definition = ToolDefinition {
            name: "lookup".into(),
            description: Some("Look something up".into()),
            parameters: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        };
        let call = ToolCall {
            id: "call-1".into(),
            name: definition.name.clone(),
            arguments: json!({"query": "rust"}),
        };
        let round_trip: ToolCall =
            serde_json::from_str(&serde_json::to_string(&call).expect("serializes"))
                .expect("deserializes");
        assert_eq!(round_trip.arguments["query"], "rust");
    }
}
