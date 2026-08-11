use std::{future::Future, pin::Pin};

use luminus_core::model::AccountId;

use crate::{StorageError, StoredAccount};

pub type AccountRepositoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'a>>;

pub trait AccountRepository: Send + Sync {
    fn list_accounts(&self) -> AccountRepositoryFuture<'_, Vec<StoredAccount>>;
    fn get_account(&self, id: &AccountId) -> AccountRepositoryFuture<'_, Option<StoredAccount>>;
}
