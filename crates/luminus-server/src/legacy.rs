use luminus_runtime_bootstrap::BlackboxSourceOrder;
use luminus_secrets::SecretString;
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyPreflightError {
    InvalidConfiguration,
    PathMissing,
    PathInvalid,
    OpenFailed,
    SchemaMissing,
    RequiredColumnMissing,
    EmptyKey,
    InvalidSourceOrder,
}
impl std::fmt::Display for LegacyPreflightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "legacy source preflight failed: {:?}", self)
    }
}
impl std::error::Error for LegacyPreflightError {}

#[derive(Debug)]
pub struct ExperimentalLegacySourceConfig {
    pub database_path: PathBuf,
    pub legacy_key: SecretString,
    pub source_order: BlackboxSourceOrder,
}

fn enabled(v: Option<&str>, name: &str) -> Result<bool, LegacyPreflightError> {
    match v.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        None | Some("false") | Some("off") | Some("0") => Ok(false),
        Some("true") | Some("on") | Some("1") => Ok(true),
        Some(_) => {
            let _ = name;
            Err(LegacyPreflightError::InvalidConfiguration)
        }
    }
}
pub fn parse_source_order(v: Option<&str>) -> Result<BlackboxSourceOrder, LegacyPreflightError> {
    match v.map(str::trim) {
        Some("native-then-legacy") => Ok(BlackboxSourceOrder::NativeThenLegacy),
        Some("legacy-then-native") => Ok(BlackboxSourceOrder::LegacyThenNative),
        _ => Err(LegacyPreflightError::InvalidSourceOrder),
    }
}
pub fn parse_config(
    mode: super::RuntimeStartupMode,
    flag: Option<&str>,
    path: Option<&str>,
    key: Option<&str>,
    order: Option<&str>,
) -> Result<Option<ExperimentalLegacySourceConfig>, LegacyPreflightError> {
    let requested = enabled(flag, "legacy")?;
    let stray = path.is_some() || key.is_some() || order.is_some();
    if !requested {
        if stray {
            return Err(LegacyPreflightError::InvalidConfiguration);
        }
        return Ok(None);
    }
    if mode != super::RuntimeStartupMode::ExperimentalBootstrap {
        return Err(LegacyPreflightError::InvalidConfiguration);
    }
    let path = path
        .filter(|p| !p.trim().is_empty())
        .map(PathBuf::from)
        .ok_or(LegacyPreflightError::PathMissing)?;
    let key = key.ok_or(LegacyPreflightError::EmptyKey)?;
    if key.trim().is_empty() {
        return Err(LegacyPreflightError::EmptyKey);
    }
    Ok(Some(ExperimentalLegacySourceConfig {
        database_path: path,
        legacy_key: SecretString::new(key),
        source_order: parse_source_order(order)?,
    }))
}
pub fn preflight(c: &ExperimentalLegacySourceConfig) -> Result<(), LegacyPreflightError> {
    if c.legacy_key.expose_secret().trim().is_empty() {
        return Err(LegacyPreflightError::EmptyKey);
    }
    if !c.database_path.exists() {
        return Err(LegacyPreflightError::PathMissing);
    }
    if !c.database_path.is_file() {
        return Err(LegacyPreflightError::PathInvalid);
    }
    let db = Connection::open_with_flags(&c.database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| LegacyPreflightError::OpenFailed)?;
    let mut s = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='accounts'")
        .map_err(|_| LegacyPreflightError::OpenFailed)?;
    if s.query_row([], |_| Ok(())).is_err() {
        return Err(LegacyPreflightError::SchemaMissing);
    }
    let mut s = db
        .prepare("PRAGMA table_info(accounts)")
        .map_err(|_| LegacyPreflightError::OpenFailed)?;
    let cols: std::collections::HashSet<String> = s
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|_| LegacyPreflightError::OpenFailed)?
        .filter_map(Result::ok)
        .collect();
    for col in ["id", "provider", "enabled", "password", "tokens"] {
        if !cols.contains(col) {
            return Err(LegacyPreflightError::RequiredColumnMissing);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "luminus-r26-{label}-{}-{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn valid_config(path: &std::path::Path) -> ExperimentalLegacySourceConfig {
        ExperimentalLegacySourceConfig {
            database_path: path.to_path_buf(),
            legacy_key: SecretString::new("synthetic-r26-key"),
            source_order: BlackboxSourceOrder::NativeThenLegacy,
        }
    }

    fn make_db(path: &std::path::Path, columns: &str) {
        let db = Connection::open(path).unwrap();
        db.execute_batch(&format!("CREATE TABLE accounts ({columns});"))
            .unwrap();
    }

    #[test]
    fn flags_are_strict() {
        assert!(
            parse_config(
                super::super::RuntimeStartupMode::Current,
                Some("true"),
                None,
                None,
                None,
            )
            .is_err()
        );
        assert!(
            parse_config(
                super::super::RuntimeStartupMode::ExperimentalBootstrap,
                None,
                Some("x"),
                None,
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn preflight_accepts_empty_valid_database() {
        let path = temp_path("empty");
        make_db(
            &path,
            "id INTEGER, provider TEXT, enabled INTEGER, password TEXT, tokens TEXT",
        );
        assert_eq!(preflight(&valid_config(&path)), Ok(()));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn preflight_rejects_empty_key_before_database_access() {
        let path = temp_path("empty-key");
        let config = ExperimentalLegacySourceConfig {
            database_path: path.clone(),
            legacy_key: SecretString::new("   "),
            source_order: BlackboxSourceOrder::NativeThenLegacy,
        };
        assert_eq!(preflight(&config), Err(LegacyPreflightError::EmptyKey));
        assert!(!path.exists());
    }

    #[test]
    fn preflight_rejects_missing_path_without_creating_file() {
        let path = temp_path("missing");
        assert!(!path.exists());
        assert_eq!(
            preflight(&valid_config(&path)),
            Err(LegacyPreflightError::PathMissing)
        );
        assert!(!path.exists());
    }

    #[test]
    fn preflight_rejects_directory_and_invalid_sqlite() {
        let directory = temp_path("directory");
        fs::create_dir(&directory).unwrap();
        assert_eq!(
            preflight(&valid_config(&directory)),
            Err(LegacyPreflightError::PathInvalid)
        );
        fs::remove_dir(directory).unwrap();

        let invalid = temp_path("invalid");
        fs::write(&invalid, b"not sqlite").unwrap();
        assert_eq!(
            preflight(&valid_config(&invalid)),
            Err(LegacyPreflightError::OpenFailed)
        );
        fs::remove_file(invalid).unwrap();
    }

    #[test]
    fn preflight_rejects_missing_table_and_each_required_column() {
        let missing_table = temp_path("table");
        Connection::open(&missing_table).unwrap();
        assert_eq!(
            preflight(&valid_config(&missing_table)),
            Err(LegacyPreflightError::SchemaMissing)
        );
        fs::remove_file(missing_table).unwrap();

        for missing in ["id", "provider", "enabled", "password", "tokens"] {
            let path = temp_path(missing);
            let columns = ["id", "provider", "enabled", "password", "tokens"]
                .into_iter()
                .filter(|column| *column != missing)
                .map(|column| format!("{column} TEXT"))
                .collect::<Vec<_>>()
                .join(", ");
            make_db(&path, &columns);
            assert_eq!(
                preflight(&valid_config(&path)),
                Err(LegacyPreflightError::RequiredColumnMissing),
                "missing {missing}"
            );
            fs::remove_file(path).unwrap();
        }
    }
}
