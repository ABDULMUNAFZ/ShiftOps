//! Managed clone jobs: one validated `git clone` into the approved
//! workspace, streamed progress, cancellation, and linking plus a scan on
//! success. A failed or cancelled clone keeps its folder; only the user's
//! confirmed cleanup (`workspace::cleanup`) removes it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::json;

use super::{clone_url, create_destination, preview, resolve_workspace};
use crate::db::activity::{self, NewEvent, Severity};
use crate::db::github::{self as gh_store, GithubRepository};
use crate::db::repositories::{LinkMethod, Ownership, Repository};
use crate::db::workspace::{self as store, Checkout, CloneStatus, NewClone, RepositoryClone};
use crate::db::Database;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::git::inspect::{self, GitKind};
use crate::git::policy::{clone_args, CLONE_ENV, GIT_ENV};
use crate::github::client::GithubConfig;
use crate::process::{
    CancelOutcome, CommandCategory, CommandSpec, EventSink, ExecutionPolicy, ProcessEvent,
    ProcessRunner, Program, RunResult, RunStatus,
};
use crate::scanner::{ScanSink, Scans};
use crate::shell_path::SearchPath;

pub const TAURI_EVENT: &str = "shiftops://clone";
const CLONE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CLONE_OUTPUT_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneResult {
    pub clone: RepositoryClone,
    /// The linked repository after a successful clone.
    pub repository: Option<Repository>,
    pub error: Option<AppError>,
    pub scan_started: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CloneEvent {
    Progress {
        clone_id: String,
        phase: Option<String>,
        percent: Option<u8>,
        line: String,
    },
    Completed {
        result: Box<CloneResult>,
    },
}

pub trait CloneSink: Send + Sync {
    fn emit(&self, event: &CloneEvent);
}

pub struct TauriCloneSink<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> CloneSink for TauriCloneSink<R> {
    fn emit(&self, event: &CloneEvent) {
        use tauri::Emitter;
        if let Err(e) = self.0.emit(TAURI_EVENT, event) {
            crate::logging::warn("clone", &format!("failed to emit clone event: {e}"));
        }
    }
}

/// `Receiving objects:  45% (9/20)` becomes (`Receiving objects`, 45).
pub fn parse_progress(line: &str) -> (Option<String>, Option<u8>) {
    let text = line.trim().strip_prefix("remote: ").unwrap_or(line.trim());
    let Some((phase, rest)) = text.split_once(':') else {
        return (None, None);
    };
    let percent = rest
        .trim_start()
        .split_once('%')
        .and_then(|(n, _)| n.trim().parse::<u8>().ok())
        .filter(|n| *n <= 100);
    let phase_ok = !phase.is_empty() && phase.chars().all(|c| c.is_ascii_alphabetic() || c == ' ');
    match percent {
        Some(p) if phase_ok => (Some(phase.trim().to_string()), Some(p)),
        _ => (None, None),
    }
}

/// Turns the runner's redacted output batches into clone progress events.
struct ProgressSink {
    clone_id: String,
    sink: Arc<dyn CloneSink>,
}

impl EventSink for ProgressSink {
    fn emit(&self, event: &ProcessEvent) {
        if let ProcessEvent::Output { lines, .. } = event {
            if let Some(last) = lines.iter().rev().find(|l| !l.text.trim().is_empty()) {
                let (phase, percent) = parse_progress(&last.text);
                self.sink.emit(&CloneEvent::Progress {
                    clone_id: self.clone_id.clone(),
                    phase,
                    percent,
                    line: last.text.clone(),
                });
            }
        }
    }
}

/// Everything a clone job needs from the application.
#[derive(Clone)]
pub struct CloneContext {
    pub db: Arc<Database>,
    pub config: GithubConfig,
    pub search: &'static SearchPath,
    pub log_dir: Option<PathBuf>,
    pub sink: Arc<dyn CloneSink>,
    pub scans: Scans,
    pub scan_sink: Arc<dyn ScanSink>,
}

#[derive(Clone, Default)]
pub struct Clones {
    running: Arc<Mutex<HashMap<String, (ProcessRunner, String)>>>,
}

impl Clones {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, (ProcessRunner, String)>> {
        self.running.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn is_running(&self, clone_id: &str) -> bool {
        self.lock().contains_key(clone_id)
    }

    /// Request cancellation. The folder is kept; the completion event reports `cancelled`.
    pub fn cancel(&self, clone_id: &str) -> bool {
        match self.lock().get(clone_id) {
            Some((runner, run_id)) => runner.cancel(run_id) == CancelOutcome::Requested,
            None => false,
        }
    }

    /// Start a clone the user approved from `preview`. `destination` must be
    /// the previewed path; if anything changed, the user must review again.
    pub fn start(
        &self,
        ctx: CloneContext,
        github_repository_id: &str,
        destination: &str,
    ) -> AppResult<(RepositoryClone, tokio::task::JoinHandle<CloneResult>)> {
        let p = preview(&ctx.db, &ctx.config, github_repository_id)?;
        if !p.ready {
            return Err(AppError::validation(format!(
                "The clone cannot start: {}",
                p.problems.join(" ")
            ))
            .with_context("problems", p.problems.clone()));
        }
        if p.destination.as_deref() != Some(destination) {
            return Err(AppError::validation(
                "The destination changed since the preview. Review the new preview and confirm again.",
            )
            .with_context("previewed", destination));
        }
        let ws = p
            .workspace
            .ok_or_else(|| AppError::validation("No managed workspace is approved."))?;
        let root = resolve_workspace(&ws)?;
        let dest = PathBuf::from(destination);
        let gh = ctx
            .db
            .read(|c| gh_store::get_repository(c, github_repository_id))?;
        let url = clone_url(&ctx.config, &gh.owner, &gh.name)?;

        create_destination(&root, &dest)?;
        let begun = ctx.db.write(|c| {
            let (repo, _) = gh_store::import_metadata_only(c, github_repository_id)?;
            let attempt = store::begin_clone(
                c,
                &NewClone {
                    repository_id: repo.id.clone(),
                    github_repository_id: Some(gh.id.clone()),
                    workspace_id: ws.id.clone(),
                    full_name: gh.full_name.clone(),
                    clone_url: url.to_string(),
                    destination_path: destination.to_string(),
                },
            )?;
            activity::record(
                c,
                NewEvent::new(
                    "repository_clone",
                    "clone.started",
                    Severity::Info,
                    format!("Cloning {} into {destination}", gh.full_name),
                )
                .entity(attempt.id.clone())
                .metadata(json!({ "repositoryId": repo.id, "url": url.as_str(), "destination": destination })),
            )?;
            Ok((repo, attempt))
        });
        let (repo, attempt) = match begun {
            Ok(v) => v,
            Err(e) => {
                // Nothing was cloned: remove the folder created a moment ago,
                // and only if it is still empty.
                let _ = std::fs::remove_dir(&dest);
                return Err(e);
            }
        };

        let runner = ProcessRunner::new(
            ExecutionPolicy::managed_clone(root.clone()),
            ctx.log_dir.clone(),
            Some(ctx.db.clone()),
        );
        let mut spec = CommandSpec::new(
            format!("git clone {}", gh.full_name),
            CommandCategory::MutatingGit,
            Program::Named("git".into()),
            &[],
        );
        spec.args = clone_args(&url, &dest);
        spec.cwd = Some(root);
        spec.env = GIT_ENV
            .iter()
            .chain(CLONE_ENV)
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        spec.timeout = CLONE_TIMEOUT;
        spec.max_output_bytes = CLONE_OUTPUT_LIMIT;
        let progress = Arc::new(ProgressSink {
            clone_id: attempt.id.clone(),
            sink: ctx.sink.clone(),
        });
        let started = match runner.start(spec, ctx.search, progress) {
            Ok(s) => s,
            Err(e) => {
                record_finish(&ctx.db, &attempt, CloneStatus::Failed, Some(&e), &dest)?;
                return Err(e);
            }
        };
        ctx.db
            .write(|c| store::set_run_id(c, &attempt.id, &started.run_id))?;
        self.lock()
            .insert(attempt.id.clone(), (runner, started.run_id.clone()));

        let this = self.clone();
        let job = attempt.clone();
        let handle = tokio::spawn(async move {
            let run = started
                .handle
                .await
                .map_err(|e| AppError::internal(format!("clone supervisor task failed: {e}")));
            let result = conclude(&ctx, &job, &repo, &gh, &dest, run).await;
            this.lock().remove(&job.id);
            ctx.sink.emit(&CloneEvent::Completed {
                result: Box::new(result.clone()),
            });
            result
        });
        Ok((attempt, handle))
    }
}

/// Map a failed run to an actionable error. Authentication problems get
/// their own code; an `insteadOf` rewrite to SSH is called out.
pub fn classify(run: &RunResult, private: bool) -> AppError {
    let text = run.stderr.to_ascii_lowercase();
    let tail: String = run
        .stderr
        .lines()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let auth = [
        "could not read username",
        "could not read password",
        "authentication failed",
        "terminal prompts disabled",
        "invalid username or password",
        "returned error: 401",
        "returned error: 403",
    ]
    .iter()
    .any(|needle| text.contains(needle))
        || (private && text.contains("not found"));
    if auth {
        return AppError::new(
            ErrorCode::GitAuthenticationRequired,
            "Git could not authenticate to clone this repository.",
        )
        .with_details(tail);
    }
    if text.contains("transport 'ssh' not allowed") || text.contains("transport 'git' not allowed")
    {
        return AppError::new(
            ErrorCode::CommandNotAllowed,
            "Your Git configuration rewrites this HTTPS URL to another transport, and ShiftOps clones over HTTPS only.",
        )
        .with_details(tail)
        .with_action("Remove the `url.<base>.insteadOf` rule for this host from your Git configuration, or link an existing clone instead.");
    }
    match &run.error {
        Some(e) if run.status == RunStatus::TimedOut => e.clone(),
        Some(e) => {
            let mut e = e.clone();
            if e.technical_details.is_none() && !tail.is_empty() {
                e = e.with_details(tail);
            }
            e
        }
        None => AppError::new(ErrorCode::CommandFailed, "git clone failed.").with_details(tail),
    }
}

fn record_finish(
    db: &Database,
    attempt: &RepositoryClone,
    status: CloneStatus,
    error: Option<&AppError>,
    dest: &Path,
) -> AppResult<RepositoryClone> {
    let partial = std::fs::symlink_metadata(dest).is_ok();
    db.write(|c| {
        let done = store::finish_clone(
            c,
            &attempt.id,
            status,
            error.map(|e| (e.code.as_str(), e.message.as_str())),
            partial && status != CloneStatus::Succeeded,
        )?;
        let (event_type, severity, summary) = match status {
            CloneStatus::Succeeded => (
                "clone.completed",
                Severity::Notice,
                format!(
                    "Cloned {} into {}",
                    attempt.full_name, attempt.destination_path
                ),
            ),
            CloneStatus::Cancelled => (
                "clone.cancelled",
                Severity::Notice,
                format!(
                    "Clone of {} cancelled; the folder was kept",
                    attempt.full_name
                ),
            ),
            _ => (
                "clone.failed",
                Severity::Error,
                format!("Clone of {} failed; the folder was kept", attempt.full_name),
            ),
        };
        activity::record(
            c,
            NewEvent::new("repository_clone", event_type, severity, summary)
                .entity(attempt.id.clone())
                .metadata(json!({
                    "repositoryId": attempt.repository_id,
                    "destination": attempt.destination_path,
                    "partialPathExists": done.partial_path_exists,
                    "errorCode": error.map(|e| e.code.as_str()),
                    "runId": done.run_id,
                })),
        )?;
        Ok(done)
    })
}

/// Confirm the new checkout is the requested repository, then attach it.
async fn attach(
    ctx: &CloneContext,
    repo: &Repository,
    gh: &GithubRepository,
    dest: &Path,
) -> AppResult<Repository> {
    let git_dir = match inspect::detect_marker(dest) {
        Some(Ok((GitKind::Standard, git_dir))) => git_dir,
        _ => {
            return Err(AppError::new(
                ErrorCode::CommandFailed,
                "git clone finished but the folder is not a standard Git repository.",
            ))
        }
    };
    let runner = ProcessRunner::new(
        ExecutionPolicy::readonly_git(vec![dest.to_path_buf()]),
        None,
        None,
    );
    let metadata = inspect::collect(&runner, ctx.search, dest, &git_dir).await;
    let origin = metadata.origin.as_ref();
    let matches = origin.is_some_and(|o| {
        o.host
            .as_deref()
            .is_some_and(|h| h.eq_ignore_ascii_case(&ctx.config.host_name()))
            && o.owner
                .as_deref()
                .is_some_and(|v| v.eq_ignore_ascii_case(&gh.owner))
            && o.repo
                .as_deref()
                .is_some_and(|v| v.eq_ignore_ascii_case(&gh.name))
    });
    if !matches {
        return Err(AppError::new(
            ErrorCode::CommandFailed,
            "The cloned repository's origin does not match the selected GitHub repository.",
        )
        .with_context(
            "origin",
            origin.map(|o| o.sanitized_url.clone()).unwrap_or_default(),
        ));
    }
    ctx.db.write(|c| {
        store::attach_checkout(
            c,
            &repo.id,
            &Checkout {
                local_path: dest.display().to_string(),
                git_kind: GitKind::Standard,
                metadata: metadata.clone(),
                ownership: Ownership::ShiftopsManaged,
                method: LinkMethod::ManagedClone,
            },
        )
    })
}

async fn conclude(
    ctx: &CloneContext,
    attempt: &RepositoryClone,
    repo: &Repository,
    gh: &GithubRepository,
    dest: &Path,
    run: AppResult<RunResult>,
) -> CloneResult {
    let fail = |status: CloneStatus, error: AppError| {
        let clone =
            record_finish(&ctx.db, attempt, status, Some(&error), dest).unwrap_or_else(|e| {
                e.log();
                attempt.clone()
            });
        CloneResult {
            clone,
            repository: None,
            error: Some(error),
            scan_started: false,
        }
    };
    let run = match run {
        Ok(r) => r,
        Err(e) => return fail(CloneStatus::Failed, e),
    };
    match run.status {
        RunStatus::Succeeded => {}
        RunStatus::Cancelled => {
            return fail(
                CloneStatus::Cancelled,
                AppError::new(ErrorCode::CommandCancelled, "The clone was cancelled.").with_action(
                    "The folder was kept. Clean it up from Repositories when you no longer need it, then clone again.",
                ),
            )
        }
        RunStatus::Failed | RunStatus::TimedOut => {
            return fail(CloneStatus::Failed, classify(&run, gh.private))
        }
    }
    let linked = match attach(ctx, repo, gh, dest).await {
        Ok(r) => r,
        Err(e) => return fail(CloneStatus::Failed, e),
    };
    let clone = match record_finish(&ctx.db, attempt, CloneStatus::Succeeded, None, dest) {
        Ok(c) => c,
        Err(e) => {
            e.log();
            attempt.clone()
        }
    };
    let scan_started = ctx
        .scans
        .start(
            &linked.id,
            false,
            ctx.db.clone(),
            ctx.search,
            ctx.scan_sink.clone(),
        )
        .inspect_err(|e| e.log())
        .is_ok();
    CloneResult {
        clone,
        repository: Some(linked),
        error: None,
        scan_started,
    }
}

#[cfg(all(test, unix))]
pub(crate) mod tests {
    //! Real `git clone` runs against a local dumb-HTTP server serving a bare
    //! repository, so no network or credentials are involved.

    use super::*;
    use crate::scanner::ScanEvent;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::process::Command;

    #[derive(Default)]
    pub struct Collect(pub Mutex<Vec<CloneEvent>>);
    impl CloneSink for Collect {
        fn emit(&self, event: &CloneEvent) {
            self.0.lock().unwrap().push(event.clone());
        }
    }
    #[derive(Default)]
    pub struct ScanCollect(pub Mutex<Vec<ScanEvent>>);
    impl ScanSink for ScanCollect {
        fn emit(&self, event: &ScanEvent) {
            self.0.lock().unwrap().push(event.clone());
        }
    }

    pub fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=ShiftOps Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "protocol.file.allow=always",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[derive(Clone, Copy, PartialEq)]
    pub enum Serve {
        Files,
        /// Accept connections and never answer, so a clone hangs until cancelled.
        Hang,
    }

    /// A minimal static HTTP server for Git's dumb protocol. Returns its base URL.
    pub fn serve(dir: PathBuf, mode: Serve) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request = String::new();
                    if reader.read_line(&mut request).is_err() {
                        return;
                    }
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                            break;
                        }
                    }
                    if mode == Serve::Hang {
                        std::thread::sleep(Duration::from_secs(120));
                        return;
                    }
                    let path = request.split_whitespace().nth(1).unwrap_or("/");
                    let path = path
                        .split('?')
                        .next()
                        .unwrap_or("/")
                        .trim_start_matches('/');
                    let file = dir.join(path);
                    let mut stream = stream;
                    match (path.contains(".."), std::fs::read(&file)) {
                        (false, Ok(body)) => {
                            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n", body.len());
                            let _ = stream.write_all(&body);
                        }
                        _ => {
                            let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        }
                    }
                });
            }
        });
        format!("http://127.0.0.1:{port}/")
    }

    /// `<served>/acme/api.git`: a bare repository with a package.json and a
    /// submodule entry that must not be cloned.
    pub fn bare_repo(served: &Path) {
        let t = served.join("src");
        std::fs::create_dir_all(&t).unwrap();
        git(&t, &["init", "-q"]);
        std::fs::write(
            t.join("package.json"),
            r#"{"name":"api","engines":{"node":">=20"}}"#,
        )
        .unwrap();
        git(&t, &["add", "package.json"]);
        git(&t, &["commit", "-q", "-m", "init"]);
        let sub = served.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        git(&sub, &["init", "-q"]);
        std::fs::write(sub.join("x"), "x").unwrap();
        git(&sub, &["add", "x"]);
        git(&sub, &["commit", "-q", "-m", "sub"]);
        git(
            &t,
            &[
                "submodule",
                "add",
                "-q",
                &sub.display().to_string(),
                "vendor/sub",
            ],
        );
        git(&t, &["commit", "-q", "-m", "add submodule"]);
        std::fs::create_dir_all(served.join("acme")).unwrap();
        git(served, &["clone", "-q", "--bare", "src", "acme/api.git"]);
        git(&served.join("acme/api.git"), &["update-server-info"]);
    }

    pub fn search() -> &'static SearchPath {
        Box::leak(Box::new(SearchPath {
            dirs: std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect(),
            source: "test".into(),
        }))
    }

    /// An account for `host` and a cached GitHub repository `acme/api`.
    pub fn github_record(db: &Database, host: &str, private: bool) -> String {
        db.write(|c| {
            c.execute(
                "INSERT INTO github_accounts (id, host, api_base_url, github_user_id, login, auth_method, credential_kind,
                     created_at, updated_at)
                 VALUES ('acct', ?1, 'http://127.0.0.1/', 1, 'me', 'personal_access_token',
                     'github_personal_access_token', 't', 't')",
                [host],
            )?;
            let gh = gh_store::upsert_repository(
                c,
                "acct",
                &gh_store::RepositoryUpsert {
                    github_id: 42,
                    owner: "acme".into(),
                    name: "api".into(),
                    full_name: "acme/api".into(),
                    private,
                    html_url: "https://github.com/acme/api".into(),
                    ..Default::default()
                },
            )?;
            Ok(gh.id)
        })
        .unwrap()
    }

    pub struct Env {
        pub _t: tempfile::TempDir,
        pub root: PathBuf,
        pub db: Arc<Database>,
        pub ctx: CloneContext,
        pub events: Arc<Collect>,
        pub scans_seen: Arc<ScanCollect>,
        pub gh_id: String,
    }

    pub fn env(mode: Serve, private: bool) -> Env {
        let t = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(t.path()).unwrap();
        let served = base.join("served");
        if mode == Serve::Files {
            bare_repo(&served);
        }
        let url = serve(served, mode);
        let root = base.join("workspace");
        std::fs::create_dir(&root).unwrap();
        let db = Arc::new(Database::open_in_memory());
        super::super::approve_workspace(&db, &root.display().to_string()).unwrap();
        let config = GithubConfig {
            host: url::Url::parse(&url).unwrap(),
            ..super::super::tests::config()
        };
        let gh_id = github_record(&db, &config.host_name(), private);
        let events = Arc::new(Collect::default());
        let scans_seen = Arc::new(ScanCollect::default());
        let ctx = CloneContext {
            db: db.clone(),
            config,
            search: search(),
            log_dir: Some(base.join("logs")),
            sink: events.clone(),
            scans: Scans::default(),
            scan_sink: scans_seen.clone(),
        };
        Env {
            _t: t,
            root,
            db,
            ctx,
            events,
            scans_seen,
            gh_id,
        }
    }

    #[test]
    fn progress_lines_parse() {
        assert_eq!(
            parse_progress("Receiving objects:  45% (9/20), 1.2 MiB"),
            (Some("Receiving objects".into()), Some(45))
        );
        assert_eq!(
            parse_progress("remote: Counting objects: 100% (3/3), done."),
            (Some("Counting objects".into()), Some(100))
        );
        assert_eq!(
            parse_progress("Cloning into '/w/acme/api'..."),
            (None, None)
        );
        assert_eq!(parse_progress("fatal: 404% wrong"), (None, None));
    }

    #[test]
    fn failures_are_classified_and_redacted() {
        let run = |stderr: &str| RunResult {
            run_id: "r".into(),
            label: "git clone".into(),
            category: CommandCategory::MutatingGit,
            status: RunStatus::Failed,
            exit_code: Some(128),
            duration_ms: 1,
            started_at: "t".into(),
            completed_at: "t".into(),
            stdout: String::new(),
            stderr: stderr.into(),
            output_bytes: 0,
            output_limit_bytes: 1,
            output_truncated: false,
            log_path: None,
            error: Some(AppError::new(
                ErrorCode::CommandFailed,
                "git clone exited with code 128.",
            )),
        };
        let auth = classify(
            &run("fatal: could not read Username for 'https://github.com': terminal prompts disabled"),
            true,
        );
        assert_eq!(auth.code, ErrorCode::GitAuthenticationRequired);
        assert!(auth.suggested_action.unwrap().contains("gh auth setup-git"));
        let hidden = classify(
            &run("remote: Repository not found.\nfatal: repository not found"),
            true,
        );
        assert_eq!(hidden.code, ErrorCode::GitAuthenticationRequired);
        let public_missing = classify(&run("fatal: repository not found"), false);
        assert_eq!(public_missing.code, ErrorCode::CommandFailed);
        let rewritten = classify(&run("fatal: transport 'ssh' not allowed"), false);
        assert!(rewritten.suggested_action.unwrap().contains("insteadOf"));
        let leaked = classify(
            &run("fatal: unable to access 'https://x-access-token:ghs_abcdefghijklmnopqrstuvwxyz0123456789@github.com/acme/api.git/'"),
            false,
        );
        let shown = serde_json::to_string(&leaked).unwrap();
        assert!(
            !shown.contains("ghs_abcdefghijklmnopqrstuvwxyz0123456789"),
            "{shown}"
        );
    }

    #[tokio::test]
    async fn clones_links_marks_managed_and_scans() {
        let e = env(Serve::Files, false);
        // Metadata-only import first: the clone must reuse this record.
        let (meta, _) = crate::github::import(&e.db, &e.gh_id).unwrap();
        let p = preview(&e.db, &e.ctx.config, &e.gh_id).unwrap();
        assert!(p.ready, "{:?}", p.problems);
        let dest = e.root.join("acme/api");
        assert_eq!(p.destination.as_deref(), Some(dest.to_str().unwrap()));
        assert_eq!(p.repository_id.as_deref(), Some(meta.id.as_str()));
        assert!(!p.clone_url.as_ref().unwrap().contains('@'));

        let clones = Clones::default();
        let (attempt, handle) = clones
            .start(e.ctx.clone(), &e.gh_id, dest.to_str().unwrap())
            .unwrap();
        assert_eq!(attempt.status, CloneStatus::Running);
        let result = handle.await.unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(result.clone.status, CloneStatus::Succeeded);
        assert!(!result.clone.partial_path_exists);
        let repo = result.repository.unwrap();
        assert_eq!(repo.id, meta.id, "no duplicate record");
        assert_eq!(repo.ownership, Some(Ownership::ShiftopsManaged));
        assert_eq!(repo.github_link_method, Some(LinkMethod::ManagedClone));
        assert_eq!(repo.local_path.as_deref(), Some(dest.to_str().unwrap()));
        assert_eq!(repo.current_branch.as_deref(), Some("main"));
        assert_eq!(
            e.db.read(|c| crate::db::repositories::list(c, &Default::default()))
                .unwrap()
                .total,
            1
        );
        // Safe clone: files present, no hooks copied, submodule not cloned.
        assert!(dest.join("package.json").is_file());
        assert!(!dest.join(".git/hooks").exists());
        let sub = dest.join("vendor/sub");
        assert!(!sub.join("x").exists(), "submodules are not recursed");

        assert!(result.scan_started);
        for _ in 0..200 {
            if e.scans_seen
                .0
                .lock()
                .unwrap()
                .iter()
                .any(|ev| matches!(ev, ScanEvent::Completed { .. }))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let intel =
            e.db.read(|c| crate::db::intelligence::latest(c, &repo.id))
                .unwrap();
        assert!(intel.scan.is_some(), "the safe scanner ran on the clone");

        let events = e.events.0.lock().unwrap().clone();
        assert!(matches!(events.last(), Some(CloneEvent::Completed { .. })));
        let log =
            e.db.read(|c| activity::list(c, &Default::default()))
                .unwrap();
        let types: Vec<_> = log.items.iter().map(|a| a.event_type.as_str()).collect();
        assert!(
            types.contains(&"clone.started") && types.contains(&"clone.completed"),
            "{types:?}"
        );
        // A second clone of the same repository is refused.
        assert!(!preview(&e.db, &e.ctx.config, &e.gh_id).unwrap().ready);
        assert!(clones
            .start(e.ctx.clone(), &e.gh_id, dest.to_str().unwrap())
            .is_err());
    }

    #[tokio::test]
    async fn managed_checkout_deletion_is_explicit_bounded_and_reversible_to_metadata() {
        let e = env(Serve::Files, false);
        let dest = e.root.join("acme/api");
        let (_, handle) = Clones::default()
            .start(e.ctx.clone(), &e.gh_id, dest.to_str().unwrap())
            .unwrap();
        let repo = handle.await.unwrap().repository.unwrap();
        let path = repo.local_path.clone().unwrap();
        let del = |confirm: &str, clean: bool| {
            super::super::delete_managed_checkout(&e.db, &repo.id, confirm, clean)
        };
        assert!(del(e.root.to_str().unwrap(), true)
            .unwrap_err()
            .message
            .contains("does not match"));
        assert!(del(&path, false)
            .unwrap_err()
            .message
            .contains("uncommitted"));
        assert!(
            dest.join("package.json").exists(),
            "nothing deleted by refused attempts"
        );

        let done = del(&path, true).unwrap();
        assert!(!dest.exists());
        assert!(e.root.exists(), "the workspace stays");
        assert_eq!(done.id, repo.id, "same record");
        assert_eq!(done.local_path, None);
        assert_eq!(done.ownership, None);
        assert_eq!(done.github_link_method, Some(LinkMethod::MetadataOnly));
        let log =
            e.db.read(|c| activity::list(c, &Default::default()))
                .unwrap();
        assert!(log
            .items
            .iter()
            .any(|a| a.event_type == "repository.managed_files_deleted"));
        // It can be cloned again afterwards.
        assert!(preview(&e.db, &e.ctx.config, &e.gh_id).unwrap().ready);
    }

    #[test]
    fn external_checkouts_are_never_deleted() {
        let db = Database::open_in_memory();
        let t = tempfile::tempdir().unwrap();
        db.write(|c| {
            c.execute(
                "INSERT INTO repositories (id, source_type, name, local_path, ownership, created_at, updated_at)
                 VALUES ('ext', 'local', 'mine', ?1, 'external', 't', 't')",
                [t.path().display().to_string()],
            )?;
            Ok(())
        })
        .unwrap();
        let err =
            super::super::delete_managed_checkout(&db, "ext", t.path().to_str().unwrap(), true)
                .unwrap_err();
        assert!(err.message.contains("never deleted"));
        assert!(t.path().exists());
    }

    #[tokio::test]
    async fn failed_clone_keeps_folder_and_offers_cleanup() {
        // Nothing is served: every request is a 404.
        let e = env(Serve::Files, true);
        std::fs::remove_dir_all(e.root.parent().unwrap().join("served/acme")).unwrap();
        let dest = e.root.join("acme/api");
        let clones = Clones::default();
        let (_, handle) = clones
            .start(e.ctx.clone(), &e.gh_id, dest.to_str().unwrap())
            .unwrap();
        let result = handle.await.unwrap();
        assert_eq!(result.clone.status, CloneStatus::Failed);
        assert!(result.repository.is_none());
        assert!(
            dest.is_dir(),
            "the folder is retained, never removed automatically"
        );
        assert!(result.clone.partial_path_exists);
        let repo =
            e.db.read(|c| {
                crate::db::repositories::get(c, result.clone.repository_id.as_deref().unwrap())
            })
            .unwrap();
        assert_eq!(repo.local_path, None, "stays metadata-only");
        // The retained folder blocks a new clone until it is cleaned up.
        let p = preview(&e.db, &e.ctx.config, &e.gh_id).unwrap();
        assert!(!p.ready);
        let cleaned =
            super::super::cleanup(&e.db, &result.clone.id, &result.clone.destination_path).unwrap();
        assert_eq!(cleaned.outcome, super::super::CleanupOutcome::Removed);
        assert!(!dest.exists());
        assert!(e.root.exists());
        assert!(preview(&e.db, &e.ctx.config, &e.gh_id).unwrap().ready);
    }

    #[tokio::test]
    async fn cancelled_clone_keeps_folder() {
        let e = env(Serve::Hang, false);
        let dest = e.root.join("acme/api");
        let clones = Clones::default();
        let (attempt, handle) = clones
            .start(e.ctx.clone(), &e.gh_id, dest.to_str().unwrap())
            .unwrap();
        assert!(clones.is_running(&attempt.id));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(clones.cancel(&attempt.id));
        let result = handle.await.unwrap();
        assert_eq!(result.clone.status, CloneStatus::Cancelled);
        assert_eq!(result.error.unwrap().code, ErrorCode::CommandCancelled);
        assert!(dest.is_dir());
        assert!(
            !clones.cancel(&attempt.id),
            "repeated cancellation is harmless"
        );
        let log =
            e.db.read(|c| activity::list(c, &Default::default()))
                .unwrap();
        assert!(log.items.iter().any(|a| a.event_type == "clone.cancelled"));
    }

    #[tokio::test]
    async fn stale_previews_are_refused() {
        let e = env(Serve::Files, false);
        let clones = Clones::default();
        let other = e.root.join("acme/other");
        let err = clones
            .start(e.ctx.clone(), &e.gh_id, other.to_str().unwrap())
            .unwrap_err();
        assert!(err.message.contains("changed since the preview"));
        assert!(!other.exists());
        // An existing folder at the destination is a collision, not a target.
        std::fs::create_dir_all(e.root.join("acme/api")).unwrap();
        std::fs::write(e.root.join("acme/api/mine.txt"), "user data").unwrap();
        let p = preview(&e.db, &e.ctx.config, &e.gh_id).unwrap();
        assert!(!p.ready && p.problems.iter().any(|m| m.contains("already exists")));
        assert!(clones
            .start(
                e.ctx.clone(),
                &e.gh_id,
                e.root.join("acme/api").to_str().unwrap()
            )
            .is_err());
        assert!(e.root.join("acme/api/mine.txt").exists());
    }
}
