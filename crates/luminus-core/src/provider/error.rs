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

    pub fn fallback_allowed(&self) -> bool {
        !matches!(self.category, ProviderErrorCategory::InvalidRequest)
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

    #[test]
    fn fallbackability_is_independent_from_same_target_retryability() {
        let categories = [
            ProviderErrorCategory::Authentication,
            ProviderErrorCategory::RateLimit,
            ProviderErrorCategory::QuotaExceeded,
            ProviderErrorCategory::Timeout,
            ProviderErrorCategory::UpstreamUnavailable,
            ProviderErrorCategory::ProviderFailure,
            ProviderErrorCategory::UnsupportedCapability,
        ];
        for category in categories {
            let error = ProviderError::new(category, "target failure", false);
            assert!(error.fallback_allowed());
            assert!(!error.retryable);
        }
        assert!(
            !ProviderError::new(ProviderErrorCategory::InvalidRequest, "bad request", true,)
                .fallback_allowed()
        );
    }
}
