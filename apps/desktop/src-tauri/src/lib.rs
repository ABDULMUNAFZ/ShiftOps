mod commands;
mod credentials;
mod db;
mod discovery;
mod environment;
mod error;
mod git;
mod github;
mod logging;
mod paths;
mod process;
mod redact;
mod scanner;
mod shell_path;
mod time_util;
mod urls;

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tauri::{Manager, Runtime};

use crate::credentials::CredentialStore;
use crate::db::activity::{self, NewEvent, Severity};
use crate::db::Database;
use crate::process::{ExecutionPolicy, ProcessRunner};

pub use crate::process::{run_diagnostic_ticker, DIAGNOSTIC_TICKER_FLAG};

pub struct AppState {
    pub db: Arc<Database>,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub credentials: CredentialStore,
    pub runner: ProcessRunner,
    /// Program and arguments for the bundled long-running diagnostic.
    pub diagnostic_program: (PathBuf, Vec<String>),
    pub discoveries: discovery::Discoveries,
    pub scans: scanner::Scans,
    /// GitHub endpoints, or the configuration error to report.
    pub github_config: Result<github::client::GithubConfig, error::AppError>,
    pub github_flows: github::DeviceFlows,
    pub github_requests: github::Requests,
}

impl AppState {
    fn open(
        data_dir: PathBuf,
        log_dir: PathBuf,
        credentials: CredentialStore,
        diagnostic_program: (PathBuf, Vec<String>),
    ) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(&data_dir)?;
        let log_file = logging::init(&log_dir)?;
        let db_path = data_dir.join(db::DB_FILE_NAME);
        let (database, report) = Database::open(&db_path).inspect_err(|e| e.log())?;
        let db = Arc::new(database);

        db.write(|c| {
            for m in &report.applied {
                activity::record(
                    c,
                    NewEvent::new(
                        "database",
                        "database.migration_applied",
                        Severity::Notice,
                        format!("Applied database migration {} ({})", m.version, m.name),
                    )
                    .metadata(
                        json!({ "version": m.version, "name": m.name, "appliedAt": m.applied_at }),
                    ),
                )?;
            }
            activity::record(
                c,
                NewEvent::new(
                    "database",
                    "database.opened",
                    Severity::Info,
                    format!(
                        "Application database opened at schema version {}",
                        db::migrations::user_version(c)?
                    ),
                )
                .metadata(json!({
                    "path": db_path.display().to_string(),
                    "fromVersion": report.from_version,
                    "toVersion": report.to_version,
                    "backfilledVersions": report.backfilled,
                    "backupPath": report.backup_path.as_ref().map(|p| p.display().to_string()),
                    "logFile": log_file.display().to_string(),
                })),
            )
        })?;
        logging::info(
            "database",
            &format!(
                "opened {} (schema {} -> {}, {} migrations applied)",
                db_path.display(),
                report.from_version,
                report.to_version,
                report.applied.len()
            ),
        );

        let runner = ProcessRunner::new(
            ExecutionPolicy::step2(data_dir.clone()),
            Some(log_dir.clone()),
            Some(db.clone()),
        );
        Ok(Self {
            db,
            data_dir,
            log_dir,
            credentials,
            runner,
            diagnostic_program,
            discoveries: discovery::Discoveries::default(),
            scans: scanner::Scans::default(),
            github_config: github::client::GithubConfig::from_env(),
            github_flows: github::DeviceFlows::default(),
            github_requests: github::Requests::default(),
        })
    }
}

fn register_commands<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        commands::get_app_info,
        commands::get_settings,
        commands::set_setting,
        commands::get_execution_limits,
        commands::run_environment_checks,
        commands::get_database_health,
        commands::get_keychain_status,
        commands::list_credentials,
        commands::store_credential,
        commands::delete_credential,
        commands::start_diagnostic,
        commands::cancel_process,
        commands::list_active_processes,
        commands::list_activity,
        commands::list_activity_entity_types,
        commands::list_repositories,
        commands::get_repository,
        commands::list_campaigns,
        commands::get_campaign,
        commands::list_campaign_repositories,
        commands::list_agent_tasks,
        commands::list_verification_runs,
        commands::list_migration_patterns,
        commands::open_external_url,
        commands::pick_directory,
        commands::start_discovery,
        commands::cancel_discovery,
        commands::get_discovery,
        commands::import_discovered,
        commands::list_repository_roots,
        commands::remove_repository_root,
        commands::list_repository_branches,
        commands::start_repository_scan,
        commands::cancel_repository_scan,
        commands::get_repository_intelligence,
        commands::list_repository_scans,
        commands::github_status,
        commands::github_start_device_flow,
        commands::github_cancel_device_flow,
        commands::github_connect_pat,
        commands::github_refresh,
        commands::github_disconnect,
        commands::github_list_organizations,
        commands::github_browse_repositories,
        commands::github_repository_details,
        commands::github_list_pull_requests,
        commands::github_get_pull_request,
        commands::github_get_file,
        commands::github_rate_limit,
        commands::github_import_repository,
        commands::github_cancel_request,
        commands::github_repository_for,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    register_commands(tauri::Builder::default())
        // Rust-side URL opening only; no opener permission is granted to the web view.
        .plugin(tauri_plugin_opener::init())
        // Rust-side folder picker only; no dialog permission is granted to the web view.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let self_exe = std::env::current_exe()?;
            let state = AppState::open(
                app.path().app_data_dir()?,
                app.path().app_log_dir()?,
                CredentialStore::production(),
                (self_exe, vec![DIAGNOSTIC_TICKER_FLAG.to_string()]),
            )?;
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running ShiftOps");
}

#[cfg(test)]
mod ipc_tests {
    //! Exercise the commands through Tauri's IPC layer with the mock runtime,
    //! the same path the frontend's `invoke` takes. Every test uses a
    //! temporary data directory and a test keychain service.

    use super::*;
    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
    use tauri::webview::InvokeRequest;

    struct Harness {
        _app: tauri::App<MockRuntime>,
        window: tauri::WebviewWindow<MockRuntime>,
        dir: tempfile::TempDir,
        keychain: Option<crate::credentials::tests::TestKeychain>,
    }

    fn harness() -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let keychain = crate::credentials::tests::TestKeychain::new();
        let store = keychain
            .as_ref()
            .map(|k| k.store.clone())
            .unwrap_or_else(|| CredentialStore::new("dev.shiftops.desktop.test.unavailable"));
        let app = register_commands(mock_builder())
            .build(tauri::generate_context!(test = true))
            .expect("build mock app");
        let state = AppState::open(
            dir.path().join("data"),
            dir.path().join("logs"),
            store,
            (
                PathBuf::from("/bin/sh"),
                vec![
                    "-c".into(),
                    "while true; do echo tick; sleep 0.1; done".into(),
                ],
            ),
        )
        .expect("open state");
        app.manage(state);
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build window");
        Harness {
            _app: app,
            window,
            dir,
            keychain,
        }
    }

    fn invoke(h: &Harness, cmd: &str, body: Value) -> Result<Value, Value> {
        get_ipc_response(
            &h.window,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: if cfg!(windows) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|r| r.deserialize::<Value>().unwrap())
    }

    #[test]
    fn settings_round_trip_and_structured_errors() {
        let h = harness();
        assert_eq!(invoke(&h, "get_settings", json!({})).unwrap(), json!({}));
        invoke(
            &h,
            "set_setting",
            json!({ "key": "theme", "value": "\"dark\"" }),
        )
        .unwrap();
        assert_eq!(
            invoke(&h, "get_settings", json!({})).unwrap(),
            json!({ "theme": "\"dark\"" })
        );

        let err = invoke(
            &h,
            "set_setting",
            json!({ "key": "token", "value": "\"x\"" }),
        )
        .unwrap_err();
        assert_eq!(err["code"], "VALIDATION_FAILED");
        assert_eq!(err["message"], "Unknown setting key: token");
        assert_eq!(err["recoverable"], true);
        assert!(err["suggestedAction"].is_string());
        assert!(err.get("technicalDetails").is_some());
    }

    #[test]
    fn app_info_and_database_health_report_real_state() {
        let h = harness();
        let info = invoke(&h, "get_app_info", json!({})).unwrap();
        assert_eq!(
            info["dataDir"],
            json!(h.dir.path().join("data").display().to_string())
        );
        assert_eq!(
            info["schemaVersion"],
            json!(db::migrations::latest_version())
        );
        assert_eq!(
            info["latestSchemaVersion"],
            json!(db::migrations::latest_version())
        );

        let health = invoke(&h, "get_database_health", json!({})).unwrap();
        assert_eq!(health["upToDate"], true);
        assert_eq!(health["foreignKeysEnabled"], true);
        assert_eq!(health["quickCheck"], "ok");
        assert_eq!(health["journalMode"], "wal");
        assert_eq!(
            health["appliedMigrations"].as_array().unwrap().len(),
            db::migrations::MIGRATIONS.len()
        );
        assert!(
            health["activityEventCount"].as_u64().unwrap() >= 5,
            "{health}"
        );

        let events = invoke(&h, "list_activity", json!({ "query": { "limit": 10 } })).unwrap();
        let types: Vec<&str> = events["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["eventType"].as_str().unwrap())
            .collect();
        assert_eq!(types.first(), Some(&"database.opened"));
        assert_eq!(
            types
                .iter()
                .filter(|t| **t == "database.migration_applied")
                .count(),
            db::migrations::MIGRATIONS.len()
        );
    }

    #[test]
    fn domain_lists_return_empty_pages_not_sample_data() {
        let h = harness();
        for (cmd, body) in [
            ("list_repositories", json!({ "params": {} })),
            (
                "list_campaigns",
                json!({ "params": { "statuses": ["draft"] } }),
            ),
            ("list_agent_tasks", json!({ "query": {} })),
            ("list_verification_runs", json!({ "query": {} })),
            ("list_migration_patterns", json!({ "params": {} })),
        ] {
            let page = invoke(&h, cmd, body).unwrap();
            assert_eq!(page["total"], 0, "{cmd}");
            assert_eq!(page["items"], json!([]), "{cmd}");
        }
        let err = invoke(&h, "get_repository", json!({ "id": "missing" })).unwrap_err();
        assert_eq!(err["code"], "RECORD_NOT_FOUND");
        let bad = invoke(
            &h,
            "list_campaigns",
            json!({ "params": { "statuses": ["bogus"] } }),
        )
        .unwrap_err();
        assert_eq!(bad["code"], "VALIDATION_FAILED");
    }

    #[test]
    fn credential_secret_never_crosses_back_over_ipc() {
        let h = harness();
        if h.keychain.is_none() {
            return; // reason printed by TestKeychain::new
        }
        const SECRET: &str = "ghp_IpcTestOnlySecretValue0123456789abcdef";
        let stored = invoke(
            &h,
            "store_credential",
            json!({ "provider": "devin", "kind": "devin_api_token", "secret": SECRET }),
        )
        .unwrap();
        assert_eq!(stored["exists"], true);
        assert_eq!(stored["identifier"], "devin:devin_api_token");
        let listed = invoke(&h, "list_credentials", json!({})).unwrap();
        let github = listed
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == "devin_api_token")
            .unwrap();
        assert_eq!(github["exists"], true);

        let refused = invoke(
            &h,
            "delete_credential",
            json!({ "provider": "devin", "kind": "devin_api_token", "confirm": false }),
        )
        .unwrap_err();
        assert_eq!(refused["code"], "VALIDATION_FAILED");
        let deleted = invoke(
            &h,
            "delete_credential",
            json!({ "provider": "devin", "kind": "devin_api_token", "confirm": true }),
        )
        .unwrap();
        assert_eq!(deleted["exists"], false);

        let activity = invoke(&h, "list_activity", json!({ "query": { "limit": 200 } })).unwrap();
        let log = std::fs::read_to_string(h.dir.path().join("logs").join(logging::LOG_FILE_NAME))
            .unwrap_or_default();
        let db_bytes = std::fs::read(h.dir.path().join("data").join(db::DB_FILE_NAME)).unwrap();
        for haystack in [
            stored.to_string(),
            listed.to_string(),
            deleted.to_string(),
            activity.to_string(),
            log,
        ] {
            assert!(
                !haystack.contains("IpcTestOnlySecret"),
                "secret leaked: {haystack}"
            );
        }
        assert!(!String::from_utf8_lossy(&db_bytes).contains("IpcTestOnlySecret"));
    }

    #[test]
    fn credential_commands_reject_unknown_slots() {
        let h = harness();
        let err = invoke(
            &h,
            "store_credential",
            json!({ "provider": "gitlab", "kind": "provider_token", "secret": "x" }),
        )
        .unwrap_err();
        assert_eq!(err["code"], "VALIDATION_FAILED");
    }

    #[test]
    fn external_urls_are_validated_before_opening() {
        let h = harness();
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "http://example.com",
        ] {
            let err = invoke(&h, "open_external_url", json!({ "url": url })).unwrap_err();
            assert_eq!(err["code"], "EXTERNAL_URL_REJECTED", "{url}");
        }
    }

    // The test diagnostic is a `/bin/sh` loop; the runner's Unix process-group
    // handling is what this exercises.
    #[cfg(unix)]
    #[test]
    fn diagnostic_can_be_started_and_cancelled_by_run_id() {
        let h = harness();
        let started = invoke(&h, "start_diagnostic", json!({ "kind": "long_running" })).unwrap();
        let run_id = started["runId"].as_str().unwrap().to_string();
        let active = invoke(&h, "list_active_processes", json!({})).unwrap();
        assert_eq!(active[0]["runId"], json!(run_id));

        let cancelled = invoke(&h, "cancel_process", json!({ "runId": run_id })).unwrap();
        assert_eq!(cancelled["outcome"], "requested");
        let mut stopped = false;
        for _ in 0..100 {
            if invoke(&h, "list_active_processes", json!({})).unwrap() == json!([]) {
                stopped = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(stopped, "diagnostic did not stop");
        let again = invoke(&h, "cancel_process", json!({ "runId": run_id })).unwrap();
        assert_eq!(again["outcome"], "not_running");

        let events = invoke(
            &h,
            "list_activity",
            json!({ "query": { "entityId": run_id } }),
        )
        .unwrap();
        let types: Vec<&str> = events["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["eventType"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["process.cancelled", "process.started"]);

        assert!(
            invoke(&h, "start_diagnostic", json!({ "kind": "arbitrary" })).is_err(),
            "only the fixed diagnostic kinds are accepted"
        );
    }

    #[test]
    fn environment_checks_return_every_check_and_record_activity() {
        let h = harness();
        let report = invoke(&h, "run_environment_checks", json!({})).unwrap();
        let ids: Vec<&str> = report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "os",
                "app_data_dir",
                "git",
                "node",
                "pnpm",
                "npm",
                "yarn",
                "docker",
                "devin",
                "keychain"
            ]
        );
        for check in report["checks"].as_array().unwrap() {
            let status = check["status"].as_str().unwrap();
            assert!(
                ["available", "unavailable", "error"].contains(&status),
                "{check}"
            );
            assert!(!check["detail"].as_str().unwrap().is_empty(), "{check}");
        }
        let events = invoke(
            &h,
            "list_activity",
            json!({ "query": { "search": "Environment checks completed" } }),
        )
        .unwrap();
        assert_eq!(events["total"], 1);
        if std::env::var_os("SHIFTOPS_PRINT_ENV_REPORT").is_some() {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        }
    }

    #[cfg(unix)]
    #[test]
    fn discovery_commands_discover_import_and_forget_roots_over_ipc() {
        let h = harness();
        let work = tempfile::tempdir().unwrap();
        let repo = work.path().join("svc");
        std::fs::create_dir_all(&repo).unwrap();
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap();
        assert!(ok.success());

        let started = invoke(
            &h,
            "start_discovery",
            json!({ "request": { "path": work.path().display().to_string(), "mode": "directory", "maxDepth": 2, "followInternalSymlinks": false } }),
        )
        .unwrap();
        let id = started["discoveryId"].as_str().unwrap().to_string();
        let mut result = Value::Null;
        for _ in 0..200 {
            result = invoke(&h, "get_discovery", json!({ "discoveryId": id })).unwrap();
            if !result.is_null() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert_eq!(result["status"], "completed", "{result}");
        let path = result["candidates"][0]["path"]
            .as_str()
            .unwrap()
            .to_string();
        // A repository with no commits reports no HEAD commit rather than failing.
        assert!(result["candidates"][0]["metadata"]["headCommit"].is_null());

        let summary = invoke(
            &h,
            "import_discovered",
            json!({ "discoveryId": id, "paths": [path] }),
        )
        .unwrap();
        assert_eq!(summary["imported"], 1);
        let roots = invoke(&h, "list_repository_roots", json!({})).unwrap();
        assert_eq!(roots[0]["repositoryCount"], 1);
        let root_id = roots[0]["id"].as_str().unwrap().to_string();

        let refused = invoke(
            &h,
            "remove_repository_root",
            json!({ "id": root_id, "confirm": false }),
        )
        .unwrap_err();
        assert_eq!(refused["code"], "VALIDATION_FAILED");
        invoke(
            &h,
            "remove_repository_root",
            json!({ "id": root_id, "confirm": true }),
        )
        .unwrap();
        let repos = invoke(&h, "list_repositories", json!({ "params": {} })).unwrap();
        assert_eq!(repos["total"], 1, "repository records survive root removal");
        assert!(repo.exists(), "files are never deleted");

        let bad = invoke(
            &h,
            "start_discovery",
            json!({ "request": { "path": "relative/path", "mode": "directory", "maxDepth": 2, "followInternalSymlinks": false } }),
        )
        .unwrap_err();
        assert_eq!(bad["code"], "VALIDATION_FAILED");
    }

    #[test]
    fn repository_scan_commands_over_ipc() {
        let h = harness();
        let work = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(work.path()).unwrap();
        std::fs::write(
            path.join("package.json"),
            r#"{"engines":{"node":">=20"},"dependencies":{"express":"^4"}}"#,
        )
        .unwrap();
        std::fs::write(path.join(".nvmrc"), "18\n").unwrap();
        let state = h._app.state::<AppState>();
        let repo = state
            .db
            .write(|c| {
                db::repositories::create(
                    c,
                    &db::repositories::NewRepository {
                        source_type: db::repositories::SourceType::Local,
                        name: "svc".into(),
                        local_path: Some(path.display().to_string()),
                        remote_url: None,
                        github: None,
                        default_branch: None,
                    },
                )
            })
            .unwrap();

        let empty = invoke(
            &h,
            "get_repository_intelligence",
            json!({ "repositoryId": repo.id }),
        )
        .unwrap();
        assert!(empty["scan"].is_null());
        invoke(
            &h,
            "start_repository_scan",
            json!({ "repositoryId": repo.id, "force": false }),
        )
        .unwrap();
        let mut intel = Value::Null;
        for _ in 0..200 {
            intel = invoke(
                &h,
                "get_repository_intelligence",
                json!({ "repositoryId": repo.id }),
            )
            .unwrap();
            if !intel["scan"].is_null() && intel["scanRunning"] == false {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert_eq!(intel["scan"]["status"], "completed", "{intel}");
        let express = intel["facts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["key"] == "Express")
            .unwrap();
        assert_eq!(express["file"], "package.json");
        assert_eq!(express["category"], "framework");
        assert!(intel["conflicts"][0]["message"]
            .as_str()
            .unwrap()
            .contains("excludes Node 18"));

        let listed = invoke(&h, "list_repositories", json!({ "params": {} })).unwrap();
        assert_eq!(listed["items"][0]["lastScanStatus"], "completed");
        assert_eq!(
            listed["items"][0]["scanSummary"]["frameworks"],
            json!(["Express"])
        );
        let scans = invoke(
            &h,
            "list_repository_scans",
            json!({ "repositoryId": repo.id, "params": {} }),
        )
        .unwrap();
        assert_eq!(scans["total"], 1);
        let cancel = invoke(
            &h,
            "cancel_repository_scan",
            json!({ "repositoryId": repo.id }),
        )
        .unwrap();
        assert_eq!(cancel["outcome"], "not_running");
    }

    #[test]
    fn github_commands_report_real_state_without_an_account() {
        let h = harness();
        let status = invoke(&h, "github_status", json!({})).unwrap();
        assert!(status["account"].is_null());
        assert_eq!(status["credentialStored"], false);
        let browse = invoke(
            &h,
            "github_browse_repositories",
            json!({ "params": {}, "requestId": null }),
        )
        .unwrap_err();
        assert_eq!(browse["code"], "GITHUB_AUTHENTICATION_FAILED");
        let refused = invoke(&h, "github_disconnect", json!({ "confirm": false })).unwrap_err();
        assert_eq!(refused["code"], "VALIDATION_FAILED");
        assert!(invoke(&h, "github_disconnect", json!({ "confirm": true }))
            .unwrap()
            .is_null());
        let missing = invoke(
            &h,
            "github_import_repository",
            json!({ "githubRepositoryId": "nope" }),
        )
        .unwrap_err();
        assert_eq!(missing["code"], "RECORD_NOT_FOUND");
        let pat = invoke(&h, "github_connect_pat", json!({ "token": "has space" })).unwrap_err();
        assert_eq!(pat["code"], "VALIDATION_FAILED");
        assert_eq!(
            invoke(&h, "github_cancel_request", json!({ "requestId": "x" })).unwrap()["outcome"],
            "not_running"
        );
    }
}
