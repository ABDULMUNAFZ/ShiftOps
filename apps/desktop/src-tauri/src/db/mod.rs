//! Local persistence. One SQLite connection behind a mutex: ShiftOps is a
//! single-user desktop app and every operation is short. Services in the
//! submodules expose typed operations; nothing else in the app writes SQL.

#![cfg_attr(not(test), allow(dead_code))] // Shared helpers used by service operations of later steps.

pub mod activity;
pub mod agents;
pub mod campaigns;
pub mod credentials;
pub mod intelligence;
pub mod migrations;
pub mod patterns;
pub mod repositories;
pub mod roots;
pub mod settings;
pub mod verification;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorCode};

pub const DB_FILE_NAME: &str = "shiftops.db";

pub struct Database {
    conn: Mutex<Connection>,
    path: Option<PathBuf>,
}

/// Configure a connection. Foreign keys are off by default in SQLite and the
/// setting is per connection, so it is applied every time one is opened.
fn configure(conn: &Connection, file_backed: bool) -> AppResult<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    if file_backed {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
    }
    let enabled: i64 = conn.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
    if enabled != 1 {
        return Err(AppError::new(
            ErrorCode::DatabaseUnavailable,
            "SQLite foreign key enforcement could not be enabled.",
        ));
    }
    Ok(())
}

impl Database {
    /// Open (creating if needed) and migrate to the latest schema.
    pub fn open(path: &Path) -> AppResult<(Self, migrations::MigrationReport)> {
        let conn = Connection::open(path).map_err(|e| {
            AppError::new(
                ErrorCode::DatabaseUnavailable,
                "The local database could not be opened.",
            )
            .with_details(e.to_string())
            .with_context("path", path.display().to_string())
        })?;
        configure(&conn, true)?;
        let backup_dir = path.parent().map(|p| p.join("backups"));
        let report = migrations::migrate(&conn, migrations::MIGRATIONS, backup_dir.as_deref())?;
        Ok((
            Self {
                conn: Mutex::new(conn),
                path: Some(path.to_path_buf()),
            },
            report,
        ))
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Self {
        let conn = Connection::open_in_memory().unwrap();
        configure(&conn, false).unwrap();
        migrations::migrate(&conn, migrations::MIGRATIONS, None).unwrap();
        Self {
            conn: Mutex::new(conn),
            path: None,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn lock(&self) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| AppError::internal("database connection mutex poisoned"))
    }

    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
        let guard = self.lock()?;
        f(&guard)
    }

    /// Run `f` in a transaction. Any error rolls back every write made by `f`.
    pub fn write<T>(&self, f: impl FnOnce(&Transaction<'_>) -> AppResult<T>) -> AppResult<T> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }
}

/// Generates a string-backed enum with serde, `as_str`, parsing and SQLite
/// conversions. The string values match the SQL `CHECK` constraints.
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub enum $name {
            $(#[serde(rename = $value)] $variant),+
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $value),+ }
            }

            pub fn parse(value: &str) -> $crate::error::AppResult<Self> {
                match value {
                    $($value => Ok($name::$variant),)+
                    other => Err($crate::error::AppError::validation(format!(
                        "Invalid {}: {other}", stringify!($name)
                    ))),
                }
            }
        }

        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.as_str().into())
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                let text = value.as_str()?;
                $name::parse(text).map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
            }
        }
    };
}
pub(crate) use string_enum;

/// Pagination and filtering accepted by every list operation.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    /// Status values to include. Empty means all.
    pub statuses: Vec<String>,
    /// Case-insensitive substring search over the service's text columns.
    pub search: Option<String>,
}

pub const DEFAULT_PAGE_SIZE: u32 = 50;
pub const MAX_PAGE_SIZE: u32 = 200;

impl ListParams {
    pub fn limit(&self) -> u32 {
        self.limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE)
    }

    pub fn offset(&self) -> u32 {
        self.offset.unwrap_or(0)
    }

    pub fn search_pattern(&self) -> Option<String> {
        let term = self.search.as_deref()?.trim();
        if term.is_empty() {
            return None;
        }
        let escaped = term
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        Some(format!("%{escaped}%"))
    }

    /// Validate status filters against an enum's allowed values.
    pub fn statuses_as<T: Copy>(&self, parse: impl Fn(&str) -> AppResult<T>) -> AppResult<Vec<T>> {
        self.statuses.iter().map(|s| parse(s)).collect()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}

/// Builds `WHERE` clauses with positional parameters for list queries.
#[derive(Default)]
pub struct Filter {
    clauses: Vec<String>,
    pub params: Vec<rusqlite::types::Value>,
}

impl Filter {
    pub fn eq(&mut self, column: &str, value: impl Into<rusqlite::types::Value>) -> &mut Self {
        self.params.push(value.into());
        self.clauses
            .push(format!("{column} = ?{}", self.params.len()));
        self
    }

    /// Add a clause whose single `{}` placeholder binds `value`.
    pub fn clause(
        &mut self,
        template: &str,
        value: impl Into<rusqlite::types::Value>,
    ) -> &mut Self {
        self.params.push(value.into());
        self.clauses
            .push(template.replace("{}", &format!("?{}", self.params.len())));
        self
    }

    pub fn either_eq(
        &mut self,
        a: &str,
        b: &str,
        value: impl Into<rusqlite::types::Value>,
    ) -> &mut Self {
        self.params.push(value.into());
        let n = self.params.len();
        self.clauses.push(format!("({a} = ?{n} OR {b} = ?{n})"));
        self
    }

    pub fn any_of(&mut self, column: &str, values: &[&str]) -> &mut Self {
        if values.is_empty() {
            return self;
        }
        let mut placeholders = Vec::new();
        for v in values {
            self.params.push((*v).to_string().into());
            placeholders.push(format!("?{}", self.params.len()));
        }
        self.clauses
            .push(format!("{column} IN ({})", placeholders.join(", ")));
        self
    }

    pub fn search(&mut self, columns: &[&str], pattern: Option<String>) -> &mut Self {
        let Some(pattern) = pattern else { return self };
        self.params.push(pattern.into());
        let n = self.params.len();
        let ors: Vec<String> = columns
            .iter()
            .map(|c| format!("{c} LIKE ?{n} ESCAPE '\\'"))
            .collect();
        self.clauses.push(format!("({})", ors.join(" OR ")));
        self
    }

    pub fn sql(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.clauses.join(" AND "))
        }
    }
}

/// Run a paginated query: `select` is `SELECT <cols> FROM <table>`.
pub fn paginate<T>(
    conn: &Connection,
    select: &str,
    from: &str,
    filter: &Filter,
    order_by: &str,
    params: &ListParams,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> AppResult<Page<T>> {
    let where_sql = filter.sql();
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM {from} {where_sql}"),
        rusqlite::params_from_iter(filter.params.iter()),
        |r| r.get(0),
    )?;
    let limit = params.limit();
    let offset = params.offset();
    let mut all = filter.params.clone();
    all.push(i64::from(limit).into());
    all.push(i64::from(offset).into());
    let sql = format!(
        "{select} FROM {from} {where_sql} ORDER BY {order_by} LIMIT ?{} OFFSET ?{}",
        all.len() - 1,
        all.len()
    );
    let mut stmt = conn.prepare(&sql)?;
    let items = stmt
        .query_map(rusqlite::params_from_iter(all.iter()), map)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Page {
        items,
        total: u64::try_from(total).unwrap_or(0),
        limit,
        offset,
    })
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Read a JSON text column into a value. Columns are written only through
/// `json_text`, so a parse failure indicates corruption and is surfaced.
pub fn json_col(row: &rusqlite::Row<'_>, idx: &str) -> rusqlite::Result<serde_json::Value> {
    let text: String = row.get(idx)?;
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub fn json_col_opt(
    row: &rusqlite::Row<'_>,
    idx: &str,
) -> rusqlite::Result<Option<serde_json::Value>> {
    let text: Option<String> = row.get(idx)?;
    text.map(|t| {
        serde_json::from_str(&t).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
    })
    .transpose()
}

pub const MAX_JSON_BYTES: usize = 256 * 1024;

/// Serialise a JSON value for storage, enforcing an expected top-level shape.
pub fn json_text(value: &serde_json::Value, field: &str, expect: JsonShape) -> AppResult<String> {
    let ok = match expect {
        JsonShape::Object => value.is_object(),
        JsonShape::Array => value.is_array(),
        JsonShape::Any => true,
    };
    if !ok {
        return Err(AppError::validation(format!(
            "{field} must be a JSON {expect:?}."
        )));
    }
    let text = serde_json::to_string(value)?;
    if text.len() > MAX_JSON_BYTES {
        return Err(AppError::validation(format!(
            "{field} exceeds {MAX_JSON_BYTES} bytes."
        )));
    }
    Ok(text)
}

#[derive(Debug, Clone, Copy)]
pub enum JsonShape {
    Object,
    Array,
    Any,
}

pub fn require_text(value: &str, field: &str, max: usize) -> AppResult<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation(format!("{field} is required.")));
    }
    if value.chars().count() > max {
        return Err(AppError::validation(format!(
            "{field} must be at most {max} characters."
        )));
    }
    Ok(())
}

pub fn require_unit_interval(value: f64, field: &str) -> AppResult<()> {
    if !(0.0..=1.0).contains(&value) || value.is_nan() {
        return Err(AppError::validation(format!(
            "{field} must be between 0 and 1."
        )));
    }
    Ok(())
}

/// Map "no rows changed" to RECORD_NOT_FOUND for update/delete operations.
pub fn expect_changed(changed: usize, entity: &str, id: &str) -> AppResult<()> {
    if changed == 0 {
        Err(AppError::not_found(entity, id))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod rehearsal {
    /// Upgrade rehearsal against a copy of a real database:
    /// `SHIFTOPS_UPGRADE_DB=/path/copy.db cargo test upgrade_rehearsal -- --ignored --nocapture`
    #[test]
    #[ignore = "manual: needs SHIFTOPS_UPGRADE_DB pointing at a copy of a real database"]
    fn upgrade_rehearsal() {
        let path = std::env::var("SHIFTOPS_UPGRADE_DB").expect("SHIFTOPS_UPGRADE_DB");
        let (db, report) = super::Database::open(std::path::Path::new(&path)).expect("open");
        println!(
            "from {} to {}; applied {:?}; backfilled {:?}; backup {:?}",
            report.from_version,
            report.to_version,
            report.applied.iter().map(|m| m.version).collect::<Vec<_>>(),
            report.backfilled,
            report.backup_path
        );
        let settings = db.read(super::settings::all).unwrap();
        println!("settings after upgrade: {settings:?}");
    }
}
