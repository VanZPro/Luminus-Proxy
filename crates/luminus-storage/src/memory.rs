use std::collections::HashSet;

use luminus_core::model::AccountId;

use crate::{AccountRepository, AccountRepositoryFuture, StorageError, StoredAccount};

pub struct MemoryAccountRepository {
    records: Vec<StoredAccount>,
}

impl MemoryAccountRepository {
    pub fn new(records: Vec<StoredAccount>) -> Result<Self, StorageError> {
        let mut ids = HashSet::with_capacity(records.len());
        if records.iter().any(|record| !ids.insert(record.id.clone())) {
            return Err(StorageError::InvalidRecord);
        }
        Ok(Self { records })
    }

    pub fn list_accounts_sync(&self) -> Result<Vec<StoredAccount>, StorageError> {
        Ok(self.records.clone())
    }
    pub fn get_account_sync(&self, id: &AccountId) -> Result<Option<StoredAccount>, StorageError> {
        Ok(self.records.iter().find(|record| &record.id == id).cloned())
    }
}

impl AccountRepository for MemoryAccountRepository {
    fn list_accounts(&self) -> AccountRepositoryFuture<'_, Vec<StoredAccount>> {
        Box::pin(async move { self.list_accounts_sync() })
    }
    fn get_account(&self, id: &AccountId) -> AccountRepositoryFuture<'_, Option<StoredAccount>> {
        let id = id.clone();
        Box::pin(async move { self.get_account_sync(&id) })
    }
}
