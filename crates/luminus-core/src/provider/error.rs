use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorCategory {
    Authentication,
    RateLimit,
    QuotaExceeded,
    UpstreamUnavailable,
    Timeout,
    InvalidRequest,
    UnsupportedCapability,
    ProviderFailure,
}

#[derive(Debug, Error)]
#[error("{category:?}: {message}")]
pub struct ProviderError {
    pub category: ProviderErrorCategory,
    pub message: String,
    pub retryable: bool,
    pub cooldown_seconds: Option<u64>,
}

impl ProviderError {
    pub fn new(
        category: ProviderErrorCategory,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            category,
            message: message.into(),
            retryable,
            cooldown_seconds: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_categories_expose_retryability() {
        let error = ProviderError {
            category: ProviderErrorCategory::RateLimit,
            message: "slow down".into(),
            retryable: true,
            cooldown_seconds: Some(10),
        };
        assert!(error.retryable);
        assert_eq!(error.category, ProviderErrorCategory::RateLimit);
    }
}
