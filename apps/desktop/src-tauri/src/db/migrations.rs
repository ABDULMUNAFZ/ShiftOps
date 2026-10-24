//! Ordered, transactional schema migrations.
//!
//! - Each migration has a unique, increasing version and runs in its own
//!   transaction together with its bookkeeping row and `PRAGMA user_version`.
//!   A failure rolls that migration back completely and stops.
//! - Applied migrations are recorded in `schema_migrations`.
//! - Databases created before tracking existed (schema version 1) are detected
//!   by `user_version` and backfilled, then upgraded in place.
//! - Before upgrading an existing database, a copy is written with
//!   `VACUUM INTO` to `backups/`.
//! - Released migrations must never be edited; append a new one instead.
//! - No `STRICT` tables: macOS ships sqlite3 3.32, which cannot open them.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::time_util;

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "settings",
        // Released in Step 1. Kept byte-for-byte.
        sql: "CREATE TABLE settings (
        key TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL,
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
    );",
    },
    Migration {
        version: 2,
        name: "settings_value_json_and_created_at",
        sql: include_str!("sql/0002_settings_value_json.sql"),
    },
    Migration {
        version: 3,
        name: "domain_tables",
        sql: include_str!("sql/0003_domain_tables.sql"),
    },
    Migration {
        version: 4,
        name: "credential_metadata",
        sql: include_str!("sql/0004_credential_metadata.sql"),
    },
    Migration {
        version: 5,
        name: "repository_roots_and_git_state",
        sql: include_str!("sql/0005_repository_roots_and_git_state.sql"),
    },
];

pub fn latest_version() -> u32 {
    MIGRATIONS.last().map_or(0, |m| m.version)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedMigration {
    pub version: u32,
    pub name: String,
    pub applied_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct MigrationReport {
    pub from_version: u32,
    pub to_version: u32,
    pub applied: Vec<AppliedMigration>,
    pub backfilled: Vec<u32>,
    pub backup_path: Option<PathBuf>,
}

const BOOKKEEPING: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL
);";

pub fn user_version(conn: &Connection) -> AppResult<u32> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

fn migration_error(version: u32, name: &str, err: impl std::fmt::Display) -> AppError {
    let e = AppError::new(
        ErrorCode::DatabaseMigrationFailed,
        format!("Database migration {version} ({name}) failed and was rolled back."),
    )
    .with_details(err.to_string())
    .with_context("version", version)
    .with_context("migration", name);
    e.log();
    e
}

pub fn migrate(
    conn: &Connection,
    migrations: &[Migration],
    backup_dir: Option<&Path>,
) -> AppResult<MigrationReport> {
    for pair in migrations.windows(2) {
        if pair[1].version != pair[0].version + 1 {
            return Err(AppError::internal("migration versions must be contiguous"));
        }
    }

    let from_version = user_version(conn)?;
    let mut report = MigrationReport {
        from_version,
        to_version: from_version,
        ..Default::default()
    };

    conn.execute_batch(BOOKKEEPING)?;

    // Backfill bookkeeping for versions applied before tracking existed.
    let recorded: u32 =
        conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))?;
    if recorded == 0 && from_version > 0 {
        let now = time_util::now();
        for m in migrations.iter().filter(|m| m.version <= from_version) {
            conn.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![m.version, m.name, now],
            )?;
            report.backfilled.push(m.version);
        }
    }

    let pending: Vec<&Migration> = migrations
        .iter()
        .filter(|m| m.version > from_version)
        .collect();
    if pending.is_empty() {
        return Ok(report);
    }

    if from_version > 0 {
        if let Some(dir) = backup_dir {
            report.backup_path = Some(backup(conn, dir, from_version)?);
        }
    }

    for m in pending {
        apply(conn, m).map_err(|e| migration_error(m.version, m.name, e))?;
        report.to_version = m.version;
        report.applied.push(AppliedMigration {
            version: m.version,
            name: m.name.to_string(),
            applied_at: conn.query_row(
                "SELECT applied_at FROM schema_migrations WHERE version = ?1",
                [m.version],
                |r| r.get(0),
            )?,
        });
    }
    Ok(report)
}

fn apply(conn: &Connection, m: &Migration) -> rusqlite::Result<()> {
    // `unchecked_transaction` because the caller holds a shared reference; the
    // migration runner is the only user of the connection while it runs.
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(m.sql)?;
    tx.execute(
        "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![m.version, m.name, time_util::now()],
    )?;
    tx.pragma_update(None, "user_version", m.version)?;
    // Verify referential integrity before committing: a migration that
    // rebuilds tables must not leave dangling references.
    let violations: i64 =
        tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })?;
    if violations > 0 {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY),
            Some(format!(
                "{violations} foreign key violations after migration"
            )),
        ));
    }
    tx.commit()
}

fn backup(conn: &Connection, dir: &Path, version: u32) -> AppResult<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|e| {
        AppError::new(
            ErrorCode::DatabaseMigrationFailed,
            "Could not create the database backup directory.",
        )
        .with_details(e.to_string())
    })?;
    let path = dir.join(format!(
        "shiftops-v{version}-{}.db",
        time_util::file_stamp()
    ));
    conn.execute("VACUUM INTO ?1", [path.display().to_string()])
        .map_err(|e| {
            AppError::new(
                ErrorCode::DatabaseMigrationFailed,
                "Could not back up the database before migrating.",
            )
            .with_details(e.to_string())
        })?;
    Ok(path)
}

pub fn applied(conn: &Connection) -> AppResult<Vec<AppliedMigration>> {
    let mut stmt =
        conn.prepare("SELECT version, name, applied_at FROM schema_migrations ORDER BY version")?;
    let rows = stmt.query_map([], |r| {
        Ok(AppliedMigration {
            version: r.get(0)?,
            name: r.get(1)?,
            applied_at: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn
    }

    fn tables(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn empty_database_reaches_latest_version() {
        let conn = fresh();
        let report = migrate(&conn, MIGRATIONS, None).unwrap();
        assert_eq!(report.from_version, 0);
        assert_eq!(report.to_version, latest_version());
        assert_eq!(report.applied.len(), MIGRATIONS.len());
        assert_eq!(user_version(&conn).unwrap(), latest_version());
        assert_eq!(
            tables(&conn),
            [
                "activity_events",
                "agent_runs",
                "agent_tasks",
                "campaign_repositories",
                "campaigns",
                "credential_metadata",
                "migration_patterns",
                "repositories",
                "repository_dependencies",
                "repository_roots",
                "repository_scans",
                "schema_migrations",
                "settings",
                "verification_checks",
                "verification_runs",
            ]
        );
    }

    #[test]
    fn rerunning_is_idempotent() {
        let conn = fresh();
        migrate(&conn, MIGRATIONS, None).unwrap();
        let again = migrate(&conn, MIGRATIONS, None).unwrap();
        assert!(again.applied.is_empty());
        assert_eq!(applied(&conn).unwrap().len(), MIGRATIONS.len());
    }

    #[test]
    fn version_one_database_upgrades_without_data_loss() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v1.db");
        {
            // Exactly what a Step 1 build created.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0].sql).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute_batch(
                "INSERT INTO settings (key, value, updated_at) VALUES ('theme', '\"dark\"', '2026-10-24T10:00:00.000Z');
                 INSERT INTO settings (key, value, updated_at) VALUES ('onboardingCompletedAt', '\"2026-10-24T10:01:00.000Z\"', '2026-10-24T10:01:00.000Z');",
            )
            .unwrap();
        }
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        let report = migrate(&conn, MIGRATIONS, Some(&dir.path().join("backups"))).unwrap();

        assert_eq!(report.from_version, 1);
        assert_eq!(report.backfilled, [1]);
        assert_eq!(
            report.applied.iter().map(|m| m.version).collect::<Vec<_>>(),
            (2..=latest_version()).collect::<Vec<_>>()
        );
        let rows: Vec<(String, String, String, String)> = conn
            .prepare("SELECT key, value_json, created_at, updated_at FROM settings ORDER BY key")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "onboardingCompletedAt".into(),
                    "\"2026-10-24T10:01:00.000Z\"".into(),
                    "2026-10-24T10:01:00.000Z".into(),
                    "2026-10-24T10:01:00.000Z".into()
                ),
                (
                    "theme".into(),
                    "\"dark\"".into(),
                    "2026-10-24T10:00:00.000Z".into(),
                    "2026-10-24T10:00:00.000Z".into()
                ),
            ]
        );

        // The pre-migration backup is a readable version-1 database.
        let backup = report.backup_path.expect("backup written");
        let b = Connection::open(&backup).unwrap();
        assert_eq!(user_version(&b).unwrap(), 1);
        let theme: String = b
            .query_row("SELECT value FROM settings WHERE key = 'theme'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(theme, "\"dark\"");
    }

    /// Start from every released schema version with representative rows,
    /// upgrade to the latest, and check nothing was lost.
    #[test]
    fn every_schema_version_upgrades_to_latest_without_data_loss() {
        for from in 1..=latest_version() {
            let conn = fresh();
            migrate(&conn, &MIGRATIONS[..from as usize], None).unwrap();
            assert_eq!(user_version(&conn).unwrap(), from);
            let settings_column = if from == 1 { "value" } else { "value_json" };
            let extra = if from == 1 { "" } else { ", created_at" };
            let extra_val = if from == 1 {
                ""
            } else {
                ", '2026-10-24T10:00:00.000Z'"
            };
            conn.execute_batch(&format!(
                "INSERT INTO settings (key, {settings_column}, updated_at{extra}) VALUES ('theme', '\"dark\"', '2026-10-24T10:00:00.000Z'{extra_val});"
            ))
            .unwrap();
            if from >= 3 {
                conn.execute_batch(
                    "INSERT INTO repositories (id, source_type, name, local_path, trust_status, created_at, updated_at)
                     VALUES ('r1', 'local', 'api', '/work/api', 'trusted', 't', 't');
                     INSERT INTO activity_events (id, entity_type, event_type, severity, summary, created_at)
                     VALUES ('a1', 'app', 'x', 'info', 'kept', 't');",
                )
                .unwrap();
            }
            if from >= 4 {
                conn.execute_batch(
                    "INSERT INTO credential_metadata (provider, credential_kind, account, created_at, updated_at)
                     VALUES ('github', 'github_personal_access_token', 'github:github_personal_access_token', 't', 't');",
                )
                .unwrap();
            }

            let report = migrate(&conn, MIGRATIONS, None).unwrap();
            assert_eq!(report.to_version, latest_version(), "from {from}");
            let theme: String = conn
                .query_row(
                    "SELECT value_json FROM settings WHERE key = 'theme'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(theme, "\"dark\"", "from {from}");
            if from >= 3 {
                let (trust, root): (String, Option<String>) = conn
                    .query_row(
                        "SELECT trust_status, root_id FROM repositories WHERE id = 'r1'",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                assert_eq!((trust.as_str(), root), ("trusted", None), "from {from}");
                let kept: i64 = conn
                    .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(kept, 1);
            }
            if from >= 4 {
                let n: i64 = conn
                    .query_row("SELECT COUNT(*) FROM credential_metadata", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(n, 1);
            }
        }
    }

    #[test]
    fn failed_migration_rolls_back_and_stops() {
        let conn = fresh();
        let broken = [
            Migration { version: 1, name: "one", sql: "CREATE TABLE a (id INTEGER PRIMARY KEY);" },
            Migration {
                version: 2,
                name: "two",
                sql: "CREATE TABLE b (id INTEGER PRIMARY KEY); INSERT INTO a (id) VALUES (1); THIS IS NOT SQL;",
            },
            Migration { version: 3, name: "three", sql: "CREATE TABLE c (id INTEGER PRIMARY KEY);" },
        ];
        let err = migrate(&conn, &broken, None).unwrap_err();
        assert_eq!(err.code, ErrorCode::DatabaseMigrationFailed);
        assert_eq!(user_version(&conn).unwrap(), 1);
        assert_eq!(tables(&conn), ["a", "schema_migrations"]);
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM a", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            rows, 0,
            "partial writes from the failed migration must be rolled back"
        );
        assert_eq!(applied(&conn).unwrap().len(), 1);
    }

    #[test]
    fn rejects_non_contiguous_versions() {
        let conn = fresh();
        let gap = [
            Migration {
                version: 1,
                name: "one",
                sql: "",
            },
            Migration {
                version: 3,
                name: "three",
                sql: "",
            },
        ];
        assert_eq!(
            migrate(&conn, &gap, None).unwrap_err().code,
            ErrorCode::InternalError
        );
    }
}
