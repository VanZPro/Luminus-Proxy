use std::sync::Arc;

use luminus_composition::{
    BlackboxCredentials, BlackboxProviderConfig, ByokCredentials, build_blackbox_provider_account,
};
use luminus_core::model::{AccountId, ProviderId};
use luminus_legacy_provider_config::{LegacyBlackboxConfig, LegacyByokConfig};
use luminus_provider_config::{ProviderConfigError, ProviderConfigRequest, ProviderConfigResolver};
use luminus_router::AccountPool;
use luminus_secrets::{CredentialRequest, CredentialResolver, SecretError};
use luminus_storage::{AccountRepository, StorageError};

pub const BYOK_PROVIDER: &str = "byok";
pub const BLACKBOX_PROVIDER: &str = "blackbox";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyHydrationOutcome {
    HydratedBlackbox,
    SkippedOpenAiCompatible,
    SkippedUnresolved,
    ConfigurationInvalid,
    CredentialNotFound,
    CredentialInvalid,
    ProviderConstructionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyHydrationReportEntry {
    pub account_id: AccountId,
    pub outcome: LegacyHydrationOutcome,
}

#[derive(Debug, Default)]
pub struct LegacyHydrationReport {
    pub entries: Vec<LegacyHydrationReportEntry>,
}

pub struct LegacyHydrationOutcomeSet {
    pub account_pool: AccountPool,
    pub report: LegacyHydrationReport,
}

pub struct LegacyByokBlackboxHydrator {
    repository: Arc<dyn AccountRepository>,
    config: Arc<dyn ProviderConfigResolver<LegacyByokConfig>>,
    credentials: Arc<dyn CredentialResolver<ByokCredentials>>,
}

impl LegacyByokBlackboxHydrator {
    pub fn new(
        repository: Arc<dyn AccountRepository>,
        config: Arc<dyn ProviderConfigResolver<LegacyByokConfig>>,
        credentials: Arc<dyn CredentialResolver<ByokCredentials>>,
    ) -> Self {
        Self {
            repository,
            config,
            credentials,
        }
    }

    pub async fn hydrate(&self) -> Result<LegacyHydrationOutcomeSet, StorageError> {
        let records = self.repository.list_accounts().await?;
        let mut pool = AccountPool::new();
        let mut report = LegacyHydrationReport::default();
        for record in records {
            if record.provider != ProviderId::from(BYOK_PROVIDER) || !record.enabled {
                continue;
            }
            let request = ProviderConfigRequest::new(record.id.clone(), BYOK_PROVIDER);
            let config = match self.config.resolve(&request).await {
                Ok(value) => value,
                Err(error) => {
                    let outcome = match error {
                        ProviderConfigError::InvalidConfiguration => {
                            LegacyHydrationOutcome::ConfigurationInvalid
                        }
                        ProviderConfigError::Unsupported
                        | ProviderConfigError::NotFound
                        | ProviderConfigError::Unavailable
                        | ProviderConfigError::Internal => {
                            LegacyHydrationOutcome::SkippedUnresolved
                        }
                    };
                    report.entries.push(LegacyHydrationReportEntry {
                        account_id: record.id,
                        outcome,
                    });
                    continue;
                }
            };
            let LegacyByokConfig::Blackbox(LegacyBlackboxConfig { base_url, .. }) = config else {
                report.entries.push(LegacyHydrationReportEntry {
                    account_id: record.id,
                    outcome: LegacyHydrationOutcome::SkippedOpenAiCompatible,
                });
                continue;
            };
            let credentials = match self
                .credentials
                .resolve(&CredentialRequest::new(record.id.clone(), BYOK_PROVIDER))
                .await
            {
                Ok(value) => value,
                Err(error) => {
                    let outcome = match error {
                        SecretError::NotFound => LegacyHydrationOutcome::CredentialNotFound,
                        _ => LegacyHydrationOutcome::CredentialInvalid,
                    };
                    report.entries.push(LegacyHydrationReportEntry {
                        account_id: record.id,
                        outcome,
                    });
                    continue;
                }
            };
            let runtime = build_blackbox_provider_account(
                record.id.clone(),
                BlackboxProviderConfig { base_url },
                BlackboxCredentials {
                    api_key: credentials.api_key,
                },
            )
            .map_err(|_| LegacyHydrationOutcome::ProviderConstructionFailed);
            match runtime {
                Ok(account) => {
                    pool.register(account)
                        .map_err(|_| StorageError::InvalidRecord)?;
                    report.entries.push(LegacyHydrationReportEntry {
                        account_id: record.id,
                        outcome: LegacyHydrationOutcome::HydratedBlackbox,
                    });
                }
                Err(outcome) => report.entries.push(LegacyHydrationReportEntry {
                    account_id: record.id,
                    outcome,
                }),
            }
        }
        Ok(LegacyHydrationOutcomeSet {
            account_pool: pool,
            report,
        })
    }
}

pub fn runtime_provider() -> ProviderId {
    ProviderId::from(BLACKBOX_PROVIDER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use luminus_core::model::AccountId;
    use luminus_provider_config::ProviderConfigResolverFuture;
    use luminus_secrets::{CredentialResolverFuture, SecretString};
    use luminus_storage::{MemoryAccountRepository, StoredAccount};
    use std::sync::Mutex;

    struct Config {
        values: std::collections::HashMap<AccountId, Result<LegacyByokConfig, ProviderConfigError>>,
        calls: Arc<Mutex<Vec<AccountId>>>,
    }
    impl ProviderConfigResolver<LegacyByokConfig> for Config {
        fn resolve<'a>(
            &'a self,
            r: &'a ProviderConfigRequest,
        ) -> ProviderConfigResolverFuture<'a, LegacyByokConfig> {
            self.calls.lock().unwrap().push(r.account_id.clone());
            let v = self
                .values
                .get(&r.account_id)
                .cloned()
                .unwrap_or(Err(ProviderConfigError::Unsupported));
            Box::pin(async move { v })
        }
    }
    struct Creds {
        values: std::collections::HashMap<AccountId, Result<String, SecretError>>,
        calls: Arc<Mutex<Vec<AccountId>>>,
    }
    impl CredentialResolver<ByokCredentials> for Creds {
        fn resolve<'a>(
            &'a self,
            r: &'a CredentialRequest,
        ) -> CredentialResolverFuture<'a, ByokCredentials> {
            self.calls.lock().unwrap().push(r.account_id.clone());
            let v = match self.values.get(&r.account_id) {
                Some(Ok(value)) => Ok(value.clone()),
                Some(Err(SecretError::Unavailable)) => Err(SecretError::Unavailable),
                Some(Err(SecretError::InvalidMaterial)) => Err(SecretError::InvalidMaterial),
                Some(Err(SecretError::DecryptionFailed)) => Err(SecretError::DecryptionFailed),
                Some(Err(SecretError::Internal)) => Err(SecretError::Internal),
                Some(Err(SecretError::NotFound)) | None => Err(SecretError::NotFound),
            };
            Box::pin(async move {
                v.map(|x| ByokCredentials {
                    api_key: SecretString::new(x),
                })
            })
        }
    }
    fn bb(url: &str) -> LegacyByokConfig {
        LegacyByokConfig::Blackbox(luminus_legacy_provider_config::LegacyBlackboxConfig {
            base_url: url.into(),
            format: luminus_legacy_provider_config::LegacyByokFormat::Openai,
            models: vec!["m".into()],
        })
    }
    #[tokio::test]
    async fn classification_gates_credentials_and_rewrites_runtime_id() {
        let a = AccountId::from("a");
        let b = AccountId::from("b");
        let cc = Arc::new(Mutex::new(Vec::new()));
        let kc = Arc::new(Mutex::new(Vec::new()));
        let cfg = Arc::new(Config {
            values: [
                (a.clone(), Ok(bb("http://127.0.0.1:1"))),
                (b.clone(), Err(ProviderConfigError::Unsupported)),
            ]
            .into(),
            calls: cc.clone(),
        });
        let cred = Arc::new(Creds {
            values: [(a.clone(), Ok("synthetic-key".into()))].into(),
            calls: kc.clone(),
        });
        let h = LegacyByokBlackboxHydrator::new(
            Arc::new(
                MemoryAccountRepository::new(vec![
                    StoredAccount::new("a", "byok", true),
                    StoredAccount::new("b", "byok", true),
                ])
                .unwrap(),
            ),
            cfg,
            cred,
        );
        let out = h.hydrate().await.unwrap();
        assert_eq!(
            out.account_pool
                .ordered_ids_for_provider(&runtime_provider()),
            vec![a.clone()]
        );
        assert_eq!(*kc.lock().unwrap(), vec![a]);
    }
    #[tokio::test]
    async fn openai_and_disabled_are_not_read() {
        let cc = Arc::new(Mutex::new(Vec::new()));
        let kc = Arc::new(Mutex::new(Vec::new()));
        let cfg = Arc::new(Config {
            values: [].into(),
            calls: cc.clone(),
        });
        let cred = Arc::new(Creds {
            values: [].into(),
            calls: kc.clone(),
        });
        let h = LegacyByokBlackboxHydrator::new(
            Arc::new(
                MemoryAccountRepository::new(vec![
                    StoredAccount::new("o", "byok", true),
                    StoredAccount::new("d", "byok", false),
                    StoredAccount::new("x", "other", true),
                ])
                .unwrap(),
            ),
            cfg,
            cred,
        );
        let _ = h.hydrate().await.unwrap();
        assert_eq!(*cc.lock().unwrap(), vec![AccountId::from("o")]);
        assert!(kc.lock().unwrap().is_empty());
    }
}
