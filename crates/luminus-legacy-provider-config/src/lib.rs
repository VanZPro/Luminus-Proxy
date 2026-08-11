use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use luminus_core::model::{AccountId, ProviderId};
use luminus_provider_config::{
    ProviderConfigError, ProviderConfigRequest, ProviderConfigResolver,
    ProviderConfigResolverFuture,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Deserialize;

const LEGACY_PROVIDER: &str = "byok";
const BLACKBOX: &str = "blackbox";
const OPENAI_PREFIX: &str = "openai-compatible";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyByokFormat {
    Openai,
    Anthropic,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyBlackboxConfig {
    pub base_url: String,
    pub format: LegacyByokFormat,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyOpenAiCompatibleConfig {
    pub base_url: String,
    pub format: LegacyByokFormat,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyByokConfig {
    Blackbox(LegacyBlackboxConfig),
    OpenAiCompatible(LegacyOpenAiCompatibleConfig),
}

#[derive(Debug, Deserialize)]
struct LegacyByokTokenProjection {
    original_provider: Option<String>,
    base_url: Option<String>,
    format: Option<LegacyByokFormat>,
    models: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for LegacyByokFormat {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match String::deserialize(d)?.as_str() {
            "openai" => Ok(Self::Openai),
            "anthropic" => Ok(Self::Anthropic),
            "auto" => Ok(Self::Auto),
            _ => Err(serde::de::Error::custom("unsupported BYOK format")),
        }
    }
}

pub struct LegacyByokConfigResolver {
    path: PathBuf,
}
impl LegacyByokConfigResolver {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

fn account_number(id: &AccountId) -> Option<i64> {
    id.0.strip_prefix("legacy-ts:")?.parse().ok()
}
fn parse(raw: String) -> Result<LegacyByokConfig, ProviderConfigError> {
    let p: LegacyByokTokenProjection =
        serde_json::from_str(&raw).map_err(|_| ProviderConfigError::InvalidConfiguration)?;
    let provider = p
        .original_provider
        .as_deref()
        .ok_or(ProviderConfigError::Unsupported)?;
    let base_url = p
        .base_url
        .ok_or(ProviderConfigError::InvalidConfiguration)?;
    let format = p.format.unwrap_or(LegacyByokFormat::Auto);
    let models = p.models.ok_or(ProviderConfigError::InvalidConfiguration)?;
    if provider == BLACKBOX {
        Ok(LegacyByokConfig::Blackbox(LegacyBlackboxConfig {
            base_url,
            format,
            models,
        }))
    } else if provider.starts_with(OPENAI_PREFIX) {
        Ok(LegacyByokConfig::OpenAiCompatible(
            LegacyOpenAiCompatibleConfig {
                base_url,
                format,
                models,
            },
        ))
    } else {
        Err(ProviderConfigError::Unsupported)
    }
}

fn resolve_sync(
    path: &Path,
    request: &ProviderConfigRequest,
) -> Result<LegacyByokConfig, ProviderConfigError> {
    if request.provider_id != ProviderId::from(LEGACY_PROVIDER) {
        return Err(ProviderConfigError::Unsupported);
    }
    let id = account_number(&request.account_id).ok_or(ProviderConfigError::NotFound)?;
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| ProviderConfigError::Internal)?;
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT provider, tokens FROM accounts WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|_| ProviderConfigError::Internal)?;
    let (provider, tokens) = row.ok_or(ProviderConfigError::NotFound)?;
    if provider != LEGACY_PROVIDER {
        return Err(ProviderConfigError::Unsupported);
    }
    parse(tokens.ok_or(ProviderConfigError::InvalidConfiguration)?)
}

impl ProviderConfigResolver<LegacyByokConfig> for LegacyByokConfigResolver {
    fn resolve<'a>(
        &'a self,
        request: &'a ProviderConfigRequest,
    ) -> ProviderConfigResolverFuture<'a, LegacyByokConfig> {
        let path = self.path.clone();
        let request = request.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || resolve_sync(&path, &request))
                .await
                .map_err(|_| ProviderConfigError::Internal)?
        })
    }
}

pub type LegacyByokConfigResolverObject = Arc<dyn ProviderConfigResolver<LegacyByokConfig>>;

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct F(PathBuf);
    impl F {
        fn new(rows: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "luminus-r22-{}-{}.db",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let c = Connection::open(&p).unwrap();
            c.execute_batch(
                "CREATE TABLE accounts (id INTEGER PRIMARY KEY, provider TEXT, tokens TEXT);",
            )
            .unwrap();
            c.execute_batch(rows).unwrap();
            Self(p)
        }
    }
    impl Drop for F {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    fn json(provider: &str, url: &str) -> String {
        format!(
            r#"{{"original_provider":"{provider}","base_url":"{url}","format":"openai","models":["m"],"api_key":{{"unexpected":["SYNTHETIC_SECRET_MUST_NOT_ESCAPE"]}},"cookies":"SYNTHETIC_COOKIE"}}"#
        )
    }
    async fn get(f: &F, id: i64) -> Result<LegacyByokConfig, ProviderConfigError> {
        LegacyByokConfigResolver::new(&f.0)
            .resolve(&ProviderConfigRequest::new(
                format!("legacy-ts:{id}"),
                "byok",
            ))
            .await
    }
    #[tokio::test]
    async fn variants_and_isolation() {
        let f = F::new(&format!(
            "INSERT INTO accounts VALUES (1,'byok', '{}'); INSERT INTO accounts VALUES (2,'byok', '{}');",
            json("blackbox", "https://bb"),
            json("openai-compatible-custom", "https://oa")
        ));
        assert!(matches!(
            get(&f, 1).await.unwrap(),
            LegacyByokConfig::Blackbox(_)
        ));
        assert!(matches!(
            get(&f, 2).await.unwrap(),
            LegacyByokConfig::OpenAiCompatible(_)
        ));
    }
    #[tokio::test]
    async fn safe_boundaries() {
        let f = F::new(
            "INSERT INTO accounts VALUES (1,'byok','{\"base_url\":\"https://x\",\"models\":[\"m\"]}');",
        );
        assert_eq!(get(&f, 1).await, Err(ProviderConfigError::Unsupported));
        assert_eq!(get(&f, 9).await, Err(ProviderConfigError::NotFound));
        assert_eq!(
            LegacyByokConfigResolver::new(&f.0)
                .resolve(&ProviderConfigRequest::new("legacy-ts:1", "blackbox"))
                .await,
            Err(ProviderConfigError::Unsupported)
        );
    }
    #[tokio::test]
    async fn malformed_and_unknown_are_safe() {
        let f = F::new(
            "INSERT INTO accounts VALUES (1,'byok','not-json'); INSERT INTO accounts VALUES (2,'byok','{\"original_provider\":\"unknown\",\"base_url\":\"https://secret\",\"format\":\"openai\",\"models\":[]}');",
        );
        assert_eq!(
            get(&f, 1).await,
            Err(ProviderConfigError::InvalidConfiguration)
        );
        assert_eq!(get(&f, 2).await, Err(ProviderConfigError::Unsupported));
        assert!(!format!("{:?}", get(&f, 1).await).contains("secret"));
    }
    #[tokio::test]
    async fn schema_failure_is_safe() {
        let p = std::env::temp_dir().join(format!("luminus-r22-missing-{}.db", std::process::id()));
        let _ = fs::remove_file(&p);
        assert_eq!(
            get(&F(p.clone()), 1).await,
            Err(ProviderConfigError::Internal)
        );
        let _ = fs::remove_file(p);
    }
}
