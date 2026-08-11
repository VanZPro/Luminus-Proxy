use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::Engine;
use luminus_composition::LegacyByokResolver;
use luminus_core::model::{AccountId, ProviderId};
use luminus_legacy_composition::{LegacyByokBlackboxHydrator, LegacyHydrationOutcome};
use luminus_legacy_credentials::LegacyPasswordReader;
use luminus_legacy_provider_config::LegacyByokConfigResolver;
use luminus_secrets::SecretString;
use luminus_storage_sqlite::LegacyTsAccountRepository;
use rusqlite::{Connection, params};

const LEGACY_KEY: &str = "synthetic-r23a-key";
const API_KEY: &str = "synthetic-api-key-r23a";
const TOKEN_SECRET: &str = "synthetic-token-secret-r23a";

struct SyntheticDatabase {
    path: PathBuf,
}

impl SyntheticDatabase {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("luminus-r23a-{}-{nonce}.db", std::process::id()));
        let connection = Connection::open(&path).expect("create synthetic database");
        connection
            .execute_batch(
                "CREATE TABLE accounts (
                    id INTEGER PRIMARY KEY,
                    provider TEXT NOT NULL,
                    email TEXT NOT NULL,
                    password TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'active',
                    enabled INTEGER NOT NULL DEFAULT 1,
                    tokens TEXT NOT NULL,
                    metadata TEXT,
                    quota INTEGER,
                    extra TEXT,
                    UNIQUE(provider, email)
                );",
            )
            .expect("create TypeScript-shaped accounts table");
        Self { path }
    }

    fn insert_blackbox(&self) {
        let connection = Connection::open(&self.path).expect("open synthetic database for setup");
        let tokens = format!(
            r#"{{
                "original_provider": "blackbox",
                "base_url": "http://127.0.0.1:1/v1",
                "format": "openai",
                "models": ["blackbox-model"],
                "api_key": "{API_KEY}",
                "token": {{"value": "{TOKEN_SECRET}"}},
                "cookies": ["{TOKEN_SECRET}"]
            }}"#
        );
        connection
            .execute(
                "INSERT INTO accounts
                 (id, provider, email, password, enabled, tokens, metadata)
                 VALUES (?1, 'byok', ?2, ?3, 1, ?4, ?5)",
                params![
                    2301_i64,
                    "synthetic-r23a@example.invalid",
                    encode_legacy_password(API_KEY, LEGACY_KEY),
                    tokens,
                    "synthetic metadata"
                ],
            )
            .expect("insert synthetic Blackbox BYOK account");
    }
}

impl Drop for SyntheticDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn encode_legacy_password(value: &str, key: &str) -> String {
    let key = key.as_bytes();
    let ciphertext = value
        .as_bytes()
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % key.len()])
        .collect::<Vec<_>>();
    base64::engine::general_purpose::STANDARD.encode(ciphertext)
}

#[tokio::test]
async fn real_legacy_adapters_hydrate_synthetic_blackbox_into_account_pool() {
    let database = SyntheticDatabase::new();
    database.insert_blackbox();

    let repository = Arc::new(LegacyTsAccountRepository::new(&database.path));
    let config = Arc::new(LegacyByokConfigResolver::new(&database.path));
    let password_reader = Arc::new(LegacyPasswordReader::new(&database.path));
    let credentials = Arc::new(LegacyByokResolver::new(
        password_reader,
        SecretString::new(LEGACY_KEY),
    ));
    let hydrator = LegacyByokBlackboxHydrator::new(repository, config, credentials);

    let hydrated = hydrator.hydrate().await.expect("hydrate synthetic account");
    let account_id = AccountId::from("legacy-ts:2301");
    let blackbox = ProviderId::from("blackbox");

    assert_eq!(
        hydrated.account_pool.ordered_ids_for_provider(&blackbox),
        vec![account_id.clone()]
    );
    let account = hydrated
        .account_pool
        .get(&account_id)
        .expect("hydrated account exists");
    assert_eq!(account.descriptor.id, account_id);
    assert_eq!(account.descriptor.provider, blackbox);
    assert!(account.descriptor.enabled);
    assert_eq!(
        hydrated.report.entries[0].outcome,
        LegacyHydrationOutcome::HydratedBlackbox
    );

    let debug = format!("{:?}", hydrated.report);
    assert!(!debug.contains(API_KEY));
    assert!(!debug.contains(TOKEN_SECRET));
    assert!(!debug.contains(LEGACY_KEY));
    assert!(!debug.contains("original_provider"));
    assert!(!debug.contains("blackbox-model"));
    assert!(!debug.contains("synthetic-r23a-key"));

    assert!(!debug.contains("LegacyByokConfig"));
    assert!(!debug.contains("LegacyCiphertext"));
}
