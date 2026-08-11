use std::future::Future;
use std::pin::Pin;

use luminus_core::model::{AccountId, ProviderId};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfigRequest {
    pub account_id: AccountId,
    pub provider_id: ProviderId,
}

impl ProviderConfigRequest {
    pub fn new(account_id: impl Into<AccountId>, provider_id: impl Into<ProviderId>) -> Self {
        Self {
            account_id: account_id.into(),
            provider_id: provider_id.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProviderConfigError {
    #[error("provider configuration was not found")]
    NotFound,
    #[error("provider configuration is unavailable")]
    Unavailable,
    #[error("provider configuration is invalid")]
    InvalidConfiguration,
    #[error("provider configuration is unsupported")]
    Unsupported,
    #[error("provider configuration failed internally")]
    Internal,
}

pub type ProviderConfigResolverFuture<'a, C> =
    Pin<Box<dyn Future<Output = Result<C, ProviderConfigError>> + Send + 'a>>;

pub trait ProviderConfigResolver<C>: Send + Sync {
    fn resolve<'a>(
        &'a self,
        request: &'a ProviderConfigRequest,
    ) -> ProviderConfigResolverFuture<'a, C>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SyntheticConfig {
        base_url: String,
    }

    struct Resolver;
    impl ProviderConfigResolver<SyntheticConfig> for Resolver {
        fn resolve<'a>(
            &'a self,
            request: &'a ProviderConfigRequest,
        ) -> ProviderConfigResolverFuture<'a, SyntheticConfig> {
            Box::pin(async move {
                if request.provider_id != ProviderId::from("blackbox") {
                    return Err(ProviderConfigError::Unsupported);
                }
                if request.account_id != AccountId::from("known") {
                    return Err(ProviderConfigError::NotFound);
                }
                Ok(SyntheticConfig {
                    base_url: "http://127.0.0.1:1".into(),
                })
            })
        }
    }

    #[tokio::test]
    async fn typed_resolver_supports_trait_object_and_safe_errors() {
        let resolver: Arc<dyn ProviderConfigResolver<SyntheticConfig>> = Arc::new(Resolver);
        let config = resolver
            .resolve(&ProviderConfigRequest::new("known", "blackbox"))
            .await
            .unwrap();
        assert_eq!(config.base_url, "http://127.0.0.1:1");
        assert_eq!(
            resolver
                .resolve(&ProviderConfigRequest::new("missing", "blackbox"))
                .await,
            Err(ProviderConfigError::NotFound)
        );
        let error = resolver
            .resolve(&ProviderConfigRequest::new("known", "other"))
            .await
            .unwrap_err();
        assert!(!format!("{error}").contains("127.0.0.1"));
    }
}

pub use ProviderConfigError as ConfigError;

pub mod resolver {
    pub use super::{ProviderConfigRequest, ProviderConfigResolver, ProviderConfigResolverFuture};
}
