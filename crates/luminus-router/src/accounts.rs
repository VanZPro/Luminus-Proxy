use std::{collections::HashMap, sync::Arc};

use luminus_core::{
    model::{AccountDescriptor, AccountId, ModelId, ProviderId},
    provider::ProviderAdapter,
};

#[derive(Clone)]
pub struct ProviderAccount {
    pub descriptor: AccountDescriptor,
    pub adapter: Arc<dyn ProviderAdapter>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccountPoolError {
    #[error("duplicate account ID")]
    DuplicateAccount,
}

#[derive(Clone, Default)]
pub struct AccountPool {
    accounts: HashMap<AccountId, ProviderAccount>,
    order: Vec<AccountId>,
}

impl AccountPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, account: ProviderAccount) -> Result<(), AccountPoolError> {
        if self.accounts.contains_key(&account.descriptor.id) {
            return Err(AccountPoolError::DuplicateAccount);
        }
        self.order.push(account.descriptor.id.clone());
        self.accounts.insert(account.descriptor.id.clone(), account);
        Ok(())
    }

    pub fn get(&self, id: &AccountId) -> Option<&ProviderAccount> {
        self.accounts.get(id)
    }

    pub fn list_for_provider(&self, provider: &ProviderId) -> Vec<&ProviderAccount> {
        self.order
            .iter()
            .filter_map(|id| self.accounts.get(id))
            .filter(|account| account.descriptor.provider == *provider)
            .collect()
    }

    pub fn ordered_ids_for_provider(&self, provider: &ProviderId) -> Vec<AccountId> {
        self.list_for_provider(provider)
            .into_iter()
            .map(|account| account.descriptor.id.clone())
            .collect()
    }

    pub fn eligible_for_provider(
        &self,
        provider: &ProviderId,
        _model: &ModelId,
    ) -> Vec<&ProviderAccount> {
        self.list_for_provider(provider)
            .into_iter()
            .filter(|account| account.descriptor.enabled)
            .collect()
    }
}

impl std::fmt::Debug for ProviderAccount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderAccount")
            .field("descriptor", &self.descriptor)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_ids_are_rejected() {
        let mut pool = AccountPool::new();
        assert!(pool.register(test_account("a", true)).is_ok());
        assert_eq!(
            pool.register(test_account("a", true)),
            Err(AccountPoolError::DuplicateAccount)
        );
    }

    #[test]
    fn provider_listing_preserves_order_and_skips_disabled_accounts() {
        let mut pool = AccountPool::new();
        pool.register(test_account("disabled", false)).unwrap();
        pool.register(test_account("enabled", true)).unwrap();
        let provider = ProviderId::from("fake");
        assert_eq!(pool.list_for_provider(&provider).len(), 2);
        let eligible = pool.eligible_for_provider(&provider, &ModelId("m".into()));
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].descriptor.id, AccountId::from("enabled"));
    }

    fn test_account(id: &str, enabled: bool) -> ProviderAccount {
        let provider = ProviderId::from("fake");
        ProviderAccount {
            descriptor: AccountDescriptor {
                id: AccountId::from(id),
                provider: provider.clone(),
                enabled,
            },
            adapter: Arc::new(NoopProvider(provider)),
        }
    }

    struct NoopProvider(ProviderId);

    impl ProviderAdapter for NoopProvider {
        fn provider_id(&self) -> &ProviderId {
            &self.0
        }

        fn models(&self) -> Vec<luminus_core::model::ModelInfo> {
            Vec::new()
        }

        fn execute<'a>(
            &'a self,
            _request: &'a luminus_core::protocol::CanonicalRequest,
            _context: &'a luminus_core::provider::ProviderContext,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            luminus_core::protocol::CanonicalResponse,
                            luminus_core::provider::ProviderError,
                        >,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async {
                Result::<
                    luminus_core::protocol::CanonicalResponse,
                    luminus_core::provider::ProviderError,
                >::Err(luminus_core::provider::ProviderError::new(
                    luminus_core::provider::ProviderErrorCategory::ProviderFailure,
                    "noop",
                    false,
                ))
            })
        }
    }
}

// Keep the account pool API provider-neutral; model validation remains the adapter's responsibility.
