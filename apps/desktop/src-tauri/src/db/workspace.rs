//! Managed workspaces, managed clone attempts and attaching a checkout to an
//! existing repository record. Nothing here touches the filesystem; the
//! `workspace` module validates paths and runs Git.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::repositories::{self, LinkMethod, Ownership, Repository};
use super::{new_id, string_enum};
use crate::error::{AppError, AppResult};
use crate::git::inspect::{GitKind, GitMetadata};
use crate::time_util;

string_enum!(CloneStatus {
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
    Cancelled => "cancelled",
    CleanedUp => "cleaned_up",
});

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedWorkspace {
    pub id: String,
    pub canonical_path: String,
    pub approved_at: String,
    pub created_at: String,
}

fn map_workspace(r: &rusqlite::Row<'_>) -> rusqlite::Result<ManagedWorkspace> {
    Ok(ManagedWorkspace {
        id: r.get("id")?,
        canonical_path: r.get("canonical_path")?,
        approved_at: r.get("approved_at")?,
        created_at: r.get("created_at")?,
    })
}

const WORKSPACE_COLUMNS: &str =
    "SELECT id, canonical_path, approved_at, created_at FROM managed_workspaces";

/// Record the user's approval of a canonical workspace root. Approving an
/// earlier root again makes it the active one.
pub fn approve(conn: &Connection, canonical_path: &str) -> AppResult<ManagedWorkspace> {
    let now = time_util::now();
    conn.execute(
        "INSERT INTO managed_workspaces (id, canonical_path, approved_at, approval_seq, created_at)
         VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(approval_seq), 0) + 1 FROM managed_workspaces), ?3)
         ON CONFLICT (canonical_path) DO UPDATE SET approved_at = excluded.approved_at,
             approval_seq = excluded.approval_seq",
        params![new_id(), canonical_path, now],
    )?;
    Ok(conn.query_row(
        &format!("{WORKSPACE_COLUMNS} WHERE canonical_path = ?1"),
        [canonical_path],
        map_workspace,
    )?)
}

/// The most recently approved workspace root.
pub fn active(conn: &Connection) -> AppResult<Option<ManagedWorkspace>> {
    Ok(conn
        .query_row(
            &format!("{WORKSPACE_COLUMNS} ORDER BY approval_seq DESC LIMIT 1"),
            [],
            map_workspace,
        )
        .optional()?)
}

pub fn get_workspace(conn: &Connection, id: &str) -> AppResult<ManagedWorkspace> {
    conn.query_row(
        &format!("{WORKSPACE_COLUMNS} WHERE id = ?1"),
        [id],
        map_workspace,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Managed workspace", id))
}

// ---------------------------------------------------------------- clones

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryClone {
    pub id: String,
    pub repository_id: Option<String>,
    pub github_repository_id: Option<String>,
    pub workspace_id: String,
    pub full_name: String,
    pub clone_url: String,
    pub destination_path: String,
    pub status: CloneStatus,
    pub run_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub partial_path_exists: bool,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub cleaned_at: Option<String>,
}

const CLONE_COLUMNS: &str = "SELECT id, repository_id, github_repository_id, workspace_id, full_name, clone_url, \
    destination_path, status, run_id, error_code, error_message, partial_path_exists, started_at, completed_at, \
    cleaned_at FROM repository_clones";

fn map_clone(r: &rusqlite::Row<'_>) -> rusqlite::Result<RepositoryClone> {
    Ok(RepositoryClone {
        id: r.get("id")?,
        repository_id: r.get("repository_id")?,
        github_repository_id: r.get("github_repository_id")?,
        workspace_id: r.get("workspace_id")?,
        full_name: r.get("full_name")?,
        clone_url: r.get("clone_url")?,
        destination_path: r.get("destination_path")?,
        status: r.get("status")?,
        run_id: r.get("run_id")?,
        error_code: r.get("error_code")?,
        error_message: r.get("error_message")?,
        partial_path_exists: r.get("partial_path_exists")?,
        started_at: r.get("started_at")?,
        completed_at: r.get("completed_at")?,
        cleaned_at: r.get("cleaned_at")?,
    })
}

#[derive(Debug, Clone)]
pub struct NewClone {
    pub repository_id: String,
    pub github_repository_id: Option<String>,
    pub workspace_id: String,
    pub full_name: String,
    pub clone_url: String,
    pub destination_path: String,
}

/// Record a clone attempt as `running`. Fails when another open attempt
/// (running, failed or cancelled and not cleaned up) holds the destination.
pub fn begin_clone(conn: &Connection, new: &NewClone) -> AppResult<RepositoryClone> {
    let id = new_id();
    conn.execute(
        "INSERT INTO repository_clones (id, repository_id, github_repository_id, workspace_id, full_name, clone_url,
             destination_path, status, started_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', ?8)",
        params![
            id,
            new.repository_id,
            new.github_repository_id,
            new.workspace_id,
            new.full_name,
            new.clone_url,
            new.destination_path,
            time_util::now()
        ],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::validation(
                "An earlier clone attempt still holds this destination. Clean it up first.",
            )
            .with_context("path", new.destination_path.as_str())
        } else {
            e.into()
        }
    })?;
    get_clone(conn, &id)
}

pub fn set_run_id(conn: &Connection, id: &str, run_id: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE repository_clones SET run_id = ?2 WHERE id = ?1",
        params![id, run_id],
    )?;
    Ok(())
}

/// Move a running attempt to its final status.
pub fn finish_clone(
    conn: &Connection,
    id: &str,
    status: CloneStatus,
    error: Option<(&str, &str)>,
    partial_path_exists: bool,
) -> AppResult<RepositoryClone> {
    if !matches!(
        status,
        CloneStatus::Succeeded | CloneStatus::Failed | CloneStatus::Cancelled
    ) {
        return Err(AppError::validation(
            "A clone can only finish as succeeded, failed or cancelled.",
        ));
    }
    let changed = conn.execute(
        "UPDATE repository_clones SET status = ?2, error_code = ?3, error_message = ?4, partial_path_exists = ?5,
             completed_at = ?6
         WHERE id = ?1 AND status = 'running'",
        params![
            id,
            status,
            error.map(|e| e.0),
            error.map(|e| crate::redact::redact(e.1)),
            partial_path_exists,
            time_util::now()
        ],
    )?;
    if changed == 0 {
        return Err(AppError::validation("This clone attempt is not running."));
    }
    get_clone(conn, id)
}

/// Record that the user removed the incomplete clone directory.
pub fn mark_cleaned(conn: &Connection, id: &str) -> AppResult<RepositoryClone> {
    let changed = conn.execute(
        "UPDATE repository_clones SET status = 'cleaned_up', partial_path_exists = 0, cleaned_at = ?2
         WHERE id = ?1 AND status IN ('failed', 'cancelled')",
        params![id, time_util::now()],
    )?;
    if changed == 0 {
        return Err(AppError::validation(
            "Only a failed or cancelled clone can be cleaned up.",
        ));
    }
    get_clone(conn, id)
}

/// A failed or cancelled attempt whose directory no longer exists holds
/// nothing to clean up; release its destination.
pub fn release_missing(conn: &Connection, id: &str) -> AppResult<RepositoryClone> {
    conn.execute(
        "UPDATE repository_clones SET partial_path_exists = 0 WHERE id = ?1",
        [id],
    )?;
    mark_cleaned(conn, id)
}

pub fn get_clone(conn: &Connection, id: &str) -> AppResult<RepositoryClone> {
    conn.query_row(&format!("{CLONE_COLUMNS} WHERE id = ?1"), [id], map_clone)
        .optional()?
        .ok_or_else(|| AppError::not_found("Clone attempt", id))
}

/// Most recent attempts first.
pub fn list_clones(conn: &Connection, limit: u32) -> AppResult<Vec<RepositoryClone>> {
    let mut stmt = conn.prepare(&format!(
        "{CLONE_COLUMNS} ORDER BY started_at DESC, rowid DESC LIMIT ?1"
    ))?;
    let rows = stmt.query_map([limit.clamp(1, 200)], map_clone)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The open attempt (running, failed or cancelled) holding `path`, if any.
pub fn open_attempt_at(conn: &Connection, path: &str) -> AppResult<Option<RepositoryClone>> {
    Ok(conn
        .query_row(
            &format!(
                "{CLONE_COLUMNS} WHERE destination_path = ?1 AND status IN ('running', 'failed', 'cancelled')"
            ),
            [path],
            map_clone,
        )
        .optional()?)
}

/// Mark attempts left `running` by a previous session (the app quit or
/// crashed mid-clone) as failed so their directories can be cleaned up.
pub fn fail_interrupted(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE repository_clones SET status = 'failed', error_code = 'COMMAND_FAILED',
             error_message = 'ShiftOps stopped before the clone finished.', partial_path_exists = 1,
             completed_at = ?1
         WHERE status = 'running'",
        [time_util::now()],
    )?)
}

// ---------------------------------------------------------------- attaching a checkout

/// A local checkout to attach to an existing repository record.
#[derive(Debug, Clone)]
pub struct Checkout {
    pub local_path: String,
    pub git_kind: GitKind,
    pub metadata: GitMetadata,
    pub ownership: Ownership,
    pub method: LinkMethod,
}

/// Give a GitHub-linked record (usually metadata-only) its local checkout.
/// The record keeps its id, GitHub identity and trust status, so the
/// transition never creates a duplicate.
pub fn attach_checkout(
    conn: &Connection,
    repository_id: &str,
    checkout: &Checkout,
) -> AppResult<Repository> {
    let current = repositories::get(conn, repository_id)?;
    if current.github_owner.is_none() {
        return Err(AppError::validation(
            "Only a repository with a GitHub identity can be linked this way.",
        ));
    }
    if let Some(existing) = &current.local_path {
        if existing != &checkout.local_path {
            return Err(AppError::validation(format!(
                "This repository is already linked to {existing}."
            ))
            .with_context("path", existing.as_str()));
        }
    }
    let holder: Option<(String, String)> = conn
        .query_row(
            "SELECT id, name FROM repositories WHERE local_path = ?1 AND id <> ?2",
            params![checkout.local_path, repository_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((_, name)) = holder {
        return Err(AppError::validation(format!(
            "This folder is already registered as {name}. Remove that record first, then link again."
        ))
        .with_context("path", checkout.local_path.as_str()));
    }
    let m = &checkout.metadata;
    let wt = m.working_tree.as_ref();
    conn.execute(
        "UPDATE repositories SET local_path = ?2, remote_url = COALESCE(?3, remote_url),
             default_branch = COALESCE(?4, default_branch), current_branch = ?5, current_commit = ?6, git_kind = ?7,
             is_detached = ?8, is_dirty = ?9, staged_count = ?10, modified_count = ?11, untracked_count = ?12,
             last_commit_at = ?13, git_collected_at = ?14, git_warnings_json = ?15, ownership = ?16,
             github_link_method = ?17, updated_at = ?14
         WHERE id = ?1",
        params![
            repository_id,
            checkout.local_path,
            m.origin.as_ref().map(|o| o.sanitized_url.clone()),
            m.default_branch,
            m.current_branch,
            m.head_commit,
            checkout.git_kind.as_str(),
            m.detached,
            wt.map(|w| w.dirty()),
            wt.map(|w| w.staged),
            wt.map(|w| w.modified),
            wt.map(|w| w.untracked),
            m.last_commit_at,
            time_util::now(),
            serde_json::to_string(&m.warnings)?,
            checkout.ownership,
            checkout.method,
        ],
    )?;
    repositories::get(conn, repository_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::repositories::{GithubCoordinates, NewRepository, SourceType};
    use crate::db::Database;

    fn metadata_only(conn: &Connection) -> Repository {
        repositories::create(
            conn,
            &NewRepository {
                source_type: SourceType::Github,
                name: "api".into(),
                local_path: None,
                remote_url: Some("https://github.com/acme/api.git".into()),
                github: Some(GithubCoordinates {
                    host: "github.com".into(),
                    owner: "acme".into(),
                    repo: "api".into(),
                }),
                default_branch: Some("main".into()),
            },
        )
        .unwrap()
    }

    fn checkout(path: &str, method: LinkMethod) -> Checkout {
        Checkout {
            local_path: path.into(),
            git_kind: GitKind::Standard,
            metadata: GitMetadata {
                current_branch: Some("main".into()),
                detached: false,
                head_commit: Some("a".repeat(40)),
                default_branch: Some("main".into()),
                origin: Some(crate::git::remote::parse_remote(
                    "https://github.com/acme/api.git",
                )),
                last_commit_at: None,
                working_tree: None,
                warnings: vec![],
            },
            ownership: Ownership::ShiftopsManaged,
            method,
        }
    }

    #[test]
    fn workspace_approval_is_idempotent_and_latest_wins() {
        let db = Database::open_in_memory();
        db.write(|c| {
            assert!(active(c)?.is_none());
            let a = approve(c, "/work/a")?;
            let b = approve(c, "/work/b")?;
            assert_eq!(active(c)?.unwrap().id, b.id);
            let again = approve(c, "/work/a")?;
            assert_eq!(again.id, a.id, "re-approval keeps the record");
            assert_eq!(active(c)?.unwrap().id, a.id);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn metadata_record_becomes_managed_without_duplicates() {
        let db = Database::open_in_memory();
        db.write(|c| {
            let m = metadata_only(c);
            assert_eq!(m.ownership, None);
            assert_eq!(m.github_link_method, Some(LinkMethod::MetadataOnly));
            let linked =
                attach_checkout(c, &m.id, &checkout("/w/acme/api", LinkMethod::ManagedClone))?;
            assert_eq!(linked.id, m.id);
            assert_eq!(linked.local_path.as_deref(), Some("/w/acme/api"));
            assert_eq!(linked.ownership, Some(Ownership::ShiftopsManaged));
            assert_eq!(linked.github_link_method, Some(LinkMethod::ManagedClone));
            assert_eq!(linked.trust_status, repositories::TrustStatus::Untrusted);
            assert_eq!(repositories::list(c, &Default::default())?.total, 1);
            // Linking again to the same path is a refresh; another path is refused.
            attach_checkout(c, &m.id, &checkout("/w/acme/api", LinkMethod::ManagedClone))?;
            let err = attach_checkout(c, &m.id, &checkout("/elsewhere", LinkMethod::RemoteMatch))
                .unwrap_err();
            assert!(err.message.contains("already linked"));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn clone_attempts_hold_their_destination_until_cleaned() {
        let db = Database::open_in_memory();
        db.write(|c| {
            let m = metadata_only(c);
            let ws = approve(c, "/w")?;
            let new = NewClone {
                repository_id: m.id.clone(),
                github_repository_id: None,
                workspace_id: ws.id.clone(),
                full_name: "acme/api".into(),
                clone_url: "https://github.com/acme/api.git".into(),
                destination_path: "/w/acme/api".into(),
            };
            let first = begin_clone(c, &new)?;
            assert_eq!(first.status, CloneStatus::Running);
            assert!(begin_clone(c, &new)
                .unwrap_err()
                .message
                .contains("still holds"));
            let failed = finish_clone(
                c,
                &first.id,
                CloneStatus::Failed,
                Some((
                    "COMMAND_FAILED",
                    "fatal: Authorization: Bearer ghp_abcdefghijklmnopqrstuvwxyz0123456789",
                )),
                true,
            )?;
            assert!(!failed.error_message.unwrap().contains("ghp_abcdef"));
            assert!(finish_clone(c, &first.id, CloneStatus::Succeeded, None, false).is_err());
            assert_eq!(open_attempt_at(c, "/w/acme/api")?.unwrap().id, first.id);
            assert!(
                begin_clone(c, &new).is_err(),
                "a retained failure still holds the path"
            );
            let cleaned = mark_cleaned(c, &first.id)?;
            assert_eq!(cleaned.status, CloneStatus::CleanedUp);
            assert!(mark_cleaned(c, &first.id).is_err());
            assert!(open_attempt_at(c, "/w/acme/api")?.is_none());
            let second = begin_clone(c, &new)?;
            assert_eq!(fail_interrupted(c)?, 1);
            assert_eq!(get_clone(c, &second.id)?.status, CloneStatus::Failed);
            assert_eq!(list_clones(c, 10)?.len(), 2);
            Ok(())
        })
        .unwrap();
    }
}
