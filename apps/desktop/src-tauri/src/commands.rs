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
use crate::db::intelligence;
use crate::db::migrations::{self, AppliedMigration};
use crate::db::patterns::{self, MigrationPattern};
use crate::db::repositories::{self, Repository};
use crate::db::roots::{self, RepositoryRoot};
use crate::db::settings::{self, ExecutionLimits};
use crate::db::verification::{self, RunQuery, VerificationRun};
use crate::db::{ListParams, Page};
use crate::devin::{self, DevinStatus};
use crate::discovery::{
    DiscoveryMode, DiscoveryRequest, DiscoveryResult, ImportSummary, TauriDiscoverySink,
    DEFAULT_MAX_ENTRIES,
};
use crate::environment::{self, Report, Status};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::process::{
    ActiveProcess, CancelOutcome, CommandCategory, CommandSpec, ExecutionPolicy, ProcessRunner,
    Program, TauriSink,
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

/// Change trust only after an explicit UI action and record who/what changed.
#[tauri::command]
pub fn set_repository_trust(
    id: String,
    trust: String,
    confirm: bool,
    state: State<'_, AppState>,
) -> AppResult<Repository> {
    if !confirm {
        return Err(AppError::validation(
            "Changing repository trust requires confirmation.",
        ));
    }
    let trust = repositories::TrustStatus::parse(&trust)?;
    state.db.write(|c| {
        let before = repositories::get(c, &id)?;
        let updated = repositories::set_trust(c, &id, trust)?;
        activity::record(
            c,
            NewEvent::new(
                "repository",
                "repository.trust_changed",
                if trust == repositories::TrustStatus::Blocked {
                    Severity::Warning
                } else {
                    Severity::Notice
                },
                format!(
                    "Changed {} trust from {} to {}",
                    before.name,
                    before.trust_status.as_str(),
                    trust.as_str()
                ),
            )
            .entity(id)
            .metadata(json!({ "from": before.trust_status, "to": trust })),
        )?;
        Ok(updated)
    })
}

/// Forget a repository record. This command never touches its checkout.
#[tauri::command]
pub fn remove_repository_record(
    id: String,
    confirm: bool,
    state: State<'_, AppState>,
) -> AppResult<Repository> {
    if !confirm {
        return Err(AppError::validation(
            "Removing a repository record requires confirmation.",
        ));
    }
    state.db.write(|c| {
        let repository = repositories::get(c, &id)?;
        activity::record(
            c,
            NewEvent::new(
                "repository",
                "repository.record_removed",
                Severity::Notice,
                format!(
                    "Forgot repository {} (no files were deleted)",
                    repository.name
                ),
            )
            .metadata(json!({
                "repositoryId": repository.id,
                "localPath": repository.local_path,
                "ownership": repository.ownership,
                "filesDeleted": false,
            })),
        )?;
        repositories::remove(c, &repository.id)?;
        Ok(repository)
    })
}

// ---------------------------------------------------------------- GitHub

fn github_config(state: &AppState) -> AppResult<&crate::github::client::GithubConfig> {
    state.github_config.as_ref().map_err(Clone::clone)
}

#[tauri::command]
pub fn github_status(state: State<'_, AppState>) -> AppResult<crate::github::GithubStatus> {
    crate::github::status(github_config(&state)?, &state.db, &state.credentials)
}

/// Start OAuth device sign-in. Progress arrives as `shiftops://github-auth` events.
#[tauri::command]
pub async fn github_start_device_flow<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<crate::github::DeviceFlowStart> {
    let config = github_config(&state)?.clone();
    let (start, _handle) = state
        .github_flows
        .start(
            config,
            state.db.clone(),
            state.credentials.clone(),
            Arc::new(crate::github::TauriAuthSink(app)),
        )
        .await?;
    Ok(start)
}

#[tauri::command]
pub fn github_cancel_device_flow(
    flow_id: String,
    state: State<'_, AppState>,
) -> AppResult<CancelDiscoveryResponse> {
    let outcome = if state.github_flows.cancel(&flow_id) {
        CancelOutcome::Requested
    } else {
        CancelOutcome::NotRunning
    };
    Ok(CancelDiscoveryResponse {
        discovery_id: flow_id,
        outcome,
    })
}

/// Validate and store a personal access token. The token is never returned.
#[tauri::command]
pub async fn github_connect_pat(
    token: String,
    state: State<'_, AppState>,
) -> AppResult<crate::db::github::GithubAccount> {
    let token = Secret::new(token);
    crate::github::connect_pat(github_config(&state)?, &state.db, &state.credentials, token).await
}

#[tauri::command]
pub async fn github_refresh(
    state: State<'_, AppState>,
) -> AppResult<crate::db::github::GithubAccount> {
    crate::github::refresh(github_config(&state)?, &state.db, &state.credentials).await
}

#[tauri::command]
pub fn github_disconnect(
    confirm: bool,
    state: State<'_, AppState>,
) -> AppResult<Option<crate::db::github::GithubAccount>> {
    if !confirm {
        return Err(AppError::validation(
            "Disconnecting GitHub requires confirmation.",
        ));
    }
    crate::github::disconnect(&state.db, &state.credentials)
}

#[tauri::command]
pub fn github_list_organizations(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::db::github::GithubOrganization>> {
    let account = state.db.read(crate::db::github::active)?.ok_or_else(|| {
        AppError::new(
            crate::error::ErrorCode::GithubAuthenticationFailed,
            "GitHub is not connected.",
        )
    })?;
    state
        .db
        .read(|c| crate::db::github::organizations(c, &account.id))
}

#[tauri::command]
pub async fn github_browse_repositories(
    params: crate::github::BrowseParams,
    request_id: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<crate::github::BrowsePage> {
    let config = github_config(&state)?;
    state
        .github_requests
        .run(
            request_id,
            crate::github::browse(config, &state.db, &state.credentials, &params),
        )
        .await
}

#[tauri::command]
pub async fn github_repository_details(
    owner: String,
    name: String,
    request_id: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<crate::github::RepositoryDetails> {
    let config = github_config(&state)?;
    state
        .github_requests
        .run(
            request_id,
            crate::github::details(config, &state.db, &state.credentials, &owner, &name),
        )
        .await
}

#[tauri::command]
pub async fn github_list_pull_requests(
    owner: String,
    name: String,
    request_id: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::db::github::GithubPullRequest>> {
    let config = github_config(&state)?;
    state
        .github_requests
        .run(
            request_id,
            crate::github::pull_requests(config, &state.db, &state.credentials, &owner, &name),
        )
        .await
}

#[tauri::command]
pub async fn github_get_pull_request(
    owner: String,
    name: String,
    number: i64,
    state: State<'_, AppState>,
) -> AppResult<crate::db::github::GithubPullRequest> {
    crate::github::pull_request(
        github_config(&state)?,
        &state.db,
        &state.credentials,
        &owner,
        &name,
        number,
    )
    .await
}

#[tauri::command]
pub async fn github_get_file(
    owner: String,
    name: String,
    path: String,
    git_ref: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<crate::github::RemoteFile> {
    crate::github::file(
        github_config(&state)?,
        &state.db,
        &state.credentials,
        &owner,
        &name,
        &path,
        git_ref.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn github_rate_limit(state: State<'_, AppState>) -> AppResult<serde_json::Value> {
    crate::github::rate_limit(github_config(&state)?, &state.db, &state.credentials).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubImportResult {
    repository: Repository,
    created: bool,
}

/// Import a browsed GitHub repository as metadata only (no clone).
#[tauri::command]
pub fn github_import_repository(
    github_repository_id: String,
    state: State<'_, AppState>,
) -> AppResult<GithubImportResult> {
    let (repository, created) = crate::github::import(&state.db, &github_repository_id)?;
    Ok(GithubImportResult {
        repository,
        created,
    })
}

#[tauri::command]
pub fn github_cancel_request(
    request_id: String,
    state: State<'_, AppState>,
) -> AppResult<CancelDiscoveryResponse> {
    let outcome = if state.github_requests.cancel(&request_id) {
        CancelOutcome::Requested
    } else {
        CancelOutcome::NotRunning
    };
    Ok(CancelDiscoveryResponse {
        discovery_id: request_id,
        outcome,
    })
}

/// The cached GitHub repository record linked to a ShiftOps repository, if any.
#[tauri::command]
pub fn github_repository_for(
    repository_id: String,
    state: State<'_, AppState>,
) -> AppResult<Option<crate::db::github::GithubRepository>> {
    state.db.read(|c| {
        use rusqlite::OptionalExtension;
        let id: Option<String> = c
            .query_row("SELECT id FROM github_repositories WHERE repository_id = ?1 ORDER BY synced_at DESC LIMIT 1", [&repository_id], |r| r.get(0))
            .optional()?;
        id.map(|id| crate::db::github::get_repository(c, &id)).transpose()
    })
}

// ---------------------------------------------------------------- managed workspaces and GitHub checkout imports

#[tauri::command]
pub fn get_managed_workspace(
    state: State<'_, AppState>,
) -> AppResult<Option<crate::db::workspace::ManagedWorkspace>> {
    state.db.read(crate::db::workspace::active)
}

#[tauri::command]
pub fn approve_managed_workspace(
    path: String,
    state: State<'_, AppState>,
) -> AppResult<crate::db::workspace::ManagedWorkspace> {
    crate::workspace::approve_workspace(&state.db, &path)
}

#[tauri::command]
pub fn preview_managed_clone(
    github_repository_id: String,
    state: State<'_, AppState>,
) -> AppResult<crate::workspace::ClonePreview> {
    crate::workspace::preview(&state.db, github_config(&state)?, &github_repository_id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedClone {
    clone: crate::db::workspace::RepositoryClone,
}

#[tauri::command]
pub async fn start_managed_clone<R: Runtime>(
    github_repository_id: String,
    destination: String,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<StartedClone> {
    let search = shell_path::search_path().await;
    let config = github_config(&state)?.clone();
    let context = crate::workspace::clone::CloneContext {
        db: state.db.clone(),
        config,
        search,
        log_dir: Some(state.log_dir.clone()),
        sink: Arc::new(crate::workspace::clone::TauriCloneSink(app.clone())),
        scans: state.scans.clone(),
        scan_sink: Arc::new(crate::scanner::TauriScanSink(app)),
    };
    let (clone, _job) = state
        .clones
        .start(context, &github_repository_id, &destination)?;
    Ok(StartedClone { clone })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelCloneResponse {
    clone_id: String,
    outcome: CancelOutcome,
}

#[tauri::command]
pub fn cancel_managed_clone(
    clone_id: String,
    state: State<'_, AppState>,
) -> AppResult<CancelCloneResponse> {
    let outcome = if state.clones.cancel(&clone_id) {
        CancelOutcome::Requested
    } else {
        CancelOutcome::NotRunning
    };
    Ok(CancelCloneResponse { clone_id, outcome })
}

#[tauri::command]
pub fn list_repository_clones(
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::db::workspace::RepositoryClone>> {
    state
        .db
        .read(|c| crate::db::workspace::list_clones(c, limit.unwrap_or(25).clamp(1, 100)))
}

#[tauri::command]
pub fn cleanup_managed_clone(
    clone_id: String,
    confirmed_path: String,
    state: State<'_, AppState>,
) -> AppResult<crate::workspace::CleanupResult> {
    crate::workspace::cleanup(&state.db, &clone_id, &confirmed_path)
}

#[tauri::command]
pub async fn inspect_existing_clone(
    github_repository_id: String,
    path: String,
    state: State<'_, AppState>,
) -> AppResult<crate::workspace::link::LinkInspection> {
    let search = shell_path::search_path().await;
    crate::workspace::link::inspect_link(&state.db, search, &github_repository_id, &path)
        .await
        .map(|(inspection, _)| inspection)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedCloneResult {
    repository: Repository,
    inspection: crate::workspace::link::LinkInspection,
    scan_started: bool,
}

#[tauri::command]
pub async fn link_existing_clone<R: Runtime>(
    github_repository_id: String,
    path: String,
    mismatch_confirmation: Option<String>,
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<LinkedCloneResult> {
    let search = shell_path::search_path().await;
    let (repository, inspection) = crate::workspace::link::link(
        &state.db,
        search,
        &github_repository_id,
        &path,
        mismatch_confirmation.as_deref(),
    )
    .await?;
    state.scans.start(
        &repository.id,
        false,
        state.db.clone(),
        search,
        Arc::new(crate::scanner::TauriScanSink(app)),
    )?;
    Ok(LinkedCloneResult {
        repository,
        inspection,
        scan_started: true,
    })
}

#[tauri::command]
pub async fn get_submission_readiness(
    state: State<'_, AppState>,
) -> AppResult<crate::readiness::SubmissionReadiness> {
    crate::readiness::collect(&state.db).await
}

#[tauri::command]
pub fn open_submission_file<R: Runtime>(name: String, app: AppHandle<R>) -> AppResult<()> {
    if !["README.md", "CHANGELOG.md", "AI_USAGE.md", "SUBMISSION.md"].contains(&name.as_str()) {
        return Err(AppError::validation(
            "Only a required submission document can be opened.",
        ));
    }
    let path = crate::readiness::source_root()?.join(name);
    if !path.is_file() {
        return Err(AppError::new(
            crate::error::ErrorCode::RecordNotFound,
            "Submission document does not exist.",
        )
        .with_context("path", path.display().to_string()));
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(|e| AppError::internal(format!("opening submission document failed: {e}")))
}

// ------------------------------------------------------------ campaigns

fn campaign_activity(
    c: &rusqlite::Connection,
    kind: &'static str,
    severity: Severity,
    message: String,
    entity: String,
    metadata: serde_json::Value,
) -> AppResult<()> {
    activity::record(
        c,
        NewEvent::new("campaign", kind, severity, message)
            .entity(entity)
            .metadata(metadata),
    )?;
    Ok(())
}

#[tauri::command]
pub fn create_campaign(
    definition: campaigns::CampaignDefinition,
    repository_ids: Vec<String>,
    state: State<'_, AppState>,
) -> AppResult<Campaign> {
    state.db.write(|c| {
        let campaign = campaigns::create_with_repositories(c, &definition, &repository_ids)?;
        campaign_activity(
            c,
            "campaign.created",
            Severity::Notice,
            format!(
                "Created campaign {} ({} repositories)",
                campaign.name,
                repository_ids.len()
            ),
            campaign.id.clone(),
            json!({ "repositories": repository_ids }),
        )?;
        Ok(campaign)
    })
}

#[tauri::command]
pub fn update_campaign(
    id: String,
    definition: campaigns::CampaignDefinition,
    state: State<'_, AppState>,
) -> AppResult<Campaign> {
    state.db.write(|c| {
        let campaign = campaigns::update_definition(c, &id, &definition)?;
        campaign_activity(
            c,
            "campaign.updated",
            Severity::Notice,
            format!("Updated campaign {}", campaign.name),
            campaign.id.clone(),
            json!({}),
        )?;
        Ok(campaign)
    })
}

#[tauri::command]
pub fn delete_campaign(
    id: String,
    confirm: bool,
    state: State<'_, AppState>,
) -> AppResult<Campaign> {
    if !confirm {
        return Err(AppError::validation(
            "Deleting a campaign requires confirmation.",
        ));
    }
    state.db.write(|c| {
        let campaign = campaigns::get(c, &id)?;
        campaigns::delete(c, &id)?;
        campaign_activity(
            c,
            "campaign.deleted",
            Severity::Warning,
            format!("Deleted campaign {}", campaign.name),
            campaign.id.clone(),
            json!({}),
        )?;
        Ok(campaign)
    })
}

#[tauri::command]
pub fn set_campaign_status(
    id: String,
    status: String,
    state: State<'_, AppState>,
) -> AppResult<Campaign> {
    let to = campaigns::CampaignStatus::parse(&status)?;
    state.db.write(|c| {
        let campaign = campaigns::set_status(c, &id, to)?;
        campaign_activity(
            c,
            "campaign.status_changed",
            Severity::Notice,
            format!("Campaign {} is now {}", campaign.name, to.as_str()),
            campaign.id.clone(),
            json!({ "status": to.as_str() }),
        )?;
        Ok(campaign)
    })
}

#[tauri::command]
pub fn add_campaign_repositories(
    campaign_id: String,
    repository_ids: Vec<String>,
    state: State<'_, AppState>,
) -> AppResult<Campaign> {
    state.db.write(|c| {
        campaigns::add_repositories(c, &campaign_id, &repository_ids)?;
        let campaign = campaigns::get(c, &campaign_id)?;
        campaign_activity(
            c,
            "campaign.repositories_added",
            Severity::Notice,
            format!(
                "Added {} repositories to {}",
                repository_ids.len(),
                campaign.name
            ),
            campaign_id,
            json!({ "repositories": repository_ids }),
        )?;
        Ok(campaign)
    })
}

#[tauri::command]
pub fn remove_campaign_repository(
    member_id: String,
    confirm: bool,
    state: State<'_, AppState>,
) -> AppResult<CampaignRepository> {
    if !confirm {
        return Err(AppError::validation(
            "Removing a repository from a campaign requires confirmation.",
        ));
    }
    state.db.write(|c| {
        let member = campaigns::get_member(c, &member_id)?;
        campaigns::remove_member(c, &member_id)?;
        campaign_activity(
            c,
            "campaign.repository_removed",
            Severity::Notice,
            format!("Removed {} from the campaign", member.repository_name),
            member.campaign_id.clone(),
            json!({ "repositoryId": member.repository_id }),
        )?;
        Ok(member)
    })
}

#[tauri::command]
pub fn exclude_campaign_repository(
    member_id: String,
    reason: String,
    confirm: bool,
    state: State<'_, AppState>,
) -> AppResult<CampaignRepository> {
    if !confirm {
        return Err(AppError::validation(
            "Excluding a repository requires confirmation.",
        ));
    }
    state.db.write(|c| {
        let member = campaigns::exclude_member(c, &member_id, &reason)?;
        campaign_activity(
            c,
            "campaign.repository_excluded",
            Severity::Notice,
            format!("Excluded {} from the campaign", member.repository_name),
            member.campaign_id.clone(),
            json!({ "repositoryId": member.repository_id, "reason": reason }),
        )?;
        Ok(member)
    })
}

#[tauri::command]
pub fn include_campaign_repository(
    member_id: String,
    state: State<'_, AppState>,
) -> AppResult<CampaignRepository> {
    state.db.write(|c| {
        campaigns::set_member_status(c, &member_id, campaigns::MembershipStatus::Pending)
    })
}

#[tauri::command]
pub fn resolve_member_review(
    member_id: String,
    confirm: bool,
    state: State<'_, AppState>,
) -> AppResult<CampaignRepository> {
    if !confirm {
        return Err(AppError::validation(
            "Resolving a review flag requires confirmation.",
        ));
    }
    state.db.write(|c| {
        let member = campaigns::resolve_member_review(c, &member_id)?;
        campaign_activity(
            c,
            "campaign.review_resolved",
            Severity::Notice,
            format!("Cleared the review flag on {}", member.repository_name),
            member.campaign_id.clone(),
            json!({ "repositoryId": member.repository_id }),
        )?;
        Ok(member)
    })
}

// ------------------------------------------------- planning evidence & apply

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEvidenceMember {
    pub member_id: String,
    pub repository_id: String,
    pub name: String,
    pub trust_status: String,
    pub ownership: Option<String>,
    pub has_local_checkout: bool,
    pub uncommitted_changes: Option<u32>,
    pub last_scanned_at: Option<String>,
    pub facts: Vec<intelligence::StoredFact>,
    pub conflicts: Vec<intelligence::StoredConflict>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEvidenceEdge {
    pub from: String,
    pub to: String,
    pub confidence: f64,
    pub confirmation_status: String,
    pub detection_method: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignPlanEvidence {
    pub campaign: Campaign,
    pub members: Vec<PlanEvidenceMember>,
    pub edges: Vec<PlanEvidenceEdge>,
}

#[tauri::command]
pub fn campaign_plan_evidence(
    campaign_id: String,
    state: State<'_, AppState>,
) -> AppResult<CampaignPlanEvidence> {
    state.db.read(|c| {
        let campaign = campaigns::get(c, &campaign_id)?;
        let members = campaigns::all_members(c, &campaign_id)?;
        let mut evidence = Vec::with_capacity(members.len());
        let mut edges = Vec::new();
        let mut seen_edges = std::collections::HashSet::new();
        for member in &members {
            let repo = repositories::get(c, &member.repository_id)?;
            let intel = intelligence::latest(c, &member.repository_id)?;
            let uncommitted = [repo.staged_count, repo.modified_count, repo.untracked_count]
                .into_iter()
                .flatten()
                .reduce(|a, b| a + b);
            let page = repositories::list_dependencies(
                c,
                &member.repository_id,
                &ListParams {
                    limit: Some(200),
                    ..Default::default()
                },
            )?;
            for dep in page.items {
                let key = (
                    dep.source_repository_id.clone(),
                    dep.target_repository_id.clone(),
                    dep.relationship_type.clone(),
                );
                if seen_edges.insert(key) {
                    edges.push(PlanEvidenceEdge {
                        from: dep.source_repository_id,
                        to: dep.target_repository_id,
                        confidence: dep.confidence,
                        confirmation_status: dep.confirmation_status.as_str().into(),
                        detection_method: dep.detection_method,
                    });
                }
            }
            evidence.push(PlanEvidenceMember {
                member_id: member.id.clone(),
                repository_id: member.repository_id.clone(),
                name: repo.name.clone(),
                trust_status: repo.trust_status.as_str().into(),
                ownership: repo.ownership.map(|o| o.as_str().into()),
                has_local_checkout: repo
                    .local_path
                    .as_ref()
                    .is_some_and(|p| std::path::Path::new(p).is_dir()),
                uncommitted_changes: uncommitted,
                last_scanned_at: repo.last_scanned_at.clone(),
                facts: intel.facts,
                conflicts: intel.conflicts,
            });
        }
        Ok(CampaignPlanEvidence {
            campaign,
            members: evidence,
            edges,
        })
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanMemberInput {
    pub repository_id: String,
    pub classification: String,
    pub risk_score: Option<f64>,
    pub reasons: Vec<String>,
    pub wave_number: Option<u32>,
    #[serde(default)]
    pub review_required: bool,
}

#[tauri::command]
pub fn apply_campaign_plan(
    campaign_id: String,
    members: Vec<PlanMemberInput>,
    mark_ready: bool,
    state: State<'_, AppState>,
) -> AppResult<Campaign> {
    state.db.write(|c| {
        for input in &members {
            let member = campaigns::member_for(c, &campaign_id, &input.repository_id)?;
            campaigns::assess_member(
                c,
                &member.id,
                &campaigns::Assessment {
                    classification: campaigns::Classification::parse(&input.classification)?,
                    risk_score: input.risk_score,
                    risk_explanation: json!(input.reasons),
                    wave_number: input.wave_number,
                    review_required: input.review_required,
                },
            )?;
        }
        let mut campaign = campaigns::get(c, &campaign_id)?;
        if mark_ready {
            if campaign.status == campaigns::CampaignStatus::Draft {
                campaign =
                    campaigns::set_status(c, &campaign_id, campaigns::CampaignStatus::Planning)?;
            }
            if campaign.status == campaigns::CampaignStatus::Planning {
                campaign =
                    campaigns::set_status(c, &campaign_id, campaigns::CampaignStatus::Ready)?;
            }
        }
        campaign_activity(
            c,
            "campaign.plan_applied",
            Severity::Notice,
            format!(
                "Applied the rollout plan to {} ({} members)",
                campaign.name,
                members.len()
            ),
            campaign.id.clone(),
            json!({ "markReady": mark_ready }),
        )?;
        Ok(campaign)
    })
}

// ------------------------------------------------------- dependency edges

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeRefresh {
    pub repositories_with_scan: u32,
    pub candidates: u32,
    pub upserted: u32,
}

/// Match a declared dependency name to a registered repository. Name or
/// GitHub repo equal (case-insensitive), or scoped name's last segment.
fn dependency_matches(candidate: &str, repos: &[repositories::Repository]) -> Vec<String> {
    let norm = |s: &str| s.trim().to_ascii_lowercase();
    let cand = norm(candidate);
    let last = cand.rsplit('/').next().unwrap_or(&cand).to_string();
    repos
        .iter()
        .filter(|r| {
            norm(&r.name) == cand
                || r.github_repo.as_deref().map(norm).as_deref() == Some(cand.as_str())
                || norm(&r.name) == last
                || r.github_repo.as_deref().map(norm).as_deref() == Some(last.as_str())
        })
        .map(|r| r.id.clone())
        .collect()
}

#[tauri::command]
pub fn refresh_dependency_edges(state: State<'_, AppState>) -> AppResult<EdgeRefresh> {
    state.db.write(|c| {
        let repos = repositories::all(c)?;
        let mut scanned = 0u32;
        let mut candidates = 0u32;
        let mut upserted = 0u32;
        for repo in &repos {
            let intel = intelligence::latest(c, &repo.id)?;
            if intel.scan.is_none() {
                continue;
            }
            scanned += 1;
            for stored in &intel.facts {
                let fact = &stored.fact;
                if fact.category.as_str() != "dependency" {
                    continue;
                }
                let name = fact.key.trim();
                if name.len() < 2 || name == "dependencies" {
                    continue;
                }
                let confidence = if fact.value.starts_with("workspace:")
                    || fact.value.starts_with("file:")
                    || fact.value.starts_with("link:")
                {
                    0.85
                } else {
                    0.6
                };
                for target in dependency_matches(name, &repos) {
                    if target == repo.id {
                        continue;
                    }
                    candidates += 1;
                    repositories::upsert_dependency(
                        c,
                        &repositories::NewDependency {
                            source_repository_id: repo.id.clone(),
                            target_repository_id: target,
                            relationship_type: "package_dependency".into(),
                            confidence,
                            evidence: json!([{
                                "key": fact.key,
                                "value": fact.value,
                                "file": fact.file,
                                "line": fact.line,
                                "pointer": fact.pointer,
                                "method": fact.method,
                            }]),
                            detection_method: "declared_dependency_name".into(),
                        },
                    )?;
                    upserted += 1;
                }
            }
        }
        if upserted > 0 {
            campaign_activity(
                c,
                "campaign.dependency_edges_refreshed",
                Severity::Notice,
                format!("Recorded {upserted} inferred dependency edges from declared packages"),
                "repositories".into(),
                json!({ "scanned": scanned, "candidates": candidates, "upserted": upserted }),
            )?;
        }
        Ok(EdgeRefresh {
            repositories_with_scan: scanned,
            candidates,
            upserted,
        })
    })
}

#[tauri::command]
pub fn list_repository_dependencies(
    repository_id: String,
    params: ListParams,
    state: State<'_, AppState>,
) -> AppResult<Page<repositories::RepositoryDependency>> {
    state
        .db
        .read(|c| repositories::list_dependencies(c, &repository_id, &params))
}

// ---------------------------------------------------------------- Devin

#[tauri::command]
pub async fn devin_status() -> AppResult<DevinStatus> {
    devin::status().await
}

fn devin_evidence_lines(intel: &intelligence::Intelligence) -> Vec<String> {
    intel
        .facts
        .iter()
        .take(40)
        .map(|stored| {
            let f = &stored.fact;
            let where_ = if let (Some(file), Some(pointer)) = (&f.file, &f.pointer) {
                format!("{file}{pointer}")
            } else if let (Some(file), Some(line)) = (&f.file, f.line) {
                format!("{file}:{line}")
            } else {
                f.file.clone().unwrap_or_else(|| "?".into())
            };
            format!("{}={} ({})", f.key, f.value, where_)
        })
        .collect()
}

fn verification_commands(intel: &intelligence::Intelligence) -> Vec<String> {
    let pm = intel
        .facts
        .iter()
        .find(|f| f.fact.category.as_str() == "package_manager")
        .map(|f| f.fact.key.clone())
        .unwrap_or_else(|| "npm".into());
    let runner = match pm.as_str() {
        "pnpm" | "npm" | "yarn" => pm.clone(),
        other => {
            let _ = other;
            "npm".to_string()
        }
    };
    intel
        .facts
        .iter()
        .filter(|f| {
            f.fact.category.as_str() == "script"
                && matches!(
                    f.fact.key.as_str(),
                    "test" | "build" | "lint" | "check" | "typecheck"
                )
        })
        .map(|f| format!("{runner} run {}", f.fact.key))
        .collect()
}

#[tauri::command]
pub fn create_agent_task(member_id: String, state: State<'_, AppState>) -> AppResult<AgentTask> {
    state.db.write(|c| {
        let member = campaigns::get_member(c, &member_id)?;
        if member.status == campaigns::MembershipStatus::Excluded {
            return Err(AppError::validation(
                "An excluded repository cannot get a task.",
            ));
        }
        if let Some(existing) = agents::task_for_member(c, &member_id)? {
            if !matches!(
                existing.status,
                agents::TaskStatus::Completed
                    | agents::TaskStatus::Failed
                    | agents::TaskStatus::Cancelled
            ) {
                return Err(AppError::validation(
                    "An open Devin task already exists for this repository.",
                ));
            }
        }
        let campaign = campaigns::get(c, &member.campaign_id)?;
        let repo = repositories::get(c, &member.repository_id)?;
        let intel = intelligence::latest(c, &member.repository_id)?;
        let constraints: Vec<String> = campaign
            .constraints
            .as_object()
            .map(|o| {
                o.iter()
                    .take(20)
                    .map(|(k, v)| format!("{k}: {}", v.as_str().unwrap_or(&v.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let github = match (&repo.github_owner, &repo.github_repo) {
            (Some(o), Some(r)) => Some(format!("{o}/{r}")),
            _ => None,
        };
        let instructions = devin::build_instructions(&devin::TaskContext {
            campaign_name: &campaign.name,
            objective: &campaign.objective,
            migration_type: &campaign.migration_type,
            repository_name: &repo.name,
            local_path: repo.local_path.as_deref().unwrap_or("(no checkout)"),
            github: github.as_deref(),
            current_branch: repo.current_branch.as_deref(),
            evidence: &devin_evidence_lines(&intel),
            verification_commands: &verification_commands(&intel),
            constraints: &constraints,
        });
        let task = agents::create_task(
            c,
            &agents::NewAgentTask {
                campaign_repository_id: member.id.clone(),
                provider: agents::AgentProvider::Devin,
                execution_mode: agents::ExecutionMode::LocalCli,
                instructions_markdown: instructions,
            },
        )?;
        campaign_activity(
            c,
            "agent.task_created",
            Severity::Notice,
            format!("Drafted a Devin task for {}", member.repository_name),
            member.campaign_id.clone(),
            json!({ "memberId": member.id, "taskId": task.id }),
        )?;
        Ok(task)
    })
}

#[tauri::command]
pub fn approve_agent_task(
    id: String,
    confirm: bool,
    state: State<'_, AppState>,
) -> AppResult<AgentTask> {
    if !confirm {
        return Err(AppError::validation(
            "Approving a Devin task requires confirmation.",
        ));
    }
    state.db.write(|c| {
        let task = agents::approve_task(c, &id)?;
        activity::record(
            c,
            NewEvent::new(
                "agent",
                "agent.task_approved",
                Severity::Notice,
                "Devin task approved for dispatch",
            )
            .entity(task.id.clone())
            .metadata(json!({ "memberId": task.campaign_repository_id })),
        )?;
        Ok(task)
    })
}

#[tauri::command]
pub async fn dispatch_agent_task<R: Runtime>(
    id: String,
    confirm: bool,
    state: State<'_, AppState>,
    app: AppHandle<R>,
) -> AppResult<AgentTask> {
    if !confirm {
        return Err(AppError::validation(
            "Dispatching a Devin task requires confirmation.",
        ));
    }
    let (task, member) = state.db.read(|c| {
        Ok::<_, AppError>((
            agents::get_task(c, &id)?,
            campaigns::get_member(c, &agents::get_task(c, &id)?.campaign_repository_id)?,
        ))
    })?;
    if task.approved_at.is_none() {
        return Err(AppError::validation(
            "A Devin task must be approved before dispatch.",
        ));
    }
    if !matches!(
        task.status,
        agents::TaskStatus::Pending | agents::TaskStatus::Queued
    ) {
        return Err(AppError::validation(format!(
            "Task is {}; only a pending task can dispatch.",
            task.status.as_str()
        )));
    }
    if member.review_required {
        return Err(AppError::validation(
            "Planning flagged this repository for human review; resolve it first.",
        ));
    }
    if member.classification != campaigns::Classification::Affected {
        return Err(AppError::validation(
            "Only repositories classified affected can be delegated.",
        ));
    }
    let repository = state
        .db
        .read(|c| repositories::get(c, &member.repository_id))?;
    if repository.trust_status != repositories::TrustStatus::Trusted {
        return Err(AppError::validation(
            "Dispatch requires the repository to be trusted.",
        ));
    }
    let checkout = repository
        .local_path
        .as_ref()
        .map(std::path::PathBuf::from)
        .and_then(|p| std::fs::canonicalize(p).ok())
        .filter(|p| p.is_dir())
        .ok_or_else(|| AppError::validation("The repository has no readable local checkout."))?;

    let status = devin::status().await?;
    if !(status.installed && status.authenticated) {
        return Err(AppError::new(
            ErrorCode::DevinNotConfigured,
            "Devin CLI is not installed or not authenticated; task stays undispatched.",
        ));
    }

    let runner = ProcessRunner::new(
        ExecutionPolicy::devin_dispatch(checkout.clone()),
        Some(state.log_dir.clone()),
        Some(state.db.clone()),
    );
    let spec = devin::dispatch_spec(&task.instructions_markdown, &checkout)?;
    let search = shell_path::search_path().await;
    let started = runner.start(spec, search, Arc::new(TauriSink(app.clone())))?;
    let run_id = started.run_id.clone();
    let task_id = task.id.clone();

    let updated = state.db.write(|c| {
        agents::start_run(c, &task.id, "devin", None, Some(&run_id))?;
        let task = agents::set_task_status(c, &task.id, agents::TaskStatus::Running)?;
        campaigns::set_member_status(c, &member.id, campaigns::MembershipStatus::InProgress)?;
        campaign_activity(
            c,
            "agent.task_dispatched",
            Severity::Notice,
            format!("Dispatched a Devin task for {}", member.repository_name),
            member.campaign_id.clone(),
            json!({ "taskId": task.id, "runId": run_id }),
        )?;
        Ok(task)
    })?;

    state
        .devin_runs
        .lock()
        .map_err(|_| AppError::internal("Devin run registry is poisoned."))?
        .insert(task_id.clone(), (runner, run_id.clone()));

    let db = state.db.clone();
    let devin_runs = state.devin_runs.clone();
    let member_id = member.id.clone();
    let campaign_id = member.campaign_id.clone();
    tokio::spawn(async move {
        let outcome = started.handle.await;
        let (task_status, run_status, member_status, note) = match outcome {
            Ok(ref r) => match r.status {
                crate::process::RunStatus::Succeeded => (
                    agents::TaskStatus::AwaitingReview,
                    agents::RunStatus::Succeeded,
                    campaigns::MembershipStatus::AwaitingVerification,
                    "Devin session finished".to_string(),
                ),
                crate::process::RunStatus::Cancelled => (
                    agents::TaskStatus::Cancelled,
                    agents::RunStatus::Cancelled,
                    campaigns::MembershipStatus::Pending,
                    "Devin session cancelled".to_string(),
                ),
                crate::process::RunStatus::TimedOut => (
                    agents::TaskStatus::Failed,
                    agents::RunStatus::TimedOut,
                    campaigns::MembershipStatus::Failed,
                    "Devin session timed out".to_string(),
                ),
                crate::process::RunStatus::Failed => (
                    agents::TaskStatus::Failed,
                    agents::RunStatus::Failed,
                    campaigns::MembershipStatus::Failed,
                    "Devin session failed".to_string(),
                ),
            },
            Err(ref e) => (
                agents::TaskStatus::Failed,
                agents::RunStatus::Failed,
                campaigns::MembershipStatus::Failed,
                format!("Devin session lost: {e}"),
            ),
        };
        let _ = db.write(|c| -> AppResult<()> {
            if let Ok(Some(run)) = agents::run_by_external_id(c, &run_id) {
                let _ = agents::finish_run(
                    c,
                    &run.id,
                    run_status,
                    outcome.ok().and_then(|r| r.exit_code),
                    None,
                    None,
                );
            }
            let _ = agents::set_task_status(c, &task_id, task_status);
            let _ = campaigns::set_member_status(c, &member_id, member_status);
            let _ = campaign_activity(
                c,
                "agent.task_finished",
                Severity::Notice,
                note,
                campaign_id,
                json!({ "taskId": task_id }),
            );
            Ok(())
        });
        if let Ok(mut m) = devin_runs.lock() {
            m.remove(&task_id);
        }
    });

    Ok(updated)
}

#[tauri::command]
pub fn cancel_agent_task(id: String, state: State<'_, AppState>) -> AppResult<AgentTask> {
    let entry = state
        .devin_runs
        .lock()
        .map_err(|_| AppError::internal("Devin run registry is poisoned."))?
        .get(&id)
        .map(|(r, run_id)| (r.clone(), run_id.clone()));
    match entry {
        Some((runner, run_id)) => {
            runner.cancel(&run_id);
            state.db.read(|c| agents::get_task(c, &id))
        }
        None => Err(AppError::validation("No live Devin session for this task.")),
    }
}

#[tauri::command]
pub fn list_agent_runs(
    task_id: String,
    params: ListParams,
    state: State<'_, AppState>,
) -> AppResult<Page<agents::AgentRun>> {
    state.db.read(|c| agents::list_runs(c, &task_id, &params))
}
