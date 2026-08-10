mod app;
mod routes;

use luminus_core::{AppConfig, model::ProviderId};
use luminus_providers::{BlackboxConfig, BlackboxProvider};
use luminus_router::{ProviderRegistry, Router as LuminusRouter};
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::from_env()?;
    tracing_subscriber::fmt()
        .with_env_filter(&config.log)
        .with_target(false)
        .init();

    let address = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    info!(service = "luminus", version = env!("CARGO_PKG_VERSION"), %address, environment = %config.environment, "server started");

    let provider = match (
        std::env::var("BLACKBOX_BASE_URL"),
        std::env::var("BLACKBOX_API_KEY"),
    ) {
        (Ok(base_url), Ok(api_key)) => Some(Arc::new(BlackboxProvider::new(BlackboxConfig::new(
            base_url, api_key,
        ))?)),
        _ => None,
    };
    let mut registry = ProviderRegistry::new();
    if let Some(provider) = provider {
        registry.register(provider);
    }
    let router = Arc::new(LuminusRouter::new(
        Arc::new(registry),
        Some(ProviderId("blackbox".into())),
    ));
    axum::serve(listener, app::experimental_app(router))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("server shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install terminate signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("shutdown signal received"),
        _ = terminate => info!("shutdown signal received"),
    }
}
