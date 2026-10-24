//! Tauri commands exposed to the web view. Each returns `AppResult<T>`; the
//! error side serializes to the shared structured error contract.
//!
//! Mutating commands exist only for state the UI manages in this step
//! (settings, credentials, diagnostics). Repository, campaign, agent,
//! verification and pattern data is read-only from the web view until the
//! features that create it are built.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Runtime, State};

use crate::credentials::{self, CredentialMetadata, KeychainStatus, Secret};
use crate::db::activity::{self, ActivityEvent, ActivityQuery, NewEvent, Severity};
use crate::db::agents::{self, AgentTask, TaskQuery};
use crate::db::campaigns::{self, Campaign, CampaignRepository};
use crate::db::credentials::{CredentialKind, Provider};
use crate::db::migrations::{self, AppliedMigration};
use crate::db::patterns::{self, MigrationPattern};
use crate::db::repositories::{self, Repository};
use crate::db::roots::{self, RepositoryRoot};
use crate::db::settings::{self, ExecutionLimits};
use crate::db::verification::{self, RunQuery, VerificationRun};
use crate::db::{ListParams, Page};
use crate::discovery::{
    DiscoveryMode, DiscoveryRequest, DiscoveryResult, ImportSummary, TauriDiscoverySink,
    DEFAULT_MAX_ENTRIES,
};
use crate::environment::{self, Report, Status};
use crate::error::{AppError, AppResult};
use crate::process::{
    ActiveProcess, CancelOutcome, CommandCategory, CommandSpec, Program, TauriSink,
};
use crate::shell_path;
use crate::AppState;

// ---------------------------------------------------------------- app & settings

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    data_dir: String,
    log_dir: String,
    database_path: String,
    schema_version: u32,
    latest_schema_version: u32,
}

#[tauri::command]
pub fn get_app_info<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<AppInfo> {
    let schema_version = state.db.read(migrations::user_version)?;
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: state.data_dir.display().to_string(),
        log_dir: state.log_dir.display().to_string(),
        database_path: state
            .db
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        schema_version,
        latest_schema_version: migrations::latest_version(),
    })
}

/// All stored settings as raw JSON strings keyed by setting name.
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<BTreeMap<String, String>> {
    state.db.read(settings::all)
}

#[tauri::command]
pub fn set_setting(key: String, value: String, state: State<'_, AppState>) -> AppResult<()> {
    state.db.write(|c| settings::set(c, &key, &value))
}

#[tauri::command]
pub fn get_execution_limits(state: State<'_, AppState>) -> AppResult<ExecutionLimits> {
    state.db.read(settings::execution_limits)
}

// ---------------------------------------------------------------- health

#[tauri::command]
pub async fn run_environment_checks(state: State<'_, AppState>) -> AppResult<Report> {
    let report = environment::run_all(&state.data_dir, state.credentials.clone()).await;
    let count = |s: Status| report.checks.iter().filter(|c| c.status == s).count();
    let (available, unavailable, errors) = (
        count(Status::Available),
        count(Status::Unavailable),
        count(Status::Error),
    );
    state.db.write(|c| {
        activity::record(
            c,
            NewEvent::new(
                "environment",
                "environment.checks_completed",
                if errors > 0 { Severity::Warning } else { Severity::Info },
                format!("Environment checks completed: {available} available, {unavailable} unavailable, {errors} errors"),
            )
            .metadata(json!({
                "available": available,
                "unavailable": unavailable,
                "errors": errors,
                "checks": report.checks.iter().map(|c| json!({ "id": c.id, "status": c.status, "version": c.version })).collect::<Vec<_>>(),
            })),
        )
    })?;
    Ok(report)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseHealth {
    path: String,
    size_bytes: Option<u64>,
    schema_version: u32,
    latest_schema_version: u32,
    up_to_date: bool,
    applied_migrations: Vec<AppliedMigration>,
    foreign_keys_enabled: bool,
    journal_mode: String,
    quick_check: String,
    activity_event_count: u64,
}

#[tauri::command]
pub fn get_database_health(state: State<'_, AppState>) -> AppResult<DatabaseHealth> {
    let path = state.db.path().map(|p| p.to_path_buf());
    state.db.read(|c| {
        let schema_version = migrations::user_version(c)?;
        let fk: i64 = c.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
        let journal_mode: String = c.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
        let quick_check: String = c.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        Ok(DatabaseHealth {
            path: path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            size_bytes: path
                .as_ref()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len()),
            schema_version,
            latest_schema_version: migrations::latest_version(),
            up_to_date: schema_version == migrations::latest_version(),
            applied_migrations: migrations::applied(c)?,
            foreign_keys_enabled: fk == 1,
            journal_mode,
            quick_check,
            activity_event_count: activity::count(c)?,
        })
    })
}

#[tauri::command]
pub async fn get_keychain_status(state: State<'_, AppState>) -> AppResult<KeychainStatus> {
    let store = state.credentials.clone();
    tokio::task::spawn_blocking(move || store.probe())
        .await
        .map_err(|e| AppError::internal(format!("keychain probe task failed: {e}")))
}

// ---------------------------------------------------------------- credentials

fn parse_slot(provider: &str, kind: &str) -> AppResult<(Provider, CredentialKind)> {
    Ok((Provider::parse(provider)?, CredentialKind::parse(kind)?))
}

#[tauri::command]
pub async fn list_credentials(state: State<'_, AppState>) -> AppResult<Vec<CredentialMetadata>> {
    let (db, store) = (state.db.clone(), state.credentials.clone());
    tokio::task::spawn_blocking(move || credentials::list(&db, &store))
        .await
        .map_err(|e| AppError::internal(format!("credential task failed: {e}")))?
}

/// Store or replace a credential. Returns metadata only; the value is never
/// sent back to the web view.
#[tauri::command]
pub async fn store_credential(
    provider: String,
    kind: String,
    secret: String,
    state: State<'_, AppState>,
) -> AppResult<CredentialMetadata> {
    let secret = Secret::new(secret);
    let (provider, kind) = parse_slot(&provider, &kind)?;
    let (db, store) = (state.db.clone(), state.credentials.clone());
    tokio::task::spawn_blocking(move || {
        credentials::store_credential(&db, &store, provider, kind, secret)
    })
    .await
    .map_err(|e| AppError::internal(format!("credential task failed: {e}")))?
}

/// `confirm` must be true: the UI asks the user before calling this.
#[tauri::command]
pub async fn delete_credential(
    provider: String,
    kind: String,
    confirm: bool,
    state: State<'_, AppState>,
) -> AppResult<CredentialMetadata> {
    if !confirm {
        return Err(AppError::validation(
            "Deleting a credential requires confirmation.",
        ));
    }
    let (provider, kind) = parse_slot(&provider, &kind)?;
    let (db, store) = (state.db.clone(), state.credentials.clone());
    tokio::task::spawn_blocking(move || credentials::delete_credential(&db, &store, provider, kind))
        .await
        .map_err(|e| AppError::internal(format!("credential task failed: {e}")))?
}

// ---------------------------------------------------------------- diagnostics

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    GitVersion,
    NodeVersion,
    PnpmVersion,
    LongRunning,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedProcess {
    run_id: String,
}

/// Start one of a fixed set of diagnostics. There is no way to pass an
/// arbitrary program or argument from the web view.
#[tauri::command]
pub async fn start_diagnostic<R: Runtime>(
    kind: DiagnosticKind,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<StartedProcess> {
    let limits = state.db.read(settings::execution_limits)?;
    let mut spec = match kind {
        DiagnosticKind::GitVersion => CommandSpec::new(
            "git --version",
            CommandCategory::EnvironmentCheck,
            Program::Named("git".into()),
            &["--version"],
        ),
        DiagnosticKind::NodeVersion => CommandSpec::new(
            "node --version",
            CommandCategory::EnvironmentCheck,
            Program::Named("node".into()),
            &["--version"],
        ),
        DiagnosticKind::PnpmVersion => CommandSpec::new(
            "pnpm --version",
            CommandCategory::EnvironmentCheck,
            Program::Named("pnpm".into()),
            &["--version"],
        ),
        DiagnosticKind::LongRunning => {
            let (program, args) = &state.diagnostic_program;
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            CommandSpec::new(
                "Diagnostic ticker (60 s)",
                CommandCategory::EnvironmentCheck,
                Program::Path(program.clone()),
                &args,
            )
        }
    };
    spec.timeout = Duration::from_secs(limits.timeout_seconds);
    spec.max_output_bytes = usize::try_from(limits.max_output_bytes).unwrap_or(usize::MAX);
    let search = shell_path::search_path().await;
    let started = state.runner.start(spec, search, Arc::new(TauriSink(app)))?;
    Ok(StartedProcess {
        run_id: started.run_id,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelResponse {
    run_id: String,
    outcome: CancelOutcome,
}

#[tauri::command]
pub fn cancel_process(run_id: String, state: State<'_, AppState>) -> AppResult<CancelResponse> {
    let outcome = state.runner.cancel(&run_id);
    Ok(CancelResponse { run_id, outcome })
}

#[tauri::command]
pub fn list_active_processes(state: State<'_, AppState>) -> AppResult<Vec<ActiveProcess>> {
    Ok(state.runner.active())
}

// ---------------------------------------------------------------- activity & domain reads

#[tauri::command]
pub fn list_activity(
    query: ActivityQuery,
    state: State<'_, AppState>,
) -> AppResult<Page<ActivityEvent>> {
    state.db.read(|c| activity::list(c, &query))
}

#[tauri::command]
pub fn list_activity_entity_types(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    state.db.read(activity::entity_types)
}

#[tauri::command]
pub fn list_repositories(
    params: ListParams,
    state: State<'_, AppState>,
) -> AppResult<Page<Repository>> {
    state.db.read(|c| repositories::list(c, &params))
}

#[tauri::command]
pub fn get_repository(id: String, state: State<'_, AppState>) -> AppResult<Repository> {
    state.db.read(|c| repositories::get(c, &id))
}

#[tauri::command]
pub fn list_campaigns(params: ListParams, state: State<'_, AppState>) -> AppResult<Page<Campaign>> {
    state.db.read(|c| campaigns::list(c, &params))
}

#[tauri::command]
pub fn get_campaign(id: String, state: State<'_, AppState>) -> AppResult<Campaign> {
    state.db.read(|c| campaigns::get(c, &id))
}

#[tauri::command]
pub fn list_campaign_repositories(
    campaign_id: String,
    params: ListParams,
    state: State<'_, AppState>,
) -> AppResult<Page<CampaignRepository>> {
    state
        .db
        .read(|c| campaigns::list_members(c, &campaign_id, &params))
}

#[tauri::command]
pub fn list_agent_tasks(
    query: TaskQuery,
    state: State<'_, AppState>,
) -> AppResult<Page<AgentTask>> {
    state.db.read(|c| agents::list_tasks(c, &query))
}

#[tauri::command]
pub fn list_verification_runs(
    query: RunQuery,
    state: State<'_, AppState>,
) -> AppResult<Page<VerificationRun>> {
    state.db.read(|c| verification::list_runs(c, &query))
}

#[tauri::command]
pub fn list_migration_patterns(
    params: ListParams,
    state: State<'_, AppState>,
) -> AppResult<Page<MigrationPattern>> {
    state.db.read(|c| patterns::list(c, &params))
}

// ---------------------------------------------------------------- external links

/// Open a validated external URL in the system browser.
#[tauri::command]
pub fn open_external_url<R: Runtime>(url: String, app: AppHandle<R>) -> AppResult<()> {
    let url = crate::urls::validate_external_url(&url)?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|e| AppError::internal(format!("opening URL failed: {e}")))
}

// ---------------------------------------------------------------- local discovery

/// Native folder picker. Runs in Rust; the web view has no dialog permission.
#[tauri::command]
pub async fn pick_directory<R: Runtime>(
    title: String,
    app: AppHandle<R>,
) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let title: String = title.chars().take(120).collect();
    tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title(title)
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.display().to_string())
    })
    .await
    .map_err(|e| AppError::internal(format!("folder picker task failed: {e}")))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDiscoveryArgs {
    path: String,
    mode: DiscoveryMode,
    max_depth: u32,
    follow_internal_symlinks: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedDiscovery {
    discovery_id: String,
}

#[tauri::command]
pub async fn start_discovery<R: Runtime>(
    request: StartDiscoveryArgs,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<StartedDiscovery> {
    let search = shell_path::search_path().await;
    let (discovery_id, _handle) = state.discoveries.start(
        DiscoveryRequest {
            path: request.path,
            mode: request.mode,
            max_depth: request.max_depth,
            follow_internal_symlinks: request.follow_internal_symlinks,
            max_entries: DEFAULT_MAX_ENTRIES,
        },
        state.db.clone(),
        search,
        Arc::new(TauriDiscoverySink(app)),
    )?;
    Ok(StartedDiscovery { discovery_id })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelDiscoveryResponse {
    discovery_id: String,
    outcome: CancelOutcome,
}

#[tauri::command]
pub fn cancel_discovery(
    discovery_id: String,
    state: State<'_, AppState>,
) -> AppResult<CancelDiscoveryResponse> {
    let outcome = if state.discoveries.cancel(&discovery_id) {
        CancelOutcome::Requested
    } else {
        CancelOutcome::NotRunning
    };
    Ok(CancelDiscoveryResponse {
        discovery_id,
        outcome,
    })
}

/// The finished result, or `null` while the discovery is still running.
#[tauri::command]
pub fn get_discovery(
    discovery_id: String,
    state: State<'_, AppState>,
) -> AppResult<Option<DiscoveryResult>> {
    state.discoveries.result(&discovery_id)
}

#[tauri::command]
pub fn import_discovered(
    discovery_id: String,
    paths: Vec<String>,
    state: State<'_, AppState>,
) -> AppResult<ImportSummary> {
    state.discoveries.import(&state.db, &discovery_id, &paths)
}

#[tauri::command]
pub fn list_repository_roots(state: State<'_, AppState>) -> AppResult<Vec<RepositoryRoot>> {
    state.db.read(roots::list)
}

/// Forget a root. Repositories stay registered and no file is touched.
#[tauri::command]
pub fn remove_repository_root(
    id: String,
    confirm: bool,
    state: State<'_, AppState>,
) -> AppResult<RepositoryRoot> {
    if !confirm {
        return Err(AppError::validation(
            "Removing a root requires confirmation.",
        ));
    }
    state.db.write(|c| {
        let root = roots::remove(c, &id)?;
        activity::record(
            c,
            NewEvent::new(
                "repository_root",
                "repository_root.removed",
                Severity::Notice,
                format!(
                    "Stopped tracking {} (no files were deleted)",
                    root.display_name
                ),
            )
            .entity(root.id.clone())
            .metadata(
                json!({ "path": root.canonical_path, "repositories": root.repository_count }),
            ),
        )?;
        Ok(root)
    })
}

/// Local branches of a registered repository, via read-only `git for-each-ref`.
#[tauri::command]
pub async fn list_repository_branches(
    id: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::git::inspect::BranchInfo>> {
    let repo = state.db.read(|c| repositories::get(c, &id))?;
    let path = repo
        .local_path
        .ok_or_else(|| AppError::validation("This repository has no local checkout."))?;
    let canonical = crate::discovery::validate_root(&path)?;
    let runner = crate::process::ProcessRunner::new(
        crate::process::ExecutionPolicy::readonly_git(vec![canonical.clone()]),
        None,
        None,
    );
    let search = shell_path::search_path().await;
    crate::git::inspect::branches(&runner, search, &canonical)
        .await
        .map_err(|e| {
            AppError::new(
                crate::error::ErrorCode::CommandFailed,
                "Branches could not be read.",
            )
            .with_details(e)
        })
}

// ---------------------------------------------------------------- repository intelligence

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedScan {
    repository_id: String,
}

/// Start a read-only intelligence scan. `force` ignores the cache.
#[tauri::command]
pub async fn start_repository_scan<R: Runtime>(
    repository_id: String,
    force: bool,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<StartedScan> {
    let search = shell_path::search_path().await;
    state.scans.start(
        &repository_id,
        force,
        state.db.clone(),
        search,
        Arc::new(crate::scanner::TauriScanSink(app)),
    )?;
    Ok(StartedScan { repository_id })
}

#[tauri::command]
pub fn cancel_repository_scan(
    repository_id: String,
    state: State<'_, AppState>,
) -> AppResult<CancelDiscoveryResponse> {
    let outcome = if state.scans.cancel(&repository_id) {
        CancelOutcome::Requested
    } else {
        CancelOutcome::NotRunning
    };
    Ok(CancelDiscoveryResponse {
        discovery_id: repository_id,
        outcome,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceResponse {
    #[serde(flatten)]
    intelligence: crate::db::intelligence::Intelligence,
    scan_running: bool,
}

#[tauri::command]
pub fn get_repository_intelligence(
    repository_id: String,
    state: State<'_, AppState>,
) -> AppResult<IntelligenceResponse> {
    let intelligence = state
        .db
        .read(|c| crate::db::intelligence::latest(c, &repository_id))?;
    Ok(IntelligenceResponse {
        intelligence,
        scan_running: state.scans.is_running(&repository_id),
    })
}

#[tauri::command]
pub fn list_repository_scans(
    repository_id: String,
    params: ListParams,
    state: State<'_, AppState>,
) -> AppResult<Page<repositories::RepositoryScan>> {
    state
        .db
        .read(|c| repositories::list_scans(c, &repository_id, &params))
}
