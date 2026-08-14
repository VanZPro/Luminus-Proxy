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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum MigrationReadinessDecision {
    #[serde(rename = "go")]
    Go,
    #[serde(rename = "no-go")]
    NoGo,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum NativeMigrationReadinessReason {
    #[serde(rename = "experimental-snapshot-not-ready")]
    ExperimentalSnapshotNotReady,
    #[serde(rename = "native-parity-mismatch")]
    NativeParityMismatch,
    #[serde(rename = "startup-validation-not-passed")]
    StartupValidationNotPassed,
    #[serde(rename = "native-validation-not-passed")]
    NativeValidationNotPassed,
    #[serde(rename = "runtime-snapshot-validation-not-passed")]
    RuntimeSnapshotValidationNotPassed,
    #[serde(rename = "legacy-outside-native-scope")]
    LegacyOutsideNativeScope,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NativeMigrationReadinessReport {
    pub decision: MigrationReadinessDecision,
    pub experimental_snapshot_ready: bool,
    pub native_startup_parity: bool,
    pub startup_configuration_validated: bool,
    pub native_configuration_validated: bool,
    pub runtime_snapshot_validated: bool,
    pub native_scope_valid: bool,
    pub reasons: Vec<NativeMigrationReadinessReason>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum NativeMigrationCutoverEligibility {
    #[serde(rename = "eligible")]
    Eligible,
    #[serde(rename = "ineligible")]
    Ineligible,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NativeMigrationCutoverEligibilityReport {
    pub eligibility: NativeMigrationCutoverEligibility,
    pub readiness_decision: MigrationReadinessDecision,
    pub readiness_reasons: Vec<NativeMigrationReadinessReason>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum NativeMigrationCutoverDirection {
    #[serde(rename = "current-to-experimental-native")]
    CurrentToExperimentalNative,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum NativeMigrationCutoverStep {
    ConfirmEligibility,
    StopCurrentRuntime,
    SelectExperimentalBootstrap,
    StartExperimentalRuntime,
    VerifyHealth,
    VerifyExperimentalReadiness,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum NativeMigrationRollbackStep {
    StopExperimentalRuntime,
    RestoreCurrentStartupSelection,
    StartCurrentRuntime,
    VerifyCurrentHealth,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NativeMigrationCutoverPlan {
    pub direction: NativeMigrationCutoverDirection,
    pub steps: Vec<NativeMigrationCutoverStep>,
    pub rollback_steps: Vec<NativeMigrationRollbackStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeMigrationCutoverPlanError {
    Ineligible,
    InconsistentEligibility,
}

pub fn build_native_migration_cutover_plan(
    eligibility: &NativeMigrationCutoverEligibilityReport,
) -> Result<NativeMigrationCutoverPlan, NativeMigrationCutoverPlanError> {
    let consistent = match eligibility.eligibility {
        NativeMigrationCutoverEligibility::Eligible => {
            eligibility.readiness_decision == MigrationReadinessDecision::Go
                && eligibility.readiness_reasons.is_empty()
        }
        NativeMigrationCutoverEligibility::Ineligible => {
            eligibility.readiness_decision == MigrationReadinessDecision::NoGo
        }
    };
    if !consistent {
        return Err(NativeMigrationCutoverPlanError::InconsistentEligibility);
    }
    if eligibility.eligibility == NativeMigrationCutoverEligibility::Ineligible {
        return Err(NativeMigrationCutoverPlanError::Ineligible);
    }
    Ok(NativeMigrationCutoverPlan {
        direction: NativeMigrationCutoverDirection::CurrentToExperimentalNative,
        steps: vec![
            NativeMigrationCutoverStep::ConfirmEligibility,
            NativeMigrationCutoverStep::StopCurrentRuntime,
            NativeMigrationCutoverStep::SelectExperimentalBootstrap,
            NativeMigrationCutoverStep::StartExperimentalRuntime,
            NativeMigrationCutoverStep::VerifyHealth,
            NativeMigrationCutoverStep::VerifyExperimentalReadiness,
        ],
        rollback_steps: vec![
            NativeMigrationRollbackStep::StopExperimentalRuntime,
            NativeMigrationRollbackStep::RestoreCurrentStartupSelection,
            NativeMigrationRollbackStep::StartCurrentRuntime,
            NativeMigrationRollbackStep::VerifyCurrentHealth,
        ],
    })
}

pub fn assess_native_migration_cutover_eligibility(
    readiness: &NativeMigrationReadinessReport,
) -> NativeMigrationCutoverEligibilityReport {
    let eligibility =
        if readiness.decision == MigrationReadinessDecision::Go && readiness.reasons.is_empty() {
            NativeMigrationCutoverEligibility::Eligible
        } else {
            NativeMigrationCutoverEligibility::Ineligible
        };

    NativeMigrationCutoverEligibilityReport {
        eligibility,
        readiness_decision: readiness.decision,
        readiness_reasons: readiness.reasons.clone(),
    }
}

pub fn assess_native_migration_readiness(
    diagnostics: &ExperimentalRuntimeDiagnostics,
    parity: &StartupParityReport,
) -> NativeMigrationReadinessReport {
    let experimental_snapshot_ready =
        diagnostics.ready && diagnostics.runtime_mode == "experimental-bootstrap";
    let startup_configuration_validated =
        diagnostics.configuration.startup_config == ValidationStatus::Passed;
    let native_configuration_validated =
        diagnostics.configuration.native.validation == ValidationStatus::Passed;
    let runtime_snapshot_validated =
        diagnostics.configuration.runtime_snapshot == ValidationStatus::Passed;
    let native_scope_valid = !diagnostics.legacy_enabled
        && diagnostics.configuration.legacy.validation == ValidationStatus::NotApplicable
        && diagnostics.configuration.legacy_preflight == ValidationStatus::NotApplicable;

    let mut reasons = Vec::new();
    if !experimental_snapshot_ready {
        reasons.push(NativeMigrationReadinessReason::ExperimentalSnapshotNotReady);
    }
    if !parity.equivalent {
        reasons.push(NativeMigrationReadinessReason::NativeParityMismatch);
    }
    if !startup_configuration_validated {
        reasons.push(NativeMigrationReadinessReason::StartupValidationNotPassed);
    }
    if !native_configuration_validated {
        reasons.push(NativeMigrationReadinessReason::NativeValidationNotPassed);
    }
    if !runtime_snapshot_validated {
        reasons.push(NativeMigrationReadinessReason::RuntimeSnapshotValidationNotPassed);
    }
    if !native_scope_valid {
        reasons.push(NativeMigrationReadinessReason::LegacyOutsideNativeScope);
    }

    NativeMigrationReadinessReport {
        decision: if reasons.is_empty() {
            MigrationReadinessDecision::Go
        } else {
            MigrationReadinessDecision::NoGo
        },
        experimental_snapshot_ready,
        native_startup_parity: parity.equivalent,
        startup_configuration_validated,
        native_configuration_validated,
        runtime_snapshot_validated,
        native_scope_valid,
        reasons,
    }
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
    MigrationReadinessDryRun,
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
    parse_startup_config_with_readiness(
        runtime_value,
        legacy_flag,
        legacy_path,
        legacy_key,
        source_order,
        dry_run_value,
        parity_dry_run_value,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn parse_startup_config_with_readiness(
    runtime_value: Option<&str>,
    legacy_flag: Option<&str>,
    legacy_path: Option<&str>,
    legacy_key: Option<&str>,
    source_order: Option<&str>,
    dry_run_value: Option<&str>,
    parity_dry_run_value: Option<&str>,
    readiness_dry_run_value: Option<&str>,
) -> Result<ServerStartupConfig, Box<dyn std::error::Error>> {
    let runtime_mode = RuntimeStartupMode::parse(runtime_value)?;
    let ordinary_dry_run = ExperimentalRuntimeExecution::parse(dry_run_value)?;
    let readiness_dry_run = match readiness_dry_run_value
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        None | Some("false") | Some("off") | Some("0") => false,
        Some("true") | Some("on") | Some("1") => true,
        Some(_) => {
            return Err("invalid LUMINUS_EXPERIMENTAL_MIGRATION_READINESS_DRY_RUN value".into());
        }
    };
    let parity_dry_run = match parity_dry_run_value
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        None | Some("false") | Some("off") | Some("0") => false,
        Some("true") | Some("on") | Some("1") => true,
        Some(_) => return Err("invalid LUMINUS_EXPERIMENTAL_PARITY_DRY_RUN value".into()),
    };
    if (parity_dry_run || readiness_dry_run)
        && ordinary_dry_run == ExperimentalRuntimeExecution::DryRun
        || (readiness_dry_run && parity_dry_run)
    {
        return Err("offline dry-run modes are mutually exclusive".into());
    }
    let execution = if readiness_dry_run {
        ExperimentalRuntimeExecution::MigrationReadinessDryRun
    } else if parity_dry_run {
        ExperimentalRuntimeExecution::ParityDryRun
    } else {
        ordinary_dry_run
    };
    if matches!(
        execution,
        ExperimentalRuntimeExecution::DryRun
            | ExperimentalRuntimeExecution::ParityDryRun
            | ExperimentalRuntimeExecution::MigrationReadinessDryRun
    ) && runtime_mode != RuntimeStartupMode::ExperimentalBootstrap
    {
        return Err("dry-run requires experimental runtime bootstrap".into());
    }
    if matches!(
        execution,
        ExperimentalRuntimeExecution::ParityDryRun
            | ExperimentalRuntimeExecution::MigrationReadinessDryRun
    ) && (legacy_flag.is_some()
        || legacy_path.is_some()
        || legacy_key.is_some()
        || source_order.is_some())
    {
        return Err(if execution == ExperimentalRuntimeExecution::ParityDryRun {
            "parity dry-run requires legacy compatibility to be disabled"
        } else {
            "migration-readiness dry-run requires legacy compatibility to be disabled"
        }
        .into());
    }
    let legacy = legacy::parse_config(
        runtime_mode,
        legacy_flag,
        legacy_path,
        legacy_key,
        source_order,
    )?;
    if matches!(
        execution,
        ExperimentalRuntimeExecution::ParityDryRun
            | ExperimentalRuntimeExecution::MigrationReadinessDryRun
    ) && legacy.is_some()
    {
        return Err(if execution == ExperimentalRuntimeExecution::ParityDryRun {
            "parity dry-run requires legacy compatibility to be disabled"
        } else {
            "migration-readiness dry-run requires legacy compatibility to be disabled"
        }
        .into());
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

#[derive(Debug, thiserror::Error)]
pub enum NativeStartupParityError {
    #[error("native startup parity mismatch")]
    Mismatch(StartupParityReport),
    #[error(transparent)]
    Preparation(#[from] Box<dyn std::error::Error>),
}

pub async fn execute_native_startup_parity_dry_run(
    base_url: String,
    api_key: String,
) -> Result<StartupParityReport, NativeStartupParityError> {
    let (_, report) = build_native_startup_evidence(base_url, api_key)
        .await
        .map_err(NativeStartupParityError::Preparation)?;
    if report.equivalent {
        Ok(report)
    } else {
        Err(NativeStartupParityError::Mismatch(report))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NativeMigrationReadinessError {
    #[error("native migration readiness is not satisfied")]
    NoGo(NativeMigrationReadinessReport),
    #[error(transparent)]
    Preparation(#[from] Box<dyn std::error::Error>),
}

async fn build_native_startup_evidence(
    base_url: String,
    api_key: String,
) -> Result<(ExperimentalRuntimeDiagnostics, StartupParityReport), Box<dyn std::error::Error>> {
    let (current_pool, current_router) =
        prepare_current_runtime(base_url.clone(), api_key.clone())?;
    let experimental = prepare_experimental_runtime(base_url, api_key, None).await?;
    let parity = audit_native_startup_parity(&current_pool, &current_router, &experimental);
    Ok((experimental.diagnostics, parity))
}

pub async fn execute_native_migration_readiness_dry_run(
    base_url: String,
    api_key: String,
) -> Result<NativeMigrationReadinessReport, NativeMigrationReadinessError> {
    let (diagnostics, parity) = build_native_startup_evidence(base_url, api_key)
        .await
        .map_err(NativeMigrationReadinessError::Preparation)?;
    let report = assess_native_migration_readiness(&diagnostics, &parity);
    if report.decision == MigrationReadinessDecision::Go {
        Ok(report)
    } else {
        Err(NativeMigrationReadinessError::NoGo(report))
    }
}

#[derive(Debug)]
pub enum ExperimentalRuntimeExecutionOutcome {
    DryRun(ExperimentalRuntimeDiagnostics),
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
        ExperimentalRuntimeExecution::ParityDryRun
        | ExperimentalRuntimeExecution::MigrationReadinessDryRun => Err(
            "offline parity/readiness dry-run must be executed through the top-level startup path"
                .into(),
        ),
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

    fn matching_parity_report() -> StartupParityReport {
        StartupParityReport {
            equivalent: true,
            account_count_matches: true,
            account_identity_order_matches: true,
            provider_identity_order_matches: true,
            enabled_state_matches: true,
            routing_policy_matches: true,
            selection_policy_matches: true,
            fresh_health_state_matches: true,
        }
    }

    #[tokio::test]
    async fn native_migration_readiness_real_evidence_is_go_and_offline() {
        let (current_pool, current_router) = prepare_current_runtime(
            "http://127.0.0.1:9".into(),
            "SYNTHETIC_R32_API_KEY_DO_NOT_LEAK".into(),
        )
        .unwrap();
        let experimental = prepare_experimental_runtime(
            "http://127.0.0.1:9".into(),
            "SYNTHETIC_R32_API_KEY_DO_NOT_LEAK".into(),
            None,
        )
        .await
        .unwrap();
        let parity = audit_native_startup_parity(&current_pool, &current_router, &experimental);
        let report = assess_native_migration_readiness(&experimental.diagnostics, &parity);
        assert_eq!(report.decision, MigrationReadinessDecision::Go);
        assert!(report.reasons.is_empty());
        let eligibility = assess_native_migration_cutover_eligibility(&report);
        assert_eq!(
            eligibility.eligibility,
            NativeMigrationCutoverEligibility::Eligible
        );
        let plan = build_native_migration_cutover_plan(&eligibility).unwrap();
        assert_eq!(
            plan.direction,
            NativeMigrationCutoverDirection::CurrentToExperimentalNative
        );
        assert_eq!(
            report,
            assess_native_migration_readiness(&experimental.diagnostics, &parity)
        );
        let json = serde_json::to_string(&report).unwrap();
        for forbidden in [
            "SYNTHETIC_R32_API_KEY_DO_NOT_LEAK",
            "127.0.0.1:9",
            "blackbox-default",
            "Authorization",
        ] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn native_migration_readiness_collects_safe_no_go_reasons_deterministically() {
        let mut diagnostics = ExperimentalRuntimeDiagnostics {
            ready: true,
            runtime_mode: "experimental-bootstrap",
            runtime_accounts: 1,
            native_hydrated: 1,
            native_failed: 0,
            legacy_enabled: false,
            legacy_preflight: "not-applicable",
            legacy_hydrated: 0,
            legacy_failed: 0,
            legacy_skipped: 0,
            source_order: None,
            configuration: ConfigurationDiagnostics {
                startup_config: ValidationStatus::Passed,
                native: NativeConfigurationDiagnostics {
                    metadata_origin: ConfigurationOrigin::Environment,
                    provider_config_origin: ConfigurationOrigin::Environment,
                    credential_origin: ConfigurationOrigin::Environment,
                    validation: ValidationStatus::Passed,
                },
                legacy: LegacyConfigurationDiagnostics {
                    activation_origin: ConfigurationOrigin::NotApplicable,
                    database_origin: ConfigurationOrigin::NotApplicable,
                    credential_key_origin: ConfigurationOrigin::NotApplicable,
                    source_order_origin: ConfigurationOrigin::NotApplicable,
                    validation: ValidationStatus::NotApplicable,
                },
                legacy_preflight: ValidationStatus::NotApplicable,
                runtime_snapshot: ValidationStatus::Passed,
            },
        };
        let parity = matching_parity_report();
        diagnostics.ready = false;
        diagnostics.configuration.startup_config = ValidationStatus::NotApplicable;
        diagnostics.configuration.native.validation = ValidationStatus::NotApplicable;
        diagnostics.configuration.runtime_snapshot = ValidationStatus::NotApplicable;
        diagnostics.legacy_enabled = true;
        diagnostics.configuration.legacy.validation = ValidationStatus::Passed;
        diagnostics.configuration.legacy_preflight = ValidationStatus::Passed;
        let report = assess_native_migration_readiness(&diagnostics, &parity);
        assert_eq!(report.decision, MigrationReadinessDecision::NoGo);
        assert_eq!(
            report.reasons,
            vec![
                NativeMigrationReadinessReason::ExperimentalSnapshotNotReady,
                NativeMigrationReadinessReason::StartupValidationNotPassed,
                NativeMigrationReadinessReason::NativeValidationNotPassed,
                NativeMigrationReadinessReason::RuntimeSnapshotValidationNotPassed,
                NativeMigrationReadinessReason::LegacyOutsideNativeScope,
            ]
        );
    }

    #[test]
    fn native_migration_readiness_parity_mismatch_is_no_go() {
        let diagnostics = ExperimentalRuntimeDiagnostics {
            ready: true,
            runtime_mode: "experimental-bootstrap",
            runtime_accounts: 1,
            native_hydrated: 1,
            native_failed: 0,
            legacy_enabled: false,
            legacy_preflight: "not-applicable",
            legacy_hydrated: 0,
            legacy_failed: 0,
            legacy_skipped: 0,
            source_order: None,
            configuration: ConfigurationDiagnostics {
                startup_config: ValidationStatus::Passed,
                native: NativeConfigurationDiagnostics {
                    metadata_origin: ConfigurationOrigin::Environment,
                    provider_config_origin: ConfigurationOrigin::Environment,
                    credential_origin: ConfigurationOrigin::Environment,
                    validation: ValidationStatus::Passed,
                },
                legacy: LegacyConfigurationDiagnostics {
                    activation_origin: ConfigurationOrigin::NotApplicable,
                    database_origin: ConfigurationOrigin::NotApplicable,
                    credential_key_origin: ConfigurationOrigin::NotApplicable,
                    source_order_origin: ConfigurationOrigin::NotApplicable,
                    validation: ValidationStatus::NotApplicable,
                },
                legacy_preflight: ValidationStatus::NotApplicable,
                runtime_snapshot: ValidationStatus::Passed,
            },
        };
        let mut parity = matching_parity_report();
        parity.equivalent = false;
        let report = assess_native_migration_readiness(&diagnostics, &parity);
        assert_eq!(report.decision, MigrationReadinessDecision::NoGo);
        assert_eq!(
            report.reasons,
            vec![NativeMigrationReadinessReason::NativeParityMismatch]
        );
    }

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
    fn migration_readiness_flag_matrix_and_conflicts_are_strict() {
        for value in [None, Some("false"), Some("off"), Some("0")] {
            let config = parse_startup_config_with_readiness(
                Some("true"),
                None,
                None,
                None,
                None,
                None,
                None,
                value,
            )
            .expect("disabled readiness flag should preserve serve mode");
            assert_eq!(config.execution, ExperimentalRuntimeExecution::Serve);
        }
        for value in ["true", "on", "1"] {
            let config = parse_startup_config_with_readiness(
                Some("true"),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(value),
            )
            .expect("enabled readiness flag should parse");
            assert_eq!(
                config.execution,
                ExperimentalRuntimeExecution::MigrationReadinessDryRun
            );
        }
        assert!(
            parse_startup_config_with_readiness(
                Some("true"),
                None,
                None,
                None,
                None,
                None,
                None,
                Some("maybe")
            )
            .is_err()
        );
        assert!(
            parse_startup_config_with_readiness(
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some("true")
            )
            .is_err()
        );
        assert!(
            parse_startup_config_with_readiness(
                Some("true"),
                None,
                None,
                None,
                None,
                Some("true"),
                None,
                Some("true")
            )
            .is_err()
        );
        assert!(
            parse_startup_config_with_readiness(
                Some("true"),
                None,
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
            parse_startup_config_with_readiness(
                Some("true"),
                Some("true"),
                Some("nonexistent.db"),
                Some("key"),
                Some("native"),
                None,
                None,
                Some("true")
            )
            .is_err()
        );
    }

    #[test]
    fn native_cutover_eligibility_maps_no_go_reasons_and_fails_closed() {
        let no_go = NativeMigrationReadinessReport {
            decision: MigrationReadinessDecision::NoGo,
            experimental_snapshot_ready: true,
            native_startup_parity: false,
            startup_configuration_validated: true,
            native_configuration_validated: true,
            runtime_snapshot_validated: true,
            native_scope_valid: true,
            reasons: vec![NativeMigrationReadinessReason::NativeParityMismatch],
        };
        let legacy_no_go = NativeMigrationReadinessReport {
            reasons: vec![NativeMigrationReadinessReason::LegacyOutsideNativeScope],
            ..no_go.clone()
        };
        let inconsistent_go = NativeMigrationReadinessReport {
            decision: MigrationReadinessDecision::Go,
            reasons: vec![NativeMigrationReadinessReason::NativeParityMismatch],
            ..no_go.clone()
        };

        for report in [&no_go, &legacy_no_go, &inconsistent_go] {
            let result = assess_native_migration_cutover_eligibility(report);
            assert_eq!(
                result.eligibility,
                NativeMigrationCutoverEligibility::Ineligible
            );
            assert_eq!(result.readiness_decision, report.decision);
            assert_eq!(result.readiness_reasons, report.reasons);
        }
    }

    #[test]
    fn native_cutover_plan_builds_deterministically_from_real_eligible_report() {
        let readiness = NativeMigrationReadinessReport {
            decision: MigrationReadinessDecision::Go,
            experimental_snapshot_ready: true,
            native_startup_parity: true,
            startup_configuration_validated: true,
            native_configuration_validated: true,
            runtime_snapshot_validated: true,
            native_scope_valid: true,
            reasons: Vec::new(),
        };
        let eligibility = assess_native_migration_cutover_eligibility(&readiness);
        let first = build_native_migration_cutover_plan(&eligibility).unwrap();
        let second = build_native_migration_cutover_plan(&eligibility).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.direction,
            NativeMigrationCutoverDirection::CurrentToExperimentalNative
        );
        assert_eq!(
            first.steps,
            vec![
                NativeMigrationCutoverStep::ConfirmEligibility,
                NativeMigrationCutoverStep::StopCurrentRuntime,
                NativeMigrationCutoverStep::SelectExperimentalBootstrap,
                NativeMigrationCutoverStep::StartExperimentalRuntime,
                NativeMigrationCutoverStep::VerifyHealth,
                NativeMigrationCutoverStep::VerifyExperimentalReadiness,
            ]
        );
        assert_eq!(
            first.rollback_steps,
            vec![
                NativeMigrationRollbackStep::StopExperimentalRuntime,
                NativeMigrationRollbackStep::RestoreCurrentStartupSelection,
                NativeMigrationRollbackStep::StartCurrentRuntime,
                NativeMigrationRollbackStep::VerifyCurrentHealth,
            ]
        );
        let json = serde_json::to_string(&first).unwrap();
        for forbidden in [
            "SYNTHETIC_R35_API_KEY_DO_NOT_LEAK",
            "http://127.0.0.1",
            "blackbox-default",
            "Authorization",
            "set ",
            "export ",
            "cmd.exe",
            "powershell",
            "cargo run",
            "--experimental",
            "timestamp",
            "hostname",
            "pid",
        ] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn native_cutover_plan_rejects_ineligible_and_inconsistent_reports() {
        let no_go = NativeMigrationCutoverEligibilityReport {
            eligibility: NativeMigrationCutoverEligibility::Ineligible,
            readiness_decision: MigrationReadinessDecision::NoGo,
            readiness_reasons: vec![NativeMigrationReadinessReason::NativeParityMismatch],
        };
        assert_eq!(
            build_native_migration_cutover_plan(&no_go),
            Err(NativeMigrationCutoverPlanError::Ineligible)
        );
        let legacy = NativeMigrationCutoverEligibilityReport {
            readiness_reasons: vec![NativeMigrationReadinessReason::LegacyOutsideNativeScope],
            ..no_go.clone()
        };
        assert_eq!(
            build_native_migration_cutover_plan(&legacy),
            Err(NativeMigrationCutoverPlanError::Ineligible)
        );
        let inconsistent = NativeMigrationCutoverEligibilityReport {
            eligibility: NativeMigrationCutoverEligibility::Eligible,
            ..no_go
        };
        assert_eq!(
            build_native_migration_cutover_plan(&inconsistent),
            Err(NativeMigrationCutoverPlanError::InconsistentEligibility)
        );
    }

    #[test]
    fn native_cutover_eligibility_is_deterministic_immutable_and_safe() {
        let readiness = NativeMigrationReadinessReport {
            decision: MigrationReadinessDecision::Go,
            experimental_snapshot_ready: true,
            native_startup_parity: true,
            startup_configuration_validated: true,
            native_configuration_validated: true,
            runtime_snapshot_validated: true,
            native_scope_valid: true,
            reasons: Vec::new(),
        };
        let first = assess_native_migration_cutover_eligibility(&readiness);
        let second = assess_native_migration_cutover_eligibility(&readiness);
        assert_eq!(first, second);
        assert_eq!(
            first.eligibility,
            NativeMigrationCutoverEligibility::Eligible
        );
        assert_eq!(readiness.decision, MigrationReadinessDecision::Go);
        let serialized = serde_json::to_string(&first).unwrap();
        for forbidden in [
            "SYNTHETIC_R34_API_KEY_DO_NOT_LEAK",
            "blackbox-default",
            "Authorization",
            "http://127.0.0.1",
            "fingerprint",
            "timestamp",
            "hostname",
            "pid",
        ] {
            assert!(!serialized.contains(forbidden));
        }
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
    async fn native_startup_parity_execution_boundary_is_safe_and_equivalent() {
        let report = execute_native_startup_parity_dry_run(
            "http://127.0.0.1:9".into(),
            "SYNTHETIC_R31B_API_KEY_DO_NOT_LEAK".into(),
        )
        .await
        .expect("native parity execution should succeed");
        assert!(report.equivalent);
        let json = serde_json::to_string(&report).unwrap();
        for forbidden in [
            "SYNTHETIC_R31B_API_KEY_DO_NOT_LEAK",
            "127.0.0.1:9",
            "blackbox-default",
            "Authorization",
            "fingerprint",
            "email",
        ] {
            assert!(!json.contains(forbidden));
        }
        assert!(json.contains("equivalent"));
        assert!(json.contains("account_count_matches"));
        assert!(json.contains("fresh_health_state_matches"));
    }

    #[test]
    fn native_startup_parity_mismatch_is_safe_non_success() {
        let report = StartupParityReport {
            equivalent: false,
            account_count_matches: false,
            account_identity_order_matches: true,
            provider_identity_order_matches: true,
            enabled_state_matches: true,
            routing_policy_matches: true,
            selection_policy_matches: true,
            fresh_health_state_matches: true,
        };
        let error = NativeStartupParityError::Mismatch(report);
        assert!(error.to_string().contains("parity mismatch"));
        match error {
            NativeStartupParityError::Mismatch(report) => assert!(!report.equivalent),
            NativeStartupParityError::Preparation(_) => panic!("unexpected preparation error"),
        }
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
