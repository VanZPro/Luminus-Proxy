use std::path::{Path, PathBuf};

use luminus_core::model::{AccountId, ProviderId};
use luminus_storage::{AccountRepository, AccountRepositoryFuture, StorageError, StoredAccount};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::error::{map_join_error, map_sqlite_error};

/// Read-only metadata projection of the current TypeScript `accounts` table.
/// Legacy numeric IDs are represented as `legacy-ts:<id>`.
pub struct LegacyTsAccountRepository {
    path: PathBuf,
}

impl LegacyTsAccountRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn open(path: &Path) -> Result<Connection, StorageError> {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(map_sqlite_error)
    }

    fn decode_id(id: &AccountId) -> Option<i64> {
        id.0.strip_prefix("legacy-ts:")?.parse::<i64>().ok()
    }

    fn encode_id(id: i64) -> Result<AccountId, rusqlite::Error> {
        if id < 0 {
            return Err(rusqlite::Error::IntegralValueOutOfRange(0, id));
        }
        Ok(AccountId::from(format!("legacy-ts:{id}")))
    }

    fn read_row(row: &rusqlite::Row<'_>) -> Result<StoredAccount, rusqlite::Error> {
        let id: i64 = row.get(0)?;
        let provider: String = row.get(1)?;
        if provider.is_empty() {
            return Err(rusqlite::Error::InvalidColumnType(
                1,
                "provider".into(),
                rusqlite::types::Type::Null,
            ));
        }
        let enabled: i64 = row.get(2)?;
        let enabled = match enabled {
            0 => false,
            1 => true,
            _ => return Err(rusqlite::Error::IntegralValueOutOfRange(2, enabled)),
        };
        Ok(StoredAccount {
            id: Self::encode_id(id)?,
            provider: ProviderId(provider),
            enabled,
        })
    }

    fn query_error(error: rusqlite::Error) -> StorageError {
        let text = error.to_string();
        if text.contains("no such table") || text.contains("no such column") {
            return StorageError::CorruptData;
        }
        match error {
            rusqlite::Error::InvalidColumnType(_, _, _)
            | rusqlite::Error::IntegralValueOutOfRange(_, _)
            | rusqlite::Error::QueryReturnedNoRows => StorageError::CorruptData,
            other => map_sqlite_error(other),
        }
    }

    fn list_sync(path: &Path) -> Result<Vec<StoredAccount>, StorageError> {
        let connection = Self::open(path)?;
        let mut statement = connection
            .prepare("SELECT id, provider, enabled FROM accounts ORDER BY id")
            .map_err(Self::query_error)?;
        statement
            .query_map([], Self::read_row)
            .map_err(Self::query_error)?
            .map(|row| row.map_err(Self::query_error))
            .collect()
    }

    fn get_sync(path: &Path, id: &AccountId) -> Result<Option<StoredAccount>, StorageError> {
        let Some(legacy_id) = Self::decode_id(id) else {
            return Ok(None);
        };
        let connection = Self::open(path)?;
        connection
            .query_row(
                "SELECT id, provider, enabled FROM accounts WHERE id = ?1",
                params![legacy_id],
                Self::read_row,
            )
            .optional()
            .map_err(Self::query_error)
    }
}

impl AccountRepository for LegacyTsAccountRepository {
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
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::{SystemTime, UNIX_EPOCH},
    };

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        path: PathBuf,
    }
    impl Fixture {
        fn new(schema: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "luminus-r16-{nonce}-{}-{sequence}.db",
                std::process::id()
            ));
            let db = Connection::open(&path).unwrap();
            db.execute_batch(schema).unwrap();
            drop(db);
            Self { path }
        }
        fn populate(&self) {
            let db = Connection::open(&self.path).unwrap();
            db.execute("INSERT INTO accounts (id, provider, email, password, status, enabled, tokens, metadata, extra) VALUES (3,'kiro','fake@example.invalid','synthetic-password','pending',0,'not-json','provider-sentinel','ignored')", []).unwrap();
            db.execute("INSERT INTO accounts (id, provider, email, password, status, enabled, tokens, metadata, extra) VALUES (1,'future-provider','fake2@example.invalid','synthetic-password-2','active',1,'still-not-json','sentinel-2','ignored-2')", []).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }
    fn schema() -> &'static str {
        "CREATE TABLE accounts (id INTEGER PRIMARY KEY AUTOINCREMENT, provider TEXT NOT NULL, email TEXT NOT NULL, password TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', enabled INTEGER NOT NULL DEFAULT 1, tokens TEXT, metadata TEXT, extra TEXT, UNIQUE(provider,email));"
    }

    #[tokio::test]
    async fn projects_current_shape_safely_and_deterministically() {
        let f = Fixture::new(schema());
        f.populate();
        let r = LegacyTsAccountRepository::new(&f.path);
        let rows = r.list_accounts().await.unwrap();
        assert_eq!(
            rows.iter().map(|x| x.id.0.as_str()).collect::<Vec<_>>(),
            ["legacy-ts:1", "legacy-ts:3"]
        );
        assert!(rows[0].enabled);
        assert!(!rows[1].enabled);
        assert_eq!(rows[0].provider.0, "future-provider");
        assert_eq!(
            r.get_account(&rows[1].id).await.unwrap().unwrap().id,
            "legacy-ts:3".into()
        );
    }

    #[tokio::test]
    async fn missing_and_foreign_ids_are_safe() {
        let f = Fixture::new(schema());
        let r = LegacyTsAccountRepository::new(&f.path);
        assert_eq!(
            r.get_account(&AccountId::from("legacy-ts:99"))
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            r.get_account(&AccountId::from("native:1")).await.unwrap(),
            None
        );
        let arc: Arc<dyn AccountRepository> = Arc::new(r);
        assert!(arc.list_accounts().await.is_ok());
    }

    #[tokio::test]
    async fn schema_errors_and_malformed_values_fail_without_mutation() {
        let missing = Fixture::new("CREATE TABLE other (id INTEGER);");
        assert_eq!(
            LegacyTsAccountRepository::new(&missing.path)
                .list_accounts()
                .await,
            Err(StorageError::CorruptData)
        );
        let no_enabled =
            Fixture::new("CREATE TABLE accounts (id INTEGER PRIMARY KEY, provider TEXT NOT NULL);");
        assert_eq!(
            LegacyTsAccountRepository::new(&no_enabled.path)
                .list_accounts()
                .await,
            Err(StorageError::CorruptData)
        );
        let bad = Fixture::new(schema());
        let db = Connection::open(&bad.path).unwrap();
        db.execute("INSERT INTO accounts (id,provider,email,password,status,enabled) VALUES (1,'p','e','x','x',2)", []).unwrap();
        drop(db);
        assert_eq!(
            LegacyTsAccountRepository::new(&bad.path)
                .list_accounts()
                .await,
            Err(StorageError::CorruptData)
        );
    }

    #[tokio::test]
    async fn databases_are_isolated() {
        let a = Fixture::new(schema());
        a.populate();
        let b = Fixture::new(schema());
        assert_eq!(
            LegacyTsAccountRepository::new(&b.path)
                .list_accounts()
                .await
                .unwrap()
                .len(),
            0
        );
    }
}
