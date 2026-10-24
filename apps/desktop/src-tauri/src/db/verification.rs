//! Verification runs and their individual checks.

// Service operations beyond the read paths are used by later steps
// (discovery, planning, delegation, verification) and are covered by
// this module's tests; only test builds exercise them today.
#![cfg_attr(not(test), allow(dead_code))]

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    json_col, json_col_opt, json_text, new_id, paginate, require_text, string_enum, Filter,
    JsonShape, ListParams, Page,
};
use crate::error::{AppError, AppResult};
use crate::redact::redact;
use crate::time_util;

string_enum!(VerificationStatus {
    Queued => "queued",
    Running => "running",
    Passed => "passed",
    Failed => "failed",
    Error => "error",
    Cancelled => "cancelled",
});
string_enum!(RunTrigger { Manual => "manual", AfterRepair => "after_repair" });
string_enum!(CheckCategory {
    Install => "install",
    Build => "build",
    Test => "test",
    Lint => "lint",
    Typecheck => "typecheck",
    Security => "security",
    Custom => "custom",
});
string_enum!(CheckStatus {
    Pending => "pending",
    Running => "running",
    Passed => "passed",
    Failed => "failed",
    Skipped => "skipped",
    Error => "error",
    Cancelled => "cancelled",
});

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationRun {
    pub id: String,
    pub campaign_repository_id: String,
    pub agent_task_id: Option<String>,
    pub profile: String,
    pub verified_commit_sha: String,
    pub status: VerificationStatus,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub summary: Value,
    /// Hash of the verification plan this run executed.
    pub plan_hash: Option<String>,
    pub trigger: RunTrigger,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationCheck {
    pub id: String,
    pub verification_run_id: String,
    pub name: String,
    pub category: CheckCategory,
    pub command: Option<Value>,
    pub status: CheckStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<i64>,
    pub redacted_output: Option<String>,
    pub evidence: Option<Value>,
    pub suggested_action: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewVerificationRun {
    pub campaign_repository_id: String,
    pub agent_task_id: Option<String>,
    pub profile: String,
    pub verified_commit_sha: String,
    #[serde(default)]
    pub plan_hash: Option<String>,
    #[serde(default = "manual")]
    pub trigger: RunTrigger,
}

fn manual() -> RunTrigger {
    RunTrigger::Manual
}

const RUN_COLUMNS: &str =
    "SELECT id, campaign_repository_id, agent_task_id, profile, verified_commit_sha, status, \
    started_at, completed_at, summary_json, plan_hash, trigger";

fn map_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<VerificationRun> {
    Ok(VerificationRun {
        id: r.get("id")?,
        campaign_repository_id: r.get("campaign_repository_id")?,
        agent_task_id: r.get("agent_task_id")?,
        profile: r.get("profile")?,
        verified_commit_sha: r.get("verified_commit_sha")?,
        status: r.get("status")?,
        started_at: r.get("started_at")?,
        completed_at: r.get("completed_at")?,
        summary: json_col(r, "summary_json")?,
        plan_hash: r.get("plan_hash")?,
        trigger: r.get("trigger")?,
    })
}

const CHECK_COLUMNS: &str =
    "SELECT id, verification_run_id, name, category, command_json, status, exit_code, \
    duration_ms, redacted_output, evidence_json, suggested_action, started_at, completed_at";

fn map_check(r: &rusqlite::Row<'_>) -> rusqlite::Result<VerificationCheck> {
    Ok(VerificationCheck {
        id: r.get("id")?,
        verification_run_id: r.get("verification_run_id")?,
        name: r.get("name")?,
        category: r.get("category")?,
        command: json_col_opt(r, "command_json")?,
        status: r.get("status")?,
        exit_code: r.get("exit_code")?,
        duration_ms: r.get("duration_ms")?,
        redacted_output: r.get("redacted_output")?,
        evidence: json_col_opt(r, "evidence_json")?,
        suggested_action: r.get("suggested_action")?,
        started_at: r.get("started_at")?,
        completed_at: r.get("completed_at")?,
    })
}

pub fn create_run(conn: &Connection, new: &NewVerificationRun) -> AppResult<VerificationRun> {
    require_text(&new.profile, "Profile", 64)?;
    require_text(&new.verified_commit_sha, "Verified commit", 64)?;
    let id = new_id();
    conn.execute(
        "INSERT INTO verification_runs (id, campaign_repository_id, agent_task_id, profile, verified_commit_sha,
             status, started_at, plan_hash, trigger) VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6, ?7, ?8)",
        params![id, new.campaign_repository_id, new.agent_task_id, new.profile, new.verified_commit_sha,
            time_util::now(), new.plan_hash, new.trigger],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::validation("A verification run is already live for this repository.")
        } else {
            e.into()
        }
    })?;
    get_run(conn, &id)
}

pub fn get_run(conn: &Connection, id: &str) -> AppResult<VerificationRun> {
    conn.query_row(
        &format!("{RUN_COLUMNS} FROM verification_runs WHERE id = ?1"),
        [id],
        map_run,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Verification run", id))
}

pub fn finish_run(
    conn: &Connection,
    id: &str,
    status: VerificationStatus,
    summary: &Value,
) -> AppResult<VerificationRun> {
    if matches!(
        status,
        VerificationStatus::Queued | VerificationStatus::Running
    ) {
        return Err(AppError::validation(
            "A verification run can only finish with a terminal status.",
        ));
    }
    let current = get_run(conn, id)?;
    if !matches!(
        current.status,
        VerificationStatus::Queued | VerificationStatus::Running
    ) {
        return Err(AppError::validation(format!(
            "Verification run is already {}.",
            current.status.as_str()
        )));
    }
    let summary_json = json_text(summary, "summary", JsonShape::Object)?;
    conn.execute(
        "UPDATE verification_runs SET status = ?2, summary_json = ?3, completed_at = ?4 WHERE id = ?1",
        params![id, status, summary_json, time_util::now()],
    )?;
    get_run(conn, id)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RunQuery {
    #[serde(flatten)]
    pub page: ListParams,
    pub campaign_repository_id: Option<String>,
}

pub fn list_runs(conn: &Connection, query: &RunQuery) -> AppResult<Page<VerificationRun>> {
    let statuses = query.page.statuses_as(VerificationStatus::parse)?;
    let values: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
    let mut filter = Filter::default();
    filter.any_of("status", &values);
    if let Some(id) = &query.campaign_repository_id {
        filter.eq("campaign_repository_id", id.clone());
    }
    filter.search(
        &["profile", "verified_commit_sha"],
        query.page.search_pattern(),
    );
    paginate(
        conn,
        RUN_COLUMNS,
        "verification_runs",
        &filter,
        "started_at DESC, rowid DESC",
        &query.page,
        map_run,
    )
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCheck {
    pub name: String,
    pub category: CheckCategory,
    pub command: Option<Value>,
}

pub fn start_check(
    conn: &Connection,
    run_id: &str,
    new: &NewCheck,
) -> AppResult<VerificationCheck> {
    require_text(&new.name, "Check name", 200)?;
    let command = new
        .command
        .as_ref()
        .map(|c| json_text(c, "command", JsonShape::Array))
        .transpose()?;
    let id = new_id();
    conn.execute(
        "INSERT INTO verification_checks (id, verification_run_id, name, category, command_json, status, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6)",
        params![id, run_id, new.name, new.category, command, time_util::now()],
    )?;
    get_check(conn, &id)
}

pub fn get_check(conn: &Connection, id: &str) -> AppResult<VerificationCheck> {
    conn.query_row(
        &format!("{CHECK_COLUMNS} FROM verification_checks WHERE id = ?1"),
        [id],
        map_check,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Verification check", id))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckOutcome {
    pub status: CheckStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<i64>,
    pub output: Option<String>,
    pub evidence: Option<Value>,
    pub suggested_action: Option<String>,
}

const MAX_CHECK_OUTPUT_CHARS: usize = 64 * 1024;

pub fn finish_check(
    conn: &Connection,
    id: &str,
    outcome: &CheckOutcome,
) -> AppResult<VerificationCheck> {
    if matches!(outcome.status, CheckStatus::Pending | CheckStatus::Running) {
        return Err(AppError::validation(
            "A check can only finish with a terminal status.",
        ));
    }
    let output = outcome.output.as_deref().map(|o| {
        let redacted = redact(o);
        if redacted.chars().count() > MAX_CHECK_OUTPUT_CHARS {
            let tail: String = redacted
                .chars()
                .rev()
                .take(MAX_CHECK_OUTPUT_CHARS)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("[output truncated to the last {MAX_CHECK_OUTPUT_CHARS} characters]\n{tail}")
        } else {
            redacted
        }
    });
    let evidence = outcome
        .evidence
        .as_ref()
        .map(|e| json_text(e, "evidence", JsonShape::Any))
        .transpose()?;
    let changed = conn.execute(
        "UPDATE verification_checks SET status = ?2, exit_code = ?3, duration_ms = ?4, redacted_output = ?5,
             evidence_json = ?6, suggested_action = ?7, completed_at = ?8
         WHERE id = ?1 AND status IN ('pending', 'running')",
        params![id, outcome.status, outcome.exit_code, outcome.duration_ms, output, evidence, outcome.suggested_action, time_util::now()],
    )?;
    if changed == 0 {
        get_check(conn, id)?;
        return Err(AppError::validation(
            "Verification check has already finished.",
        ));
    }
    get_check(conn, id)
}

/// The newest run for a campaign member, if any.
pub fn latest_run_for_member(
    conn: &Connection,
    member_id: &str,
) -> AppResult<Option<VerificationRun>> {
    Ok(conn
        .query_row(
            &format!("{RUN_COLUMNS} FROM verification_runs WHERE campaign_repository_id = ?1 ORDER BY started_at DESC, rowid DESC LIMIT 1"),
            [member_id],
            map_run,
        )
        .optional()?)
}

/// Close runs and checks left live by an interrupted session as `error` /
/// `cancelled`. An interrupted run never counts as passed.
pub fn recover_interrupted(conn: &Connection) -> AppResult<usize> {
    let now = time_util::now();
    conn.execute(
        "UPDATE verification_checks SET status = 'cancelled', completed_at = ?1,
             suggested_action = 'ShiftOps stopped during this check. Run verification again.'
         WHERE status IN ('pending', 'running')",
        [&now],
    )?;
    Ok(conn.execute(
        "UPDATE verification_runs SET status = 'error', completed_at = ?1,
             summary_json = json_set(summary_json, '$.interrupted', json('true'))
         WHERE status IN ('queued', 'running')",
        [&now],
    )?)
}

pub fn list_checks(conn: &Connection, run_id: &str) -> AppResult<Vec<VerificationCheck>> {
    let mut stmt = conn.prepare(&format!(
        "{CHECK_COLUMNS} FROM verification_checks WHERE verification_run_id = ?1 ORDER BY started_at, rowid"
    ))?;
    let rows = stmt.query_map([run_id], map_check)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::agents::tests::{member, task};
    use crate::db::Database;
    use crate::error::ErrorCode;
    use serde_json::json;

    #[test]
    fn run_with_checks_redacts_output_and_cascades() {
        let db = Database::open_in_memory();
        let (_, member_id) = member(&db);
        let t = task(&db, &member_id);
        let run = db
            .write(|c| {
                create_run(
                    c,
                    &NewVerificationRun {
                        plan_hash: None,
                        trigger: RunTrigger::Manual,
                        campaign_repository_id: member_id.clone(),
                        agent_task_id: Some(t.id.clone()),
                        profile: "node".into(),
                        verified_commit_sha: "abc123".into(),
                    },
                )
            })
            .unwrap();
        let check = db
            .write(|c| {
                start_check(
                    c,
                    &run.id,
                    &NewCheck {
                        name: "test".into(),
                        category: CheckCategory::Test,
                        command: Some(json!(["pnpm", "test"])),
                    },
                )
            })
            .unwrap();
        let outcome = CheckOutcome {
            status: CheckStatus::Failed,
            exit_code: Some(1),
            duration_ms: Some(1200),
            output: Some("FAIL\nAuthorization: Bearer verysecretvalue123".into()),
            evidence: None,
            suggested_action: Some("Fix the failing test".into()),
        };
        let finished = db.write(|c| finish_check(c, &check.id, &outcome)).unwrap();
        assert!(!finished
            .redacted_output
            .unwrap()
            .contains("verysecretvalue123"));
        assert!(db.write(|c| finish_check(c, &check.id, &outcome)).is_err());

        let done = db
            .write(|c| {
                finish_run(
                    c,
                    &run.id,
                    VerificationStatus::Failed,
                    &json!({ "failed": ["test"] }),
                )
            })
            .unwrap();
        assert_eq!(done.status, VerificationStatus::Failed);
        assert!(db
            .write(|c| finish_run(c, &run.id, VerificationStatus::Passed, &json!({})))
            .is_err());

        let failed = db
            .read(|c| {
                list_runs(
                    c,
                    &RunQuery {
                        page: ListParams {
                            statuses: vec!["failed".into()],
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(failed.total, 1);

        // Deleting the agent task keeps the verification run but clears the link (SET NULL).
        db.write(|c| Ok(c.execute("DELETE FROM agent_tasks WHERE id = ?1", [&t.id])?))
            .unwrap();
        assert_eq!(
            db.read(|c| get_run(c, &run.id)).unwrap().agent_task_id,
            None
        );

        db.write(|c| Ok(c.execute("DELETE FROM verification_runs WHERE id = ?1", [&run.id])?))
            .unwrap();
        assert!(db.read(|c| list_checks(c, &run.id)).unwrap().is_empty());
    }

    #[test]
    fn requires_existing_membership() {
        let db = Database::open_in_memory();
        let err = db
            .write(|c| {
                create_run(
                    c,
                    &NewVerificationRun {
                        plan_hash: None,
                        trigger: RunTrigger::Manual,
                        campaign_repository_id: "missing".into(),
                        agent_task_id: None,
                        profile: "node".into(),
                        verified_commit_sha: "abc".into(),
                    },
                )
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationFailed);
    }
}
