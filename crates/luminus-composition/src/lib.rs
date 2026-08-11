use std::fmt;
use std::sync::Arc;

use luminus_core::model::{AccountId, ProviderId};
use luminus_legacy_credentials::{LegacyCredentialError, LegacyPasswordReader};
use luminus_provider_config::{ProviderConfigError, ProviderConfigRequest, ProviderConfigResolver};
use luminus_providers::providers::blackbox::{BlackboxConfig, BlackboxProvider};
use luminus_router::{AccountPool, ProviderAccount};
use luminus_secrets::{
    CredentialRequest, CredentialResolver, CredentialResolverFuture, SecretError, SecretString,
};
use luminus_storage::{AccountRepository, StorageError};

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

pub struct BlackboxCredentials {
    pub api_key: SecretString,
}

impl fmt::Debug for BlackboxCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlackboxCredentials")
            .field("api_key", &self.api_key)
            .finish()
    }
}

pub struct BlackboxProviderConfig {
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HydrationFailure {
    ConfigurationNotFound,
    ConfigurationUnavailable,
    ConfigurationInvalid,
    ConfigurationUnsupported,
    ConfigurationInternal,
    CredentialNotFound,
    CredentialUnavailable,
    CredentialInvalid,
    CredentialInternal,
    ProviderConstruction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HydrationReportEntry {
    pub account_id: AccountId,
    pub provider_id: ProviderId,
    pub failure: HydrationFailure,
}

#[derive(Debug, Default)]
pub struct HydrationReport {
    pub failures: Vec<HydrationReportEntry>,
}

pub struct HydrationOutcome {
    pub account_pool: AccountPool,
    pub report: HydrationReport,
}

pub struct BlackboxAccountHydrator {
    repository: Arc<dyn AccountRepository>,
    resolver: Arc<dyn CredentialResolver<BlackboxCredentials>>,
    config_resolver: Arc<dyn ProviderConfigResolver<BlackboxProviderConfig>>,
}

impl BlackboxAccountHydrator {
    pub fn new(
        repository: Arc<dyn AccountRepository>,
        resolver: Arc<dyn CredentialResolver<BlackboxCredentials>>,
        config_resolver: Arc<dyn ProviderConfigResolver<BlackboxProviderConfig>>,
    ) -> Self {
        Self {
            repository,
            resolver,
            config_resolver,
        }
    }

    pub async fn hydrate(&self) -> Result<HydrationOutcome, StorageError> {
        let records = self.repository.list_accounts().await?;
        let blackbox = ProviderId::from("blackbox");
        let mut pool = AccountPool::new();
        let mut report = HydrationReport::default();
        for record in records {
            if record.provider != blackbox || !record.enabled {
                continue;
            }
            let request = CredentialRequest::new(record.id.clone(), blackbox.clone());
            let config = match self
                .config_resolver
                .resolve(&ProviderConfigRequest::new(
                    record.id.clone(),
                    blackbox.clone(),
                ))
                .await
            {
                Ok(value) => value,
                Err(error) => {
                    let failure = match error {
                        ProviderConfigError::NotFound => HydrationFailure::ConfigurationNotFound,
                        ProviderConfigError::Unavailable => {
                            HydrationFailure::ConfigurationUnavailable
                        }
                        ProviderConfigError::InvalidConfiguration => {
                            HydrationFailure::ConfigurationInvalid
                        }
                        ProviderConfigError::Unsupported => {
                            HydrationFailure::ConfigurationUnsupported
                        }
                        ProviderConfigError::Internal => HydrationFailure::ConfigurationInternal,
                    };
                    report.failures.push(HydrationReportEntry {
                        account_id: record.id,
                        provider_id: blackbox.clone(),
                        failure,
                    });
                    continue;
                }
            };
            let credentials = match self.resolver.resolve(&request).await {
                Ok(value) => value,
                Err(error) => {
                    let failure = match error {
                        SecretError::NotFound => HydrationFailure::CredentialNotFound,
                        SecretError::Unavailable => HydrationFailure::CredentialUnavailable,
                        SecretError::InvalidMaterial | SecretError::DecryptionFailed => {
                            HydrationFailure::CredentialInvalid
                        }
                        SecretError::Internal => HydrationFailure::CredentialInternal,
                    };
                    report.failures.push(HydrationReportEntry {
                        account_id: record.id,
                        provider_id: blackbox.clone(),
                        failure,
                    });
                    continue;
                }
            };
            let provider = match BlackboxProvider::new(BlackboxConfig::new(
                config.base_url,
                credentials.api_key.expose_secret(),
            )) {
                Ok(provider) => provider,
                Err(_) => {
                    report.failures.push(HydrationReportEntry {
                        account_id: record.id,
                        provider_id: blackbox.clone(),
                        failure: HydrationFailure::ProviderConstruction,
                    });
                    continue;
                }
            };
            pool.register(ProviderAccount {
                descriptor: record.into(),
                adapter: Arc::new(provider),
            })
            .map_err(|_| StorageError::InvalidRecord)?;
        }
        Ok(HydrationOutcome {
            account_pool: pool,
            report,
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

#[cfg(test)]
mod hydration_tests {
    use super::*;
    use luminus_core::model::AccountId;
    use luminus_storage::{MemoryAccountRepository, StoredAccount};
    use std::sync::Mutex;

    struct SyntheticResolver {
        values: std::collections::HashMap<AccountId, Result<String, SecretError>>,
        calls: Arc<Mutex<Vec<AccountId>>>,
    }
    impl CredentialResolver<BlackboxCredentials> for SyntheticResolver {
        fn resolve<'a>(
            &'a self,
            request: &'a CredentialRequest,
        ) -> CredentialResolverFuture<'a, BlackboxCredentials> {
            self.calls.lock().unwrap().push(request.account_id.clone());
            let result = match self.values.get(&request.account_id) {
                Some(Ok(api_key)) => Ok(api_key.clone()),
                Some(Err(SecretError::NotFound)) | None => Err(SecretError::NotFound),
                Some(Err(SecretError::Unavailable)) => Err(SecretError::Unavailable),
                Some(Err(SecretError::InvalidMaterial)) => Err(SecretError::InvalidMaterial),
                Some(Err(SecretError::DecryptionFailed)) => Err(SecretError::DecryptionFailed),
                Some(Err(SecretError::Internal)) => Err(SecretError::Internal),
            };
            Box::pin(async move {
                result.map(|api_key| BlackboxCredentials {
                    api_key: SecretString::new(api_key),
                })
            })
        }
    }

    struct SyntheticConfigResolver;

    impl ProviderConfigResolver<BlackboxProviderConfig> for SyntheticConfigResolver {
        fn resolve<'a>(
            &'a self,
            request: &'a ProviderConfigRequest,
        ) -> luminus_provider_config::ProviderConfigResolverFuture<'a, BlackboxProviderConfig>
        {
            Box::pin(async move {
                if request.provider_id != ProviderId::from("blackbox") {
                    return Err(ProviderConfigError::Unsupported);
                }
                if request.account_id == AccountId::from("bad-config") {
                    return Err(ProviderConfigError::NotFound);
                }
                Ok(BlackboxProviderConfig {
                    base_url: "http://127.0.0.1:1".into(),
                })
            })
        }
    }

    fn make_hydrator(
        records: Vec<StoredAccount>,
        values: &[(&str, Result<&str, SecretError>)],
    ) -> (BlackboxAccountHydrator, Arc<Mutex<Vec<AccountId>>>) {
        let repository = Arc::new(MemoryAccountRepository::new(records).unwrap());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let map = values
            .iter()
            .map(|(id, value)| {
                let value = match value {
                    Ok(value) => Ok((*value).to_string()),
                    Err(SecretError::NotFound) => Err(SecretError::NotFound),
                    Err(SecretError::Unavailable) => Err(SecretError::Unavailable),
                    Err(SecretError::InvalidMaterial) => Err(SecretError::InvalidMaterial),
                    Err(SecretError::DecryptionFailed) => Err(SecretError::DecryptionFailed),
                    Err(SecretError::Internal) => Err(SecretError::Internal),
                };
                (AccountId::from(*id), value)
            })
            .collect();
        let resolver = Arc::new(SyntheticResolver {
            values: map,
            calls: calls.clone(),
        });
        (
            BlackboxAccountHydrator::new(repository, resolver, Arc::new(SyntheticConfigResolver)),
            calls,
        )
    }

    #[tokio::test]
    async fn hydrates_enabled_accounts_in_repository_order() {
        let (hydrator, calls) = make_hydrator(
            vec![
                StoredAccount::new("a", "blackbox", true),
                StoredAccount::new("b", "blackbox", true),
                StoredAccount::new("disabled", "blackbox", false),
                StoredAccount::new("other", "kiro", true),
            ],
            &[("a", Ok("key-a")), ("b", Ok("key-b"))],
        );
        let outcome = hydrator.hydrate().await.unwrap();
        assert_eq!(
            outcome
                .account_pool
                .ordered_ids_for_provider(&ProviderId::from("blackbox")),
            vec![AccountId::from("a"), AccountId::from("b")]
        );
        assert_eq!(
            *calls.lock().unwrap(),
            vec![AccountId::from("a"), AccountId::from("b")]
        );
        assert!(outcome.report.failures.is_empty());
    }

    #[tokio::test]
    async fn skips_disabled_and_unsupported_without_resolution() {
        let (hydrator, calls) = make_hydrator(
            vec![
                StoredAccount::new("disabled", "blackbox", false),
                StoredAccount::new("other", "kiro", true),
            ],
            &[],
        );
        let outcome = hydrator.hydrate().await.unwrap();
        assert!(
            outcome
                .account_pool
                .ordered_ids_for_provider(&ProviderId::from("blackbox"))
                .is_empty()
        );
        assert!(calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn credential_failure_does_not_block_later_account() {
        let (hydrator, _) = make_hydrator(
            vec![
                StoredAccount::new("bad", "blackbox", true),
                StoredAccount::new("good", "blackbox", true),
            ],
            &[
                ("bad", Err(SecretError::InvalidMaterial)),
                ("good", Ok("key-good")),
            ],
        );
        let outcome = hydrator.hydrate().await.unwrap();
        assert_eq!(
            outcome
                .account_pool
                .ordered_ids_for_provider(&ProviderId::from("blackbox")),
            vec![AccountId::from("good")]
        );
        assert_eq!(
            outcome.report.failures[0].account_id,
            AccountId::from("bad")
        );
        assert!(!format!("{:?}", outcome.report).contains("key-good"));
    }

    #[tokio::test]
    async fn configuration_failure_prevents_credential_resolution() {
        let (hydrator, calls) = make_hydrator(
            vec![
                StoredAccount::new("bad-config", "blackbox", true),
                StoredAccount::new("good", "blackbox", true),
            ],
            &[
                ("bad-config", Ok("must-not-be-read")),
                ("good", Ok("key-good")),
            ],
        );
        let outcome = hydrator.hydrate().await.unwrap();
        assert_eq!(*calls.lock().unwrap(), vec![AccountId::from("good")]);
        assert_eq!(
            outcome.report.failures[0].failure,
            HydrationFailure::ConfigurationNotFound
        );
        assert!(!format!("{:?}", outcome.report).contains("must-not-be-read"));
    }
}

// Startup hydration is offline and does not wire LegacyByokResolver or server configuration.
