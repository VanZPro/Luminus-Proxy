pub mod app;
pub mod legacy;
pub mod routes;

use std::sync::{Arc, Mutex};

use luminus_composition::{BlackboxAccountHydrator, BlackboxCredentials, BlackboxProviderConfig};
use luminus_legacy_composition::LegacyByokBlackboxHydrator;
use luminus_legacy_credentials::LegacyPasswordReader;
use luminus_legacy_provider_config::LegacyByokConfigResolver;
use luminus_provider_config::{
    ProviderConfigError, ProviderConfigRequest, ProviderConfigResolver,
    ProviderConfigResolverFuture,
};
use luminus_router::ProviderRegistry;
use luminus_runtime_bootstrap::{
    BlackboxRuntimeBootstrap, NativeOnlyRuntimeBootstrap, RuntimeSnapshot,
};
use luminus_secrets::{
    CredentialRequest, CredentialResolver, CredentialResolverFuture, SecretError, SecretString,
};
use luminus_storage::{MemoryAccountRepository, StoredAccount};
use luminus_storage_sqlite::LegacyTsAccountRepository;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStartupMode {
    Current,
    ExperimentalBootstrap,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid LUMINUS_EXPERIMENTAL_RUNTIME_BOOTSTRAP value")]
pub struct StartupModeError;

impl RuntimeStartupMode {
    pub fn parse(value: Option<&str>) -> Result<Self, StartupModeError> {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            None | Some("false") | Some("off") | Some("0") => Ok(Self::Current),
            Some("true") | Some("on") | Some("1") => Ok(Self::ExperimentalBootstrap),
            Some(_) => Err(StartupModeError),
        }
    }
}

#[derive(Debug)]
pub struct ServerStartupConfig {
    pub runtime_mode: RuntimeStartupMode,
    pub legacy: Option<legacy::ExperimentalLegacySourceConfig>,
}

pub fn parse_startup_config(
    runtime_value: Option<&str>,
    legacy_flag: Option<&str>,
    legacy_path: Option<&str>,
    legacy_key: Option<&str>,
    source_order: Option<&str>,
) -> Result<ServerStartupConfig, Box<dyn std::error::Error>> {
    let runtime_mode = RuntimeStartupMode::parse(runtime_value)?;
    let legacy = legacy::parse_config(
        runtime_mode,
        legacy_flag,
        legacy_path,
        legacy_key,
        source_order,
    )?;
    Ok(ServerStartupConfig {
        runtime_mode,
        legacy,
    })
}

struct StaticConfig(Option<String>);

impl ProviderConfigResolver<BlackboxProviderConfig> for StaticConfig {
    fn resolve<'a>(
        &'a self,
        _: &'a ProviderConfigRequest,
    ) -> ProviderConfigResolverFuture<'a, BlackboxProviderConfig> {
        let value = self.0.clone();
        Box::pin(async move {
            value
                .map(|base_url| BlackboxProviderConfig { base_url })
                .ok_or(ProviderConfigError::NotFound)
        })
    }
}

struct OneShotCredentials(Mutex<Option<BlackboxCredentials>>);

impl CredentialResolver<BlackboxCredentials> for OneShotCredentials {
    fn resolve<'a>(
        &'a self,
        _: &'a CredentialRequest,
    ) -> CredentialResolverFuture<'a, BlackboxCredentials> {
        let value = self.0.lock().expect("credential lock poisoned").take();
        Box::pin(async move { value.ok_or(SecretError::Unavailable) })
    }
}

pub fn experimental_bootstrap(base_url: String, api_key: String) -> NativeOnlyRuntimeBootstrap {
    let repository = Arc::new(
        MemoryAccountRepository::new(vec![StoredAccount::new(
            "blackbox-default",
            "blackbox",
            true,
        )])
        .expect("static native account metadata is valid"),
    );
    let hydrator = BlackboxAccountHydrator::new(
        repository,
        Arc::new(OneShotCredentials(Mutex::new(Some(BlackboxCredentials {
            api_key: SecretString::new(api_key),
        })))),
        Arc::new(StaticConfig(Some(base_url))),
    );
    BlackboxRuntimeBootstrap::native_only(hydrator, Arc::new(ProviderRegistry::new()))
}

pub async fn build_experimental_snapshot(
    base_url: String,
    api_key: String,
) -> Result<RuntimeSnapshot, Box<dyn std::error::Error>> {
    Ok(experimental_bootstrap(base_url, api_key).build().await?)
}

pub async fn build_experimental_snapshot_with_legacy(
    base_url: String,
    api_key: String,
    legacy: legacy::ExperimentalLegacySourceConfig,
) -> Result<RuntimeSnapshot, Box<dyn std::error::Error>> {
    legacy::preflight(&legacy)?;
    let native = experimental_bootstrap(base_url, api_key);
    let repository = Arc::new(LegacyTsAccountRepository::new(&legacy.database_path));
    let config = Arc::new(LegacyByokConfigResolver::new(&legacy.database_path));
    let reader = Arc::new(LegacyPasswordReader::new(&legacy.database_path));
    let credentials = Arc::new(luminus_composition::LegacyByokResolver::new(
        reader.clone(),
        legacy.legacy_key,
    ));
    let legacy_hydrator = LegacyByokBlackboxHydrator::new(repository, config, credentials);
    let native = native.into_native();
    Ok(BlackboxRuntimeBootstrap::new(
        native,
        legacy_hydrator,
        legacy.source_order,
        Arc::new(ProviderRegistry::new()),
    )
    .build()
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router as AxumRouter,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::post,
    };
    use luminus_core::model::AccountId;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;

    #[test]
    fn startup_mode_defaults_current() {
        assert_eq!(
            RuntimeStartupMode::parse(None),
            Ok(RuntimeStartupMode::Current)
        );
        assert_eq!(
            RuntimeStartupMode::parse(Some("off")),
            Ok(RuntimeStartupMode::Current)
        );
    }

    #[test]
    fn startup_mode_accepts_explicit_experimental_values() {
        for value in ["true", "on", "1"] {
            assert_eq!(
                RuntimeStartupMode::parse(Some(value)),
                Ok(RuntimeStartupMode::ExperimentalBootstrap)
            );
        }
    }

    #[test]
    fn startup_mode_rejects_malformed_values() {
        assert!(RuntimeStartupMode::parse(Some("maybe")).is_err());
    }

    #[test]
    fn startup_config_validates_legacy_before_dispatch() {
        let error = parse_startup_config(Some("false"), Some("true"), None, None, None)
            .expect_err("legacy must not be accepted by current startup");
        assert!(!error.to_string().contains("SYNTHETIC"));

        assert!(parse_startup_config(None, None, Some("synthetic.db"), None, None).is_err());
        assert!(parse_startup_config(None, None, None, Some("synthetic-key"), None).is_err());
        assert!(parse_startup_config(None, None, None, None, Some("native-then-legacy")).is_err());
    }

    #[test]
    fn startup_config_requires_all_legacy_fields() {
        for input in [
            (
                None,
                Some("true"),
                None,
                Some("key"),
                Some("native-then-legacy"),
            ),
            (
                None,
                Some("true"),
                Some("db"),
                None,
                Some("native-then-legacy"),
            ),
            (
                None,
                Some("true"),
                Some("db"),
                Some("   "),
                Some("native-then-legacy"),
            ),
            (None, Some("true"), Some("db"), Some("key"), None),
        ] {
            assert!(parse_startup_config(input.0, input.1, input.2, input.3, input.4).is_err());
        }
    }

    #[test]
    fn startup_config_accepts_explicit_legacy_configuration() {
        let config = parse_startup_config(
            Some("on"),
            Some("1"),
            Some("synthetic-r26.db"),
            Some("SYNTHETIC_R26_LEGACY_KEY_DO_NOT_LEAK"),
            Some("legacy-then-native"),
        )
        .unwrap();
        assert_eq!(
            config.runtime_mode,
            RuntimeStartupMode::ExperimentalBootstrap
        );
        assert!(config.legacy.is_some());
    }

    #[tokio::test]
    async fn invalid_requested_legacy_source_fails_before_native_snapshot() {
        let path =
            std::env::temp_dir().join(format!("luminus-r26-invalid-{}.db", std::process::id()));
        let config = legacy::ExperimentalLegacySourceConfig {
            database_path: path,
            legacy_key: SecretString::new("SYNTHETIC_R26_LEGACY_KEY_DO_NOT_LEAK"),
            source_order: luminus_runtime_bootstrap::BlackboxSourceOrder::NativeThenLegacy,
        };
        assert!(
            build_experimental_snapshot_with_legacy(
                "http://127.0.0.1:1".into(),
                "SYNTHETIC_R26_NATIVE_KEY_DO_NOT_LEAK".into(),
                config,
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn experimental_snapshot_preserves_native_identity() {
        let snapshot = build_experimental_snapshot("http://127.0.0.1:1".into(), "synthetic".into())
            .await
            .unwrap();
        let account = snapshot
            .account_pool
            .get(&AccountId::from("blackbox-default"))
            .unwrap();
        assert_eq!(account.descriptor.provider.0.as_str(), "blackbox");
        assert!(account.descriptor.enabled);
    }

    #[tokio::test]
    async fn experimental_server_route_reaches_localhost_with_native_key() {
        let expected = "Bearer SYNTHETIC_R25_SERVER_API_KEY_DO_NOT_LEAK";
        let seen = Arc::new(Mutex::new(false));
        let state = seen.clone();
        let upstream = AxumRouter::new()
            .route("/chat/completions", post(move |headers: HeaderMap| {
                let state = state.clone();
                async move {
                    *state.lock().unwrap() = headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok()) == Some(expected);
                    (StatusCode::OK, r#"{"id":"r25","model":"bb/claude-sonnet-4.6","choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}"#).into_response()
                }
            }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        });
        let snapshot = build_experimental_snapshot(
            format!("http://{address}"),
            "SYNTHETIC_R25_SERVER_API_KEY_DO_NOT_LEAK".into(),
        )
        .await
        .unwrap();
        let app = crate::app::experimental_app(Arc::new(snapshot.router));
        let response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::post("/experimental/v1/chat/completions")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"model":"bb/claude-sonnet-4.6","messages":[{"role":"User","content":"hello"}],"temperature":null,"top_p":null,"max_tokens":null,"max_completion_tokens":null,"stop":null,"tools":null,"tool_choice":null}"#))
                .unwrap(),
        )
        .await
        .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        assert!(*seen.lock().unwrap());
    }
}
