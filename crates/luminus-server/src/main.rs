use luminus_core::AppConfig;
use luminus_router::{AccountPool, ProviderRegistry, Router as LuminusRouter};
use luminus_server::{
    ExperimentalRuntimeExecutionOutcome, RuntimeConfigurationProvenance, RuntimeStartupMode, app,
    execute_native_migration_readiness_dry_run, execute_native_startup_parity_dry_run,
    execute_prepared_experimental_runtime, parse_startup_config_with_readiness,
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
    let startup = parse_startup_config_with_readiness(
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
        std::env::var("LUMINUS_EXPERIMENTAL_PARITY_DRY_RUN")
            .ok()
            .as_deref(),
        std::env::var("LUMINUS_EXPERIMENTAL_MIGRATION_READINESS_DRY_RUN")
            .ok()
            .as_deref(),
    )?;

    if startup.runtime_mode == RuntimeStartupMode::ExperimentalBootstrap {
        if startup.execution == luminus_server::ExperimentalRuntimeExecution::ParityDryRun {
            let base_url = std::env::var("BLACKBOX_BASE_URL")?;
            let api_key = std::env::var("BLACKBOX_API_KEY")?;
            let report = execute_native_startup_parity_dry_run(base_url, api_key).await?;
            println!("{}", serde_json::to_string(&report)?);
            return Ok(());
        }
        if startup.execution
            == luminus_server::ExperimentalRuntimeExecution::MigrationReadinessDryRun
        {
            let base_url = std::env::var("BLACKBOX_BASE_URL")?;
            let api_key = std::env::var("BLACKBOX_API_KEY")?;
            match execute_native_migration_readiness_dry_run(base_url, api_key).await {
                Ok(report) => println!("{}", serde_json::to_string(&report)?),
                Err(luminus_server::NativeMigrationReadinessError::NoGo(report)) => {
                    println!("{}", serde_json::to_string(&report)?);
                    return Err("native migration readiness is not satisfied".into());
                }
                Err(luminus_server::NativeMigrationReadinessError::Preparation(error)) => {
                    return Err(error);
                }
            }
            return Ok(());
        }
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
    let (account_pool, router) = match (
        std::env::var("BLACKBOX_BASE_URL"),
        std::env::var("BLACKBOX_API_KEY"),
    ) {
        (Ok(base_url), Ok(api_key)) => luminus_server::prepare_current_runtime(base_url, api_key)?,
        _ => (
            Arc::new(AccountPool::new()),
            LuminusRouter::new(Arc::new(ProviderRegistry::new()), None),
        ),
    };
    drop(account_pool);
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
