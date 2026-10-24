//! Repository intelligence scanner. Read-only: it reads allow-listed files
//! and runs one read-only Git command (`rev-parse HEAD`). It never executes
//! repository code, installs dependencies or reads secret files.

pub mod detect;
pub mod facts;
pub mod files;
pub mod semver;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::db::activity::{self, NewEvent, Severity};
use crate::db::intelligence;
use crate::db::repositories::{self, RepositoryScan};
use crate::db::Database;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::git::policy::{command_args, GIT_ENV, HEAD_COMMAND};
use crate::process::{
    CommandCategory, CommandSpec, ExecutionPolicy, ProcessRunner, Program, RunStatus,
};
use crate::shell_path::SearchPath;
use facts::Category;

/// Bump when detection changes so cached results are not reused.
pub const SCANNER_VERSION: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanOutcomeStatus {
    Completed,
    Cached,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    pub repository_id: String,
    pub status: ScanOutcomeStatus,
    pub scan: Option<RepositoryScan>,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ScanEvent {
    Progress {
        repository_id: String,
        entries_seen: u64,
        current_path: String,
    },
    Completed {
        outcome: Box<ScanOutcome>,
    },
}

pub trait ScanSink: Send + Sync {
    fn emit(&self, event: &ScanEvent);
}

pub const TAURI_EVENT: &str = "shiftops://scan";

pub struct TauriScanSink<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> ScanSink for TauriScanSink<R> {
    fn emit(&self, event: &ScanEvent) {
        use tauri::Emitter;
        if let Err(e) = self.0.emit(TAURI_EVENT, event) {
            crate::logging::warn("scanner", &format!("failed to emit scan event: {e}"));
        }
    }
}

/// One running scan per repository.
#[derive(Clone, Default)]
pub struct Scans {
    running: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl Scans {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<AtomicBool>>> {
        self.running.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn is_running(&self, repository_id: &str) -> bool {
        self.lock().contains_key(repository_id)
    }

    pub fn cancel(&self, repository_id: &str) -> bool {
        match self.lock().get(repository_id) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    pub fn start(
        &self,
        repository_id: &str,
        force: bool,
        db: Arc<Database>,
        search: &'static SearchPath,
        sink: Arc<dyn ScanSink>,
    ) -> AppResult<tokio::task::JoinHandle<ScanOutcome>> {
        let repo = db.read(|c| repositories::get(c, repository_id))?;
        let path = repo.local_path.ok_or_else(|| {
            AppError::validation("This repository has no local checkout to scan.")
        })?;
        let root = crate::discovery::validate_root(&path).map_err(|e| {
            e.with_action(
                "The folder may have been moved or deleted. Run discovery again to update it.",
            )
        })?;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut running = self.lock();
            if running.contains_key(repository_id) {
                return Err(AppError::validation(
                    "A scan of this repository is already running.",
                ));
            }
            running.insert(repository_id.to_string(), cancel.clone());
        }
        let this = self.clone();
        let id = repository_id.to_string();
        Ok(tokio::spawn(async move {
            let outcome = run(&id, &root, force, &cancel, &db, search, sink.clone()).await;
            this.lock().remove(&id);
            record_activity(&db, &outcome);
            sink.emit(&ScanEvent::Completed {
                outcome: Box::new(outcome.clone()),
            });
            outcome
        }))
    }
}

async fn head_commit(root: &Path, search: &SearchPath) -> Option<String> {
    let runner = ProcessRunner::new(
        ExecutionPolicy::readonly_git(vec![root.to_path_buf()]),
        None,
        None,
    );
    let mut spec = CommandSpec::new(
        "git rev-parse",
        CommandCategory::ReadonlyGit,
        Program::Named("git".into()),
        &[],
    );
    spec.args = command_args(HEAD_COMMAND);
    spec.cwd = Some(root.to_path_buf());
    spec.env = GIT_ENV
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    spec.record_activity = false;
    spec.max_output_bytes = 4096;
    spec.timeout = std::time::Duration::from_secs(15);
    let result = runner.run_captured(spec, search).await.ok()?;
    (result.status == RunStatus::Succeeded)
        .then(|| result.stdout.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn cache_key(commit: Option<&str>, fingerprint: &str) -> String {
    let mut h = Sha256::new();
    h.update(SCANNER_VERSION.as_bytes());
    h.update([0]);
    h.update(commit.unwrap_or("no-commit").as_bytes());
    h.update([0]);
    h.update(fingerprint.as_bytes());
    files::hex(&h.finalize())
}

/// Headline facts for list views, stored in the scan's `result_json`.
pub fn summarize(found: &detect::Findings, collected: &files::Collected, cache_key: &str) -> Value {
    let languages: Vec<(&str, u64)> = {
        let mut l: Vec<_> = collected.languages.iter().map(|(k, v)| (*k, *v)).collect();
        l.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        l
    };
    let mut runtimes: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut best: BTreeMap<&str, f64> = BTreeMap::new();
    for f in found
        .facts
        .iter()
        .filter(|f| f.category == Category::Runtime)
    {
        let conf = best.entry(f.key.as_str()).or_insert(0.0);
        let list = runtimes.entry(f.key.as_str()).or_default();
        if f.confidence > *conf {
            *conf = f.confidence;
            list.clear();
        }
        if (f.confidence - *conf).abs() < f64::EPSILON && !list.contains(&f.value.as_str()) {
            list.push(f.value.as_str());
        }
    }
    let set = |cat: Category| -> Vec<&str> {
        found
            .facts
            .iter()
            .filter(|f| f.category == cat)
            .map(|f| f.key.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    json!({
        "scannerVersion": SCANNER_VERSION,
        "cacheKey": cache_key,
        "primaryLanguage": languages.first().map(|l| l.0),
        "languages": languages.iter().take(8).map(|(k, v)| json!({ "name": k, "files": v })).collect::<Vec<_>>(),
        "runtimes": runtimes,
        "packageManagers": set(Category::PackageManager),
        "frameworks": set(Category::Framework),
        "hasDocker": found.facts.iter().any(|f| f.category == Category::Docker),
        "ci": set(Category::Ci),
        "facts": found.facts.len(),
        "conflicts": found.conflicts.len(),
        "relevantFiles": collected.files.len(),
        "entriesSeen": collected.entries_seen,
        "truncated": collected.truncated,
        "warnings": found.warnings.iter().chain(collected.warnings.iter()).collect::<Vec<_>>(),
    })
}

async fn run(
    repository_id: &str,
    root: &Path,
    force: bool,
    cancel: &Arc<AtomicBool>,
    db: &Database,
    search: &SearchPath,
    sink: Arc<dyn ScanSink>,
) -> ScanOutcome {
    let started = Instant::now();
    let outcome = |status, scan, error| ScanOutcome {
        repository_id: repository_id.to_string(),
        status,
        scan,
        error,
    };

    let walk_root: PathBuf = root.to_path_buf();
    let walk_cancel = cancel.clone();
    let id = repository_id.to_string();
    let progress_sink = sink.clone();
    let collected = tokio::task::spawn_blocking(move || {
        files::collect(&walk_root, &walk_cancel, &mut |entries, path| {
            progress_sink.emit(&ScanEvent::Progress {
                repository_id: id.clone(),
                entries_seen: entries,
                current_path: path.to_string(),
            });
        })
    })
    .await;
    let collected = match collected {
        Ok(c) => c,
        Err(e) => {
            return outcome(
                ScanOutcomeStatus::Failed,
                None,
                Some(AppError::internal(format!("scan task failed: {e}"))),
            )
        }
    };
    if collected.cancelled {
        let scan = db.write(|c| {
            let s = repositories::create_scan(c, repository_id)?;
            repositories::cancel_scan(c, &s.id)
        });
        return outcome(
            ScanOutcomeStatus::Cancelled,
            scan.ok(),
            Some(AppError::new(
                ErrorCode::CommandCancelled,
                "Scan cancelled.",
            )),
        );
    }

    let commit = head_commit(root, search).await;
    let key = cache_key(commit.as_deref(), &collected.fingerprint);
    if !force {
        if let Ok(Some(cached)) = db.read(|c| intelligence::find_cached(c, repository_id, &key)) {
            return outcome(ScanOutcomeStatus::Cached, Some(cached), None);
        }
    }

    let found = detect::detect(&collected.files);
    if cancel.load(Ordering::Relaxed) {
        let scan = db.write(|c| {
            let s = repositories::create_scan(c, repository_id)?;
            repositories::cancel_scan(c, &s.id)
        });
        return outcome(
            ScanOutcomeStatus::Cancelled,
            scan.ok(),
            Some(AppError::new(
                ErrorCode::CommandCancelled,
                "Scan cancelled.",
            )),
        );
    }
    let mut summary = summarize(&found, &collected, &key);
    summary["durationMs"] = json!(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
    let stored = db.write(|c| {
        let scan = repositories::create_scan(c, repository_id)?;
        repositories::start_scan(c, &scan.id)?;
        intelligence::store(
            c,
            repository_id,
            &scan.id,
            SCANNER_VERSION,
            &key,
            &found.facts,
            &found.conflicts,
        )?;
        repositories::complete_scan(
            c,
            &scan.id,
            commit.as_deref(),
            Some(&collected.fingerprint),
            &summary,
        )
    });
    match stored {
        Ok(scan) => outcome(ScanOutcomeStatus::Completed, Some(scan), None),
        Err(e) => {
            let failed = db.write(|c| {
                let s = repositories::create_scan(c, repository_id)?;
                repositories::fail_scan(c, &s.id, e.code.as_str(), &e.message)
            });
            outcome(ScanOutcomeStatus::Failed, failed.ok(), Some(e))
        }
    }
}

fn record_activity(db: &Database, o: &ScanOutcome) {
    let (event_type, severity, summary) = match o.status {
        ScanOutcomeStatus::Completed => {
            ("repository.scanned", Severity::Info, "Repository scanned")
        }
        ScanOutcomeStatus::Cached => (
            "repository.scan_reused",
            Severity::Info,
            "Scan reused: nothing relevant changed",
        ),
        ScanOutcomeStatus::Cancelled => (
            "repository.scan_cancelled",
            Severity::Notice,
            "Repository scan cancelled",
        ),
        ScanOutcomeStatus::Failed => (
            "repository.scan_failed",
            Severity::Error,
            "Repository scan failed",
        ),
    };
    let result = o
        .scan
        .as_ref()
        .and_then(|s| s.result.clone())
        .unwrap_or(Value::Null);
    let event = NewEvent::new("repository", event_type, severity, summary)
        .entity(o.repository_id.clone())
        .metadata(json!({
            "scanId": o.scan.as_ref().map(|s| s.id.clone()),
            "facts": result.get("facts"),
            "conflicts": result.get("conflicts"),
            "errorCode": o.error.as_ref().map(|e| e.code.as_str()),
        }));
    if let Err(e) = db.write(|c| activity::record(c, event)) {
        crate::logging::error("scanner", &format!("failed to record activity: {e}"));
    }
}

#[cfg(all(test, unix))]
mod tests {
    //! Scans the deterministic fixtures in `fixtures/repositories`, copied to
    //! temporary directories so tests never modify the repository.

    use super::*;
    use crate::db::intelligence::Intelligence;
    use crate::db::repositories::{NewRepository, SourceType};

    #[derive(Default)]
    struct Collect(Mutex<Vec<ScanEvent>>);
    impl ScanSink for Collect {
        fn emit(&self, event: &ScanEvent) {
            self.0.lock().unwrap().push(event.clone());
        }
    }

    fn fixture_dir(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/repositories")
            .join(name)
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    fn search() -> &'static SearchPath {
        Box::leak(Box::new(SearchPath {
            dirs: std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect(),
            source: "test".into(),
        }))
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        path: PathBuf,
        db: Arc<Database>,
        scans: Scans,
        repo_id: String,
    }

    fn fixture(name: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(dir.path()).unwrap().join(name);
        copy_dir(&fixture_dir(name), &path);
        let db = Arc::new(Database::open_in_memory());
        let repo = db
            .write(|c| {
                repositories::create(
                    c,
                    &NewRepository {
                        source_type: SourceType::Local,
                        name: name.into(),
                        local_path: Some(path.display().to_string()),
                        remote_url: None,
                        github: None,
                        default_branch: None,
                    },
                )
            })
            .unwrap();
        Fixture {
            _dir: dir,
            path,
            db,
            scans: Scans::default(),
            repo_id: repo.id,
        }
    }

    async fn scan(f: &Fixture, force: bool) -> (ScanOutcome, Intelligence) {
        let handle = f
            .scans
            .start(
                &f.repo_id,
                force,
                f.db.clone(),
                search(),
                Arc::new(Collect::default()),
            )
            .unwrap();
        let outcome = handle.await.unwrap();
        let intel = f.db.read(|c| intelligence::latest(c, &f.repo_id)).unwrap();
        (outcome, intel)
    }

    fn values(i: &Intelligence, cat: Category, key: &str) -> Vec<String> {
        i.facts
            .iter()
            .filter(|f| f.fact.category == cat && f.fact.key == key)
            .map(|f| f.fact.value.clone())
            .collect()
    }

    fn keys(i: &Intelligence, cat: Category) -> BTreeSet<String> {
        i.facts
            .iter()
            .filter(|f| f.fact.category == cat)
            .map(|f| f.fact.key.clone())
            .collect()
    }

    #[tokio::test]
    async fn node18_express() {
        let f = fixture("node18-express");
        let (o, i) = scan(&f, false).await;
        assert_eq!(o.status, ScanOutcomeStatus::Completed);
        assert_eq!(values(&i, Category::Framework, "Express"), ["^4.19.2"]);
        assert_eq!(values(&i, Category::Framework, "Jest"), ["^29.7.0"]);
        let mut node = values(&i, Category::Runtime, "node");
        node.sort();
        assert_eq!(node, ["18-alpine", "18.20.4", ">=18 <19"]);
        assert!(keys(&i, Category::PackageManager).contains("npm"));
        assert!(i.conflicts.is_empty(), "{:?}", i.conflicts);
        let summary = i.scan.unwrap().result.unwrap();
        assert_eq!(summary["primaryLanguage"], "JavaScript");
        assert_eq!(summary["hasDocker"], true);
    }

    #[tokio::test]
    async fn node20_next_pnpm() {
        let f = fixture("node20-next-pnpm");
        let (_, i) = scan(&f, false).await;
        assert_eq!(values(&i, Category::Framework, "Next.js"), ["14.2.15"]);
        assert!(keys(&i, Category::Framework)
            .is_superset(&["React", "TypeScript", "Vitest"].map(String::from).into()));
        let pnpm = values(&i, Category::PackageManager, "pnpm");
        assert!(pnpm.contains(&"9.12.0".to_string()));
        assert!(i.conflicts.is_empty(), "{:?}", i.conflicts);
        let engines = i
            .facts
            .iter()
            .find(|x| x.fact.method == "package.json engines")
            .unwrap();
        assert_eq!(
            (
                engines.fact.file.as_deref(),
                engines.fact.line,
                engines.fact.pointer.as_deref()
            ),
            (Some("package.json"), Some(6), Some("/engines/node"))
        );
    }

    #[tokio::test]
    async fn shared_typescript() {
        let f = fixture("shared-typescript");
        let (_, i) = scan(&f, false).await;
        assert_eq!(
            values(&i, Category::Script, "build"),
            ["tsc -p tsconfig.json"]
        );
        assert!(keys(&i, Category::Manifest).contains("tsconfig"));
        assert_eq!(
            i.scan.unwrap().result.unwrap()["primaryLanguage"],
            "TypeScript"
        );
    }

    #[tokio::test]
    async fn runtime_conflicts_are_reported() {
        let f = fixture("runtime-conflict");
        let (_, i) = scan(&f, false).await;
        let messages: Vec<&str> = i.conflicts.iter().map(|c| c.message.as_str()).collect();
        assert!(
            messages
                .iter()
                .any(|m| m.starts_with("node versions disagree")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("excludes Node 18")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("excludes Node 16.20.2")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("npm, yarn")),
            "{messages:?}"
        );
        let ci = i
            .facts
            .iter()
            .find(|x| x.fact.method == "GitHub Actions actions/setup-node")
            .unwrap();
        assert_eq!((ci.fact.value.as_str(), ci.fact.line), ("22", Some(10)));
    }

    #[tokio::test]
    async fn python_java_dotnet_go_rust_fixtures() {
        let (_, py) = scan(&fixture("fastapi-service"), false).await;
        assert_eq!(values(&py, Category::Framework, "FastAPI"), [">=0.115.0"]);
        assert!(values(&py, Category::Runtime, "python").contains(&"3.12.7".to_string()));
        assert!(
            py.conflicts.is_empty(),
            "3.12 in .python-version and Docker agree: {:?}",
            py.conflicts
        );

        let (_, java) = scan(&fixture("spring-boot-service"), false).await;
        assert_eq!(values(&java, Category::Framework, "Spring Boot"), ["3.3.4"]);
        assert_eq!(values(&java, Category::Runtime, "java"), ["21"]);

        let (_, net) = scan(&fixture("dotnet-api"), false).await;
        let mut dotnet = values(&net, Category::Runtime, "dotnet");
        dotnet.sort();
        assert_eq!(dotnet, ["8.0.403", "net8.0"]);
        assert!(keys(&net, Category::Framework).contains("ASP.NET Core"));

        let (_, go) = scan(&fixture("go-service"), false).await;
        assert!(keys(&go, Category::Framework).contains("Gin"));
        assert!(values(&go, Category::Runtime, "go").contains(&"1.22".to_string()));

        let (_, rust) = scan(&fixture("rust-workspace"), false).await;
        assert_eq!(
            values(&rust, Category::Workspace, "cargo_workspace"),
            ["crates/core, crates/cli"]
        );
        assert!(keys(&rust, Category::Framework).contains("Tokio"));
        assert!(values(&rust, Category::Runtime, "rust").contains(&"1.82.0".to_string()));
    }

    #[tokio::test]
    async fn js_monorepo_workspaces_and_nested_frameworks() {
        let f = fixture("js-monorepo");
        let (_, i) = scan(&f, false).await;
        assert_eq!(
            values(&i, Category::Workspace, "npm_workspaces"),
            ["packages/*"]
        );
        assert!(keys(&i, Category::Workspace).contains("turbo"));
        assert!(keys(&i, Category::Framework)
            .is_superset(&["NestJS", "React", "Vite"].map(String::from).into()));
        let nest = i.facts.iter().find(|x| x.fact.key == "NestJS").unwrap();
        assert_eq!(nest.fact.file.as_deref(), Some("packages/api/package.json"));
        assert!(i.conflicts.is_empty(), "{:?}", i.conflicts);
    }

    #[tokio::test]
    async fn malformed_package_json_completes_with_warning() {
        let f = fixture("malformed-package-json");
        let (o, i) = scan(&f, false).await;
        assert_eq!(o.status, ScanOutcomeStatus::Completed);
        let warnings = i.scan.unwrap().result.unwrap()["warnings"].to_string();
        assert!(warnings.contains("Malformed package.json"), "{warnings}");
    }

    #[tokio::test]
    async fn worktree_git_file_and_symlink_escape_are_handled() {
        let f = fixture("node18-express");
        std::fs::write(f.path.join(".git"), "gitdir: /elsewhere/.git/worktrees/x\n").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(
            outside.path().join("pyproject.toml"),
            "[project]\nrequires-python = \">=3.13\"\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("pyproject.toml"),
            f.path.join("pyproject.toml"),
        )
        .unwrap();
        std::fs::write(
            f.path.join(".env"),
            "SECRET=ghp_neverReadThisValue0123456789",
        )
        .unwrap();
        let (o, i) = scan(&f, false).await;
        assert_eq!(o.status, ScanOutcomeStatus::Completed);
        assert!(
            values(&i, Category::Runtime, "python").is_empty(),
            "symlinked file outside the repo must not be read"
        );
        let all =
            serde_json::to_string(&i.facts).unwrap() + &i.scan.unwrap().result.unwrap().to_string();
        assert!(all.contains("Symbolic link not followed"));
        assert!(!all.contains("ghp_neverReadThisValue"));
    }

    #[tokio::test]
    async fn cache_reuses_unchanged_scans_and_invalidates_on_uncommitted_edits() {
        let f = fixture("shared-typescript");
        let (first, _) = scan(&f, false).await;
        assert_eq!(first.status, ScanOutcomeStatus::Completed);
        let (second, _) = scan(&f, false).await;
        assert_eq!(second.status, ScanOutcomeStatus::Cached);
        assert_eq!(
            second.scan.as_ref().unwrap().id,
            first.scan.as_ref().unwrap().id
        );

        // Uncommitted edit to a relevant file invalidates the cache.
        let pj = f.path.join("package.json");
        let text = std::fs::read_to_string(&pj)
            .unwrap()
            .replace("vitest run", "vitest run --coverage");
        std::fs::write(&pj, text).unwrap();
        let (third, i) = scan(&f, false).await;
        assert_eq!(third.status, ScanOutcomeStatus::Completed);
        assert_eq!(
            values(&i, Category::Script, "test"),
            ["vitest run --coverage"]
        );

        // Irrelevant files do not; force always rescans.
        std::fs::write(f.path.join("NOTES.md"), "x").unwrap();
        assert_eq!(scan(&f, false).await.0.status, ScanOutcomeStatus::Cached);
        assert_eq!(scan(&f, true).await.0.status, ScanOutcomeStatus::Completed);

        let events =
            f.db.read(|c| activity::list(c, &Default::default()))
                .unwrap();
        let types: Vec<_> = events.items.iter().map(|e| e.event_type.as_str()).collect();
        assert!(types.contains(&"repository.scan_reused") && types.contains(&"repository.scanned"));
        // Older scans beyond the retained window are pruned with their facts.
        let scans =
            f.db.read(|c| repositories::list_scans(c, &f.repo_id, &Default::default()))
                .unwrap();
        assert!(scans.total <= 3);
    }

    #[tokio::test]
    async fn one_scan_per_repository_and_cancellation() {
        let f = fixture("js-monorepo");
        let cancel = Arc::new(AtomicBool::new(true));
        f.scans
            .running
            .lock()
            .unwrap()
            .insert(f.repo_id.clone(), cancel.clone());
        let err = f
            .scans
            .start(
                &f.repo_id,
                false,
                f.db.clone(),
                search(),
                Arc::new(Collect::default()),
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationFailed);
        f.scans.running.lock().unwrap().clear();

        let outcome = run(
            &f.repo_id,
            &f.path,
            false,
            &cancel,
            &f.db,
            search(),
            Arc::new(Collect::default()),
        )
        .await;
        assert_eq!(outcome.status, ScanOutcomeStatus::Cancelled);
        assert!(!f.scans.cancel("unknown"));
    }

    #[tokio::test]
    async fn missing_checkout_is_a_structured_error() {
        let f = fixture("go-service");
        std::fs::remove_dir_all(&f.path).unwrap();
        let err = f
            .scans
            .start(
                &f.repo_id,
                false,
                f.db.clone(),
                search(),
                Arc::new(Collect::default()),
            )
            .unwrap_err();
        assert!(err.suggested_action.unwrap().contains("moved or deleted"));
    }
}

#[cfg(test)]
mod rehearsal {
    /// Scan a real checkout read-only and print the summary and conflicts:
    /// `SHIFTOPS_SCAN_PATH=/path/to/repo cargo test scan_rehearsal -- --ignored --nocapture`
    #[test]
    #[ignore = "manual: needs SHIFTOPS_SCAN_PATH"]
    fn scan_rehearsal() {
        use std::sync::atomic::AtomicBool;
        let path = std::path::PathBuf::from(
            std::env::var("SHIFTOPS_SCAN_PATH").expect("SHIFTOPS_SCAN_PATH"),
        );
        let collected = super::files::collect(&path, &AtomicBool::new(false), &mut |_, _| {});
        let found = super::detect::detect(&collected.files);
        let summary = super::summarize(&found, &collected, "rehearsal");
        println!("{}", serde_json::to_string_pretty(&summary).unwrap());
        for c in &found.conflicts {
            println!("CONFLICT {}", c.message);
        }
    }
}
