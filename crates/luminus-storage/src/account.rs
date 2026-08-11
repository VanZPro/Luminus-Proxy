use luminus_core::model::{AccountId, ProviderId};

/// Generic persisted account metadata. Credentials and runtime state are intentionally absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAccount {
    pub id: AccountId,
    pub provider: ProviderId,
    pub enabled: bool,
}

impl StoredAccount {
    pub fn new(id: impl Into<AccountId>, provider: impl Into<ProviderId>, enabled: bool) -> Self {
        Self {
            id: id.into(),
            provider: provider.into(),
            enabled,
        }
    }
}

impl From<StoredAccount> for luminus_core::model::AccountDescriptor {
    fn from(account: StoredAccount) -> Self {
        Self {
            id: account.id,
            provider: account.provider,
            enabled: account.enabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_preserves_metadata_without_runtime_state() {
        let stored = StoredAccount::new("a", "p", false);
        let descriptor: luminus_core::model::AccountDescriptor = stored.clone().into();
        assert_eq!(descriptor.id, stored.id);
        assert_eq!(descriptor.provider, stored.provider);
        assert!(!descriptor.enabled);
    }
}
