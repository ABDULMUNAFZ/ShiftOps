//! Settings service. Values are JSON text; keys are allow-listed and the
//! execution limits are range-checked here because native code reads them.

use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::time_util;

pub const THEME: &str = "theme";
pub const SIDEBAR_COLLAPSED: &str = "sidebarCollapsed";
pub const ONBOARDING_COMPLETED_AT: &str = "onboardingCompletedAt";
pub const EXECUTION_TIMEOUT_SECONDS: &str = "execution.defaultTimeoutSeconds";
pub const EXECUTION_MAX_OUTPUT_BYTES: &str = "execution.maxOutputBytes";

/// Setting keys the frontend may read and write. Anything else is rejected so
/// the settings table cannot be used as a general-purpose key-value store.
pub const ALLOWED_KEYS: &[&str] = &[
    THEME,
    SIDEBAR_COLLAPSED,
    ONBOARDING_COMPLETED_AT,
    EXECUTION_TIMEOUT_SECONDS,
    EXECUTION_MAX_OUTPUT_BYTES,
];
const MAX_VALUE_BYTES: usize = 4096;

pub const TIMEOUT_RANGE: (u64, u64) = (1, 3600);
pub const OUTPUT_RANGE: (u64, u64) = (4 * 1024, 16 * 1024 * 1024);
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
pub const DEFAULT_MAX_OUTPUT_BYTES: u64 = 1024 * 1024;

pub fn all(conn: &Connection) -> AppResult<BTreeMap<String, String>> {
    let mut stmt = conn.prepare("SELECT key, value_json FROM settings")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value_json FROM settings WHERE key = ?1",
            [key],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn set(conn: &Connection, key: &str, value_json: &str) -> AppResult<()> {
    if !ALLOWED_KEYS.contains(&key) {
        return Err(
            AppError::validation(format!("Unknown setting key: {key}")).with_context("key", key)
        );
    }
    if value_json.len() > MAX_VALUE_BYTES {
        return Err(AppError::validation(format!(
            "Setting value exceeds {MAX_VALUE_BYTES} bytes."
        )));
    }
    let value: Value = serde_json::from_str(value_json).map_err(|e| {
        AppError::validation("Setting value is not valid JSON.").with_details(e.to_string())
    })?;
    let range = match key {
        EXECUTION_TIMEOUT_SECONDS => Some(TIMEOUT_RANGE),
        EXECUTION_MAX_OUTPUT_BYTES => Some(OUTPUT_RANGE),
        _ => None,
    };
    if let Some((min, max)) = range {
        let n = value
            .as_u64()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| {
                AppError::validation(format!("{key} must be an integer between {min} and {max}."))
            })?;
        debug_assert!(n >= min);
    }
    let now = time_util::now();
    conn.execute(
        "INSERT INTO settings (key, value_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
        params![key, value_json, now],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionLimits {
    pub timeout_seconds: u64,
    pub max_output_bytes: u64,
}

pub fn execution_limits(conn: &Connection) -> AppResult<ExecutionLimits> {
    let read = |key: &str, default: u64, (min, max): (u64, u64)| -> AppResult<u64> {
        Ok(get(conn, key)?
            .and_then(|v| serde_json::from_str::<u64>(&v).ok())
            .filter(|n| (min..=max).contains(n))
            .unwrap_or(default))
    };
    Ok(ExecutionLimits {
        timeout_seconds: read(
            EXECUTION_TIMEOUT_SECONDS,
            DEFAULT_TIMEOUT_SECONDS,
            TIMEOUT_RANGE,
        )?,
        max_output_bytes: read(
            EXECUTION_MAX_OUTPUT_BYTES,
            DEFAULT_MAX_OUTPUT_BYTES,
            OUTPUT_RANGE,
        )?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::error::ErrorCode;

    #[test]
    fn round_trip_upsert_keeps_created_at() {
        let db = Database::open_in_memory();
        db.write(|c| set(c, THEME, "\"dark\"")).unwrap();
        let created: String = db
            .read(|c| {
                Ok(c.query_row(
                    "SELECT created_at FROM settings WHERE key='theme'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        db.write(|c| set(c, THEME, "\"light\"")).unwrap();
        let (value, created2, updated): (String, String, String) = db
            .read(|c| {
                Ok(c.query_row(
                    "SELECT value_json, created_at, updated_at FROM settings WHERE key='theme'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap();
        assert_eq!(value, "\"light\"");
        assert_eq!(created, created2);
        assert!(updated > created);
    }

    #[test]
    fn rejects_unknown_keys_invalid_json_and_out_of_range_limits() {
        let db = Database::open_in_memory();
        for (k, v) in [
            ("githubToken", "\"x\""),
            (THEME, "dark"),
            (EXECUTION_TIMEOUT_SECONDS, "0"),
            (EXECUTION_TIMEOUT_SECONDS, "\"60\""),
            (EXECUTION_MAX_OUTPUT_BYTES, "99999999999"),
        ] {
            let err = db.write(|c| set(c, k, v)).unwrap_err();
            assert_eq!(err.code, ErrorCode::ValidationFailed, "{k}={v}");
        }
        let long = format!("\"{}\"", "a".repeat(5000));
        assert!(db.write(|c| set(c, THEME, &long)).is_err());
    }

    #[test]
    fn execution_limits_default_and_override() {
        let db = Database::open_in_memory();
        let defaults = db.read(execution_limits).unwrap();
        assert_eq!(defaults.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
        db.write(|c| set(c, EXECUTION_TIMEOUT_SECONDS, "30"))
            .unwrap();
        assert_eq!(db.read(execution_limits).unwrap().timeout_seconds, 30);
    }
}
