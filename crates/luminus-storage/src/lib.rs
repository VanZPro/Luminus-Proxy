mod account;
mod error;
mod memory;
mod repository;

pub use account::StoredAccount;
pub use error::StorageError;
pub use memory::MemoryAccountRepository;
pub use repository::{AccountRepository, AccountRepositoryFuture};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use luminus_core::model::{AccountId, ProviderId};

    use super::{AccountRepository, MemoryAccountRepository, StoredAccount};

    fn records() -> Vec<StoredAccount> {
        vec![
            StoredAccount::new("a-2", "blackbox", false),
            StoredAccount::new("a-1", "kiro", true),
            StoredAccount::new("a-3", "blackbox", true),
        ]
    }

    #[test]
    fn lists_records_in_deterministic_input_order() {
        let repository = MemoryAccountRepository::new(records()).unwrap();
        let listed = repository.list_accounts_sync().unwrap();
        assert_eq!(listed, records());
    }

    #[test]
    fn lookup_and_missing_account_are_explicit() {
        let repository = MemoryAccountRepository::new(records()).unwrap();
        assert_eq!(
            repository
                .get_account_sync(&AccountId::from("a-1"))
                .unwrap(),
            Some(records()[1].clone())
        );
        assert_eq!(
            repository
                .get_account_sync(&AccountId::from("missing"))
                .unwrap(),
            None
        );
    }

    #[test]
    fn preserves_provider_and_disabled_metadata() {
        let repository = MemoryAccountRepository::new(records()).unwrap();
        let listed = repository.list_accounts_sync().unwrap();
        assert_eq!(listed[0].provider, ProviderId::from("blackbox"));
        assert!(!listed[0].enabled);
        assert_eq!(listed[1].provider, ProviderId::from("kiro"));
    }

    #[test]
    fn duplicate_ids_are_rejected_deterministically() {
        let duplicate = vec![
            StoredAccount::new("same", "p1", true),
            StoredAccount::new("same", "p2", true),
        ];
        assert!(MemoryAccountRepository::new(duplicate).is_err());
    }

    #[test]
    fn repository_is_object_safe_behind_arc() {
        let repository: Arc<dyn AccountRepository> =
            Arc::new(MemoryAccountRepository::new(records()).unwrap());
        let listed = futures_block_on(repository.list_accounts()).unwrap();
        assert_eq!(listed.len(), 3);
    }

    fn futures_block_on<F: std::future::Future>(future: F) -> F::Output {
        // The memory implementation completes immediately; use the standard test runtime-free poll path.
        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);
        let mut future = std::pin::pin!(future);
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => value,
            std::task::Poll::Pending => panic!("memory repository unexpectedly yielded"),
        }
    }
}
