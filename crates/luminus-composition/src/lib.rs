use std::fmt;
use std::sync::Arc;

use luminus_core::model::ProviderId;
use luminus_legacy_credentials::{LegacyCredentialError, LegacyPasswordReader};
use luminus_secrets::{
    CredentialRequest, CredentialResolver, CredentialResolverFuture, SecretError, SecretString,
};

pub const BYOK_PROVIDER: &str = "byok";

pub struct ByokCredentials {
    pub api_key: SecretString,
}

impl fmt::Debug for ByokCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ByokCredentials")
            .field("api_key", &self.api_key)
            .finish()
    }
}

pub struct LegacyByokResolver {
    reader: Arc<LegacyPasswordReader>,
    key: SecretString,
}

impl LegacyByokResolver {
    pub fn new(reader: Arc<LegacyPasswordReader>, key: SecretString) -> Self {
        Self { reader, key }
    }

    fn map_error(error: LegacyCredentialError) -> SecretError {
        match error {
            LegacyCredentialError::Unavailable => SecretError::Unavailable,
            LegacyCredentialError::InvalidKey
            | LegacyCredentialError::InvalidCiphertext
            | LegacyCredentialError::InvalidMaterial
            | LegacyCredentialError::CorruptSchema => SecretError::InvalidMaterial,
            LegacyCredentialError::Internal => SecretError::Internal,
        }
    }
}

impl CredentialResolver<ByokCredentials> for LegacyByokResolver {
    fn resolve<'a>(
        &'a self,
        request: &'a CredentialRequest,
    ) -> CredentialResolverFuture<'a, ByokCredentials> {
        Box::pin(async move {
            if request.provider_id != ProviderId::from(BYOK_PROVIDER) {
                return Err(SecretError::InvalidMaterial);
            }

            let row = self
                .reader
                .get(&request.account_id)
                .await
                .map_err(Self::map_error)?
                .ok_or(SecretError::NotFound)?;

            if row.provider_id != ProviderId::from(BYOK_PROVIDER) {
                return Err(SecretError::InvalidMaterial);
            }

            let api_key = self
                .reader
                .decode(&row.ciphertext, &self.key)
                .map_err(Self::map_error)?;
            Ok(ByokCredentials { api_key })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use rusqlite::Connection;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn encode(value: &str, key: &str) -> String {
        let key = key.as_bytes();
        let bytes: Vec<u8> = value
            .as_bytes()
            .iter()
            .enumerate()
            .map(|(i, byte)| byte ^ key[i % key.len()])
            .collect();
        STANDARD.encode(bytes)
    }

    fn fixture(rows: &[(i64, &str, &str)]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "luminus-r19-{}-{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE accounts (id INTEGER PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL, password TEXT NOT NULL, tokens TEXT, metadata TEXT, UNIQUE(provider,email));").unwrap();
        for (id, provider, value) in rows {
            db.execute("INSERT INTO accounts (id,provider,email,password,tokens,metadata) VALUES (?1,?2,?3,?4,?5,?6)", rusqlite::params![id, provider, format!("synthetic-{id}@example.invalid"), encode(value, "r19-key"), "ignored-token", "ignored-metadata"]).unwrap();
        }
        drop(db);
        path
    }

    fn resolver(path: &PathBuf) -> LegacyByokResolver {
        LegacyByokResolver::new(
            Arc::new(LegacyPasswordReader::new(path)),
            SecretString::new("r19-key"),
        )
    }

    #[tokio::test]
    async fn resolves_typed_credentials_and_trait_object() {
        let path = fixture(&[(1, BYOK_PROVIDER, "synthetic-api-key-a")]);
        let resolver = resolver(&path);
        let object: Arc<dyn CredentialResolver<ByokCredentials>> = Arc::new(resolver);
        let credentials = object
            .resolve(&CredentialRequest::new("legacy-ts:1", BYOK_PROVIDER))
            .await
            .unwrap();
        assert_eq!(credentials.api_key.expose_secret(), "synthetic-api-key-a");
        assert!(!format!("{credentials:?}").contains("synthetic-api-key-a"));
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn validates_provider_and_account_boundaries() {
        let path = fixture(&[(1, BYOK_PROVIDER, "key-a"), (2, "other", "key-b")]);
        let resolver = resolver(&path);
        assert!(matches!(
            resolver
                .resolve(&CredentialRequest::new("legacy-ts:99", BYOK_PROVIDER))
                .await,
            Err(SecretError::NotFound)
        ));
        assert!(matches!(
            resolver
                .resolve(&CredentialRequest::new("legacy-ts:1", "other"))
                .await,
            Err(SecretError::InvalidMaterial)
        ));
        assert!(matches!(
            resolver
                .resolve(&CredentialRequest::new("legacy-ts:2", BYOK_PROVIDER))
                .await,
            Err(SecretError::InvalidMaterial)
        ));
        assert!(matches!(
            resolver
                .resolve(&CredentialRequest::new("native:1", BYOK_PROVIDER))
                .await,
            Err(SecretError::NotFound)
        ));
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn maps_malformed_ciphertext_and_empty_key_safely() {
        let path = fixture(&[(1, BYOK_PROVIDER, "key-a")]);
        let db = Connection::open(&path).unwrap();
        db.execute("UPDATE accounts SET password = ?1 WHERE id = 1", ["!"])
            .unwrap();
        drop(db);
        assert!(matches!(
            resolver(&path)
                .resolve(&CredentialRequest::new("legacy-ts:1", BYOK_PROVIDER))
                .await,
            Err(SecretError::InvalidMaterial)
        ));
        let empty = LegacyByokResolver::new(
            Arc::new(LegacyPasswordReader::new(&path)),
            SecretString::new(""),
        );
        assert!(matches!(
            empty
                .resolve(&CredentialRequest::new("legacy-ts:1", BYOK_PROVIDER))
                .await,
            Err(SecretError::InvalidMaterial)
        ));
        fs::remove_file(path).unwrap();
    }
}

// This composition is offline and read-only. It does not hydrate ProviderAccount or AccountPool.
