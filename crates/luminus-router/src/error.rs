use luminus_core::provider::ProviderError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RouterError {
    #[error("provider not found")]
    ProviderNotFound,
    #[error("model not found")]
    ModelNotFound,
    #[error("required capability is unsupported")]
    UnsupportedCapability,
    #[error("no eligible provider")]
    NoEligibleProvider,
    #[error("provider execution failed: {0}")]
    ProviderExecution(#[from] ProviderError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouterErrorCategory {
    ProviderNotFound,
    ModelNotFound,
    UnsupportedCapability,
    NoEligibleProvider,
    ProviderExecution,
}
impl RouterError {
    pub fn category(&self) -> RouterErrorCategory {
        match self {
            Self::ProviderNotFound => RouterErrorCategory::ProviderNotFound,
            Self::ModelNotFound => RouterErrorCategory::ModelNotFound,
            Self::UnsupportedCapability => RouterErrorCategory::UnsupportedCapability,
            Self::NoEligibleProvider => RouterErrorCategory::NoEligibleProvider,
            Self::ProviderExecution(_) => RouterErrorCategory::ProviderExecution,
        }
    }
}
