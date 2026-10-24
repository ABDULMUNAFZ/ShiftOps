mod commands;
mod environment;
mod error;
mod keychain;
mod shell_path;
mod store;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{Manager, Runtime};

pub struct AppState {
    pub store: Mutex<store::Store>,
    pub data_dir: PathBuf,
}

impl AppState {
    fn open(data_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(&data_dir)?;
        let store = store::Store::open(&data_dir.join(store::DB_FILE_NAME))?;
        Ok(Self {
            store: Mutex::new(store),
            data_dir,
        })
    }
}

fn register_commands<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.invoke_handler(tauri::generate_handler![
        commands::get_app_info,
        commands::get_settings,
        commands::set_setting,
        commands::run_environment_checks,
        commands::credential_status,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    register_commands(tauri::Builder::default())
        .setup(|app| {
            let state = AppState::open(app.path().app_data_dir()?)?;
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running ShiftOps");
}

#[cfg(test)]
mod ipc_tests {
    //! Exercise the commands through Tauri's IPC layer with the mock runtime,
    //! the same path the frontend's `invoke` takes.

    use super::*;
    use serde_json::{json, Value};
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, INVOKE_KEY};
    use tauri::webview::InvokeRequest;

    fn app(
        data_dir: PathBuf,
    ) -> (
        tauri::App<tauri::test::MockRuntime>,
        tauri::WebviewWindow<tauri::test::MockRuntime>,
    ) {
        let app = register_commands(mock_builder())
            .build(tauri::generate_context!(test = true))
            .expect("build mock app");
        app.manage(AppState::open(data_dir).expect("open state"));
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("build window");
        (app, window)
    }

    fn invoke(
        window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        cmd: &str,
        body: Value,
    ) -> Result<Value, Value> {
        get_ipc_response(
            window,
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
    fn settings_round_trip_over_ipc() {
        let dir = tempfile::tempdir().unwrap();
        let (_app, window) = app(dir.path().to_path_buf());

        assert_eq!(
            invoke(&window, "get_settings", json!({})).unwrap(),
            json!({})
        );
        invoke(
            &window,
            "set_setting",
            json!({ "key": "theme", "value": "\"dark\"" }),
        )
        .unwrap();
        assert_eq!(
            invoke(&window, "get_settings", json!({})).unwrap(),
            json!({ "theme": "\"dark\"" })
        );

        let err = invoke(
            &window,
            "set_setting",
            json!({ "key": "token", "value": "\"x\"" }),
        )
        .unwrap_err();
        assert_eq!(err, json!("unknown setting key: token"));
    }

    #[test]
    fn app_info_reports_real_paths() {
        let dir = tempfile::tempdir().unwrap();
        let (_app, window) = app(dir.path().to_path_buf());
        let info = invoke(&window, "get_app_info", json!({})).unwrap();
        assert_eq!(info["dataDir"], json!(dir.path().display().to_string()));
        assert_eq!(info["schemaVersion"], json!(1));
        assert!(dir.path().join(store::DB_FILE_NAME).exists());
    }

    #[test]
    fn environment_checks_over_ipc_return_every_check() {
        let dir = tempfile::tempdir().unwrap();
        let (_app, window) = app(dir.path().to_path_buf());
        let report = invoke(&window, "run_environment_checks", json!({})).unwrap();
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
        if std::env::var_os("SHIFTOPS_PRINT_ENV_REPORT").is_some() {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        }
    }

    #[test]
    fn credential_status_rejects_unknown_provider() {
        let dir = tempfile::tempdir().unwrap();
        let (_app, window) = app(dir.path().to_path_buf());
        let err = invoke(
            &window,
            "credential_status",
            json!({ "provider": "gitlab" }),
        )
        .unwrap_err();
        assert_eq!(err, json!("unknown provider: gitlab"));
    }
}
