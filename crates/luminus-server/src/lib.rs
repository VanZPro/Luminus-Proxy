pub mod app;
pub mod routes;

use std::sync::{Arc, Mutex};

use luminus_composition::{BlackboxAccountHydrator, BlackboxCredentials, BlackboxProviderConfig};
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
        ).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        assert!(*seen.lock().unwrap());
    }
}
