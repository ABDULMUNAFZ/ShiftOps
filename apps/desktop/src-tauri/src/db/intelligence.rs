//! Persistence for repository intelligence: facts and conflicts per scan,
//! scan cache lookup and the latest-scan view.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use super::repositories::{self, RepositoryScan};
use super::{json_col, new_id};
use crate::error::AppResult;
use crate::scanner::facts::{Category, Conflict, ConflictValue, Fact};
use crate::time_util;

/// Completed scans kept per repository; older facts are pruned.
const RETAINED_SCANS: u32 = 3;

pub fn find_cached(
    conn: &Connection,
    repository_id: &str,
    cache_key: &str,
) -> AppResult<Option<RepositoryScan>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM repository_scans WHERE repository_id = ?1 AND cache_key = ?2 AND status = 'completed'
             ORDER BY started_at DESC, rowid DESC LIMIT 1",
            params![repository_id, cache_key],
            |r| r.get(0),
        )
        .optional()?;
    id.map(|id| repositories::get_scan(conn, &id)).transpose()
}

/// Store facts and conflicts for a completed scan, stamp its cache key and
/// prune facts of scans older than the retained window.
pub fn store(
    conn: &Connection,
    repository_id: &str,
    scan_id: &str,
    scanner_version: &str,
    cache_key: &str,
    facts: &[Fact],
    conflicts: &[Conflict],
) -> AppResult<()> {
    let now = time_util::now();
    conn.execute(
        "UPDATE repository_scans SET scanner_version = ?2, cache_key = ?3 WHERE id = ?1",
        params![scan_id, scanner_version, cache_key],
    )?;
    let mut insert = conn.prepare(
        "INSERT INTO repository_facts (id, repository_id, scan_id, category, fact_key, value, evidence_file,
             evidence_line, evidence_pointer, detection_method, confidence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;
    for f in facts {
        insert.execute(params![
            new_id(),
            repository_id,
            scan_id,
            f.category.as_str(),
            f.key,
            f.value,
            f.file,
            f.line,
            f.pointer,
            f.method,
            f.confidence,
            now
        ])?;
    }
    for c in conflicts {
        conn.execute(
            "INSERT INTO repository_conflicts (id, repository_id, scan_id, subject, message, values_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![new_id(), repository_id, scan_id, c.subject, c.message, serde_json::to_string(&c.values)?, now],
        )?;
    }
    conn.execute(
        "DELETE FROM repository_scans WHERE repository_id = ?1 AND status IN ('completed', 'failed', 'cancelled')
           AND id NOT IN (SELECT id FROM repository_scans WHERE repository_id = ?1
                          ORDER BY started_at DESC, rowid DESC LIMIT ?2)",
        params![repository_id, RETAINED_SCANS],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredFact {
    pub id: String,
    #[serde(flatten)]
    pub fact: Fact,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredConflict {
    pub id: String,
    pub subject: String,
    pub message: String,
    pub values: Vec<ConflictValue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Intelligence {
    /// Latest completed scan, if any.
    pub scan: Option<RepositoryScan>,
    /// Latest scan of any status (shows a failure or cancellation after a success).
    pub latest_attempt: Option<RepositoryScan>,
    pub facts: Vec<StoredFact>,
    pub conflicts: Vec<StoredConflict>,
}

pub fn latest(conn: &Connection, repository_id: &str) -> AppResult<Intelligence> {
    repositories::get(conn, repository_id)?;
    let latest_id = |completed_only: bool| -> AppResult<Option<String>> {
        Ok(conn
            .query_row(
                &format!(
                    "SELECT id FROM repository_scans WHERE repository_id = ?1 {} ORDER BY started_at DESC, rowid DESC LIMIT 1",
                    if completed_only { "AND status = 'completed'" } else { "" }
                ),
                [repository_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
    };
    let scan = latest_id(true)?
        .map(|id| repositories::get_scan(conn, &id))
        .transpose()?;
    let latest_attempt = latest_id(false)?
        .map(|id| repositories::get_scan(conn, &id))
        .transpose()?;
    let (mut facts, mut conflicts) = (Vec::new(), Vec::new());
    if let Some(s) = &scan {
        let mut stmt = conn.prepare(
            "SELECT id, category, fact_key, value, evidence_file, evidence_line, evidence_pointer, detection_method, confidence
             FROM repository_facts WHERE scan_id = ?1 ORDER BY category, fact_key, evidence_file, rowid",
        )?;
        facts = stmt
            .query_map([&s.id], |r| {
                let category: String = r.get(1)?;
                Ok(StoredFact {
                    id: r.get(0)?,
                    fact: Fact {
                        category: Category::parse(&category).unwrap_or(Category::Metadata),
                        key: r.get(2)?,
                        value: r.get(3)?,
                        file: r.get(4)?,
                        line: r.get(5)?,
                        pointer: r.get(6)?,
                        method: r.get(7)?,
                        confidence: r.get(8)?,
                    },
                })
            })?
            .collect::<Result<_, _>>()?;
        let mut stmt = conn.prepare(
            "SELECT id, subject, message, values_json FROM repository_conflicts WHERE scan_id = ?1 ORDER BY subject, rowid",
        )?;
        conflicts = stmt
            .query_map([&s.id], |r| {
                let values: Value = json_col(r, "values_json")?;
                Ok(StoredConflict {
                    id: r.get(0)?,
                    subject: r.get(1)?,
                    message: r.get(2)?,
                    values: serde_json::from_value(values).unwrap_or_default(),
                })
            })?
            .collect::<Result<_, _>>()?;
    }
    Ok(Intelligence {
        scan,
        latest_attempt,
        facts,
        conflicts,
    })
}
