//! Approved repository roots: a single repository or a parent directory the
//! user chose for discovery. Removing a root deletes only this record;
//! repositories stay registered and nothing on disk is touched.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::{expect_changed, new_id, require_text, string_enum};
use crate::error::{AppError, AppResult};
use crate::time_util;

string_enum!(RootKind { Repository => "repository", Directory => "directory" });

pub const MAX_DEPTH: u32 = 12;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryRoot {
    pub id: String,
    pub canonical_path: String,
    pub display_name: String,
    pub root_kind: RootKind,
    pub max_depth: u32,
    pub follow_internal_symlinks: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_discovered_at: Option<String>,
    pub repository_count: u32,
}

const COLUMNS: &str = "SELECT r.id, r.canonical_path, r.display_name, r.root_kind, r.max_depth, \
    r.follow_internal_symlinks, r.created_at, r.updated_at, r.last_discovered_at, \
    (SELECT COUNT(*) FROM repositories p WHERE p.root_id = r.id) AS repository_count";

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<RepositoryRoot> {
    Ok(RepositoryRoot {
        id: r.get("id")?,
        canonical_path: r.get("canonical_path")?,
        display_name: r.get("display_name")?,
        root_kind: r.get("root_kind")?,
        max_depth: r.get("max_depth")?,
        follow_internal_symlinks: r.get("follow_internal_symlinks")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        last_discovered_at: r.get("last_discovered_at")?,
        repository_count: r.get("repository_count")?,
    })
}

#[derive(Debug, Clone)]
pub struct RootSettings {
    pub canonical_path: String,
    pub display_name: String,
    pub root_kind: RootKind,
    pub max_depth: u32,
    pub follow_internal_symlinks: bool,
}

/// Insert or update a root keyed by canonical path and stamp the discovery time.
pub fn record_discovery(conn: &Connection, settings: &RootSettings) -> AppResult<RepositoryRoot> {
    require_text(&settings.display_name, "Root name", 255)?;
    if settings.max_depth > MAX_DEPTH {
        return Err(AppError::validation(format!(
            "Maximum depth must be between 0 and {MAX_DEPTH}."
        )));
    }
    let now = time_util::now();
    conn.execute(
        "INSERT INTO repository_roots (id, canonical_path, display_name, root_kind, max_depth,
             follow_internal_symlinks, created_at, updated_at, last_discovered_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?7)
         ON CONFLICT (canonical_path) DO UPDATE SET display_name = excluded.display_name,
             root_kind = excluded.root_kind, max_depth = excluded.max_depth,
             follow_internal_symlinks = excluded.follow_internal_symlinks,
             updated_at = excluded.updated_at, last_discovered_at = excluded.last_discovered_at",
        params![
            new_id(),
            settings.canonical_path,
            settings.display_name.trim(),
            settings.root_kind,
            settings.max_depth,
            settings.follow_internal_symlinks,
            now
        ],
    )?;
    Ok(conn.query_row(
        &format!("{COLUMNS} FROM repository_roots r WHERE r.canonical_path = ?1"),
        [&settings.canonical_path],
        map,
    )?)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<RepositoryRoot> {
    conn.query_row(
        &format!("{COLUMNS} FROM repository_roots r WHERE r.id = ?1"),
        [id],
        map,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("Repository root", id))
}

pub fn list(conn: &Connection) -> AppResult<Vec<RepositoryRoot>> {
    let mut stmt = conn.prepare(&format!(
        "{COLUMNS} FROM repository_roots r ORDER BY r.display_name COLLATE NOCASE"
    ))?;
    let rows = stmt.query_map([], map)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Remove the root record. Repositories found under it keep their records
/// with `root_id = NULL`. Never touches the filesystem.
pub fn remove(conn: &Connection, id: &str) -> AppResult<RepositoryRoot> {
    let root = get(conn, id)?;
    let changed = conn.execute("DELETE FROM repository_roots WHERE id = ?1", [id])?;
    expect_changed(changed, "Repository root", id)?;
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::repositories::{self, LocalRepositoryRecord};
    use crate::db::Database;
    use crate::git::inspect::{GitKind, GitMetadata, WorkingTreeCounts};
    use crate::git::remote::parse_remote;

    pub fn settings(path: &str) -> RootSettings {
        RootSettings {
            canonical_path: path.into(),
            display_name: "work".into(),
            root_kind: RootKind::Directory,
            max_depth: 4,
            follow_internal_symlinks: false,
        }
    }

    pub fn record(path: &str, remote: Option<&str>) -> LocalRepositoryRecord {
        LocalRepositoryRecord {
            name: path.rsplit('/').next().unwrap().into(),
            local_path: path.into(),
            git_kind: GitKind::Standard,
            metadata: GitMetadata {
                current_branch: Some("main".into()),
                detached: false,
                head_commit: Some("abc123".into()),
                default_branch: Some("main".into()),
                origin: remote.map(parse_remote),
                last_commit_at: Some("2026-10-24T10:00:00+05:30".into()),
                working_tree: Some(WorkingTreeCounts {
                    staged: 1,
                    modified: 2,
                    untracked: 3,
                }),
                warnings: vec![],
            },
            collected_at: "2026-10-24T10:00:00.000Z".into(),
        }
    }

    #[test]
    fn roots_upsert_by_path_and_removal_keeps_repositories() {
        let db = Database::open_in_memory();
        let root = db
            .write(|c| record_discovery(c, &settings("/work")))
            .unwrap();
        let again = db
            .write(|c| {
                record_discovery(
                    c,
                    &RootSettings {
                        max_depth: 2,
                        ..settings("/work")
                    },
                )
            })
            .unwrap();
        assert_eq!(root.id, again.id);
        assert_eq!(again.max_depth, 2);
        assert!(db
            .write(|c| record_discovery(
                c,
                &RootSettings {
                    max_depth: 13,
                    ..settings("/x")
                }
            ))
            .is_err());

        let (repo, created) = db
            .write(|c| {
                repositories::record_local_discovery(
                    c,
                    &root.id,
                    &record("/work/api", Some("https://ghp_x@github.com/acme/api.git")),
                )
            })
            .unwrap();
        assert!(created);
        assert_eq!(repo.trust_status, repositories::TrustStatus::Untrusted);
        assert_eq!(repo.github_owner.as_deref(), Some("acme"));
        assert_eq!(
            repo.remote_url.as_deref(),
            Some("https://github.com/acme/api.git")
        );
        assert_eq!((repo.is_dirty, repo.untracked_count), (Some(true), Some(3)));
        assert_eq!(db.read(|c| get(c, &root.id)).unwrap().repository_count, 1);

        db.write(|c| remove(c, &root.id)).unwrap();
        let kept = db.read(|c| repositories::get(c, &repo.id)).unwrap();
        assert_eq!(kept.root_id, None, "repository survives root removal");
        assert!(db.read(list).unwrap().is_empty());
    }

    #[test]
    fn rediscovery_updates_facts_but_never_trust() {
        let db = Database::open_in_memory();
        let root = db
            .write(|c| record_discovery(c, &settings("/work")))
            .unwrap();
        let (repo, _) = db
            .write(|c| {
                repositories::record_local_discovery(c, &root.id, &record("/work/web", None))
            })
            .unwrap();
        assert_eq!(
            repo.github_owner, None,
            "non-GitHub repositories are kept without coordinates"
        );
        db.write(|c| repositories::set_trust(c, &repo.id, repositories::TrustStatus::Trusted))
            .unwrap();
        let mut newer = record("/work/web", Some("git@gitlab.com:team/web.git"));
        newer.metadata.head_commit = Some("def456".into());
        newer.metadata.working_tree = None;
        newer.metadata.warnings = vec!["status skipped".into()];
        let (updated, created) = db
            .write(|c| repositories::record_local_discovery(c, &root.id, &newer))
            .unwrap();
        assert!(!created);
        assert_eq!(updated.id, repo.id);
        assert_eq!(updated.current_commit.as_deref(), Some("def456"));
        assert_eq!(updated.trust_status, repositories::TrustStatus::Trusted);
        assert_eq!(updated.is_dirty, None);
        assert_eq!(updated.git_warnings, serde_json::json!(["status skipped"]));
    }
}
