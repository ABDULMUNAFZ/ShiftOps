//! Repositories, repository scans and repository dependencies.
//! Removing a repository deletes ShiftOps records only, never files on disk.

// Service operations beyond the read paths are used by later steps
// (discovery, planning, delegation, verification) and are covered by
// this module's tests; only test builds exercise them today.
#![cfg_attr(not(test), allow(dead_code))]

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    expect_changed, json_col, json_col_opt, json_text, new_id, paginate, require_text,
    require_unit_interval, string_enum, Filter, JsonShape, ListParams, Page,
};
use crate::error::{AppError, AppResult};
use crate::time_util;

string_enum!(SourceType { Local => "local", Github => "github" });
string_enum!(TrustStatus { Untrusted => "untrusted", Trusted => "trusted", Blocked => "blocked" });
string_enum!(ScanStatus {
    Queued => "queued",
    Running => "running",
    Completed => "completed",
    Failed => "failed",
    Cancelled => "cancelled",
});
string_enum!(ConfirmationStatus { Inferred => "inferred", Confirmed => "confirmed", Rejected => "rejected" });

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub id: String,
    pub source_type: SourceType,
    pub name: String,
    pub local_path: Option<String>,
    pub remote_url: Option<String>,
    pub github_host: Option<String>,
    pub github_owner: Option<String>,
    pub github_repo: Option<String>,
    pub default_branch: Option<String>,
    pub current_branch: Option<String>,
    pub current_commit: Option<String>,
    pub trust_status: TrustStatus,
    pub created_at: String,
    pub updated_at: String,
    pub root_id: Option<String>,
    pub git_kind: Option<String>,
    pub is_detached: bool,
    /// `None` when working-tree status was not collected.
    pub is_dirty: Option<bool>,
    pub staged_count: Option<u32>,
    pub modified_count: Option<u32>,
    pub untracked_count: Option<u32>,
    pub last_commit_at: Option<String>,
    pub git_collected_at: Option<String>,
    pub git_warnings: Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubCoordinates {
    pub host: String,
    pub owner: String,
    pub repo: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewRepository {
    pub source_type: SourceType,
    pub name: String,
    pub local_path: Option<String>,
    pub remote_url: Option<String>,
    pub github: Option<GithubCoordinates>,
    pub default_branch: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitState {
    pub default_branch: Option<String>,
    pub current_branch: Option<String>,
    pub current_commit: Option<String>,
    pub remote_url: Option<String>,
}

const REPO_COLUMNS: &str = "SELECT id, source_type, name, local_path, remote_url, github_host, github_owner, github_repo, \
    default_branch, current_branch, current_commit, trust_status, created_at, updated_at, root_id, git_kind, \
    is_detached, is_dirty, staged_count, modified_count, untracked_count, last_commit_at, git_collected_at, \
    git_warnings_json";

fn map_repo(r: &rusqlite::Row<'_>) -> rusqlite::Result<Repository> {
    Ok(Repository {
        id: r.get("id")?,
        source_type: r.get("source_type")?,
        name: r.get("name")?,
        local_path: r.get("local_path")?,
        remote_url: r.get("remote_url")?,
        github_host: r.get("github_host")?,
        github_owner: r.get("github_owner")?,
        github_repo: r.get("github_repo")?,
        default_branch: r.get("default_branch")?,
        current_branch: r.get("current_branch")?,
        current_commit: r.get("current_commit")?,
        trust_status: r.get("trust_status")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        root_id: r.get("root_id")?,
        git_kind: r.get("git_kind")?,
        is_detached: r.get("is_detached")?,
        is_dirty: r.get("is_dirty")?,
        staged_count: r.get("staged_count")?,
        modified_count: r.get("modified_count")?,
        untracked_count: r.get("untracked_count")?,
        last_commit_at: r.get("last_commit_at")?,
        git_collected_at: r.get("git_collected_at")?,
        git_warnings: json_col(r, "git_warnings_json")?,
    })
}

/// Everything discovery learnt about one local repository.
#[derive(Debug, Clone)]
pub struct LocalRepositoryRecord {
    pub name: String,
    pub local_path: String,
    pub git_kind: crate::git::inspect::GitKind,
    pub metadata: crate::git::inspect::GitMetadata,
    pub collected_at: String,
}

/// Insert a discovered repository (as `untrusted`), or refresh the Git facts
/// of the one already registered at the same canonical path. Trust status is
/// never changed by discovery. Returns the repository and whether it was new.
pub fn record_local_discovery(
    conn: &Connection,
    root_id: &str,
    record: &LocalRepositoryRecord,
) -> AppResult<(Repository, bool)> {
    require_text(&record.name, "Repository name", 255)?;
    let m = &record.metadata;
    let origin = m.origin.as_ref();
    let mut github = origin.filter(|o| o.is_github());
    let wt = m.working_tree.as_ref();
    let mut warnings = m.warnings.clone();
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM repositories WHERE local_path = ?1",
            [&record.local_path],
            |r| r.get(0),
        )
        .optional()?;
    // One registered repository holds each GitHub identity. Other checkouts
    // and worktrees of the same GitHub repository keep their remote URL but
    // are not linked, so the identity stays unique.
    if let Some(g) = github {
        let holder: Option<String> = conn
            .query_row(
                "SELECT COALESCE(local_path, name) FROM repositories
                 WHERE github_host = ?1 AND github_owner = ?2 AND github_repo = ?3 AND (?4 IS NULL OR id <> ?4)",
                params![g.host, g.owner, g.repo, existing],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(holder) = holder {
            warnings.push(format!(
                "Not linked to GitHub {}/{}: that repository is already linked to {holder}.",
                g.owner.as_deref().unwrap_or(""),
                g.repo.as_deref().unwrap_or("")
            ));
            github = None;
        }
    }
    let warnings = serde_json::to_string(&warnings)?;
    let id = existing.clone().unwrap_or_else(new_id);
    let now = time_util::now();
    let values = params![
        id,
        record.name,
        record.local_path,
        origin.map(|o| o.sanitized_url.clone()),
        github.and_then(|o| o.host.clone()),
        github.and_then(|o| o.owner.clone()),
        github.and_then(|o| o.repo.clone()),
        m.default_branch,
        m.current_branch,
        m.head_commit,
        root_id,
        record.git_kind.as_str(),
        m.detached,
        wt.map(|w| w.dirty()),
        wt.map(|w| w.staged),
        wt.map(|w| w.modified),
        wt.map(|w| w.untracked),
        m.last_commit_at,
        record.collected_at,
        warnings,
        now,
    ];
    if existing.is_some() {
        conn.execute(
            "UPDATE repositories SET name = ?2, remote_url = ?4, github_host = ?5, github_owner = ?6,
                 github_repo = ?7, default_branch = ?8, current_branch = ?9, current_commit = ?10, root_id = ?11,
                 git_kind = ?12, is_detached = ?13, is_dirty = ?14, staged_count = ?15, modified_count = ?16,
                 untracked_count = ?17, last_commit_at = ?18, git_collected_at = ?19, git_warnings_json = ?20,
                 updated_at = ?21
             WHERE id = ?1 AND local_path = ?3",
            values,
        )?;
    } else {
        conn.execute(
            "INSERT INTO repositories (id, source_type, name, local_path, remote_url, github_host, github_owner,
                 github_repo, default_branch, current_branch, current_commit, trust_status, root_id, git_kind,
                 is_detached, is_dirty, staged_count, modified_count, untracked_count, last_commit_at,
                 git_collected_at, git_warnings_json, created_at, updated_at)
             VALUES (?1, 'local', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'untrusted', ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17, ?18, ?19, ?20, ?21, ?21)",
            values,
        )?;
    }
    Ok((get(conn, &id)?, existing.is_none()))
}

/// Canonical local paths already registered, for marking discovery results.
pub fn registered_local_paths(conn: &Connection) -> AppResult<std::collections::HashSet<String>> {
    let mut stmt =
        conn.prepare("SELECT local_path FROM repositories WHERE local_path IS NOT NULL")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn create(conn: &Connection, new: &NewRepository) -> AppResult<Repository> {
    require_text(&new.name, "Repository name", 255)?;
    if new.source_type == SourceType::Local && new.local_path.as_deref().is_none_or(str::is_empty) {
        return Err(AppError::validation(
            "A local repository needs a local path.",
        ));
    }
    if new.source_type == SourceType::Github && new.github.is_none() {
        return Err(AppError::validation(
            "A GitHub repository needs host, owner and repository name.",
        ));
    }
    if let Some(g) = &new.github {
        require_text(&g.host, "GitHub host", 255)?;
        require_text(&g.owner, "GitHub owner", 100)?;
        require_text(&g.repo, "GitHub repository", 100)?;
    }
    let id = new_id();
    let now = time_util::now();
    let g = new.github.clone().unwrap_or_default();
    let opt = |s: String| (!s.is_empty()).then_some(s);
    conn.execute(
        "INSERT INTO repositories (id, source_type, name, local_path, remote_url, github_host, github_owner, github_repo,
             default_branch, trust_status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'untrusted', ?10, ?10)",
        params![
            id,
            new.source_type,
            new.name.trim(),
            new.local_path,
            new.remote_url,
            opt(g.host),
            opt(g.owner),
            opt(g.repo),
            new.default_branch,
            now
        ],
    )?;
    get(conn, &id)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Repository> {
    conn.query_row(
        &format!("{REPO_COLUMNS} FROM repositories WHERE id = ?1"),
        [id],
        map_repo,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Repository", id))
}

/// `statuses` filters on trust status. Search covers name, path and GitHub owner/repo.
pub fn list(conn: &Connection, params: &ListParams) -> AppResult<Page<Repository>> {
    let statuses = params.statuses_as(TrustStatus::parse)?;
    let values: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
    let mut filter = Filter::default();
    filter.any_of("trust_status", &values);
    filter.search(
        &["name", "local_path", "github_owner", "github_repo"],
        params.search_pattern(),
    );
    paginate(
        conn,
        REPO_COLUMNS,
        "repositories",
        &filter,
        "name COLLATE NOCASE, id",
        params,
        map_repo,
    )
}

pub fn update_git_state(conn: &Connection, id: &str, state: &GitState) -> AppResult<Repository> {
    let changed = conn.execute(
        "UPDATE repositories SET default_branch = COALESCE(?2, default_branch), current_branch = ?3,
             current_commit = ?4, remote_url = COALESCE(?5, remote_url), updated_at = ?6 WHERE id = ?1",
        params![id, state.default_branch, state.current_branch, state.current_commit, state.remote_url, time_util::now()],
    )?;
    expect_changed(changed, "Repository", id)?;
    get(conn, id)
}

pub fn set_trust(conn: &Connection, id: &str, trust: TrustStatus) -> AppResult<Repository> {
    let changed = conn.execute(
        "UPDATE repositories SET trust_status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, trust, time_util::now()],
    )?;
    expect_changed(changed, "Repository", id)?;
    get(conn, id)
}

/// Remove the repository record (and its scans and dependency edges). Fails
/// while the repository belongs to a campaign. Files on disk are untouched.
pub fn remove(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn
        .execute("DELETE FROM repositories WHERE id = ?1", [id])
        .map_err(|e| {
            let err: AppError = e.into();
            if err.message.contains("still in use") {
                AppError::validation(
                    "This repository belongs to a campaign. Remove it from the campaign first.",
                )
                .with_context("repositoryId", id)
            } else {
                err
            }
        })?;
    expect_changed(changed, "Repository", id)
}

// ---------------------------------------------------------------- scans

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryScan {
    pub id: String,
    pub repository_id: String,
    pub scanned_commit: Option<String>,
    pub relevant_files_hash: Option<String>,
    pub status: ScanStatus,
    pub result: Option<Value>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

const SCAN_COLUMNS: &str =
    "SELECT id, repository_id, scanned_commit, relevant_files_hash, status, result_json, \
    error_code, error_message, started_at, completed_at";

fn map_scan(r: &rusqlite::Row<'_>) -> rusqlite::Result<RepositoryScan> {
    Ok(RepositoryScan {
        id: r.get("id")?,
        repository_id: r.get("repository_id")?,
        scanned_commit: r.get("scanned_commit")?,
        relevant_files_hash: r.get("relevant_files_hash")?,
        status: r.get("status")?,
        result: json_col_opt(r, "result_json")?,
        error_code: r.get("error_code")?,
        error_message: r.get("error_message")?,
        started_at: r.get("started_at")?,
        completed_at: r.get("completed_at")?,
    })
}

pub fn create_scan(conn: &Connection, repository_id: &str) -> AppResult<RepositoryScan> {
    let id = new_id();
    conn.execute(
        "INSERT INTO repository_scans (id, repository_id, status, started_at) VALUES (?1, ?2, 'queued', ?3)",
        params![id, repository_id, time_util::now()],
    )?;
    get_scan(conn, &id)
}

pub fn get_scan(conn: &Connection, id: &str) -> AppResult<RepositoryScan> {
    conn.query_row(
        &format!("{SCAN_COLUMNS} FROM repository_scans WHERE id = ?1"),
        [id],
        map_scan,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Repository scan", id))
}

fn transition_scan(
    conn: &Connection,
    id: &str,
    to: ScanStatus,
    sql_set: &str,
    extra: &[&dyn rusqlite::ToSql],
) -> AppResult<RepositoryScan> {
    let current = get_scan(conn, id)?.status;
    let allowed = match to {
        ScanStatus::Running => current == ScanStatus::Queued,
        ScanStatus::Completed | ScanStatus::Failed | ScanStatus::Cancelled => {
            matches!(current, ScanStatus::Queued | ScanStatus::Running)
        }
        ScanStatus::Queued => false,
    };
    if !allowed {
        return Err(AppError::validation(format!(
            "Scan cannot move from {} to {}.",
            current.as_str(),
            to.as_str()
        )));
    }
    let mut values: Vec<&dyn rusqlite::ToSql> = vec![&id, &to];
    values.extend_from_slice(extra);
    conn.execute(
        &format!("UPDATE repository_scans SET status = ?2{sql_set} WHERE id = ?1"),
        rusqlite::params_from_iter(values),
    )?;
    get_scan(conn, id)
}

pub fn start_scan(conn: &Connection, id: &str) -> AppResult<RepositoryScan> {
    transition_scan(conn, id, ScanStatus::Running, "", &[])
}

pub fn complete_scan(
    conn: &Connection,
    id: &str,
    scanned_commit: Option<&str>,
    files_hash: Option<&str>,
    result: &Value,
) -> AppResult<RepositoryScan> {
    let result_json = json_text(result, "result", JsonShape::Object)?;
    let now = time_util::now();
    transition_scan(
        conn,
        id,
        ScanStatus::Completed,
        ", scanned_commit = ?3, relevant_files_hash = ?4, result_json = ?5, completed_at = ?6",
        &[&scanned_commit, &files_hash, &result_json, &now],
    )
}

pub fn fail_scan(
    conn: &Connection,
    id: &str,
    code: &str,
    message: &str,
) -> AppResult<RepositoryScan> {
    let now = time_util::now();
    let message = crate::redact::redact(message);
    transition_scan(
        conn,
        id,
        ScanStatus::Failed,
        ", error_code = ?3, error_message = ?4, completed_at = ?5",
        &[&code, &message, &now],
    )
}

pub fn cancel_scan(conn: &Connection, id: &str) -> AppResult<RepositoryScan> {
    let now = time_util::now();
    transition_scan(
        conn,
        id,
        ScanStatus::Cancelled,
        ", completed_at = ?3",
        &[&now],
    )
}

pub fn list_scans(
    conn: &Connection,
    repository_id: &str,
    params: &ListParams,
) -> AppResult<Page<RepositoryScan>> {
    let statuses = params.statuses_as(ScanStatus::parse)?;
    let values: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
    let mut filter = Filter::default();
    filter
        .eq("repository_id", repository_id.to_string())
        .any_of("status", &values);
    paginate(
        conn,
        SCAN_COLUMNS,
        "repository_scans",
        &filter,
        "started_at DESC, rowid DESC",
        params,
        map_scan,
    )
}

// ---------------------------------------------------------------- dependencies

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryDependency {
    pub id: String,
    pub source_repository_id: String,
    pub target_repository_id: String,
    pub relationship_type: String,
    pub confidence: f64,
    pub evidence: Value,
    pub detection_method: String,
    pub confirmation_status: ConfirmationStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDependency {
    pub source_repository_id: String,
    pub target_repository_id: String,
    pub relationship_type: String,
    pub confidence: f64,
    pub evidence: Value,
    pub detection_method: String,
}

const DEP_COLUMNS: &str =
    "SELECT id, source_repository_id, target_repository_id, relationship_type, confidence, \
    evidence_json, detection_method, confirmation_status, created_at, updated_at";

fn map_dep(r: &rusqlite::Row<'_>) -> rusqlite::Result<RepositoryDependency> {
    Ok(RepositoryDependency {
        id: r.get("id")?,
        source_repository_id: r.get("source_repository_id")?,
        target_repository_id: r.get("target_repository_id")?,
        relationship_type: r.get("relationship_type")?,
        confidence: r.get("confidence")?,
        evidence: json_col(r, "evidence_json")?,
        detection_method: r.get("detection_method")?,
        confirmation_status: r.get("confirmation_status")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

/// Insert or refresh an inferred edge. A user's confirmation or rejection is
/// preserved when detection runs again.
pub fn upsert_dependency(
    conn: &Connection,
    new: &NewDependency,
) -> AppResult<RepositoryDependency> {
    if new.source_repository_id == new.target_repository_id {
        return Err(AppError::validation(
            "A repository cannot depend on itself.",
        ));
    }
    require_text(&new.relationship_type, "Relationship type", 64)?;
    require_text(&new.detection_method, "Detection method", 64)?;
    require_unit_interval(new.confidence, "Confidence")?;
    let evidence = json_text(&new.evidence, "evidence", JsonShape::Array)?;
    let now = time_util::now();
    conn.execute(
        "INSERT INTO repository_dependencies (id, source_repository_id, target_repository_id, relationship_type,
             confidence, evidence_json, detection_method, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
         ON CONFLICT (source_repository_id, target_repository_id, relationship_type) DO UPDATE SET
             confidence = excluded.confidence, evidence_json = excluded.evidence_json,
             detection_method = excluded.detection_method, updated_at = excluded.updated_at",
        params![
            new_id(),
            new.source_repository_id,
            new.target_repository_id,
            new.relationship_type,
            new.confidence,
            evidence,
            new.detection_method,
            now
        ],
    )?;
    Ok(conn.query_row(
        &format!(
            "{DEP_COLUMNS} FROM repository_dependencies
             WHERE source_repository_id = ?1 AND target_repository_id = ?2 AND relationship_type = ?3"
        ),
        params![new.source_repository_id, new.target_repository_id, new.relationship_type],
        map_dep,
    )?)
}

pub fn set_dependency_confirmation(
    conn: &Connection,
    id: &str,
    status: ConfirmationStatus,
) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE repository_dependencies SET confirmation_status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status, time_util::now()],
    )?;
    expect_changed(changed, "Repository dependency", id)
}

/// Edges where the repository is either side.
pub fn list_dependencies(
    conn: &Connection,
    repository_id: &str,
    params: &ListParams,
) -> AppResult<Page<RepositoryDependency>> {
    let statuses = params.statuses_as(ConfirmationStatus::parse)?;
    let values: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
    let mut filter = Filter::default();
    filter
        .either_eq(
            "source_repository_id",
            "target_repository_id",
            repository_id.to_string(),
        )
        .any_of("confirmation_status", &values);
    paginate(
        conn,
        DEP_COLUMNS,
        "repository_dependencies",
        &filter,
        "confidence DESC, id",
        params,
        map_dep,
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::Database;
    use crate::error::ErrorCode;
    use serde_json::json;

    pub fn local(conn: &Connection, name: &str) -> Repository {
        create(
            conn,
            &NewRepository {
                source_type: SourceType::Local,
                name: name.into(),
                local_path: Some(format!("/work/{name}")),
                remote_url: None,
                github: None,
                default_branch: Some("main".into()),
            },
        )
        .unwrap()
    }

    #[test]
    fn create_get_update_remove() {
        let db = Database::open_in_memory();
        let repo = db.write(|c| Ok(local(c, "api"))).unwrap();
        assert_eq!(repo.trust_status, TrustStatus::Untrusted);
        let updated = db
            .write(|c| {
                update_git_state(
                    c,
                    &repo.id,
                    &GitState {
                        current_branch: Some("feature".into()),
                        current_commit: Some("abc123".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(updated.current_commit.as_deref(), Some("abc123"));
        assert_eq!(updated.default_branch.as_deref(), Some("main"));
        assert_eq!(
            db.write(|c| set_trust(c, &repo.id, TrustStatus::Trusted))
                .unwrap()
                .trust_status,
            TrustStatus::Trusted
        );
        db.write(|c| remove(c, &repo.id)).unwrap();
        assert_eq!(
            db.read(|c| get(c, &repo.id)).unwrap_err().code,
            ErrorCode::RecordNotFound
        );
        assert_eq!(
            db.write(|c| remove(c, &repo.id)).unwrap_err().code,
            ErrorCode::RecordNotFound
        );
    }

    #[test]
    fn unique_local_path_and_github_identity() {
        let db = Database::open_in_memory();
        db.write(|c| Ok(local(c, "api"))).unwrap();
        let dup = db.write(|c| {
            create(
                c,
                &NewRepository {
                    source_type: SourceType::Local,
                    name: "other".into(),
                    local_path: Some("/work/api".into()),
                    remote_url: None,
                    github: None,
                    default_branch: None,
                },
            )
        });
        assert_eq!(dup.unwrap_err().code, ErrorCode::ValidationFailed);

        let gh = |owner: &str| NewRepository {
            source_type: SourceType::Github,
            name: "web".into(),
            local_path: None,
            remote_url: None,
            github: Some(GithubCoordinates {
                host: "github.com".into(),
                owner: owner.into(),
                repo: "Web".into(),
            }),
            default_branch: None,
        };
        db.write(|c| create(c, &gh("acme"))).unwrap();
        // GitHub identity is case-insensitive.
        assert_eq!(
            db.write(|c| create(c, &gh("ACME"))).unwrap_err().code,
            ErrorCode::ValidationFailed
        );
        db.write(|c| create(c, &gh("other-org"))).unwrap();
    }

    #[test]
    fn validates_shape_before_insert() {
        let db = Database::open_in_memory();
        let missing_path = NewRepository {
            source_type: SourceType::Local,
            name: "x".into(),
            local_path: None,
            remote_url: None,
            github: None,
            default_branch: None,
        };
        assert_eq!(
            db.write(|c| create(c, &missing_path)).unwrap_err().code,
            ErrorCode::ValidationFailed
        );
    }

    #[test]
    fn list_filters_searches_and_paginates() {
        let db = Database::open_in_memory();
        db.write(|c| {
            for n in ["alpha", "beta", "gamma", "delta"] {
                local(c, n);
            }
            Ok(())
        })
        .unwrap();
        let beta = db
            .read(|c| {
                list(
                    c,
                    &ListParams {
                        search: Some("bet".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(beta.items.len(), 1);
        db.write(|c| set_trust(c, &beta.items[0].id, TrustStatus::Blocked))
            .unwrap();
        let page = db
            .read(|c| {
                list(
                    c,
                    &ListParams {
                        limit: Some(2),
                        offset: Some(1),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(page.total, 4);
        assert_eq!(
            page.items
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            ["beta", "delta"]
        );
        let blocked = db
            .read(|c| {
                list(
                    c,
                    &ListParams {
                        statuses: vec!["blocked".into()],
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(blocked.total, 1);
    }

    #[test]
    fn scans_follow_state_machine_and_cascade_with_repository() {
        let db = Database::open_in_memory();
        let repo = db.write(|c| Ok(local(c, "api"))).unwrap();
        let scan = db.write(|c| create_scan(c, &repo.id)).unwrap();
        assert_eq!(scan.status, ScanStatus::Queued);
        db.write(|c| start_scan(c, &scan.id)).unwrap();
        let done = db
            .write(|c| complete_scan(c, &scan.id, Some("abc"), Some("h1"), &json!({ "files": 3 })))
            .unwrap();
        assert_eq!(done.status, ScanStatus::Completed);
        assert!(done.completed_at.is_some());
        assert_eq!(
            db.write(|c| cancel_scan(c, &scan.id)).unwrap_err().code,
            ErrorCode::ValidationFailed
        );

        let failed_scan = db.write(|c| create_scan(c, &repo.id)).unwrap();
        let failed = db
            .write(|c| {
                fail_scan(
                    c,
                    &failed_scan.id,
                    "GIT",
                    "token=ghp_abcdefghijklmnopqrstuv1234",
                )
            })
            .unwrap();
        assert!(!failed.error_message.unwrap().contains("ghp_abc"));

        let page = db
            .read(|c| {
                list_scans(
                    c,
                    &repo.id,
                    &ListParams {
                        statuses: vec!["completed".into()],
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        assert_eq!(page.total, 1);

        db.write(|c| remove(c, &repo.id)).unwrap();
        assert_eq!(
            db.read(|c| get_scan(c, &scan.id)).unwrap_err().code,
            ErrorCode::RecordNotFound
        );
    }

    #[test]
    fn scan_requires_existing_repository() {
        let db = Database::open_in_memory();
        assert_eq!(
            db.write(|c| create_scan(c, "nope")).unwrap_err().code,
            ErrorCode::ValidationFailed
        );
    }

    #[test]
    fn dependencies_reject_self_reference_and_preserve_confirmation() {
        let db = Database::open_in_memory();
        let (a, b) = db.write(|c| Ok((local(c, "a"), local(c, "b")))).unwrap();
        let self_dep = NewDependency {
            source_repository_id: a.id.clone(),
            target_repository_id: a.id.clone(),
            relationship_type: "npm".into(),
            confidence: 0.9,
            evidence: json!([]),
            detection_method: "package_json".into(),
        };
        assert_eq!(
            db.write(|c| upsert_dependency(c, &self_dep))
                .unwrap_err()
                .code,
            ErrorCode::ValidationFailed
        );
        // The database constraint also holds if application validation is bypassed.
        let raw = db.write(|c| {
            Ok(c.execute(
                "INSERT INTO repository_dependencies VALUES ('x', ?1, ?1, 'npm', 0.5, '[]', 'm', 'inferred', 't', 't')",
                [&a.id],
            )?)
        });
        assert_eq!(raw.unwrap_err().code, ErrorCode::ValidationFailed);

        let edge = NewDependency {
            target_repository_id: b.id.clone(),
            ..self_dep
        };
        let dep = db.write(|c| upsert_dependency(c, &edge)).unwrap();
        db.write(|c| set_dependency_confirmation(c, &dep.id, ConfirmationStatus::Confirmed))
            .unwrap();
        let again = db
            .write(|c| {
                upsert_dependency(
                    c,
                    &NewDependency {
                        confidence: 0.4,
                        ..edge.clone()
                    },
                )
            })
            .unwrap();
        assert_eq!(again.id, dep.id);
        assert_eq!(again.confidence, 0.4);
        assert_eq!(again.confirmation_status, ConfirmationStatus::Confirmed);
        assert!(db
            .write(|c| upsert_dependency(
                c,
                &NewDependency {
                    confidence: 1.5,
                    ..edge
                }
            ))
            .is_err());

        let page = db
            .read(|c| list_dependencies(c, &b.id, &ListParams::default()))
            .unwrap();
        assert_eq!(page.total, 1);
        // Removing either repository removes the edge.
        db.write(|c| remove(c, &b.id)).unwrap();
        assert_eq!(
            db.read(|c| list_dependencies(c, &a.id, &ListParams::default()))
                .unwrap()
                .total,
            0
        );
    }
}
