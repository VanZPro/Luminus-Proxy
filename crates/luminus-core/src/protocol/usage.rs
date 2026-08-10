use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    pub cached_input_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::Usage;

    #[test]
    fn total_usage_is_explicit() {
        let usage = Usage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            ..Usage::default()
        };
        assert_eq!(usage.total_tokens, usage.input_tokens + usage.output_tokens);
    }
}
