pub mod app;
pub mod legacy;
pub mod routes;

use serde::Serialize;
use std::sync::{Arc, Mutex};

use luminus_composition::{BlackboxAccountHydrator, BlackboxCredentials, BlackboxProviderConfig};
use luminus_core::model::{AccountDescriptor, AccountId, ProviderId};
use luminus_legacy_composition::LegacyByokBlackboxHydrator;
use luminus_legacy_credentials::LegacyPasswordReader;
use luminus_legacy_provider_config::LegacyByokConfigResolver;
use luminus_provider_config::{
    ProviderConfigError, ProviderConfigRequest, ProviderConfigResolver,
    ProviderConfigResolverFuture,
};
use luminus_providers::{BlackboxConfig, BlackboxProvider};
use luminus_router::{AccountPool, ProviderAccount, ProviderRegistry, Router as LuminusRouter};
use luminus_runtime_bootstrap::{
    BlackboxRuntimeBootstrap, NativeOnlyRuntimeBootstrap, RuntimeSnapshot,
};
use luminus_secrets::{
    CredentialRequest, CredentialResolver, CredentialResolverFuture, SecretError, SecretString,
};
use luminus_storage::{MemoryAccountRepository, StoredAccount};
use luminus_storage_sqlite::LegacyTsAccountRepository;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ConfigurationOrigin {
    #[serde(rename = "environment")]
    Environment,
    #[serde(rename = "built-in")]
    BuiltIn,
    #[serde(rename = "explicit-experimental")]
    ExplicitExperimental,
    #[serde(rename = "disabled")]
    Disabled,
    #[serde(rename = "not-applicable")]
    NotApplicable,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ValidationStatus {
    #[serde(rename = "passed")]
    Passed,
    #[serde(rename = "not-applicable")]
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NativeConfigurationDiagnostics {
    pub metadata_origin: ConfigurationOrigin,
    pub provider_config_origin: ConfigurationOrigin,
    pub credential_origin: ConfigurationOrigin,
    pub validation: ValidationStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LegacyConfigurationDiagnostics {
    pub activation_origin: ConfigurationOrigin,
    pub database_origin: ConfigurationOrigin,
    pub credential_key_origin: ConfigurationOrigin,
    pub source_order_origin: ConfigurationOrigin,
    pub validation: ValidationStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConfigurationDiagnostics {
    pub startup_config: ValidationStatus,
    pub native: NativeConfigurationDiagnostics,
    pub legacy: LegacyConfigurationDiagnostics,
    pub legacy_preflight: ValidationStatus,
    pub runtime_snapshot: ValidationStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExperimentalRuntimeDiagnostics {
    pub ready: bool,
    pub runtime_mode: &'static str,
    pub runtime_accounts: usize,
    pub native_hydrated: usize,
    pub native_failed: usize,
    pub legacy_enabled: bool,
    pub legacy_preflight: &'static str,
    pub legacy_hydrated: usize,
    pub legacy_failed: usize,
    pub legacy_skipped: usize,
    pub source_order: Option<&'static str>,
    pub configuration: ConfigurationDiagnostics,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct StartupParityReport {
    pub equivalent: bool,
    pub account_count_matches: bool,
    pub account_identity_order_matches: bool,
    pub provider_identity_order_matches: bool,
    pub enabled_state_matches: bool,
    pub routing_policy_matches: bool,
    pub selection_policy_matches: bool,
    pub fresh_health_state_matches: bool,
}

pub fn prepare_current_runtime(
    base_url: String,
    api_key: String,
) -> Result<(Arc<AccountPool>, LuminusRouter), Box<dyn std::error::Error>> {
    let provider = Arc::new(BlackboxProvider::new(BlackboxConfig::new(
        base_url, api_key,
    ))?);
    let mut pool = AccountPool::new();
    pool.register(ProviderAccount {
        descriptor: AccountDescriptor {
            id: AccountId::from("blackbox-default"),
            provider: ProviderId::from("blackbox"),
            enabled: true,
        },
        adapter: provider,
    })?;
    let accounts = Arc::new(pool);
    let router = LuminusRouter::new(
        Arc::new(ProviderRegistry::new()),
        Some(ProviderId::from("blackbox")),
    )
    .with_accounts(accounts.clone());
    Ok((accounts, router))
}

pub fn audit_native_startup_parity(
    current_pool: &AccountPool,
    current_router: &LuminusRouter,
    experimental: &PreparedExperimentalRuntime,
) -> StartupParityReport {
    let provider = ProviderId::from("blackbox");
    let current_ids = current_pool.ordered_ids_for_provider(&provider);
    let experimental_ids = experimental
        .snapshot
        .account_pool
        .ordered_ids_for_provider(&provider);
    let current_enabled: Vec<bool> = current_ids
        .iter()
        .filter_map(|id| current_pool.get(id))
        .map(|a| a.descriptor.enabled)
        .collect();
    let experimental_enabled: Vec<bool> = experimental_ids
        .iter()
        .filter_map(|id| experimental.snapshot.account_pool.get(id))
        .map(|a| a.descriptor.enabled)
        .collect();
    let (current_max_attempts, current_fallback, current_selection, current_fresh) =
        current_router.runtime_invariants();
    let (
        experimental_max_attempts,
        experimental_fallback,
        experimental_selection,
        experimental_fresh,
    ) = experimental.snapshot.router.runtime_invariants();
    let account_count_matches = current_ids.len() == experimental_ids.len();
    let account_identity_order_matches = current_ids == experimental_ids;
    let provider_identity_order_matches = current_ids.iter().all(|id| {
        current_pool
            .get(id)
            .is_some_and(|a| a.descriptor.provider == provider)
    }) && experimental_ids.iter().all(|id| {
        experimental
            .snapshot
            .account_pool
            .get(id)
            .is_some_and(|a| a.descriptor.provider == provider)
    });
    let enabled_state_matches = current_enabled == experimental_enabled;
    let routing_policy_matches = current_max_attempts == experimental_max_attempts
        && current_fallback == experimental_fallback;
    let selection_policy_matches = current_selection == experimental_selection;
    let fresh_health_state_matches = current_fresh && experimental_fresh;
    let equivalent = account_count_matches
        && account_identity_order_matches
        && provider_identity_order_matches
        && enabled_state_matches
        && routing_policy_matches
        && selection_policy_matches
        && fresh_health_state_matches;
    StartupParityReport {
        equivalent,
        account_count_matches,
        account_identity_order_matches,
        provider_identity_order_matches,
        enabled_state_matches,
        routing_policy_matches,
        selection_policy_matches,
        fresh_health_state_matches,
    }
}

pub struct RuntimeConfigurationProvenance {
    pub native_metadata: ConfigurationOrigin,
    pub native_provider_config: ConfigurationOrigin,
    pub native_credentials: ConfigurationOrigin,
    pub legacy_activation: ConfigurationOrigin,
    pub legacy_database: ConfigurationOrigin,
    pub legacy_credential_key: ConfigurationOrigin,
    pub legacy_source_order: ConfigurationOrigin,
}

impl RuntimeConfigurationProvenance {
    pub fn environment_native() -> Self {
        Self {
            native_metadata: ConfigurationOrigin::BuiltIn,
            native_provider_config: ConfigurationOrigin::Environment,
            native_credentials: ConfigurationOrigin::Environment,
            legacy_activation: ConfigurationOrigin::NotApplicable,
            legacy_database: ConfigurationOrigin::NotApplicable,
            legacy_credential_key: ConfigurationOrigin::NotApplicable,
            legacy_source_order: ConfigurationOrigin::NotApplicable,
        }
    }

    pub fn with_explicit_legacy(mut self) -> Self {
        self.legacy_activation = ConfigurationOrigin::ExplicitExperimental;
        self.legacy_database = ConfigurationOrigin::ExplicitExperimental;
        self.legacy_credential_key = ConfigurationOrigin::ExplicitExperimental;
        self.legacy_source_order = ConfigurationOrigin::ExplicitExperimental;
        self
    }
}

pub fn experimental_diagnostics(
    snapshot: &RuntimeSnapshot,
    legacy_enabled: bool,
) -> ExperimentalRuntimeDiagnostics {
    experimental_diagnostics_with_provenance(
        snapshot,
        legacy_enabled,
        RuntimeConfigurationProvenance::environment_native(),
    )
}

pub fn experimental_diagnostics_with_provenance(
    snapshot: &RuntimeSnapshot,
    legacy_enabled: bool,
    provenance: RuntimeConfigurationProvenance,
) -> ExperimentalRuntimeDiagnostics {
    use luminus_legacy_composition::LegacyHydrationOutcome;
    let entries = &snapshot.report.legacy_blackbox.entries;
    let legacy_hydrated = entries
        .iter()
        .filter(|e| matches!(e.outcome, LegacyHydrationOutcome::HydratedBlackbox))
        .count();
    let legacy_skipped = entries
        .iter()
        .filter(|e| {
            matches!(
                e.outcome,
                LegacyHydrationOutcome::SkippedOpenAiCompatible
                    | LegacyHydrationOutcome::SkippedUnresolved
            )
        })
        .count();
    let legacy_failed = entries
        .len()
        .saturating_sub(legacy_hydrated + legacy_skipped);
    let source_order = match snapshot.report.source_order {
        luminus_runtime_bootstrap::BlackboxSourceOrder::NativeThenLegacy => {
            Some("native-then-legacy")
        }
        luminus_runtime_bootstrap::BlackboxSourceOrder::LegacyThenNative if legacy_enabled => {
            Some("legacy-then-native")
        }
        _ => None,
    };
    let runtime_accounts = snapshot
        .account_pool
        .ordered_ids_for_provider(&luminus_core::model::ProviderId::from("blackbox"))
        .len();
    ExperimentalRuntimeDiagnostics {
        ready: true,
        runtime_mode: "experimental-bootstrap",
        runtime_accounts,
        native_hydrated: runtime_accounts.saturating_sub(legacy_hydrated),
        native_failed: snapshot.report.native_blackbox.failures.len(),
        legacy_enabled,
        legacy_preflight: if legacy_enabled {
            "passed"
        } else {
            "not-applicable"
        },
        legacy_hydrated,
        legacy_failed: if legacy_enabled { legacy_failed } else { 0 },
        legacy_skipped: if legacy_enabled { legacy_skipped } else { 0 },
        source_order,
        configuration: ConfigurationDiagnostics {
            startup_config: ValidationStatus::Passed,
            native: NativeConfigurationDiagnostics {
                metadata_origin: provenance.native_metadata,
                provider_config_origin: provenance.native_provider_config,
                credential_origin: provenance.native_credentials,
                validation: ValidationStatus::Passed,
            },
            legacy: LegacyConfigurationDiagnostics {
                activation_origin: if legacy_enabled {
                    provenance.legacy_activation
                } else {
                    ConfigurationOrigin::Disabled
                },
                database_origin: if legacy_enabled {
                    provenance.legacy_database
                } else {
                    ConfigurationOrigin::NotApplicable
                },
                credential_key_origin: if legacy_enabled {
                    provenance.legacy_credential_key
                } else {
                    ConfigurationOrigin::NotApplicable
                },
                source_order_origin: if legacy_enabled {
                    provenance.legacy_source_order
                } else {
                    ConfigurationOrigin::NotApplicable
                },
                validation: if legacy_enabled {
                    ValidationStatus::Passed
                } else {
                    ValidationStatus::NotApplicable
                },
            },
            legacy_preflight: if legacy_enabled {
                ValidationStatus::Passed
            } else {
                ValidationStatus::NotApplicable
            },
            runtime_snapshot: ValidationStatus::Passed,
        },
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExperimentalRuntimeExecution {
    Serve,
    DryRun,
    ParityDryRun,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid LUMINUS_EXPERIMENTAL_RUNTIME_DRY_RUN value")]
pub struct DryRunModeError;

impl ExperimentalRuntimeExecution {
    pub fn parse(value: Option<&str>) -> Result<Self, DryRunModeError> {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            None | Some("false") | Some("off") | Some("0") => Ok(Self::Serve),
            Some("true") | Some("on") | Some("1") => Ok(Self::DryRun),
            Some(_) => Err(DryRunModeError),
        }
    }
}

#[derive(Debug)]
pub struct ServerStartupConfig {
    pub runtime_mode: RuntimeStartupMode,
    pub execution: ExperimentalRuntimeExecution,
    pub legacy: Option<legacy::ExperimentalLegacySourceConfig>,
}

pub fn parse_startup_config(
    runtime_value: Option<&str>,
    legacy_flag: Option<&str>,
    legacy_path: Option<&str>,
    legacy_key: Option<&str>,
    source_order: Option<&str>,
    dry_run_value: Option<&str>,
) -> Result<ServerStartupConfig, Box<dyn std::error::Error>> {
    parse_startup_config_with_parity(
        runtime_value,
        legacy_flag,
        legacy_path,
        legacy_key,
        source_order,
        dry_run_value,
        None,
    )
}

pub fn parse_startup_config_with_parity(
    runtime_value: Option<&str>,
    legacy_flag: Option<&str>,
    legacy_path: Option<&str>,
    legacy_key: Option<&str>,
    source_order: Option<&str>,
    dry_run_value: Option<&str>,
    parity_dry_run_value: Option<&str>,
) -> Result<ServerStartupConfig, Box<dyn std::error::Error>> {
    let runtime_mode = RuntimeStartupMode::parse(runtime_value)?;
    let ordinary_dry_run = ExperimentalRuntimeExecution::parse(dry_run_value)?;
    let parity_dry_run = match parity_dry_run_value
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        None | Some("false") | Some("off") | Some("0") => false,
        Some("true") | Some("on") | Some("1") => true,
        Some(_) => return Err("invalid LUMINUS_EXPERIMENTAL_PARITY_DRY_RUN value".into()),
    };
    if parity_dry_run && ordinary_dry_run == ExperimentalRuntimeExecution::DryRun {
        return Err("parity dry-run cannot be combined with runtime dry-run".into());
    }
    let execution = if parity_dry_run {
        ExperimentalRuntimeExecution::ParityDryRun
    } else {
        ordinary_dry_run
    };
    if matches!(
        execution,
        ExperimentalRuntimeExecution::DryRun | ExperimentalRuntimeExecution::ParityDryRun
    ) && runtime_mode != RuntimeStartupMode::ExperimentalBootstrap
    {
        return Err("dry-run requires experimental runtime bootstrap".into());
    }
    let legacy = legacy::parse_config(
        runtime_mode,
        legacy_flag,
        legacy_path,
        legacy_key,
        source_order,
    )?;
    if execution == ExperimentalRuntimeExecution::ParityDryRun && legacy.is_some() {
        return Err("parity dry-run requires legacy compatibility to be disabled".into());
    }
    Ok(ServerStartupConfig {
        runtime_mode,
        execution,
        legacy,
    })
}

pub struct PreparedExperimentalRuntime {
    pub snapshot: RuntimeSnapshot,
    pub diagnostics: ExperimentalRuntimeDiagnostics,
}

#[derive(Debug)]
pub enum ExperimentalRuntimeExecutionOutcome {
    DryRun(ExperimentalRuntimeDiagnostics),
    ParityDryRun(StartupParityReport),
    Serving {
        listener: tokio::net::TcpListener,
        app: axum::Router,
    },
}

pub async fn execute_prepared_experimental_runtime(
    prepared: PreparedExperimentalRuntime,
    execution: ExperimentalRuntimeExecution,
    address: &str,
) -> Result<ExperimentalRuntimeExecutionOutcome, Box<dyn std::error::Error>> {
    match execution {
        ExperimentalRuntimeExecution::DryRun => Ok(ExperimentalRuntimeExecutionOutcome::DryRun(
            prepared.diagnostics,
        )),
        ExperimentalRuntimeExecution::ParityDryRun => {
            Err("parity dry-run must be executed through the top-level startup path".into())
        }
        ExperimentalRuntimeExecution::Serve => {
            let listener = tokio::net::TcpListener::bind(address).await?;
            let app = app::experimental_app_with_diagnostics(
                Arc::new(prepared.snapshot.router),
                Arc::new(prepared.diagnostics),
            );
            Ok(ExperimentalRuntimeExecutionOutcome::Serving { listener, app })
        }
    }
}

pub async fn prepare_experimental_runtime(
    base_url: String,
    api_key: String,
    legacy: Option<legacy::ExperimentalLegacySourceConfig>,
) -> Result<PreparedExperimentalRuntime, Box<dyn std::error::Error>> {
    prepare_experimental_runtime_with_provenance(
        base_url,
        api_key,
        legacy,
        RuntimeConfigurationProvenance::environment_native(),
    )
    .await
}

pub async fn prepare_experimental_runtime_with_provenance(
    base_url: String,
    api_key: String,
    legacy: Option<legacy::ExperimentalLegacySourceConfig>,
    provenance: RuntimeConfigurationProvenance,
) -> Result<PreparedExperimentalRuntime, Box<dyn std::error::Error>> {
    let legacy_enabled = legacy.is_some();
    let snapshot = match legacy {
        Some(config) => build_experimental_snapshot_with_legacy(base_url, api_key, config).await?,
        None => build_experimental_snapshot(base_url, api_key).await?,
    };
    let provenance = if legacy_enabled {
        provenance.with_explicit_legacy()
    } else {
        provenance
    };
    let diagnostics =
        experimental_diagnostics_with_provenance(&snapshot, legacy_enabled, provenance);
    Ok(PreparedExperimentalRuntime {
        snapshot,
        diagnostics,
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
    fn dry_run_mode_parsing_and_current_mode_rejection_are_strict() {
        assert_eq!(
            ExperimentalRuntimeExecution::parse(None),
            Ok(ExperimentalRuntimeExecution::Serve)
        );
        assert_eq!(
            ExperimentalRuntimeExecution::parse(Some("off")),
            Ok(ExperimentalRuntimeExecution::Serve)
        );
        assert_eq!(
            ExperimentalRuntimeExecution::parse(Some("on")),
            Ok(ExperimentalRuntimeExecution::DryRun)
        );
        assert!(ExperimentalRuntimeExecution::parse(Some("maybe")).is_err());
        assert!(parse_startup_config(None, None, None, None, None, Some("true")).is_err());
    }

    #[test]
    fn parity_dry_run_requires_experimental_native_only_startup() {
        for value in ["true", "on", "1"] {
            let config = parse_startup_config_with_parity(
                Some("true"),
                None,
                None,
                None,
                None,
                None,
                Some(value),
            )
            .expect("parity dry-run should parse");
            assert_eq!(config.execution, ExperimentalRuntimeExecution::ParityDryRun);
            assert!(config.legacy.is_none());
        }
        assert!(
            parse_startup_config_with_parity(None, None, None, None, None, None, Some("true"))
                .is_err()
        );
        assert!(
            parse_startup_config_with_parity(
                Some("true"),
                None,
                None,
                None,
                None,
                Some("true"),
                Some("true")
            )
            .is_err()
        );
        assert!(
            parse_startup_config_with_parity(
                Some("true"),
                Some("true"),
                Some("db"),
                Some("key"),
                Some("legacy-then-native"),
                None,
                Some("true")
            )
            .is_err()
        );
        assert!(
            parse_startup_config_with_parity(
                Some("true"),
                None,
                None,
                None,
                None,
                None,
                Some("maybe")
            )
            .is_err()
        );
    }

    #[test]
    fn startup_config_validates_legacy_before_dispatch() {
        let error = parse_startup_config(Some("false"), Some("true"), None, None, None, None)
            .expect_err("legacy must not be accepted by current startup");
        assert!(!error.to_string().contains("SYNTHETIC"));

        assert!(parse_startup_config(None, None, Some("synthetic.db"), None, None, None).is_err());
        assert!(parse_startup_config(None, None, None, Some("synthetic-key"), None, None).is_err());
        assert!(
            parse_startup_config(None, None, None, None, Some("native-then-legacy"), None).is_err()
        );
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
            assert!(
                parse_startup_config(input.0, input.1, input.2, input.3, input.4, None).is_err()
            );
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
            None,
        )
        .unwrap();
        assert_eq!(
            config.runtime_mode,
            RuntimeStartupMode::ExperimentalBootstrap
        );
        assert!(config.legacy.is_some());
    }

    #[tokio::test]
    async fn native_startup_parity_is_safe_and_equivalent() {
        let (current_pool, current_router) = prepare_current_runtime(
            "http://127.0.0.1:9".into(),
            "SYNTHETIC_R30_API_KEY_DO_NOT_LEAK".into(),
        )
        .unwrap();
        let experimental = prepare_experimental_runtime(
            "http://127.0.0.1:9".into(),
            "SYNTHETIC_R30_API_KEY_DO_NOT_LEAK".into(),
            None,
        )
        .await
        .unwrap();
        let report = audit_native_startup_parity(&current_pool, &current_router, &experimental);
        assert!(report.equivalent);
        assert!(report.account_count_matches);
        assert!(report.account_identity_order_matches);
        assert!(report.provider_identity_order_matches);
        assert!(report.enabled_state_matches);
        assert!(report.routing_policy_matches);
        assert!(report.selection_policy_matches);
        assert!(report.fresh_health_state_matches);
        let safe = format!("{report:?} {}", serde_json::to_string(&report).unwrap());
        assert!(!safe.contains("SYNTHETIC_R30_API_KEY_DO_NOT_LEAK"));
        assert!(!safe.contains("127.0.0.1:9"));
        assert!(!safe.contains("blackbox-default"));
    }

    #[tokio::test]
    async fn native_startup_parity_detects_account_mismatch() {
        let (current_pool, current_router) = prepare_current_runtime(
            "http://127.0.0.1:9".into(),
            "SYNTHETIC_R30_API_KEY_DO_NOT_LEAK".into(),
        )
        .unwrap();
        let mut experimental = prepare_experimental_runtime(
            "http://127.0.0.1:9".into(),
            "SYNTHETIC_R30_API_KEY_DO_NOT_LEAK".into(),
            None,
        )
        .await
        .unwrap();
        experimental.snapshot.account_pool = std::sync::Arc::new(AccountPool::new());
        let report = audit_native_startup_parity(&current_pool, &current_router, &experimental);
        assert!(!report.equivalent);
        assert!(!report.account_count_matches);
        assert!(!report.account_identity_order_matches);
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
    async fn native_diagnostics_report_safe_provenance_and_validation() {
        let snapshot = build_experimental_snapshot(
            "http://127.0.0.1:1".into(),
            "SYNTHETIC_R29_NATIVE_API_KEY_DO_NOT_LEAK".into(),
        )
        .await
        .unwrap();
        let diagnostics = experimental_diagnostics(&snapshot, false);
        assert_eq!(
            diagnostics.configuration.startup_config,
            ValidationStatus::Passed
        );
        assert_eq!(
            diagnostics.configuration.native.metadata_origin,
            ConfigurationOrigin::BuiltIn
        );
        assert_eq!(
            diagnostics.configuration.native.provider_config_origin,
            ConfigurationOrigin::Environment
        );
        assert_eq!(
            diagnostics.configuration.native.credential_origin,
            ConfigurationOrigin::Environment
        );
        assert_eq!(
            diagnostics.configuration.legacy.validation,
            ValidationStatus::NotApplicable
        );
        assert_eq!(
            diagnostics.configuration.legacy_preflight,
            ValidationStatus::NotApplicable
        );
        assert_eq!(
            diagnostics.configuration.runtime_snapshot,
            ValidationStatus::Passed
        );
        assert_eq!(
            diagnostics.configuration.legacy.database_origin,
            ConfigurationOrigin::NotApplicable
        );
        let body = serde_json::to_string(&diagnostics).unwrap();
        assert!(!body.contains("SYNTHETIC_R29_NATIVE_API_KEY_DO_NOT_LEAK"));
        assert!(!body.contains("127.0.0.1:1"));
    }

    #[tokio::test]
    async fn explicit_legacy_diagnostics_report_safe_provenance() {
        let snapshot = build_experimental_snapshot("http://127.0.0.1:1".into(), "synthetic".into())
            .await
            .unwrap();
        let diagnostics = experimental_diagnostics_with_provenance(
            &snapshot,
            true,
            RuntimeConfigurationProvenance::environment_native().with_explicit_legacy(),
        );
        assert_eq!(
            diagnostics.configuration.legacy.activation_origin,
            ConfigurationOrigin::ExplicitExperimental
        );
        assert_eq!(
            diagnostics.configuration.legacy.database_origin,
            ConfigurationOrigin::ExplicitExperimental
        );
        assert_eq!(
            diagnostics.configuration.legacy.credential_key_origin,
            ConfigurationOrigin::ExplicitExperimental
        );
        assert_eq!(
            diagnostics.configuration.legacy.source_order_origin,
            ConfigurationOrigin::ExplicitExperimental
        );
        assert_eq!(
            diagnostics.configuration.legacy.validation,
            ValidationStatus::Passed
        );
        assert_eq!(
            diagnostics.configuration.legacy_preflight,
            ValidationStatus::Passed
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
    async fn dry_run_execution_does_not_bind_occupied_address() {
        let prepared = prepare_experimental_runtime(
            "http://127.0.0.1:1".into(),
            "SYNTHETIC_R28_NATIVE_API_KEY_DO_NOT_LEAK".into(),
            None,
        )
        .await
        .unwrap();
        let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = occupied.local_addr().unwrap().to_string();
        let outcome = execute_prepared_experimental_runtime(
            prepared,
            ExperimentalRuntimeExecution::DryRun,
            &address,
        )
        .await
        .unwrap();
        let ExperimentalRuntimeExecutionOutcome::DryRun(diagnostics) = outcome else {
            panic!("dry-run must not produce a serving outcome");
        };
        assert_eq!(diagnostics.runtime_accounts, 1);
        assert_eq!(diagnostics.native_hydrated, 1);
        assert!(!diagnostics.legacy_enabled);
        assert!(!format!("{diagnostics:?}").contains("SYNTHETIC_R28_NATIVE_API_KEY_DO_NOT_LEAK"));
        drop(occupied);
    }

    #[tokio::test]
    async fn serve_execution_reaches_bind_boundary() {
        let prepared = prepare_experimental_runtime(
            "http://127.0.0.1:1".into(),
            "SYNTHETIC_R28_NATIVE_API_KEY_DO_NOT_LEAK".into(),
            None,
        )
        .await
        .unwrap();
        let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = occupied.local_addr().unwrap().to_string();
        let error = execute_prepared_experimental_runtime(
            prepared,
            ExperimentalRuntimeExecution::Serve,
            &address,
        )
        .await
        .expect_err("serve must attempt the occupied bind");
        assert!(
            !error
                .to_string()
                .contains("SYNTHETIC_R28_NATIVE_API_KEY_DO_NOT_LEAK")
        );
        drop(occupied);
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
