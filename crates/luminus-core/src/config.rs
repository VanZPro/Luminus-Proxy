use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub environment: String,
    pub log: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_owned(),
            port: 1931,
            environment: "development".to_owned(),
            log: "info".to_owned(),
        }
    }
}

impl AppConfig {
    pub fn from_env() -> Result<Self, crate::AppError> {
        let defaults = Self::default();
        let port = match env::var("LUMINUS_PORT") {
            Ok(value) => value.parse::<u16>().map_err(|error| {
                crate::AppError::Configuration(format!("invalid LUMINUS_PORT: {error}"))
            })?,
            Err(env::VarError::NotPresent) => defaults.port,
            Err(error) => return Err(crate::AppError::Configuration(error.to_string())),
        };

        Ok(Self {
            host: env::var("LUMINUS_HOST").unwrap_or(defaults.host),
            port,
            environment: env::var("LUMINUS_ENV").unwrap_or(defaults.environment),
            log: env::var("LUMINUS_LOG").unwrap_or(defaults.log),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn defaults_are_sensible() {
        assert_eq!(AppConfig::default().host, "127.0.0.1");
        assert_eq!(AppConfig::default().port, 1931);
        assert_eq!(AppConfig::default().environment, "development");
        assert_eq!(AppConfig::default().log, "info");
    }
}
