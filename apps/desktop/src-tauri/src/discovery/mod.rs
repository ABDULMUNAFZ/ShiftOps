//! Local repository discovery jobs: walk an approved root, inspect each
//! repository with read-only Git, stream progress, support cancellation,
//! and import the user's selection into SQLite.

pub mod walk;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use serde_json::json;
use tokio::sync::Semaphore;

use crate::db::activity::{self, NewEvent, Severity};
use crate::db::repositories::{self, LocalRepositoryRecord};
use crate::db::roots::{self, RepositoryRoot, RootKind, RootSettings};
use crate::db::Database;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::git::inspect::{self, GitKind, GitMetadata};
use crate::paths::{self, PathKind, PathPolicy};
use crate::process::{ExecutionPolicy, ProcessRunner};
use crate::shell_path::SearchPath;
use crate::time_util;
use walk::{WalkOptions, WalkStats, WalkWarning};

pub const DEFAULT_MAX_ENTRIES: usize = 200_000;
const INSPECT_CONCURRENCY: usize = 4;
const MAX_RETAINED_JOBS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMode {
    /// The chosen folder must itself be a repository.
    Repository,
    /// Search the chosen folder for repositories.
    Directory,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    pub name: String,
    pub git_kind: GitKind,
    pub depth: u32,
    pub already_imported: bool,
    pub metadata: GitMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryResult {
    pub discovery_id: String,
    pub root_path: String,
    pub mode: DiscoveryMode,
    pub max_depth: u32,
    pub follow_internal_symlinks: bool,
    pub status: DiscoveryStatus,
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<WalkWarning>,
    pub stats: WalkStats,
    pub truncated: bool,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub duration_ms: u64,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DiscoveryEvent {
    Progress {
        discovery_id: String,
        phase: &'static str,
        current_path: String,
        stats: WalkStats,
        repositories_found: usize,
        inspected: usize,
    },
    Completed {
        result: DiscoveryResult,
    },
}

pub trait DiscoverySink: Send + Sync {
    fn emit(&self, event: &DiscoveryEvent);
}

pub const TAURI_EVENT: &str = "shiftops://discovery";

pub struct TauriDiscoverySink<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> DiscoverySink for TauriDiscoverySink<R> {
    fn emit(&self, event: &DiscoveryEvent) {
        use tauri::Emitter;
        if let Err(e) = self.0.emit(TAURI_EVENT, event) {
            crate::logging::warn("discovery", &format!("failed to emit discovery event: {e}"));
        }
    }
}

struct Job {
    cancel: Arc<AtomicBool>,
    result: Option<DiscoveryResult>,
    created: Instant,
}

#[derive(Clone, Default)]
pub struct Discoveries {
    jobs: Arc<Mutex<HashMap<String, Job>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub root: RepositoryRoot,
    pub imported: u32,
    pub updated: u32,
    pub repository_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DiscoveryRequest {
    pub path: String,
    pub mode: DiscoveryMode,
    pub max_depth: u32,
    pub follow_internal_symlinks: bool,
    pub max_entries: usize,
}

/// Validate a user-chosen root: absolute, existing directory, not the
/// filesystem root. Returns the canonical path.
pub fn validate_root(input: &str) -> AppResult<PathBuf> {
    let policy = PathPolicy::new([PathBuf::from(input)]);
    let validated = paths::validate(input, &policy)?;
    if validated.kind != PathKind::Directory {
        return Err(
            AppError::new(ErrorCode::PathNotAllowed, "Choose an existing folder.")
                .with_context("path", input),
        );
    }
    if validated.canonical.parent().is_none() {
        return Err(AppError::new(
            ErrorCode::PathNotAllowed,
            "The filesystem root cannot be used. Choose a narrower folder.",
        ));
    }
    Ok(validated.canonical)
}

impl Discoveries {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Job>> {
        self.jobs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Start a discovery job and return its id. Work continues in the background.
    pub fn start(
        &self,
        request: DiscoveryRequest,
        db: Arc<Database>,
        search: &'static SearchPath,
        sink: Arc<dyn DiscoverySink>,
    ) -> AppResult<(String, tokio::task::JoinHandle<DiscoveryResult>)> {
        if request.max_depth > roots::MAX_DEPTH {
            return Err(AppError::validation(format!(
                "Maximum depth must be between 0 and {}.",
                roots::MAX_DEPTH
            )));
        }
        let root = validate_root(&request.path)?;
        if request.mode == DiscoveryMode::Repository && inspect::detect_marker(&root).is_none() {
            return Err(AppError::validation("This folder is not a Git repository (no `.git`). Use Discover Folder to search inside it.")
                .with_context("path", root.display().to_string()));
        }
        let id = crate::db::new_id();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.lock();
            if jobs.len() >= MAX_RETAINED_JOBS {
                let oldest_done = jobs
                    .iter()
                    .filter(|(_, j)| j.result.is_some())
                    .min_by_key(|(_, j)| j.created)
                    .map(|(k, _)| k.clone());
                if let Some(k) = oldest_done {
                    jobs.remove(&k);
                }
            }
            jobs.insert(
                id.clone(),
                Job {
                    cancel: cancel.clone(),
                    result: None,
                    created: Instant::now(),
                },
            );
        }
        let this = self.clone();
        let job_id = id.clone();
        let handle = tokio::spawn(async move {
            let result = run(&job_id, &root, &request, &cancel, &db, search, sink.clone()).await;
            record_activity(&db, &result);
            if let Some(job) = this.lock().get_mut(&job_id) {
                job.result = Some(result.clone());
            }
            sink.emit(&DiscoveryEvent::Completed {
                result: result.clone(),
            });
            result
        });
        Ok((id, handle))
    }

    /// Request cancellation. Returns false when the job is unknown or finished.
    pub fn cancel(&self, id: &str) -> bool {
        match self.lock().get(id) {
            Some(job) if job.result.is_none() => {
                job.cancel.store(true, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }

    pub fn result(&self, id: &str) -> AppResult<Option<DiscoveryResult>> {
        self.lock()
            .get(id)
            .map(|j| j.result.clone())
            .ok_or_else(|| AppError::not_found("Discovery", id))
    }

    /// Import selected candidates from a completed discovery, in one transaction.
    pub fn import(&self, db: &Database, id: &str, selected: &[String]) -> AppResult<ImportSummary> {
        let result = self.result(id)?.ok_or_else(|| {
            AppError::validation("Discovery is still running. Wait for it to finish.")
        })?;
        if result.status != DiscoveryStatus::Completed {
            return Err(AppError::validation(
                "Only a completed discovery can be imported. Run discovery again.",
            ));
        }
        if selected.is_empty() {
            return Err(AppError::validation(
                "Select at least one repository to import.",
            ));
        }
        let by_path: HashMap<&str, &Candidate> = result
            .candidates
            .iter()
            .map(|c| (c.path.as_str(), c))
            .collect();
        let chosen: Vec<&Candidate> = selected
            .iter()
            .map(|p| {
                by_path.get(p.as_str()).copied().ok_or_else(|| {
                    AppError::validation("A selected repository is not part of this discovery.")
                        .with_context("path", p.as_str())
                })
            })
            .collect::<AppResult<_>>()?;
        let root_path = Path::new(&result.root_path);
        let display_name = root_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| result.root_path.clone());
        let summary = db.write(|c| {
            let root = roots::record_discovery(
                c,
                &RootSettings {
                    canonical_path: result.root_path.clone(),
                    display_name,
                    root_kind: match result.mode {
                        DiscoveryMode::Repository => RootKind::Repository,
                        DiscoveryMode::Directory => RootKind::Directory,
                    },
                    max_depth: result.max_depth,
                    follow_internal_symlinks: result.follow_internal_symlinks,
                },
            )?;
            let (mut imported, mut updated, mut ids) = (0, 0, Vec::new());
            for candidate in &chosen {
                let (repo, created) = repositories::record_local_discovery(
                    c,
                    &root.id,
                    &LocalRepositoryRecord {
                        name: candidate.name.clone(),
                        local_path: candidate.path.clone(),
                        git_kind: candidate.git_kind,
                        metadata: candidate.metadata.clone(),
                        collected_at: result.completed_at.clone().unwrap_or_else(time_util::now),
                    },
                )?;
                if created {
                    imported += 1;
                } else {
                    updated += 1;
                }
                ids.push(repo.id);
            }
            activity::record(
                c,
                NewEvent::new(
                    "repository_root",
                    "repositories.imported",
                    Severity::Notice,
                    format!("Imported {imported} and refreshed {updated} repositories from {}", root.display_name),
                )
                .entity(root.id.clone())
                .metadata(json!({ "rootPath": root.canonical_path, "imported": imported, "updated": updated, "repositoryIds": ids })),
            )?;
            let root = roots::get(c, &root.id)?;
            Ok(ImportSummary { root, imported, updated, repository_ids: ids })
        })?;
        Ok(summary)
    }
}

async fn run(
    id: &str,
    root: &Path,
    request: &DiscoveryRequest,
    cancel: &Arc<AtomicBool>,
    db: &Database,
    search: &SearchPath,
    sink: Arc<dyn DiscoverySink>,
) -> DiscoveryResult {
    let started = Instant::now();
    let started_at = time_util::now();
    let options = WalkOptions {
        max_depth: if request.mode == DiscoveryMode::Repository {
            0
        } else {
            request.max_depth
        },
        follow_internal_symlinks: request.follow_internal_symlinks,
        max_entries: request.max_entries,
    };
    let mut result = DiscoveryResult {
        discovery_id: id.to_string(),
        root_path: root.display().to_string(),
        mode: request.mode,
        max_depth: options.max_depth,
        follow_internal_symlinks: options.follow_internal_symlinks,
        status: DiscoveryStatus::Running,
        candidates: Vec::new(),
        warnings: Vec::new(),
        stats: WalkStats::default(),
        truncated: false,
        started_at,
        completed_at: None,
        duration_ms: 0,
        error: None,
    };

    // 1. Walk (blocking filesystem work off the async threads).
    let walk_root = root.to_path_buf();
    let walk_cancel = cancel.clone();
    let progress_sink = {
        let id = id.to_string();
        let sink = sink.clone();
        move |path: &Path, stats: &WalkStats, found: usize| {
            sink.emit(&DiscoveryEvent::Progress {
                discovery_id: id.clone(),
                phase: "walking",
                current_path: path.display().to_string(),
                stats: *stats,
                repositories_found: found,
                inspected: 0,
            });
        }
    };
    let walked = tokio::task::spawn_blocking(move || {
        walk::walk(&walk_root, options, &walk_cancel, &mut |p, s, n| {
            progress_sink(p, s, n)
        })
    })
    .await;
    let outcome = match walked {
        Ok(o) => o,
        Err(e) => {
            result.status = DiscoveryStatus::Failed;
            result.error = Some(AppError::internal(format!("discovery walk failed: {e}")));
            return finish(result, started);
        }
    };
    result.stats = outcome.stats;
    result.warnings = outcome.warnings;
    result.truncated = outcome.truncated;
    if outcome.cancelled {
        result.status = DiscoveryStatus::Cancelled;
        return finish(result, started);
    }

    // 2. Inspect each repository with read-only Git, a few at a time.
    let runner = ProcessRunner::new(
        ExecutionPolicy::readonly_git(vec![root.to_path_buf()]),
        None,
        None,
    );
    let registered = db
        .read(repositories::registered_local_paths)
        .unwrap_or_default();
    let semaphore = Arc::new(Semaphore::new(INSPECT_CONCURRENCY));
    let total = outcome.repositories.len();
    let mut set = tokio::task::JoinSet::new();
    for (index, found) in outcome.repositories.into_iter().enumerate() {
        let permit_source = semaphore.clone();
        let runner = runner.clone();
        let search: SearchPath = search.clone();
        let cancel = cancel.clone();
        set.spawn(async move {
            let _permit = permit_source.acquire_owned().await.ok()?;
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let metadata = inspect::collect(&runner, &search, &found.path, &found.git_dir).await;
            Some((index, found, metadata))
        });
    }
    let mut inspected = Vec::with_capacity(total);
    while let Some(joined) = set.join_next().await {
        if let Ok(Some(item)) = joined {
            sink.emit(&DiscoveryEvent::Progress {
                discovery_id: id.to_string(),
                phase: "inspecting",
                current_path: item.1.path.display().to_string(),
                stats: result.stats,
                repositories_found: total,
                inspected: inspected.len() + 1,
            });
            inspected.push(item);
        }
    }
    if cancel.load(Ordering::Relaxed) {
        result.status = DiscoveryStatus::Cancelled;
        return finish(result, started);
    }
    inspected.sort_by_key(|(index, _, _)| *index);
    result.candidates = inspected
        .into_iter()
        .map(|(_, found, metadata)| {
            let path = found.path.display().to_string();
            Candidate {
                name: found
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone()),
                already_imported: registered.contains(&path),
                path,
                git_kind: found.git_kind,
                depth: found.depth,
                metadata,
            }
        })
        .collect();
    result.status = DiscoveryStatus::Completed;
    finish(result, started)
}

fn finish(mut result: DiscoveryResult, started: Instant) -> DiscoveryResult {
    result.completed_at = Some(time_util::now());
    result.duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    result
}

fn record_activity(db: &Database, result: &DiscoveryResult) {
    let (event_type, severity, verb) = match result.status {
        DiscoveryStatus::Completed => {
            ("discovery.completed", Severity::Info, "Discovery completed")
        }
        DiscoveryStatus::Cancelled => (
            "discovery.cancelled",
            Severity::Notice,
            "Discovery cancelled",
        ),
        _ => ("discovery.failed", Severity::Error, "Discovery failed"),
    };
    let event = NewEvent::new(
        "discovery",
        event_type,
        severity,
        format!(
            "{verb}: {} repositories in {}",
            result.candidates.len(),
            result.root_path
        ),
    )
    .entity(result.discovery_id.clone())
    .metadata(json!({
        "rootPath": result.root_path,
        "mode": result.mode,
        "maxDepth": result.max_depth,
        "repositories": result.candidates.len(),
        "warnings": result.warnings.len(),
        "directoriesVisited": result.stats.directories_visited,
        "truncated": result.truncated,
        "durationMs": result.duration_ms,
    }));
    if let Err(e) = db.write(|c| activity::record(c, event)) {
        crate::logging::error("discovery", &format!("failed to record activity: {e}"));
    }
}

#[cfg(all(test, unix))]
mod tests {
    //! End-to-end discovery against real temporary Git repositories. Git is
    //! configured per command (`-c user.name=…`), never globally.

    use super::*;
    use std::process::Command;

    #[derive(Default)]
    struct Collect(Mutex<Vec<DiscoveryEvent>>);
    impl DiscoverySink for Collect {
        fn emit(&self, event: &DiscoveryEvent) {
            self.0.lock().unwrap().push(event.clone());
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args([
                "-c",
                "user.name=ShiftOps Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    fn make_repo(path: &Path, remote: Option<&str>) {
        std::fs::create_dir_all(path).unwrap();
        git(path, &["init", "-q"]);
        std::fs::write(path.join("README.md"), "hello\n").unwrap();
        git(path, &["add", "README.md"]);
        git(path, &["commit", "-q", "-m", "init"]);
        if let Some(url) = remote {
            git(path, &["remote", "add", "origin", url]);
        }
    }

    fn search() -> &'static SearchPath {
        Box::leak(Box::new(SearchPath {
            dirs: std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect(),
            source: "test".into(),
        }))
    }

    async fn discover(
        dir: &Path,
        mode: DiscoveryMode,
        depth: u32,
    ) -> (
        Discoveries,
        Arc<Database>,
        DiscoveryResult,
        Vec<DiscoveryEvent>,
    ) {
        let jobs = Discoveries::default();
        let db = Arc::new(Database::open_in_memory());
        let sink = Arc::new(Collect::default());
        let (_, handle) = jobs
            .start(
                DiscoveryRequest {
                    path: dir.display().to_string(),
                    mode,
                    max_depth: depth,
                    follow_internal_symlinks: false,
                    max_entries: DEFAULT_MAX_ENTRIES,
                },
                db.clone(),
                search(),
                sink.clone(),
            )
            .unwrap();
        let result = handle.await.unwrap();
        let events = sink.0.lock().unwrap().clone();
        (jobs, db, result, events)
    }

    #[tokio::test]
    async fn discovers_inspects_and_imports_real_repositories() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        make_repo(
            &root.join("api"),
            Some("https://x-access-token:ghs_secret123@github.com/acme/api.git"),
        );
        make_repo(&root.join("group/web"), Some("git@gitlab.com:team/web.git"));
        make_repo(&root.join("tools"), None);
        std::fs::write(root.join("api/new-file.txt"), "x").unwrap();
        std::fs::write(root.join("api/README.md"), "changed\n").unwrap();
        git(
            &root.join("api"),
            &["worktree", "add", "-q", "../api-feature", "-b", "feature"],
        );
        make_repo(&root.join("node_modules/dep"), None);

        let (jobs, db, result, events) = discover(root, DiscoveryMode::Directory, 3).await;
        assert_eq!(result.status, DiscoveryStatus::Completed);
        let mut names: Vec<_> = result.candidates.iter().map(|c| c.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["api", "api-feature", "tools", "web"]);

        let api = result.candidates.iter().find(|c| c.name == "api").unwrap();
        let m = &api.metadata;
        assert_eq!(m.current_branch.as_deref(), Some("main"));
        assert_eq!(m.head_commit.as_ref().map(String::len), Some(40));
        assert!(m.last_commit_at.is_some());
        let origin = m.origin.as_ref().unwrap();
        assert!(origin.is_github());
        assert!(
            !origin.sanitized_url.contains("ghs_secret123"),
            "{origin:?}"
        );
        let wt = m.working_tree.as_ref().unwrap();
        assert_eq!((wt.modified, wt.untracked), (1, 1));

        let feature = result
            .candidates
            .iter()
            .find(|c| c.name == "api-feature")
            .unwrap();
        assert_eq!(feature.git_kind, GitKind::Worktree);
        assert_eq!(feature.metadata.current_branch.as_deref(), Some("feature"));

        let web = result.candidates.iter().find(|c| c.name == "web").unwrap();
        assert!(!web.metadata.origin.as_ref().unwrap().is_github());
        assert_eq!(
            result
                .candidates
                .iter()
                .find(|c| c.name == "tools")
                .unwrap()
                .metadata
                .origin,
            None
        );

        assert!(events.iter().any(|e| matches!(
            e,
            DiscoveryEvent::Progress {
                phase: "inspecting",
                ..
            }
        )));
        assert!(matches!(
            events.last(),
            Some(DiscoveryEvent::Completed { .. })
        ));

        // Import a subset; unknown paths are refused.
        let bogus = jobs.import(&db, &result.discovery_id, &["/not/in/result".into()]);
        assert_eq!(bogus.unwrap_err().code, ErrorCode::ValidationFailed);
        let paths: Vec<String> = result
            .candidates
            .iter()
            .filter(|c| c.name != "tools")
            .map(|c| c.path.clone())
            .collect();
        let summary = jobs.import(&db, &result.discovery_id, &paths).unwrap();
        assert_eq!((summary.imported, summary.updated), (3, 0));
        assert_eq!(summary.root.repository_count, 3);
        let repos = db
            .read(|c| repositories::list(c, &Default::default()))
            .unwrap();
        assert!(repos
            .items
            .iter()
            .all(|r| r.trust_status == repositories::TrustStatus::Untrusted));
        let linked: Vec<_> = repos
            .items
            .iter()
            .filter(|r| r.github_owner.is_some())
            .collect();
        assert_eq!(linked.len(), 1, "one checkout holds the acme/api identity");
        let worktree = repos
            .items
            .iter()
            .find(|r| r.name == "api-feature")
            .unwrap();
        assert_eq!(
            worktree.remote_url.as_deref(),
            Some("https://github.com/acme/api.git")
        );
        assert!(worktree.git_warnings.to_string().contains("already linked"));

        // Re-discovery marks duplicates and import refreshes instead of duplicating.
        let jobs2 = jobs.clone();
        let sink = Arc::new(Collect::default());
        let (_, handle) = jobs2
            .start(
                DiscoveryRequest {
                    path: root.display().to_string(),
                    mode: DiscoveryMode::Directory,
                    max_depth: 3,
                    follow_internal_symlinks: false,
                    max_entries: DEFAULT_MAX_ENTRIES,
                },
                db.clone(),
                search(),
                sink,
            )
            .unwrap();
        let second = handle.await.unwrap();
        assert_eq!(
            second
                .candidates
                .iter()
                .filter(|c| c.already_imported)
                .count(),
            3
        );
        let again = jobs2.import(&db, &second.discovery_id, &paths).unwrap();
        assert_eq!((again.imported, again.updated), (0, 3));
        assert_eq!(
            db.read(|c| repositories::list(c, &Default::default()))
                .unwrap()
                .total,
            3
        );

        // Removing the root forgets it but leaves repositories and files.
        db.write(|c| roots::remove(c, &summary.root.id)).unwrap();
        assert_eq!(
            db.read(|c| repositories::list(c, &Default::default()))
                .unwrap()
                .total,
            3
        );
        assert!(root.join("api/README.md").exists());
    }

    #[tokio::test]
    async fn repository_mode_requires_a_repository_and_inspects_only_it() {
        let t = tempfile::tempdir().unwrap();
        make_repo(&t.path().join("solo"), None);
        make_repo(&t.path().join("solo/inner"), None);
        let (_, _, result, _) =
            discover(&t.path().join("solo"), DiscoveryMode::Repository, 4).await;
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.max_depth, 0);

        let jobs = Discoveries::default();
        let err = jobs
            .start(
                DiscoveryRequest {
                    path: t.path().display().to_string(),
                    mode: DiscoveryMode::Repository,
                    max_depth: 0,
                    follow_internal_symlinks: false,
                    max_entries: 10,
                },
                Arc::new(Database::open_in_memory()),
                search(),
                Arc::new(Collect::default()),
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationFailed);
    }

    #[tokio::test]
    async fn risky_repository_config_skips_status_with_warning() {
        let t = tempfile::tempdir().unwrap();
        make_repo(&t.path().join("lfs"), None);
        let marker = t.path().join("filter-ran");
        let cfg = t.path().join("lfs/.git/config");
        let mut text = std::fs::read_to_string(&cfg).unwrap();
        text.push_str(&format!(
            "[filter \"evil\"]\n\tclean = touch {}\n",
            marker.display()
        ));
        std::fs::write(&cfg, text).unwrap();
        std::fs::write(t.path().join("lfs/.gitattributes"), "* filter=evil\n").unwrap();
        let (_, _, result, _) = discover(&t.path().join("lfs"), DiscoveryMode::Repository, 0).await;
        let m = &result.candidates[0].metadata;
        assert!(m.working_tree.is_none());
        assert!(m.warnings.iter().any(|w| w.contains("not collected")));
        assert!(!marker.exists(), "repository-defined filter must never run");
    }

    #[tokio::test]
    async fn cancellation_and_root_validation() {
        let t = tempfile::tempdir().unwrap();
        for i in 0..3 {
            make_repo(&t.path().join(format!("r{i}")), None);
        }
        let jobs = Discoveries::default();
        let db = Arc::new(Database::open_in_memory());
        let (id, handle) = jobs
            .start(
                DiscoveryRequest {
                    path: t.path().display().to_string(),
                    mode: DiscoveryMode::Directory,
                    max_depth: 2,
                    follow_internal_symlinks: false,
                    max_entries: DEFAULT_MAX_ENTRIES,
                },
                db.clone(),
                search(),
                Arc::new(Collect::default()),
            )
            .unwrap();
        assert!(jobs.cancel(&id));
        let result = handle.await.unwrap();
        assert_eq!(result.status, DiscoveryStatus::Cancelled);
        assert!(!jobs.cancel(&id), "repeated cancellation is harmless");
        assert_eq!(
            jobs.import(&db, &id, &["x".into()]).unwrap_err().code,
            ErrorCode::ValidationFailed
        );
        let events = db.read(|c| activity::list(c, &Default::default())).unwrap();
        assert_eq!(events.items[0].event_type, "discovery.cancelled");

        assert_eq!(
            validate_root("relative").unwrap_err().code,
            ErrorCode::ValidationFailed
        );
        assert_eq!(
            validate_root("/").unwrap_err().code,
            ErrorCode::PathNotAllowed
        );
        assert!(validate_root(&t.path().join("missing").display().to_string()).is_err());
        assert!(jobs
            .start(
                DiscoveryRequest {
                    path: t.path().display().to_string(),
                    mode: DiscoveryMode::Directory,
                    max_depth: 13,
                    follow_internal_symlinks: false,
                    max_entries: 1
                },
                db,
                search(),
                Arc::new(Collect::default()),
            )
            .is_err());
    }
}
