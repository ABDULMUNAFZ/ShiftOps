//! Tauri commands exposed to the frontend.

use std::collections::BTreeMap;

use serde::Serialize;
use tauri::State;

use crate::environment::{self, Report};
use crate::error::{AppError, AppResult};
use crate::keychain;
use crate::store::DB_FILE_NAME;
use crate::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    data_dir: String,
    database_path: String,
    schema_version: u32,
}

#[tauri::command]
pub fn get_app_info<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
) -> AppResult<AppInfo> {
    let store = state.store.lock().map_err(|_| AppError::LockPoisoned)?;
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: state.data_dir.display().to_string(),
        database_path: state.data_dir.join(DB_FILE_NAME).display().to_string(),
        schema_version: store.schema_version()?,
    })
}

/// All stored settings as raw JSON strings keyed by setting name.
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<BTreeMap<String, String>> {
    state
        .store
        .lock()
        .map_err(|_| AppError::LockPoisoned)?
        .settings()
}

#[tauri::command]
pub fn set_setting(key: String, value: String, state: State<'_, AppState>) -> AppResult<()> {
    state
        .store
        .lock()
        .map_err(|_| AppError::LockPoisoned)?
        .set_setting(&key, &value)
}

#[tauri::command]
pub async fn run_environment_checks(state: State<'_, AppState>) -> AppResult<Report> {
    Ok(environment::run_all(&state.data_dir).await)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatus {
    provider: String,
    present: bool,
    error: Option<String>,
}

/// Whether a credential exists in the OS keychain. Never returns the secret.
#[tauri::command]
pub async fn credential_status(provider: String) -> AppResult<CredentialStatus> {
    if !keychain::PROVIDERS.contains(&provider.as_str()) {
        return Err(AppError::InvalidInput(format!(
            "unknown provider: {provider}"
        )));
    }
    let lookup = provider.clone();
    let result = tokio::task::spawn_blocking(move || keychain::credential_present(&lookup))
        .await
        .unwrap_or_else(|e| Err(format!("keychain task failed: {e}")));
    Ok(match result {
        Ok(present) => CredentialStatus {
            provider,
            present,
            error: None,
        },
        Err(e) => CredentialStatus {
            provider,
            present: false,
            error: Some(e),
        },
    })
}
