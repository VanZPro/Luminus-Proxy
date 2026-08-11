use std::path::{Path, PathBuf};

use luminus_core::model::AccountId;
use luminus_storage::{AccountRepository, AccountRepositoryFuture, StorageError, StoredAccount};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{map_join_error, map_sqlite_error};

pub struct SqliteAccountRepository {
    path: PathBuf,
}

impl SqliteAccountRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn open(path: &Path) -> Result<Connection, StorageError> {
        Connection::open(path).map_err(map_sqlite_error)
    }

    fn read_row(row: &rusqlite::Row<'_>) -> Result<StoredAccount, rusqlite::Error> {
        let id: String = row.get(0)?;
        let provider: String = row.get(1)?;
        let enabled: i64 = row.get(2)?;
        let enabled = match enabled {
            0 => false,
            1 => true,
            _ => return Err(rusqlite::Error::IntegralValueOutOfRange(2, enabled)),
        };
        Ok(StoredAccount {
            id: AccountId::from(id),
            provider: luminus_core::model::ProviderId(provider),
            enabled,
        })
    }

    fn list_sync(path: &Path) -> Result<Vec<StoredAccount>, StorageError> {
        let connection = Self::open(path)?;
        let mut statement = connection
            .prepare("SELECT id, provider, enabled FROM luminus_accounts ORDER BY id")
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map([], Self::read_row)
            .map_err(map_sqlite_error)?;
        rows.map(|row| row.map_err(map_sqlite_error)).collect()
    }

    fn get_sync(path: &Path, id: &AccountId) -> Result<Option<StoredAccount>, StorageError> {
        let connection = Self::open(path)?;
        connection
            .query_row(
                "SELECT id, provider, enabled FROM luminus_accounts WHERE id = ?1",
                params![id.0.as_str()],
                Self::read_row,
            )
            .optional()
            .map_err(map_sqlite_error)
    }
}

impl AccountRepository for SqliteAccountRepository {
    fn list_accounts(&self) -> AccountRepositoryFuture<'_, Vec<StoredAccount>> {
        let path = self.path.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || Self::list_sync(&path))
                .await
                .map_err(map_join_error)?
        })
    }

    fn get_account(&self, id: &AccountId) -> AccountRepositoryFuture<'_, Option<StoredAccount>> {
        let path = self.path.clone();
        let id = id.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || Self::get_sync(&path, &id))
                .await
                .map_err(map_join_error)?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use luminus_storage::AccountRepository;
    use std::{
        fs,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TempDb {
        path: PathBuf,
    }
    impl TempDb {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("luminus-r15-{nonce}-{}.db", std::process::id()));
            let connection = Connection::open(&path).unwrap();
            connection.execute("CREATE TABLE luminus_accounts (id TEXT PRIMARY KEY NOT NULL, provider TEXT NOT NULL, enabled INTEGER NOT NULL)", []).unwrap();
            drop(connection);
            Self { path }
        }
        fn insert(&self, id: &str, provider: &str, enabled: i64) {
            Connection::open(&self.path)
                .unwrap()
                .execute(
                    "INSERT INTO luminus_accounts (id, provider, enabled) VALUES (?1, ?2, ?3)",
                    params![id, provider, enabled],
                )
                .unwrap();
        }
    }
    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    async fn list(repo: &dyn AccountRepository) -> Result<Vec<StoredAccount>, StorageError> {
        repo.list_accounts().await
    }
    async fn get(
        repo: &dyn AccountRepository,
        id: &str,
    ) -> Result<Option<StoredAccount>, StorageError> {
        repo.get_account(&AccountId::from(id)).await
    }

    #[tokio::test]
    async fn reads_empty_and_multiple_accounts_deterministically() {
        let db = TempDb::new();
        assert!(
            list(&SqliteAccountRepository::new(&db.path))
                .await
                .unwrap()
                .is_empty()
        );
        db.insert("z", "blackbox", 1);
        db.insert("a", "kiro", 0);
        let rows = list(&SqliteAccountRepository::new(&db.path)).await.unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id.0.as_str()).collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert!(!rows[0].enabled);
    }

    #[tokio::test]
    async fn gets_existing_and_missing_accounts() {
        let db = TempDb::new();
        db.insert("a", "p", 1);
        let repo = SqliteAccountRepository::new(&db.path);
        assert_eq!(get(&repo, "a").await.unwrap().unwrap().provider.0, "p");
        assert_eq!(get(&repo, "missing").await.unwrap(), None);
    }

    #[tokio::test]
    async fn invalid_enabled_value_is_corrupt() {
        let db = TempDb::new();
        db.insert("a", "p", 2);
        assert_eq!(
            list(&SqliteAccountRepository::new(&db.path)).await,
            Err(StorageError::CorruptData)
        );
    }

    #[tokio::test]
    async fn malformed_required_field_is_corrupt() {
        let db = TempDb::new();
        Connection::open(&db.path)
            .unwrap()
            .execute(
                "INSERT INTO luminus_accounts (id, provider, enabled) VALUES (NULL, 'p', 1)",
                [],
            )
            .unwrap_err();
        assert_eq!(
            get(&SqliteAccountRepository::new(&db.path), "missing")
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn works_behind_arc_and_databases_are_isolated() {
        let first = TempDb::new();
        first.insert("a", "p", 1);
        let second = TempDb::new();
        let repo: Arc<dyn AccountRepository> = Arc::new(SqliteAccountRepository::new(&first.path));
        assert_eq!(repo.list_accounts().await.unwrap().len(), 1);
        assert!(
            list(&SqliteAccountRepository::new(&second.path))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
