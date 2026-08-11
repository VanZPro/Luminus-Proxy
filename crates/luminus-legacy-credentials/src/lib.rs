mod ciphertext;
mod codec;
mod error;
mod sqlite;

pub use ciphertext::LegacyCiphertext;
pub use codec::decode;
pub use error::LegacyCredentialError;
pub use sqlite::{LegacyEncryptedPassword, LegacyPasswordReader};

use luminus_core::model::AccountId;

pub(crate) fn encode_id(id: i64) -> Option<AccountId> {
    (id >= 0).then(|| AccountId::from(format!("legacy-ts:{id}")))
}

pub(crate) fn decode_id(id: &AccountId) -> Option<i64> {
    id.0.strip_prefix("legacy-ts:")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use luminus_secrets::SecretString;
    use rusqlite::Connection;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn encode(value: &str, key: &str) -> String {
        let key = key.as_bytes();
        let bytes = value
            .as_bytes()
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ key[i % key.len()])
            .collect::<Vec<_>>();
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
    }

    fn fixture() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "luminus-r18-{}.db",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE accounts (id INTEGER PRIMARY KEY, provider TEXT NOT NULL, email TEXT NOT NULL, password TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'active', enabled INTEGER NOT NULL DEFAULT 1, tokens TEXT, metadata TEXT, quota INTEGER, extra TEXT, UNIQUE(provider,email));").unwrap();
        db.execute("INSERT INTO accounts (id,provider,email,password,tokens,metadata,extra) VALUES (1,'synthetic','fake@example.invalid',?1,'sentinel-token','sentinel-meta','extra')", [&encode("synthetic-password-a", "synthetic-key")]).unwrap();
        path
    }

    #[test]
    fn fixed_vector_and_safe_errors() {
        let ciphertext = LegacyCiphertext::new("AAAAAAAAAAAAABsECgAOAQYMSBU=");
        assert_eq!(
            decode(&ciphertext, &SecretString::new("synthetic-key"))
                .unwrap()
                .expose_secret(),
            "synthetic-password-a"
        );
        assert_eq!(
            decode(&LegacyCiphertext::new("!"), &SecretString::new("k")),
            Err(LegacyCredentialError::InvalidCiphertext)
        );
        assert_eq!(
            decode(&LegacyCiphertext::new("AA=="), &SecretString::new("")),
            Err(LegacyCredentialError::InvalidKey)
        );
    }

    #[tokio::test]
    async fn synthetic_read_decode_round_trip() {
        let path = fixture();
        let reader = LegacyPasswordReader::new(&path);
        let row = reader
            .get(&AccountId::from("legacy-ts:1"))
            .await
            .unwrap()
            .unwrap();
        let plaintext = decode(&row.ciphertext, &SecretString::new("synthetic-key")).unwrap();
        assert_eq!(plaintext.expose_secret(), "synthetic-password-a");
        fs::remove_file(path).unwrap();
    }
}

// The legacy format is decoder-only compatibility. New Rust writes must not use it.
