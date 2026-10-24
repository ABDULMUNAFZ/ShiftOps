//! Repository import modes beyond metadata-only: cloning into a
//! ShiftOps-managed workspace (`clone`) and linking an existing checkout
//! (`link`).
//!
//! The managed workspace is a folder the user explicitly approved. ShiftOps
//! clones into `<workspace>/<owner>/<repository>`, creating that directory
//! itself, and may later delete only that exact directory, only after a clone
//! failed or was cancelled, and only when the user confirms. It never deletes
//! the workspace, an owner folder with other content, the home directory, or
//! any external checkout.

pub mod clone;
pub mod link;

use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::json;

use crate::db::activity::{self, NewEvent, Severity};
use crate::db::github as gh_store;
use crate::db::workspace::{self as store, CloneStatus, ManagedWorkspace, RepositoryClone};
use crate::db::Database;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::git::policy::{valid_segment, validate_clone_url};
use crate::github::client::GithubConfig;

fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .and_then(|p| std::fs::canonicalize(p).ok())
}

fn path_error(message: impl Into<String>, path: &Path) -> AppError {
    AppError::new(ErrorCode::PathNotAllowed, message)
        .with_context("path", path.display().to_string())
}

/// Validate a folder the user picked as the managed workspace root. Returns
/// the canonical path that will be shown for approval and persisted.
pub fn validate_workspace_root(db: &Database, input: &str) -> AppResult<PathBuf> {
    let canonical = crate::discovery::validate_root(input)?;
    if let Some(home) = home_dir() {
        if home.starts_with(&canonical) {
            return Err(path_error(
                "The home folder (or a folder containing it) cannot be the managed workspace. Choose or create a dedicated folder, for example ~/ShiftOps Workspace.",
                &canonical,
            ));
        }
    }
    if crate::git::inspect::detect_marker(&canonical).is_some() {
        return Err(path_error(
            "This folder is itself a Git repository. Choose a dedicated folder for managed clones.",
            &canonical,
        ));
    }
    let registered = db.read(crate::db::repositories::registered_local_paths)?;
    if let Some(repo) = registered
        .iter()
        .find(|p| canonical.starts_with(p) || Path::new(p).starts_with(&canonical))
    {
        return Err(path_error(
            format!("This folder overlaps the registered repository at {repo}. Choose a folder that contains no registered repositories."),
            &canonical,
        ));
    }
    Ok(canonical)
}

/// Persist the approved root and record the approval.
pub fn approve_workspace(db: &Database, input: &str) -> AppResult<ManagedWorkspace> {
    let canonical = validate_workspace_root(db, input)?;
    let path = canonical.display().to_string();
    db.write(|c| {
        let ws = store::approve(c, &path)?;
        activity::record(
            c,
            NewEvent::new(
                "managed_workspace",
                "workspace.approved",
                Severity::Notice,
                format!("Approved managed workspace {path}"),
            )
            .entity(ws.id.clone())
            .metadata(json!({ "path": path })),
        )?;
        Ok(ws)
    })
}

/// The approved root, re-resolved now: it must still exist and still resolve
/// to the same canonical path (it was not moved or replaced by a link).
pub fn resolve_workspace(ws: &ManagedWorkspace) -> AppResult<PathBuf> {
    let stored = PathBuf::from(&ws.canonical_path);
    let meta = std::fs::symlink_metadata(&stored).map_err(|_| {
        path_error(
            "The managed workspace folder no longer exists. Approve a workspace again.",
            &stored,
        )
    })?;
    let now = std::fs::canonicalize(&stored).ok();
    if meta.file_type().is_symlink() || !meta.is_dir() || now.as_deref() != Some(stored.as_path()) {
        return Err(path_error(
            "The managed workspace folder was moved or replaced by a link. Approve a workspace again.",
            &stored,
        ));
    }
    Ok(stored)
}

/// `<root>/<owner>/<name>`, after checking both are plain GitHub name segments.
pub fn destination_for(root: &Path, owner: &str, name: &str) -> AppResult<PathBuf> {
    for (value, what) in [(owner, "owner"), (name, "repository name")] {
        if !valid_segment(value) {
            return Err(AppError::validation(format!(
                "The GitHub {what} cannot be used as a folder name."
            ))
            .with_context(what, value));
        }
    }
    Ok(root.join(owner).join(name))
}

/// Exactly two normal components below `root`.
fn is_direct_destination(root: &Path, dest: &Path) -> bool {
    dest.strip_prefix(root).is_ok_and(|rel| {
        let parts: Vec<_> = rel.components().collect();
        parts.len() == 2 && parts.iter().all(|c| matches!(c, Component::Normal(_)))
    })
}

/// The clone URL, built from the configured host and the validated name. The
/// API's own `clone_url` is not used, and no credentials ever appear in it.
pub fn clone_url(config: &GithubConfig, owner: &str, name: &str) -> AppResult<url::Url> {
    let base = config.host.as_str().trim_end_matches('/');
    validate_clone_url(&format!("{base}/{owner}/{name}.git"))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClonePreview {
    pub github_repository_id: String,
    pub full_name: String,
    pub private: bool,
    pub workspace: Option<ManagedWorkspace>,
    pub clone_url: Option<String>,
    pub destination: Option<String>,
    /// The ShiftOps record the clone will attach to, when one exists already.
    pub repository_id: Option<String>,
    /// Reasons the clone cannot start. Empty means ready.
    pub problems: Vec<String>,
    pub ready: bool,
}

/// What a clone would do, without changing anything.
pub fn preview(
    db: &Database,
    config: &GithubConfig,
    github_repository_id: &str,
) -> AppResult<ClonePreview> {
    let gh = db.read(|c| gh_store::get_repository(c, github_repository_id))?;
    let mut problems = Vec::new();
    let workspace = db.read(store::active)?;
    let linked = gh
        .repository_id
        .as_deref()
        .map(|id| db.read(|c| crate::db::repositories::get(c, id)))
        .transpose()?;
    if let Some(path) = linked.as_ref().and_then(|r| r.local_path.as_deref()) {
        problems.push(format!(
            "This repository already has a local checkout at {path}."
        ));
    }
    let url = match clone_url(config, &gh.owner, &gh.name) {
        Ok(u) => Some(u.to_string()),
        Err(e) => {
            problems.push(e.message);
            None
        }
    };
    let mut destination = None;
    match &workspace {
        None => problems.push(
            "No managed workspace is approved yet. Choose and approve a folder first.".into(),
        ),
        Some(ws) => match resolve_workspace(ws)
            .and_then(|root| destination_for(&root, &gh.owner, &gh.name))
        {
            Err(e) => problems.push(e.message),
            Ok(dest) => {
                problems.extend(collisions(db, &dest)?);
                destination = Some(dest.display().to_string());
            }
        },
    }
    Ok(ClonePreview {
        github_repository_id: gh.id.clone(),
        full_name: gh.full_name.clone(),
        private: gh.private,
        workspace,
        clone_url: url,
        destination,
        repository_id: gh.repository_id.clone(),
        ready: problems.is_empty(),
        problems,
    })
}

/// Everything that makes `dest` unusable for a new clone.
fn collisions(db: &Database, dest: &Path) -> AppResult<Vec<String>> {
    let mut problems = Vec::new();
    let text = dest.display().to_string();
    if std::fs::symlink_metadata(dest).is_ok() {
        problems.push(format!(
            "{text} already exists. ShiftOps never clones into an existing folder."
        ));
    }
    if let Some(owner_dir) = dest.parent() {
        if let Ok(meta) = std::fs::symlink_metadata(owner_dir) {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                problems.push(format!(
                    "{} is a link or a file, not a folder.",
                    owner_dir.display()
                ));
            }
        }
    }
    let (registered, open) = db.read(|c| {
        Ok((
            crate::db::repositories::registered_local_paths(c)?,
            store::open_attempt_at(c, &text)?,
        ))
    })?;
    if registered.contains(&text) {
        problems.push(format!("A registered repository already uses {text}."));
    }
    if open.is_some() {
        problems.push(
            "An earlier clone attempt still holds this destination. Clean it up first.".into(),
        );
    }
    Ok(problems)
}

/// Create the owner folder if needed and the destination folder itself. The
/// destination must not exist: `create_dir` fails atomically if it does.
fn create_destination(root: &Path, dest: &Path) -> AppResult<()> {
    if !is_direct_destination(root, dest) {
        return Err(path_error(
            "The clone destination is not inside the managed workspace.",
            dest,
        ));
    }
    let owner_dir = dest.parent().unwrap_or(root);
    match std::fs::symlink_metadata(owner_dir) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(path_error(
                "The owner folder is a link or a file.",
                owner_dir,
            ))
        }
        Err(_) => std::fs::create_dir(owner_dir).map_err(|e| {
            path_error(
                format!("The owner folder could not be created: {e}"),
                owner_dir,
            )
        })?,
    }
    if std::fs::canonicalize(owner_dir).ok().as_deref() != Some(owner_dir) {
        return Err(path_error(
            "The owner folder resolves outside the managed workspace.",
            owner_dir,
        ));
    }
    std::fs::create_dir(dest).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            path_error(
                "The destination already exists. ShiftOps never clones into an existing folder.",
                dest,
            )
        } else {
            path_error(format!("The destination could not be created: {e}"), dest)
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupOutcome {
    /// The incomplete clone directory was deleted.
    Removed,
    /// The directory was already gone; the attempt is closed.
    AlreadyGone,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub outcome: CleanupOutcome,
    pub clone: RepositoryClone,
}

/// Every check that must pass before the recorded directory may be deleted.
fn cleanup_target(db: &Database, attempt: &RepositoryClone) -> AppResult<Option<PathBuf>> {
    if !matches!(attempt.status, CloneStatus::Failed | CloneStatus::Cancelled) {
        return Err(AppError::validation(
            "Only a failed or cancelled clone can be cleaned up. Successful clones and external checkouts are never deleted by ShiftOps.",
        ));
    }
    let ws = db.read(|c| store::get_workspace(c, &attempt.workspace_id))?;
    let root = resolve_workspace(&ws)?;
    let dest = PathBuf::from(&attempt.destination_path);
    if !is_direct_destination(&root, &dest) {
        return Err(path_error(
            "The recorded path is not a clone folder inside its managed workspace.",
            &dest,
        ));
    }
    let meta = match std::fs::symlink_metadata(&dest) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(path_error(
                format!("The recorded path cannot be inspected: {e}"),
                &dest,
            ))
        }
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(path_error(
            "The recorded path is now a link or a file; it will not be deleted.",
            &dest,
        ));
    }
    if std::fs::canonicalize(&dest).ok().as_deref() != Some(dest.as_path()) {
        return Err(path_error(
            "The recorded path no longer resolves to itself; it will not be deleted.",
            &dest,
        ));
    }
    if let Some(home) = home_dir() {
        if home.starts_with(&dest) {
            return Err(path_error("The home folder is never deleted.", &dest));
        }
    }
    let (registered, roots) = db.read(|c| {
        Ok((
            crate::db::repositories::registered_local_paths(c)?,
            crate::db::roots::list(c)?,
        ))
    })?;
    if let Some(p) = registered
        .iter()
        .find(|p| Path::new(p).starts_with(&dest) || dest.starts_with(p))
    {
        return Err(path_error(
            format!(
                "The registered repository at {p} overlaps this folder; it will not be deleted."
            ),
            &dest,
        ));
    }
    if let Some(r) = roots
        .iter()
        .find(|r| Path::new(&r.canonical_path).starts_with(&dest))
    {
        return Err(path_error(
            format!(
                "The tracked folder {} is inside this folder; it will not be deleted.",
                r.canonical_path
            ),
            &dest,
        ));
    }
    Ok(Some(dest))
}

/// Delete the exact incomplete clone directory of a failed or cancelled
/// attempt. `confirmed_path` must repeat the recorded path, so the user
/// confirms what is deleted rather than an id.
pub fn cleanup(db: &Database, clone_id: &str, confirmed_path: &str) -> AppResult<CleanupResult> {
    let attempt = db.read(|c| store::get_clone(c, clone_id))?;
    if confirmed_path != attempt.destination_path {
        return Err(AppError::validation(
            "The confirmed path does not match the recorded clone folder. Nothing was deleted.",
        )
        .with_context("recorded", attempt.destination_path.as_str()));
    }
    let target = cleanup_target(db, &attempt)?;
    let outcome = match &target {
        None => CleanupOutcome::AlreadyGone,
        Some(dest) => {
            // `remove_dir_all` does not follow symbolic links inside the tree.
            std::fs::remove_dir_all(dest).map_err(|e| {
                path_error(
                    format!("The folder could not be deleted completely: {e}"),
                    dest,
                )
            })?;
            // Remove the owner folder only if this left it empty.
            if let Some(owner) = dest.parent() {
                let _ = std::fs::remove_dir(owner);
            }
            CleanupOutcome::Removed
        }
    };
    db.write(|c| {
        let clone = match outcome {
            CleanupOutcome::Removed => store::mark_cleaned(c, clone_id)?,
            CleanupOutcome::AlreadyGone => store::release_missing(c, clone_id)?,
        };
        activity::record(
            c,
            NewEvent::new(
                "repository_clone",
                "clone.cleaned_up",
                Severity::Notice,
                match outcome {
                    CleanupOutcome::Removed => format!("Deleted incomplete clone folder {}", clone.destination_path),
                    CleanupOutcome::AlreadyGone => {
                        format!("Closed clone attempt; {} was already gone", clone.destination_path)
                    }
                },
            )
            .entity(clone.id.clone())
            .metadata(json!({ "path": clone.destination_path, "outcome": outcome, "repositoryId": clone.repository_id })),
        )?;
        Ok(CleanupResult { outcome, clone })
    })
}

/// Delete the files of a successful ShiftOps-managed clone after explicit
/// confirmation, then turn the record back into metadata-only.
///
/// Only a checkout ShiftOps itself cloned qualifies: ownership
/// `shiftops_managed`, a succeeded clone record for the exact path, the path
/// directly at `<approved workspace>/<owner>/<name>`, a real folder (not a
/// link) that resolves to itself, no other registered repository or tracked
/// folder inside it, and a clean working tree (`working_tree_clean`, read by
/// the caller with read-only Git). External checkouts are never deleted.
pub fn delete_managed_checkout(
    db: &Database,
    repository_id: &str,
    confirmed_path: &str,
    working_tree_clean: bool,
) -> AppResult<crate::db::repositories::Repository> {
    use crate::db::repositories::{self, Ownership};
    let repo = db.read(|c| repositories::get(c, repository_id))?;
    if repo.ownership != Some(Ownership::ShiftopsManaged) {
        return Err(AppError::validation(
            "Only checkouts ShiftOps cloned into its managed workspace can be deleted. External checkouts are never deleted; unlink or remove the record instead.",
        ));
    }
    let path = repo
        .local_path
        .clone()
        .ok_or_else(|| AppError::validation("This repository has no checkout."))?;
    if confirmed_path != path {
        return Err(AppError::validation(
            "The confirmed path does not match the checkout. Nothing was deleted.",
        )
        .with_context("recorded", path.as_str()));
    }
    if !working_tree_clean {
        return Err(AppError::validation(
            "The checkout has uncommitted changes. Commit and push or discard them yourself first; ShiftOps never deletes unsaved work.",
        ));
    }
    let clone = db
        .read(|c| store::succeeded_clone_at(c, &path))?
        .ok_or_else(|| {
            path_error(
                "ShiftOps has no record of cloning this folder, so it will not delete it.",
                Path::new(&path),
            )
        })?;
    let ws = db.read(|c| store::get_workspace(c, &clone.workspace_id))?;
    let root = resolve_workspace(&ws)?;
    let dest = PathBuf::from(&path);
    if !is_direct_destination(&root, &dest) {
        return Err(path_error(
            "The checkout is not a clone folder inside its managed workspace.",
            &dest,
        ));
    }
    let meta = std::fs::symlink_metadata(&dest)
        .map_err(|e| path_error(format!("The checkout cannot be inspected: {e}"), &dest))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(path_error(
            "The checkout is now a link or a file; it will not be deleted.",
            &dest,
        ));
    }
    if std::fs::canonicalize(&dest).ok().as_deref() != Some(dest.as_path()) {
        return Err(path_error(
            "The checkout no longer resolves to itself; it will not be deleted.",
            &dest,
        ));
    }
    if let Some(home) = home_dir() {
        if home.starts_with(&dest) {
            return Err(path_error("The home folder is never deleted.", &dest));
        }
    }
    let (registered, roots) = db.read(|c| {
        Ok((
            crate::db::repositories::registered_local_paths(c)?,
            crate::db::roots::list(c)?,
        ))
    })?;
    if let Some(p) = registered
        .iter()
        .filter(|p| **p != path)
        .find(|p| Path::new(p).starts_with(&dest) || dest.starts_with(p))
    {
        return Err(path_error(
            format!(
                "The registered repository at {p} overlaps this folder; it will not be deleted."
            ),
            &dest,
        ));
    }
    if let Some(r) = roots
        .iter()
        .find(|r| Path::new(&r.canonical_path).starts_with(&dest))
    {
        return Err(path_error(
            format!(
                "The tracked folder {} is inside this folder; it will not be deleted.",
                r.canonical_path
            ),
            &dest,
        ));
    }
    std::fs::remove_dir_all(&dest).map_err(|e| {
        path_error(
            format!("The folder could not be deleted completely: {e}"),
            &dest,
        )
    })?;
    if let Some(owner) = dest.parent() {
        let _ = std::fs::remove_dir(owner);
    }
    db.write(|c| {
        let updated = repositories::unlink_checkout(c, repository_id)?;
        activity::record(
            c,
            NewEvent::new(
                "repository",
                "repository.managed_files_deleted",
                Severity::Warning,
                format!(
                    "Deleted the managed clone folder {path}; the record is metadata-only again"
                ),
            )
            .entity(repository_id.to_string())
            .metadata(json!({ "path": path, "cloneId": clone.id, "workspace": ws.canonical_path })),
        )?;
        Ok(updated)
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::db::repositories::{self, GithubCoordinates, NewRepository, SourceType};
    use crate::db::workspace::NewClone;

    pub(crate) fn config() -> GithubConfig {
        GithubConfig {
            host: url::Url::parse("https://github.com/").unwrap(),
            api_base: url::Url::parse("https://api.github.com/").unwrap(),
            oauth_client_id: None,
            poll_unit: std::time::Duration::from_millis(1),
        }
    }

    fn approved(db: &Database) -> (tempfile::TempDir, PathBuf, ManagedWorkspace) {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap().join("ws");
        std::fs::create_dir(&root).unwrap();
        let ws = approve_workspace(db, &root.display().to_string()).unwrap();
        (t, root, ws)
    }

    fn failed_attempt(db: &Database, ws: &ManagedWorkspace, dest: &Path) -> RepositoryClone {
        db.write(|c| {
            let repo = repositories::create(
                c,
                &NewRepository {
                    source_type: SourceType::Github,
                    name: "api".into(),
                    local_path: None,
                    remote_url: None,
                    github: Some(GithubCoordinates {
                        host: "github.com".into(),
                        owner: "acme".into(),
                        repo: dest.file_name().unwrap().to_string_lossy().into_owned(),
                    }),
                    default_branch: None,
                },
            )?;
            let attempt = store::begin_clone(
                c,
                &NewClone {
                    repository_id: repo.id,
                    github_repository_id: None,
                    workspace_id: ws.id.clone(),
                    full_name: "acme/api".into(),
                    clone_url: "https://github.com/acme/api.git".into(),
                    destination_path: dest.display().to_string(),
                },
            )?;
            store::finish_clone(
                c,
                &attempt.id,
                CloneStatus::Failed,
                Some(("COMMAND_FAILED", "x")),
                true,
            )
        })
        .unwrap()
    }

    #[test]
    fn workspace_root_must_be_a_dedicated_real_folder() {
        let db = Database::open_in_memory();
        let t = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(t.path()).unwrap();
        // Symlinked choice is stored as its canonical target.
        std::fs::create_dir(base.join("real")).unwrap();
        std::os::unix::fs::symlink(base.join("real"), base.join("link")).unwrap();
        let ws = approve_workspace(&db, &base.join("link").display().to_string()).unwrap();
        assert_eq!(ws.canonical_path, base.join("real").display().to_string());
        // Traversal is normalised before it is checked.
        let traversal = format!("{}/real/../real", base.display());
        assert_eq!(
            approve_workspace(&db, &traversal).unwrap().canonical_path,
            ws.canonical_path
        );
        assert!(approve_workspace(&db, "relative/path").is_err());
        assert!(approve_workspace(&db, "/").is_err());
        if let Some(home) = home_dir() {
            let err = validate_workspace_root(&db, &home.display().to_string()).unwrap_err();
            assert_eq!(err.code, ErrorCode::PathNotAllowed);
        }
        std::fs::create_dir_all(base.join("repo/.git")).unwrap();
        assert!(validate_workspace_root(&db, &base.join("repo").display().to_string()).is_err());
        // Overlapping a registered checkout is refused in both directions.
        std::fs::create_dir_all(base.join("parent/checkout")).unwrap();
        db.write(|c| {
            c.execute(
                "INSERT INTO repositories (id, source_type, name, local_path, created_at, updated_at)
                 VALUES ('x', 'local', 'checkout', ?1, 't', 't')",
                [base.join("parent/checkout").display().to_string()],
            )?;
            Ok(())
        })
        .unwrap();
        assert!(validate_workspace_root(&db, &base.join("parent").display().to_string()).is_err());
        assert!(
            validate_workspace_root(&db, &base.join("parent/checkout").display().to_string())
                .is_err()
        );
    }

    #[test]
    fn destinations_are_validated_and_collisions_detected() {
        let db = Database::open_in_memory();
        let (_t, root, ws) = approved(&db);
        for (owner, name) in [
            ("..", "x"),
            ("acme", ".."),
            ("acme", "a/b"),
            ("-x", "y"),
            ("acme", ".git"),
            ("", "y"),
        ] {
            assert!(
                destination_for(&root, owner, name).is_err(),
                "{owner}/{name}"
            );
        }
        let dest = destination_for(&root, "acme", "api").unwrap();
        assert_eq!(dest, root.join("acme/api"));
        assert!(collisions(&db, &dest).unwrap().is_empty());
        create_destination(&root, &dest).unwrap();
        assert!(dest.is_dir());
        assert!(!collisions(&db, &dest).unwrap().is_empty(), "now exists");
        assert!(
            create_destination(&root, &dest).is_err(),
            "never reuses a folder"
        );
        // An owner folder replaced by a link is refused.
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("evil")).unwrap();
        let via_link = destination_for(&root, "evil", "api").unwrap();
        assert!(!collisions(&db, &via_link).unwrap().is_empty());
        assert!(create_destination(&root, &via_link).is_err());
        assert!(!outside.path().join("api").exists());
        assert!(create_destination(&root, &root.join("a/b/c")).is_err());
        assert!(create_destination(&root, &root.join("a")).is_err());
        // A moved workspace is detected.
        std::fs::rename(&root, root.with_file_name("moved")).unwrap();
        assert!(resolve_workspace(&ws).is_err());
    }

    #[test]
    fn clone_urls_never_carry_credentials() {
        let url = clone_url(&config(), "acme", "api").unwrap();
        assert_eq!(url.as_str(), "https://github.com/acme/api.git");
        assert!(clone_url(&config(), "acme", "a b").is_err());
    }

    #[test]
    fn cleanup_deletes_only_the_recorded_incomplete_folder_after_confirmation() {
        let db = Database::open_in_memory();
        let (_t, root, ws) = approved(&db);
        let dest = root.join("acme/api");
        create_destination(&root, &dest).unwrap();
        std::fs::write(dest.join("partial.pack"), "x").unwrap();
        // A link inside the partial clone must not be followed.
        let precious = tempfile::tempdir().unwrap();
        std::fs::write(precious.path().join("keep.txt"), "keep").unwrap();
        std::os::unix::fs::symlink(precious.path(), dest.join("link")).unwrap();
        std::fs::create_dir_all(root.join("acme/other")).unwrap();
        let attempt = failed_attempt(&db, &ws, &dest);

        let wrong = cleanup(&db, &attempt.id, &root.display().to_string()).unwrap_err();
        assert!(wrong.message.contains("does not match"));
        assert!(
            dest.exists(),
            "nothing deleted without matching confirmation"
        );

        let done = cleanup(&db, &attempt.id, &attempt.destination_path).unwrap();
        assert_eq!(done.outcome, CleanupOutcome::Removed);
        assert_eq!(done.clone.status, CloneStatus::CleanedUp);
        assert!(!dest.exists());
        assert!(
            root.join("acme/other").exists(),
            "siblings and the owner folder stay"
        );
        assert!(root.exists());
        assert!(
            precious.path().join("keep.txt").exists(),
            "links are not followed"
        );
        assert!(
            cleanup(&db, &attempt.id, &attempt.destination_path).is_err(),
            "only once"
        );
        let events = db.read(|c| activity::list(c, &Default::default())).unwrap();
        assert!(events
            .items
            .iter()
            .any(|e| e.event_type == "clone.cleaned_up"));
    }

    #[test]
    fn cleanup_refuses_unsafe_targets() {
        let db = Database::open_in_memory();
        let (_t, root, ws) = approved(&db);

        // A recorded path that escapes the workspace (tampered record) is refused.
        let outside = tempfile::tempdir().unwrap();
        let escaped = failed_attempt(&db, &ws, &outside.path().join("api"));
        std::fs::create_dir(outside.path().join("api")).unwrap();
        assert!(cleanup(&db, &escaped.id, &escaped.destination_path).is_err());
        assert!(outside.path().join("api").exists());

        // The workspace root itself, or a path one level down, is never a target.
        let shallow = failed_attempt(&db, &ws, &root.join("acme"));
        std::fs::create_dir(root.join("acme")).unwrap();
        assert!(cleanup(&db, &shallow.id, &shallow.destination_path).is_err());
        assert!(root.join("acme").exists());

        // A destination replaced by a link after the failure is refused.
        let target = tempfile::tempdir().unwrap();
        let linked = failed_attempt(&db, &ws, &root.join("acme/linked"));
        std::os::unix::fs::symlink(target.path(), root.join("acme/linked")).unwrap();
        assert!(cleanup(&db, &linked.id, &linked.destination_path).is_err());
        assert!(target.path().exists());

        // A registered external checkout at the path is never deleted.
        let registered = root.join("acme/external");
        std::fs::create_dir(&registered).unwrap();
        let attempt = failed_attempt(&db, &ws, &registered);
        db.write(|c| {
            c.execute(
                "INSERT INTO repositories (id, source_type, name, local_path, ownership, created_at, updated_at)
                 VALUES ('ext', 'local', 'external', ?1, 'external', 't', 't')",
                [registered.display().to_string()],
            )?;
            Ok(())
        })
        .unwrap();
        assert!(cleanup(&db, &attempt.id, &attempt.destination_path).is_err());
        assert!(registered.exists());

        // Running and succeeded attempts cannot be cleaned up.
        let running = db
            .write(|c| {
                let a = store::begin_clone(
                    c,
                    &NewClone {
                        repository_id: "ext".into(),
                        github_repository_id: None,
                        workspace_id: ws.id.clone(),
                        full_name: "acme/run".into(),
                        clone_url: "https://github.com/acme/run.git".into(),
                        destination_path: root.join("acme/run").display().to_string(),
                    },
                )?;
                Ok(a)
            })
            .unwrap();
        std::fs::create_dir(root.join("acme/run")).unwrap();
        assert!(cleanup(&db, &running.id, &running.destination_path).is_err());
        assert!(root.join("acme/run").exists());

        // A folder that is already gone just closes the attempt.
        let gone = failed_attempt(&db, &ws, &root.join("acme/gone"));
        let result = cleanup(&db, &gone.id, &gone.destination_path).unwrap();
        assert_eq!(result.outcome, CleanupOutcome::AlreadyGone);
    }
}
