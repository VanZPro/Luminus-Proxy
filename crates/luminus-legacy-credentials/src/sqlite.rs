use std::path::{Path, PathBuf};

use luminus_core::model::{AccountId, ProviderId};
use luminus_secrets::SecretString;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use tokio::task::spawn_blocking;

use crate::{LegacyCiphertext, LegacyCredentialError, decode_id, encode_id};

#[derive(Debug)]
pub struct LegacyEncryptedPassword {
    pub account_id: AccountId,
    pub provider_id: ProviderId,
    pub ciphertext: LegacyCiphertext,
}

pub struct LegacyPasswordReader {
    path: PathBuf,
}

impl LegacyPasswordReader {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub async fn list(&self) -> Result<Vec<LegacyEncryptedPassword>, LegacyCredentialError> {
        let path = self.path.clone();
        spawn_blocking(move || Self::list_sync(&path))
            .await
            .map_err(|_| LegacyCredentialError::Internal)?
    }

    pub async fn get(
        &self,
        id: &AccountId,
    ) -> Result<Option<LegacyEncryptedPassword>, LegacyCredentialError> {
        let Some(legacy_id) = decode_id(id) else {
            return Ok(None);
        };
        let path = self.path.clone();
        spawn_blocking(move || Self::get_sync(&path, legacy_id))
            .await
            .map_err(|_| LegacyCredentialError::Internal)?
    }

    fn open(path: &Path) -> Result<Connection, LegacyCredentialError> {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| LegacyCredentialError::Unavailable)
    }

    fn convert(
        id: i64,
        provider: String,
        password: String,
    ) -> Result<LegacyEncryptedPassword, rusqlite::Error> {
        let account_id = encode_id(id).ok_or(rusqlite::Error::InvalidQuery)?;
        Ok(LegacyEncryptedPassword {
            account_id,
            provider_id: ProviderId(provider),
            ciphertext: LegacyCiphertext::new(password),
        })
    }

    fn list_sync(path: &Path) -> Result<Vec<LegacyEncryptedPassword>, LegacyCredentialError> {
        let conn = Self::open(path)?;
        let mut stmt = conn
            .prepare("SELECT id, provider, password FROM accounts ORDER BY id")
            .map_err(|_| LegacyCredentialError::CorruptSchema)?;
        let rows = stmt
            .query_map([], |row| {
                Self::convert(row.get(0)?, row.get(1)?, row.get(2)?)
            })
            .map_err(|_| LegacyCredentialError::CorruptSchema)?;
        rows.map(|row| row.map_err(|_| LegacyCredentialError::InvalidMaterial))
            .collect()
    }

    fn get_sync(
        path: &Path,
        id: i64,
    ) -> Result<Option<LegacyEncryptedPassword>, LegacyCredentialError> {
        let conn = Self::open(path)?;
        conn.query_row(
            "SELECT id, provider, password FROM accounts WHERE id = ?1",
            params![id],
            |row| Self::convert(row.get(0)?, row.get(1)?, row.get(2)?),
        )
        .optional()
        .map_err(|error| match error {
            rusqlite::Error::InvalidColumnType(_, _, _) => LegacyCredentialError::InvalidMaterial,
            _ => LegacyCredentialError::CorruptSchema,
        })
    }

    pub fn decode(
        &self,
        ciphertext: &LegacyCiphertext,
        key: &SecretString,
    ) -> Result<SecretString, LegacyCredentialError> {
        crate::decode(ciphertext, key)
    }
}
