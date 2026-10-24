mod commands;
mod credentials;
mod db;
mod environment;
mod error;
mod logging;
mod paths;
mod process;
mod redact;
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
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    register_commands(tauri::Builder::default())
        // Rust-side URL opening only; no opener permission is granted to the web view.
        .plugin(tauri_plugin_opener::init())
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
        assert_eq!(info["schemaVersion"], json!(4));
        assert_eq!(info["latestSchemaVersion"], json!(4));

        let health = invoke(&h, "get_database_health", json!({})).unwrap();
        assert_eq!(health["upToDate"], true);
        assert_eq!(health["foreignKeysEnabled"], true);
        assert_eq!(health["quickCheck"], "ok");
        assert_eq!(health["journalMode"], "wal");
        assert_eq!(health["appliedMigrations"].as_array().unwrap().len(), 4);
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
            4
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
            json!({ "provider": "github", "kind": "github_personal_access_token", "secret": SECRET }),
        )
        .unwrap();
        assert_eq!(stored["exists"], true);
        assert_eq!(stored["identifier"], "github:github_personal_access_token");
        let listed = invoke(&h, "list_credentials", json!({})).unwrap();
        let github = listed
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == "github_personal_access_token")
            .unwrap();
        assert_eq!(github["exists"], true);

        let refused = invoke(
            &h,
            "delete_credential",
            json!({ "provider": "github", "kind": "github_personal_access_token", "confirm": false }),
        )
        .unwrap_err();
        assert_eq!(refused["code"], "VALIDATION_FAILED");
        let deleted = invoke(
            &h,
            "delete_credential",
            json!({ "provider": "github", "kind": "github_personal_access_token", "confirm": true }),
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
}
