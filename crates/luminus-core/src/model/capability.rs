use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Capability {
    Chat,
    Tools,
    Vision,
    Reasoning,
    Streaming,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderId(pub String);

impl From<&str> for ProviderId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: ModelId,
    pub provider: ProviderId,
    pub capabilities: Vec<Capability>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_round_trip() {
        let info = ModelInfo {
            id: ModelId("demo-model".into()),
            provider: ProviderId("demo-provider".into()),
            capabilities: vec![Capability::Chat, Capability::Tools],
        };
        let encoded = serde_json::to_string(&info).expect("serializes");
        let decoded: ModelInfo = serde_json::from_str(&encoded).expect("deserializes");
        assert_eq!(decoded, info);
    }
}
