use std::sync::Arc;

use luminus_composition::{BlackboxAccountHydrator, HydrationReport};
use luminus_core::model::ProviderId;
use luminus_legacy_composition::{LegacyByokBlackboxHydrator, LegacyHydrationReport};
use luminus_router::{AccountPool, ProviderRegistry, Router};
use luminus_storage::StorageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlackboxSourceOrder {
    NativeThenLegacy,
    LegacyThenNative,
}

#[derive(Debug)]
pub struct RuntimeBootstrapReport {
    pub source_order: BlackboxSourceOrder,
    pub native_blackbox: HydrationReport,
    pub legacy_blackbox: LegacyHydrationReport,
}

pub struct RuntimeSnapshot {
    pub account_pool: Arc<AccountPool>,
    pub router: Router,
    pub report: RuntimeBootstrapReport,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeBootstrapError {
    #[error("configured account source unavailable")]
    SourceUnavailable(#[source] StorageError),
    #[error("duplicate runtime account identity")]
    DuplicateAccount,
}

impl From<StorageError> for RuntimeBootstrapError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::DuplicateAccount => Self::DuplicateAccount,
            other => Self::SourceUnavailable(other),
        }
    }
}

pub struct BlackboxRuntimeBootstrap {
    native: BlackboxAccountHydrator,
    legacy: LegacyByokBlackboxHydrator,
    source_order: BlackboxSourceOrder,
    registry: Arc<ProviderRegistry>,
}

impl BlackboxRuntimeBootstrap {
    pub fn native_only(
        native: BlackboxAccountHydrator,
        registry: Arc<ProviderRegistry>,
    ) -> NativeOnlyRuntimeBootstrap {
        NativeOnlyRuntimeBootstrap { native, registry }
    }

    pub fn new(
        native: BlackboxAccountHydrator,
        legacy: LegacyByokBlackboxHydrator,
        source_order: BlackboxSourceOrder,
        registry: Arc<ProviderRegistry>,
    ) -> Self {
        Self {
            native,
            legacy,
            source_order,
            registry,
        }
    }

    pub async fn build(self) -> Result<RuntimeSnapshot, RuntimeBootstrapError> {
        let mut pool = AccountPool::new();
        let (native_blackbox, legacy_blackbox) = match self.source_order {
            BlackboxSourceOrder::NativeThenLegacy => {
                let native = self
                    .native
                    .hydrate_into(&mut pool)
                    .await
                    .map_err(RuntimeBootstrapError::from)?;
                let legacy = self
                    .legacy
                    .hydrate_into(&mut pool)
                    .await
                    .map_err(RuntimeBootstrapError::from)?;
                (native, legacy)
            }
            BlackboxSourceOrder::LegacyThenNative => {
                let legacy = self
                    .legacy
                    .hydrate_into(&mut pool)
                    .await
                    .map_err(RuntimeBootstrapError::from)?;
                let native = self
                    .native
                    .hydrate_into(&mut pool)
                    .await
                    .map_err(RuntimeBootstrapError::from)?;
                (native, legacy)
            }
        };
        let account_pool = Arc::new(pool);
        let router = Router::new(self.registry, Some(ProviderId::from("blackbox")))
            .with_accounts(account_pool.clone());
        Ok(RuntimeSnapshot {
            account_pool,
            router,
            report: RuntimeBootstrapReport {
                source_order: self.source_order,
                native_blackbox,
                legacy_blackbox,
            },
        })
    }
}

pub struct NativeOnlyRuntimeBootstrap {
    native: BlackboxAccountHydrator,
    registry: Arc<ProviderRegistry>,
}

impl NativeOnlyRuntimeBootstrap {
    pub async fn build(self) -> Result<RuntimeSnapshot, RuntimeBootstrapError> {
        let mut pool = AccountPool::new();
        let native_blackbox = self
            .native
            .hydrate_into(&mut pool)
            .await
            .map_err(RuntimeBootstrapError::from)?;
        let account_pool = Arc::new(pool);
        let router = Router::new(self.registry, Some(ProviderId::from("blackbox")))
            .with_accounts(account_pool.clone());
        Ok(RuntimeSnapshot {
            account_pool,
            router,
            report: RuntimeBootstrapReport {
                source_order: BlackboxSourceOrder::NativeThenLegacy,
                native_blackbox,
                legacy_blackbox: LegacyHydrationReport::default(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_order_is_explicit() {
        assert_ne!(
            BlackboxSourceOrder::NativeThenLegacy,
            BlackboxSourceOrder::LegacyThenNative
        );
    }
}
