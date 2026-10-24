//! Agent tasks (one per campaign repository delegation) and their runs.

// Service operations beyond the read paths are used by later steps
// (discovery, planning, delegation, verification) and are covered by
// this module's tests; only test builds exercise them today.
#![cfg_attr(not(test), allow(dead_code))]

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;

use super::{
    expect_changed, json_col_opt, json_text, new_id, paginate, require_text, string_enum, Filter,
    JsonShape, ListParams, Page,
};
use crate::error::{AppError, AppResult};
use crate::time_util;

string_enum!(AgentProvider { Devin => "devin" });
string_enum!(ExecutionMode { CloudSession => "cloud_session", LocalCli => "local_cli" });
string_enum!(TaskStatus {
    Pending => "pending",
    Queued => "queued",
    Running => "running",
    AwaitingReview => "awaiting_review",
    Completed => "completed",
    Failed => "failed",
    Cancelled => "cancelled",
});
string_enum!(RunStatus {
    Starting => "starting",
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
    Cancelled => "cancelled",
    TimedOut => "timed_out",
});

impl TaskStatus {
    fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }
}

impl RunStatus {
    fn is_terminal(self) -> bool {
        !matches!(self, RunStatus::Starting | RunStatus::Running)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    pub id: String,
    pub campaign_repository_id: String,
    pub provider: AgentProvider,
    pub execution_mode: ExecutionMode,
    pub instructions_markdown: String,
    pub external_session_id: Option<String>,
    pub external_session_url: Option<String>,
    pub worktree_path: Option<String>,
    pub branch_name: Option<String>,
    pub commit_sha: Option<String>,
    pub pull_request_url: Option<String>,
    pub status: TaskStatus,
    /// SHA-256 of `instructions_markdown`, pinned at creation. Approval and
    /// dispatch always refer to this exact content.
    pub instructions_hash: Option<String>,
    /// Set once a human approves the task for dispatch. Required before any
    /// Devin process is started.
    pub approved_at: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAgentTask {
    pub campaign_repository_id: String,
    pub provider: AgentProvider,
    pub execution_mode: ExecutionMode,
    pub instructions_markdown: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgress {
    pub external_session_id: Option<String>,
    pub external_session_url: Option<String>,
    pub worktree_path: Option<String>,
    pub branch_name: Option<String>,
    pub commit_sha: Option<String>,
    pub pull_request_url: Option<String>,
}

const TASK_COLUMNS: &str = "SELECT id, campaign_repository_id, provider, execution_mode, instructions_markdown, \
    external_session_id, external_session_url, worktree_path, branch_name, commit_sha, pull_request_url, status, \
    instructions_hash, approved_at, started_at, completed_at, created_at, updated_at";

fn map_task(r: &rusqlite::Row<'_>) -> rusqlite::Result<AgentTask> {
    Ok(AgentTask {
        id: r.get("id")?,
        campaign_repository_id: r.get("campaign_repository_id")?,
        provider: r.get("provider")?,
        execution_mode: r.get("execution_mode")?,
        instructions_markdown: r.get("instructions_markdown")?,
        external_session_id: r.get("external_session_id")?,
        external_session_url: r.get("external_session_url")?,
        worktree_path: r.get("worktree_path")?,
        branch_name: r.get("branch_name")?,
        commit_sha: r.get("commit_sha")?,
        pull_request_url: r.get("pull_request_url")?,
        status: r.get("status")?,
        instructions_hash: r.get("instructions_hash")?,
        approved_at: r.get("approved_at")?,
        started_at: r.get("started_at")?,
        completed_at: r.get("completed_at")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

pub fn create_task(conn: &Connection, new: &NewAgentTask) -> AppResult<AgentTask> {
    require_text(&new.instructions_markdown, "Instructions", 200_000)?;
    let id = new_id();
    let now = time_util::now();
    let hash =
        crate::scanner::files::hex(&sha2::Sha256::digest(new.instructions_markdown.as_bytes()));
    conn.execute(
        "INSERT INTO agent_tasks (id, campaign_repository_id, provider, execution_mode, instructions_markdown,
             status, instructions_hash, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7, ?7)",
        params![id, new.campaign_repository_id, new.provider, new.execution_mode, new.instructions_markdown, hash, now],
    )?;
    get_task(conn, &id)
}

/// Record the human approval required before dispatch. Idempotent.
pub fn approve_task(conn: &Connection, id: &str) -> AppResult<AgentTask> {
    let current = get_task(conn, id)?;
    if current.status.is_terminal() {
        return Err(AppError::validation(format!(
            "Agent task is already {}.",
            current.status.as_str()
        )));
    }
    conn.execute(
        "UPDATE agent_tasks SET approved_at = COALESCE(approved_at, ?2), updated_at = ?2 WHERE id = ?1",
        params![id, time_util::now()],
    )?;
    get_task(conn, id)
}

pub fn get_task(conn: &Connection, id: &str) -> AppResult<AgentTask> {
    conn.query_row(
        &format!("{TASK_COLUMNS} FROM agent_tasks WHERE id = ?1"),
        [id],
        map_task,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Agent task", id))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskQuery {
    #[serde(flatten)]
    pub page: ListParams,
    pub campaign_id: Option<String>,
}

pub fn list_tasks(conn: &Connection, query: &TaskQuery) -> AppResult<Page<AgentTask>> {
    let statuses = query.page.statuses_as(TaskStatus::parse)?;
    let values: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
    let mut filter = Filter::default();
    filter.any_of("status", &values);
    if let Some(campaign_id) = &query.campaign_id {
        filter.clause(
            "campaign_repository_id IN (SELECT id FROM campaign_repositories WHERE campaign_id = {})",
            campaign_id.clone(),
        );
    }
    filter.search(
        &["branch_name", "external_session_id"],
        query.page.search_pattern(),
    );
    paginate(
        conn,
        TASK_COLUMNS,
        "agent_tasks",
        &filter,
        "updated_at DESC, id",
        &query.page,
        map_task,
    )
}

pub fn set_task_status(conn: &Connection, id: &str, to: TaskStatus) -> AppResult<AgentTask> {
    let current = get_task(conn, id)?;
    if current.status.is_terminal() {
        return Err(AppError::validation(format!(
            "Agent task is already {}.",
            current.status.as_str()
        )));
    }
    let now = time_util::now();
    conn.execute(
        "UPDATE agent_tasks SET status = ?2,
             started_at = CASE WHEN ?2 = 'running' AND started_at IS NULL THEN ?3 ELSE started_at END,
             completed_at = CASE WHEN ?2 IN ('completed', 'failed', 'cancelled') THEN ?3 ELSE completed_at END,
             updated_at = ?3 WHERE id = ?1",
        params![id, to, now],
    )?;
    get_task(conn, id)
}

pub fn record_task_progress(conn: &Connection, id: &str, p: &TaskProgress) -> AppResult<AgentTask> {
    let changed = conn.execute(
        "UPDATE agent_tasks SET
             external_session_id = COALESCE(?2, external_session_id),
             external_session_url = COALESCE(?3, external_session_url),
             worktree_path = COALESCE(?4, worktree_path),
             branch_name = COALESCE(?5, branch_name),
             commit_sha = COALESCE(?6, commit_sha),
             pull_request_url = COALESCE(?7, pull_request_url),
             updated_at = ?8 WHERE id = ?1",
        params![
            id,
            p.external_session_id,
            p.external_session_url,
            p.worktree_path,
            p.branch_name,
            p.commit_sha,
            p.pull_request_url,
            time_util::now()
        ],
    )?;
    expect_changed(changed, "Agent task", id)?;
    get_task(conn, id)
}

// ---------------------------------------------------------------- runs

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRun {
    pub id: String,
    pub agent_task_id: String,
    pub provider: String,
    pub process_id: Option<u32>,
    pub external_run_id: Option<String>,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub redacted_output_path: Option<String>,
    pub error: Option<Value>,
}

const RUN_COLUMNS: &str =
    "SELECT id, agent_task_id, provider, process_id, external_run_id, status, exit_code, \
    started_at, completed_at, redacted_output_path, error_json";

fn map_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRun> {
    Ok(AgentRun {
        id: r.get("id")?,
        agent_task_id: r.get("agent_task_id")?,
        provider: r.get("provider")?,
        process_id: r.get("process_id")?,
        external_run_id: r.get("external_run_id")?,
        status: r.get("status")?,
        exit_code: r.get("exit_code")?,
        started_at: r.get("started_at")?,
        completed_at: r.get("completed_at")?,
        redacted_output_path: r.get("redacted_output_path")?,
        error: json_col_opt(r, "error_json")?,
    })
}

pub fn start_run(
    conn: &Connection,
    task_id: &str,
    provider: &str,
    process_id: Option<u32>,
    external_run_id: Option<&str>,
) -> AppResult<AgentRun> {
    let id = new_id();
    conn.execute(
        "INSERT INTO agent_runs (id, agent_task_id, provider, process_id, external_run_id, status, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6)",
        params![id, task_id, provider, process_id, external_run_id, time_util::now()],
    )?;
    get_run(conn, &id)
}

/// The run row bound to a process-runner run id, if any.
pub fn run_by_external_id(conn: &Connection, external_run_id: &str) -> AppResult<Option<AgentRun>> {
    conn.query_row(
        &format!("{RUN_COLUMNS} FROM agent_runs WHERE external_run_id = ?1 ORDER BY started_at DESC LIMIT 1"),
        [external_run_id],
        map_run,
    )
    .optional()
    .map_err(Into::into)
}

/// The newest task for a campaign membership, if one exists.
pub fn task_for_member(conn: &Connection, member_id: &str) -> AppResult<Option<AgentTask>> {
    conn.query_row(
        &format!("{TASK_COLUMNS} FROM agent_tasks WHERE campaign_repository_id = ?1 ORDER BY created_at DESC LIMIT 1"),
        [member_id],
        map_task,
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_run(conn: &Connection, id: &str) -> AppResult<AgentRun> {
    conn.query_row(
        &format!("{RUN_COLUMNS} FROM agent_runs WHERE id = ?1"),
        [id],
        map_run,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Agent run", id))
}

pub fn finish_run(
    conn: &Connection,
    id: &str,
    status: RunStatus,
    exit_code: Option<i32>,
    output_path: Option<&str>,
    error: Option<&Value>,
) -> AppResult<AgentRun> {
    if !status.is_terminal() {
        return Err(AppError::validation(
            "A run can only finish with a terminal status.",
        ));
    }
    let current = get_run(conn, id)?;
    if current.status.is_terminal() {
        return Err(AppError::validation(format!(
            "Run is already {}.",
            current.status.as_str()
        )));
    }
    let error_json = error
        .map(|e| json_text(e, "error", JsonShape::Object))
        .transpose()?;
    conn.execute(
        "UPDATE agent_runs SET status = ?2, exit_code = ?3, redacted_output_path = ?4, error_json = ?5,
             completed_at = ?6 WHERE id = ?1",
        params![id, status, exit_code, output_path, error_json, time_util::now()],
    )?;
    get_run(conn, id)
}

pub fn list_runs(
    conn: &Connection,
    task_id: &str,
    params: &ListParams,
) -> AppResult<Page<AgentRun>> {
    let statuses = params.statuses_as(RunStatus::parse)?;
    let values: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
    let mut filter = Filter::default();
    filter
        .eq("agent_task_id", task_id.to_string())
        .any_of("status", &values);
    paginate(
        conn,
        RUN_COLUMNS,
        "agent_runs",
        &filter,
        "started_at DESC, rowid DESC",
        params,
        map_run,
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::campaigns::{self, tests::definition};
    use crate::db::repositories::tests::local;
    use crate::db::Database;
    use crate::error::ErrorCode;
    use serde_json::json;

    /// Campaign with one repository; returns the membership id.
    pub fn member(db: &Database) -> (String, String) {
        db.write(|c| {
            let repo = local(c, "svc");
            let campaign = campaigns::create_with_repositories(c, &definition("x"), &[repo.id])?;
            let members = campaigns::list_members(c, &campaign.id, &ListParams::default())?;
            Ok((campaign.id, members.items[0].id.clone()))
        })
        .unwrap()
    }

    pub fn task(db: &Database, member_id: &str) -> AgentTask {
        db.write(|c| {
            create_task(
                c,
                &NewAgentTask {
                    campaign_repository_id: member_id.into(),
                    provider: AgentProvider::Devin,
                    execution_mode: ExecutionMode::CloudSession,
                    instructions_markdown: "Replace request".into(),
                },
            )
        })
        .unwrap()
    }

    #[test]
    fn task_lifecycle_and_progress() {
        let db = Database::open_in_memory();
        let (campaign_id, member_id) = member(&db);
        let t = task(&db, &member_id);
        assert_eq!(t.status, TaskStatus::Pending);
        let running = db
            .write(|c| set_task_status(c, &t.id, TaskStatus::Running))
            .unwrap();
        assert!(running.started_at.is_some());
        let progressed = db
            .write(|c| {
                record_task_progress(
                    c,
                    &t.id,
                    &TaskProgress {
                        branch_name: Some("shiftops/x".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(progressed.branch_name.as_deref(), Some("shiftops/x"));
        let done = db
            .write(|c| set_task_status(c, &t.id, TaskStatus::Completed))
            .unwrap();
        assert!(done.completed_at.is_some());
        assert!(db
            .write(|c| set_task_status(c, &t.id, TaskStatus::Running))
            .is_err());

        let by_campaign = db
            .read(|c| {
                list_tasks(
                    c,
                    &TaskQuery {
                        campaign_id: Some(campaign_id.clone()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(by_campaign.total, 1);
        let other = db
            .read(|c| {
                list_tasks(
                    c,
                    &TaskQuery {
                        campaign_id: Some("other".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(other.total, 0);
        let completed = db
            .read(|c| {
                list_tasks(
                    c,
                    &TaskQuery {
                        page: ListParams {
                            statuses: vec!["completed".into()],
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(completed.total, 1);
    }

    #[test]
    fn task_history_blocks_campaign_deletion() {
        let db = Database::open_in_memory();
        let (campaign_id, member_id) = member(&db);
        task(&db, &member_id);
        let err = db
            .write(|c| campaigns::delete(c, &campaign_id))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationFailed);
        assert!(err.message.contains("history"));
    }

    #[test]
    fn runs_finish_once_and_cascade_with_task() {
        let db = Database::open_in_memory();
        let (_, member_id) = member(&db);
        let t = task(&db, &member_id);
        let run = db
            .write(|c| start_run(c, &t.id, "devin", None, Some("sess-1")))
            .unwrap();
        assert!(db
            .write(|c| finish_run(c, &run.id, RunStatus::Running, None, None, None))
            .is_err());
        let done = db
            .write(|c| {
                finish_run(
                    c,
                    &run.id,
                    RunStatus::Failed,
                    Some(1),
                    None,
                    Some(&json!({ "code": "COMMAND_FAILED" })),
                )
            })
            .unwrap();
        assert_eq!(done.exit_code, Some(1));
        assert!(db
            .write(|c| finish_run(c, &run.id, RunStatus::Succeeded, Some(0), None, None))
            .is_err());
        assert_eq!(
            db.read(|c| list_runs(c, &t.id, &ListParams::default()))
                .unwrap()
                .total,
            1
        );
        db.write(|c| Ok(c.execute("DELETE FROM agent_tasks WHERE id = ?1", [&t.id])?))
            .unwrap();
        assert_eq!(
            db.read(|c| get_run(c, &run.id)).unwrap_err().code,
            ErrorCode::RecordNotFound
        );
    }
}
