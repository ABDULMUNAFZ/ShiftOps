//! Link an existing local clone to a cached GitHub repository. The checkout
//! is `external`: ShiftOps reads it with read-only Git and never modifies or
//! deletes its files.

#[cfg(all(test, unix))]
use std::path::Path;

use serde::Serialize;
use serde_json::json;

use crate::db::activity::{self, NewEvent, Severity};
use crate::db::github as gh_store;
use crate::db::repositories::{LinkMethod, Ownership, Repository};
use crate::db::workspace::{self as store, Checkout};
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::git::inspect::{self, GitKind, GitMetadata};
use crate::process::{ExecutionPolicy, ProcessRunner};
use crate::shell_path::SearchPath;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub host: String,
    pub owner: String,
    pub repo: String,
}

impl Identity {
    pub fn display(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.repo)
    }

    fn same(&self, other: &Identity) -> bool {
        self.host.eq_ignore_ascii_case(&other.host)
            && self.owner.eq_ignore_ascii_case(&other.owner)
            && self.repo.eq_ignore_ascii_case(&other.repo)
    }
}

/// Shown for confirmation when the folder has no recognisable origin.
pub const NO_REMOTE: &str = "(no origin remote)";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkInspection {
    pub github_repository_id: String,
    pub path: String,
    pub git_kind: GitKind,
    pub remote_url: Option<String>,
    pub expected: Identity,
    pub actual: Option<Identity>,
    /// What the user must confirm to override a mismatch.
    pub actual_display: String,
    pub matches: bool,
    pub warnings: Vec<String>,
}

/// Read the folder's remote with read-only Git and compare it with the
/// selected GitHub repository. Changes nothing.
pub async fn inspect_link(
    db: &Database,
    search: &SearchPath,
    github_repository_id: &str,
    path: &str,
) -> AppResult<(LinkInspection, GitMetadata)> {
    let canonical = crate::discovery::validate_root(path)?;
    let (git_kind, git_dir) = match inspect::detect_marker(&canonical) {
        Some(Ok(found)) => found,
        Some(Err(why)) => return Err(AppError::validation(format!("This folder is not a usable Git repository: {why}"))),
        None => {
            return Err(AppError::validation(
                "This folder is not a Git repository (no `.git`). Choose the top folder of a local clone.",
            )
            .with_context("path", canonical.display().to_string()))
        }
    };
    let (gh, host) = db.read(|c| {
        let gh = gh_store::get_repository(c, github_repository_id)?;
        let host: String = c.query_row(
            "SELECT host FROM github_accounts WHERE id = ?1",
            [&gh.account_id],
            |r| r.get(0),
        )?;
        Ok((gh, host))
    })?;
    let runner = ProcessRunner::new(
        ExecutionPolicy::readonly_git(vec![canonical.clone()]),
        None,
        None,
    );
    let metadata = inspect::collect(&runner, search, &canonical, &git_dir).await;
    let expected = Identity {
        host,
        owner: gh.owner.clone(),
        repo: gh.name.clone(),
    };
    let actual = metadata
        .origin
        .as_ref()
        .and_then(|o| match (&o.host, &o.owner, &o.repo) {
            (Some(h), Some(ow), Some(r)) => Some(Identity {
                host: h.clone(),
                owner: ow.clone(),
                repo: r.clone(),
            }),
            _ => None,
        });
    let matches = actual.as_ref().is_some_and(|a| a.same(&expected));
    let inspection = LinkInspection {
        github_repository_id: gh.id.clone(),
        path: canonical.display().to_string(),
        git_kind,
        remote_url: metadata.origin.as_ref().map(|o| o.sanitized_url.clone()),
        actual_display: actual
            .as_ref()
            .map_or_else(|| NO_REMOTE.to_string(), Identity::display),
        expected,
        actual,
        matches,
        warnings: metadata.warnings.clone(),
    };
    Ok((inspection, metadata))
}

/// Link the folder. On a mismatch the caller must pass the exact
/// `actualDisplay` the user confirmed in `confirmed_override`.
pub async fn link(
    db: &Database,
    search: &SearchPath,
    github_repository_id: &str,
    path: &str,
    confirmed_override: Option<&str>,
) -> AppResult<(Repository, LinkInspection)> {
    let (inspection, metadata) = inspect_link(db, search, github_repository_id, path).await?;
    let overridden = !inspection.matches;
    if overridden && confirmed_override != Some(inspection.actual_display.as_str()) {
        return Err(AppError::validation(format!(
            "The folder's remote ({}) does not match {}. Confirm the override to link it anyway.",
            inspection.actual_display,
            inspection.expected.display()
        ))
        .with_context("expected", inspection.expected.display())
        .with_context("actual", inspection.actual_display.clone()));
    }
    let repo = db.write(|c| {
        let (record, _) = gh_store::import_metadata_only(c, github_repository_id)?;
        let linked = store::attach_checkout(
            c,
            &record.id,
            &Checkout {
                local_path: inspection.path.clone(),
                git_kind: inspection.git_kind,
                metadata: metadata.clone(),
                ownership: Ownership::External,
                method: if overridden {
                    LinkMethod::Override
                } else {
                    LinkMethod::RemoteMatch
                },
            },
        )?;
        activity::record(
            c,
            NewEvent::new(
                "repository",
                "repository.linked_local",
                if overridden {
                    Severity::Warning
                } else {
                    Severity::Notice
                },
                format!(
                    "Linked {} to {}{}",
                    inspection.expected.display(),
                    inspection.path,
                    if overridden {
                        " (remote mismatch overridden)"
                    } else {
                        ""
                    }
                ),
            )
            .entity(linked.id.clone())
            .metadata(json!({
                "path": inspection.path,
                "expected": inspection.expected.display(),
                "actual": inspection.actual_display,
                "override": overridden,
                "ownership": "external",
            })),
        )?;
        Ok(linked)
    })?;
    Ok((repo, inspection))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::workspace::clone::tests::{git, github_record, search};

    fn checkout(dir: &Path, remote: Option<&str>) {
        std::fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q"]);
        std::fs::write(dir.join("README.md"), "hi").unwrap();
        git(dir, &["add", "README.md"]);
        git(dir, &["commit", "-q", "-m", "init"]);
        if let Some(r) = remote {
            git(dir, &["remote", "add", "origin", r]);
        }
    }

    #[tokio::test]
    async fn matching_remote_links_as_external_without_duplicates() {
        let t = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(t.path()).unwrap().join("api");
        checkout(&dir, Some("git@github.com:ACME/api.git"));
        let db = Database::open_in_memory();
        let gh = github_record(&db, "github.com", false);
        let (meta, _) = crate::github::import(&db, &gh).unwrap();
        let (repo, inspection) = link(&db, search(), &gh, dir.to_str().unwrap(), None)
            .await
            .unwrap();
        assert!(inspection.matches, "owner comparison ignores case");
        assert_eq!(repo.id, meta.id);
        assert_eq!(repo.ownership, Some(Ownership::External));
        assert_eq!(repo.github_link_method, Some(LinkMethod::RemoteMatch));
        assert_eq!(
            db.read(|c| crate::db::repositories::list(c, &Default::default()))
                .unwrap()
                .total,
            1
        );
        assert!(dir.join("README.md").exists());
    }

    #[tokio::test]
    async fn mismatch_needs_exact_confirmation_and_survives_rediscovery() {
        let t = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(t.path()).unwrap().join("fork");
        checkout(&dir, Some("https://github.com/me/api.git"));
        let db = Database::open_in_memory();
        let gh = github_record(&db, "github.com", false);
        let (i, _) = inspect_link(&db, search(), &gh, dir.to_str().unwrap())
            .await
            .unwrap();
        assert!(!i.matches);
        assert_eq!(i.expected.display(), "github.com/acme/api");
        assert_eq!(i.actual_display, "github.com/me/api");
        let err = link(&db, search(), &gh, dir.to_str().unwrap(), None)
            .await
            .unwrap_err();
        assert!(err.message.contains("does not match"));
        assert!(link(
            &db,
            search(),
            &gh,
            dir.to_str().unwrap(),
            Some("github.com/other/x")
        )
        .await
        .is_err());
        let (repo, _) = link(
            &db,
            search(),
            &gh,
            dir.to_str().unwrap(),
            Some("github.com/me/api"),
        )
        .await
        .unwrap();
        assert_eq!(repo.github_link_method, Some(LinkMethod::Override));
        assert_eq!(repo.github_owner.as_deref(), Some("acme"));
        // Rediscovering the folder keeps the overridden identity.
        let runner =
            ProcessRunner::new(ExecutionPolicy::readonly_git(vec![dir.clone()]), None, None);
        let metadata = inspect::collect(&runner, search(), &dir, &dir.join(".git")).await;
        let (again, created) = db
            .write(|c| {
                let root = crate::db::roots::record_discovery(
                    c,
                    &crate::db::roots::RootSettings {
                        canonical_path: dir.display().to_string(),
                        display_name: "fork".into(),
                        root_kind: crate::db::roots::RootKind::Repository,
                        max_depth: 0,
                        follow_internal_symlinks: false,
                    },
                )?;
                crate::db::repositories::record_local_discovery(
                    c,
                    &root.id,
                    &crate::db::repositories::LocalRepositoryRecord {
                        name: "fork".into(),
                        local_path: dir.display().to_string(),
                        git_kind: GitKind::Standard,
                        metadata,
                        collected_at: crate::time_util::now(),
                    },
                )
            })
            .unwrap();
        assert!(!created);
        assert_eq!(again.github_owner.as_deref(), Some("acme"));
        assert_eq!(again.github_link_method, Some(LinkMethod::Override));
        assert_eq!(again.ownership, Some(Ownership::External));
    }

    #[tokio::test]
    async fn non_repositories_are_refused() {
        let t = tempfile::tempdir().unwrap();
        let db = Database::open_in_memory();
        let gh = github_record(&db, "github.com", false);
        let err = inspect_link(&db, search(), &gh, t.path().to_str().unwrap())
            .await
            .unwrap_err();
        assert!(err.message.contains("not a Git repository"));
    }
}
