use luminus_core::{
    AppConfig,
    model::{AccountDescriptor, AccountId, ProviderId},
};
use luminus_providers::{BlackboxConfig, BlackboxProvider};
use luminus_router::{AccountPool, ProviderAccount, ProviderRegistry, Router as LuminusRouter};
use luminus_server::{
    ExperimentalRuntimeExecutionOutcome, RuntimeConfigurationProvenance, RuntimeStartupMode, app,
    execute_prepared_experimental_runtime, parse_startup_config,
    prepare_experimental_runtime_with_provenance,
};
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::from_env()?;
    tracing_subscriber::fmt()
        .with_env_filter(&config.log)
        .with_target(false)
        .init();
    let startup = parse_startup_config(
        std::env::var("LUMINUS_EXPERIMENTAL_RUNTIME_BOOTSTRAP")
            .ok()
            .as_deref(),
        std::env::var("LUMINUS_EXPERIMENTAL_LEGACY_SOURCE")
            .ok()
            .as_deref(),
        std::env::var("LUMINUS_EXPERIMENTAL_LEGACY_DB_PATH")
            .ok()
            .as_deref(),
        std::env::var("LUMINUS_EXPERIMENTAL_LEGACY_KEY")
            .ok()
            .as_deref(),
        std::env::var("LUMINUS_EXPERIMENTAL_SOURCE_ORDER")
            .ok()
            .as_deref(),
        std::env::var("LUMINUS_EXPERIMENTAL_RUNTIME_DRY_RUN")
            .ok()
            .as_deref(),
    )?;

    if startup.runtime_mode == RuntimeStartupMode::ExperimentalBootstrap {
        let base_url = std::env::var("BLACKBOX_BASE_URL")?;
        let api_key = std::env::var("BLACKBOX_API_KEY")?;
        let prepared = prepare_experimental_runtime_with_provenance(
            base_url,
            api_key,
            startup.legacy,
            RuntimeConfigurationProvenance::environment_native(),
        )
        .await?;
        let address = format!("{}:{}", config.host, config.port);
        match execute_prepared_experimental_runtime(prepared, startup.execution, &address).await? {
            ExperimentalRuntimeExecutionOutcome::DryRun(diagnostics) => {
                println!("{}", serde_json::to_string(&diagnostics)?);
            }
            ExperimentalRuntimeExecutionOutcome::Serving { listener, app } => {
                info!(service = "luminus", version = env!("CARGO_PKG_VERSION"), %address, environment = %config.environment, "server started");
                axum::serve(listener, app)
                    .with_graceful_shutdown(shutdown_signal())
                    .await?;
            }
        }
        return Ok(());
    }

    let address = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    let provider = match (
        std::env::var("BLACKBOX_BASE_URL"),
        std::env::var("BLACKBOX_API_KEY"),
    ) {
        (Ok(base_url), Ok(api_key)) => Some(Arc::new(BlackboxProvider::new(BlackboxConfig::new(
            base_url, api_key,
        ))?)),
        _ => None,
    };
    let registry = ProviderRegistry::new();
    let mut account_pool = AccountPool::new();
    if let Some(provider) = provider {
        account_pool.register(ProviderAccount {
            descriptor: AccountDescriptor {
                id: AccountId::from("blackbox-default"),
                provider: ProviderId::from("blackbox"),
                enabled: true,
            },
            adapter: provider,
        })?;
    }
    let router = LuminusRouter::new(Arc::new(registry), Some(ProviderId("blackbox".into())))
        .with_accounts(Arc::new(account_pool));
    axum::serve(listener, app::experimental_app(Arc::new(router)))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
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
    tokio::select! { _ = ctrl_c => info!("shutdown signal received"), _ = terminate => info!("shutdown signal received"), }
}
