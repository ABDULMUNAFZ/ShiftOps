//! Local application state in SQLite. Schema changes are applied as ordered
//! migrations tracked by `PRAGMA user_version`.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};

pub const DB_FILE_NAME: &str = "shiftops.db";

/// Setting keys the frontend may read and write. Anything else is rejected so
/// the settings table cannot be used as a general-purpose key-value store.
pub const ALLOWED_SETTING_KEYS: &[&str] = &["theme", "sidebarCollapsed", "onboardingCompletedAt"];
const MAX_SETTING_BYTES: usize = 4096;

const MIGRATIONS: &[&str] = &[
    // 1: settings
    "CREATE TABLE settings (
        key TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );",
];

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> AppResult<Self> {
        let mut store = Self {
            conn: Connection::open_in_memory()?,
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> AppResult<()> {
        let current = self.schema_version()? as usize;
        let tx = self.conn.transaction()?;
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (index + 1) as u32)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn schema_version(&self) -> AppResult<u32> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    pub fn settings(&self) -> AppResult<BTreeMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    #[cfg(test)]
    pub fn setting(&self, key: &str) -> AppResult<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    /// Store a JSON-encoded value. The value must parse as JSON so the
    /// frontend schema layer always receives well-formed input.
    pub fn set_setting(&self, key: &str, value: &str) -> AppResult<()> {
        if !ALLOWED_SETTING_KEYS.contains(&key) {
            return Err(AppError::InvalidInput(format!(
                "unknown setting key: {key}"
            )));
        }
        if value.len() > MAX_SETTING_BYTES {
            return Err(AppError::InvalidInput(format!(
                "setting value exceeds {MAX_SETTING_BYTES} bytes"
            )));
        }
        serde_json::from_str::<serde_json::Value>(value)
            .map_err(|e| AppError::InvalidInput(format!("setting value is not valid JSON: {e}")))?;
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_set_schema_version() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap() as usize, MIGRATIONS.len());
    }

    #[test]
    fn settings_round_trip_and_upsert() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.setting("theme").unwrap(), None);
        store.set_setting("theme", "\"dark\"").unwrap();
        store.set_setting("theme", "\"light\"").unwrap();
        assert_eq!(
            store.setting("theme").unwrap().as_deref(),
            Some("\"light\"")
        );
        assert_eq!(store.settings().unwrap().len(), 1);
    }

    #[test]
    fn rejects_unknown_keys_and_invalid_json() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.set_setting("githubToken", "\"x\"").is_err());
        assert!(store.set_setting("theme", "dark").is_err());
        assert!(store
            .set_setting("theme", &format!("\"{}\"", "a".repeat(5000)))
            .is_err());
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DB_FILE_NAME);
        Store::open(&path)
            .unwrap()
            .set_setting("theme", "\"dark\"")
            .unwrap();
        let reopened = Store::open(&path).unwrap();
        assert_eq!(
            reopened.setting("theme").unwrap().as_deref(),
            Some("\"dark\"")
        );
        assert_eq!(
            reopened.schema_version().unwrap() as usize,
            MIGRATIONS.len()
        );
    }
}
