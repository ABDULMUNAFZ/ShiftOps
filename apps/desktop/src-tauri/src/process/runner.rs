//! Spawning, streaming, cancellation, timeouts and cleanup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::{
    CommandSpec, EventSink, ExecutionPolicy, OutputLine, ProcessEvent, Program, RunResult,
    RunStatus, Stream, INHERITED_ENV,
};
use crate::db::activity::{self, NewEvent, Severity};
use crate::db::Database;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::paths::{self, PathKind};
use crate::redact::Redactor;
use crate::shell_path::SearchPath;
use crate::{logging, time_util};

const READ_CHUNK: usize = 8 * 1024;
/// Lines longer than this are split so one huge line cannot exhaust memory.
const MAX_LINE_BYTES: usize = 16 * 1024;
/// Output events are batched to keep the UI responsive under heavy output.
const BATCH_INTERVAL: Duration = Duration::from_millis(50);
const BATCH_MAX_LINES: usize = 256;
const TERMINATE_GRACE: Duration = Duration::from_secs(2);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveProcess {
    pub run_id: String,
    pub label: String,
    pub category: super::CommandCategory,
    pub pid: Option<u32>,
    pub started_at: String,
}

struct ActiveEntry {
    info: ActiveProcess,
    cancel: watch::Sender<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelOutcome {
    /// Termination was requested; the completion event will report `cancelled`.
    Requested,
    /// No active run has this id (already finished or never existed).
    NotRunning,
}

pub struct StartedRun {
    pub run_id: String,
    pub handle: JoinHandle<RunResult>,
}

#[derive(Clone)]
pub struct ProcessRunner {
    policy: ExecutionPolicy,
    registry: Arc<Mutex<HashMap<String, ActiveEntry>>>,
    log_dir: Option<PathBuf>,
    db: Option<Arc<Database>>,
}

impl ProcessRunner {
    pub fn new(
        policy: ExecutionPolicy,
        log_dir: Option<PathBuf>,
        db: Option<Arc<Database>>,
    ) -> Self {
        Self {
            policy,
            registry: Arc::new(Mutex::new(HashMap::new())),
            log_dir,
            db,
        }
    }

    pub fn active(&self) -> Vec<ActiveProcess> {
        self.registry
            .lock()
            .map(|r| r.values().map(|e| e.info.clone()).collect())
            .unwrap_or_default()
    }

    /// Request cancellation. Repeated or late calls are harmless.
    pub fn cancel(&self, run_id: &str) -> CancelOutcome {
        let Ok(registry) = self.registry.lock() else {
            return CancelOutcome::NotRunning;
        };
        match registry.get(run_id) {
            Some(entry) => {
                let _ = entry.cancel.send(true);
                CancelOutcome::Requested
            }
            None => CancelOutcome::NotRunning,
        }
    }

    fn record(&self, event: NewEvent) {
        if let Some(db) = &self.db {
            if let Err(e) = db.write(|c| activity::record(c, event)) {
                logging::error("process", &format!("failed to record activity: {e}"));
            }
        }
    }

    /// Validate, resolve and spawn. Errors before spawn (policy, missing
    /// executable, invalid directory) are returned here; everything after is
    /// reported in the `RunResult`.
    pub fn start(
        &self,
        spec: CommandSpec,
        search: &SearchPath,
        sink: Arc<dyn EventSink>,
    ) -> AppResult<StartedRun> {
        let prepared = self.prepare(&spec, search);
        let (exe, cwd) = match prepared {
            Ok(v) => v,
            Err(err) => {
                if spec.record_activity {
                    self.record(
                        NewEvent::new("process", "process.rejected", Severity::Warning, format!("{} was not started: {}", spec.label, err.message))
                            .metadata(json!({ "label": spec.label, "category": spec.category.as_str(), "code": err.code.as_str() })),
                    );
                }
                return Err(err);
            }
        };

        let redactor = Redactor::with_secrets(spec.secrets.iter().cloned());
        let mut cmd = Command::new(&exe);
        cmd.args(&spec.args)
            .current_dir(&cwd)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for name in INHERITED_ENV {
            if let Some(value) = std::env::var_os(name) {
                cmd.env(name, value);
            }
        }
        cmd.env("PATH", search.joined());
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        // Own process group, so cancellation reaches grandchildren too.
        #[cfg(unix)]
        cmd.process_group(0);

        let child = cmd.spawn().map_err(|e| {
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                ErrorCode::ExecutableNotFound
            } else {
                ErrorCode::CommandFailed
            };
            AppError::new(code, format!("{} could not be started.", spec.label))
                .with_details(e.to_string())
                .with_context("program", exe.display().to_string())
        })?;

        let run_id = crate::db::new_id();
        let pid = child.id();
        let started_at = time_util::now();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let info = ActiveProcess {
            run_id: run_id.clone(),
            label: spec.label.clone(),
            category: spec.category,
            pid,
            started_at: started_at.clone(),
        };
        if let Ok(mut registry) = self.registry.lock() {
            registry.insert(
                run_id.clone(),
                ActiveEntry {
                    info,
                    cancel: cancel_tx,
                },
            );
        }

        let shown_args: Vec<String> = spec.args.iter().map(|a| redactor.redact(a)).collect();
        if spec.record_activity {
            self.record(
                NewEvent::new(
                    "process",
                    "process.started",
                    Severity::Info,
                    format!("Started {}", spec.label),
                )
                .entity(run_id.clone())
                .metadata(json!({
                    "label": spec.label,
                    "category": spec.category.as_str(),
                    "program": exe.display().to_string(),
                    "args": shown_args,
                    "cwd": cwd.display().to_string(),
                    "pid": pid,
                    "timeoutSeconds": spec.timeout.as_secs(),
                    "maxOutputBytes": spec.max_output_bytes,
                })),
            );
        }
        sink.emit(&ProcessEvent::Started {
            run_id: run_id.clone(),
            label: spec.label.clone(),
            category: spec.category,
            program: exe.display().to_string(),
            args: shown_args,
            cwd: cwd.display().to_string(),
            pid,
            started_at: started_at.clone(),
        });

        let runner = self.clone();
        let id = run_id.clone();
        let handle = tokio::spawn(async move {
            let result = supervise(
                child,
                &id,
                &spec,
                started_at,
                cancel_rx,
                redactor,
                runner.log_dir.clone(),
                sink.clone(),
            )
            .await;
            if let Ok(mut registry) = runner.registry.lock() {
                registry.remove(&id);
            }
            if spec.record_activity {
                runner.record(completion_event(&result, &exe));
            }
            sink.emit(&ProcessEvent::Completed {
                result: result.clone(),
            });
            result
        });
        Ok(StartedRun { run_id, handle })
    }

    /// Run to completion without events, for short internal probes.
    pub async fn run_captured(
        &self,
        spec: CommandSpec,
        search: &SearchPath,
    ) -> AppResult<RunResult> {
        let started = self.start(spec, search, Arc::new(super::NullSink))?;
        started
            .handle
            .await
            .map_err(|e| AppError::internal(format!("process supervisor task failed: {e}")))
    }

    fn prepare(&self, spec: &CommandSpec, search: &SearchPath) -> AppResult<(PathBuf, PathBuf)> {
        self.policy.check(spec)?;
        let exe = match &spec.program {
            Program::Named(name) => search.find(name).ok_or_else(|| {
                AppError::new(
                    ErrorCode::ExecutableNotFound,
                    format!("`{name}` was not found on the search path."),
                )
                .with_context("program", name.as_str())
                .with_context("directoriesSearched", search.dirs.len())
            })?,
            Program::Path(path) => {
                if !path.is_absolute() || !path.is_file() {
                    return Err(AppError::new(
                        ErrorCode::ExecutableNotFound,
                        "The executable does not exist.",
                    )
                    .with_context("program", path.display().to_string()));
                }
                path.clone()
            }
        };
        let cwd_input = spec.cwd.clone().unwrap_or_else(std::env::temp_dir);
        let cwd = paths::validate(&cwd_input.display().to_string(), self.policy.cwd_roots())?;
        if cwd.kind != PathKind::Directory {
            return Err(AppError::new(
                ErrorCode::PathNotAllowed,
                "The working directory does not exist or is not a directory.",
            )
            .with_context("path", cwd_input.display().to_string()));
        }
        Ok((exe, cwd.canonical))
    }
}

fn completion_event(result: &RunResult, exe: &Path) -> NewEvent {
    let (event_type, severity, verb) = match result.status {
        RunStatus::Succeeded => ("process.completed", Severity::Info, "Completed"),
        RunStatus::Cancelled => ("process.cancelled", Severity::Notice, "Cancelled"),
        RunStatus::TimedOut => ("process.failed", Severity::Error, "Timed out"),
        RunStatus::Failed => ("process.failed", Severity::Error, "Failed"),
    };
    NewEvent::new(
        "process",
        event_type,
        severity,
        format!("{verb}: {}", result.label),
    )
    .entity(result.run_id.clone())
    .metadata(json!({
        "label": result.label,
        "category": result.category.as_str(),
        "program": exe.display().to_string(),
        "status": result.status,
        "exitCode": result.exit_code,
        "durationMs": result.duration_ms,
        "outputBytes": result.output_bytes,
        "outputTruncated": result.output_truncated,
        "logPath": result.log_path,
        "errorCode": result.error.as_ref().map(|e| e.code.as_str()),
    }))
}

enum Ending {
    Exited(std::process::ExitStatus),
    WaitFailed(std::io::Error),
    TimedOut,
    Cancelled,
}

#[allow(clippy::too_many_arguments)]
async fn supervise(
    mut child: Child,
    run_id: &str,
    spec: &CommandSpec,
    started_at: String,
    mut cancel_rx: watch::Receiver<bool>,
    redactor: Redactor,
    log_dir: Option<PathBuf>,
    sink: Arc<dyn EventSink>,
) -> RunResult {
    let started = Instant::now();
    let pid = child.id();
    let (line_tx, line_rx) = mpsc::channel::<OutputLine>(1024);
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(tokio::spawn(read_lines(
            out,
            Stream::Stdout,
            line_tx.clone(),
            redactor.clone(),
        )));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(tokio::spawn(read_lines(
            err,
            Stream::Stderr,
            line_tx.clone(),
            redactor.clone(),
        )));
    }
    drop(line_tx);

    let log_path = log_dir.map(|d| d.join("runs").join(format!("{run_id}.log")));
    let collector = tokio::spawn(collect(
        line_rx,
        run_id.to_string(),
        spec.max_output_bytes,
        log_path.clone(),
        format!(
            "# {} ({})\n# run {run_id} started {started_at}\n",
            spec.label,
            spec.category.as_str()
        ),
        sink,
    ));

    let ending = tokio::select! {
        status = child.wait() => match status {
            Ok(s) => Ending::Exited(s),
            Err(e) => Ending::WaitFailed(e),
        },
        _ = tokio::time::sleep(spec.timeout) => Ending::TimedOut,
        _ = cancel_rx.wait_for(|cancelled| *cancelled) => Ending::Cancelled,
    };

    let exit = match ending {
        Ending::TimedOut | Ending::Cancelled => terminate(&mut child, pid).await,
        Ending::Exited(s) => Some(s),
        Ending::WaitFailed(_) => None,
    };
    // Remove any processes the command left behind in its group.
    kill_group(pid, true);

    // Pipes close once the group is gone; don't wait forever on a process
    // that escaped the group and kept a pipe open.
    for reader in readers {
        if tokio::time::timeout(DRAIN_TIMEOUT, reader).await.is_err() {
            logging::warn(
                "process",
                &format!("run {run_id}: output reader did not finish; detached"),
            );
        }
    }
    let captured = collector.await.unwrap_or_default();

    let exit_code = exit.and_then(|s| s.code());
    let (status, error) = match &ending {
        Ending::Cancelled => (
            RunStatus::Cancelled,
            Some(AppError::new(
                ErrorCode::CommandCancelled,
                format!("{} was cancelled.", spec.label),
            )),
        ),
        Ending::TimedOut => (
            RunStatus::TimedOut,
            Some(
                AppError::new(
                    ErrorCode::CommandTimedOut,
                    format!(
                        "{} timed out after {} s.",
                        spec.label,
                        spec.timeout.as_secs()
                    ),
                )
                .with_context("timeoutSeconds", spec.timeout.as_secs()),
            ),
        ),
        Ending::WaitFailed(e) => (
            RunStatus::Failed,
            Some(
                AppError::new(ErrorCode::CommandFailed, "Waiting for the process failed.")
                    .with_details(e.to_string()),
            ),
        ),
        Ending::Exited(s) if s.success() => (RunStatus::Succeeded, None),
        Ending::Exited(s) => {
            let how = match s.code() {
                Some(code) => format!("exited with code {code}"),
                None => "was terminated by a signal".to_string(),
            };
            let tail: String = captured
                .stderr
                .lines()
                .rev()
                .take(5)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            let mut err = AppError::new(ErrorCode::CommandFailed, format!("{} {how}.", spec.label));
            if !tail.is_empty() {
                err = err.with_details(tail);
            }
            (RunStatus::Failed, Some(err))
        }
    };

    RunResult {
        run_id: run_id.to_string(),
        label: spec.label.clone(),
        category: spec.category,
        status,
        exit_code,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        started_at,
        completed_at: time_util::now(),
        stdout: captured.stdout,
        stderr: captured.stderr,
        output_bytes: captured.bytes,
        output_limit_bytes: spec.max_output_bytes as u64,
        output_truncated: captured.truncated,
        log_path: captured
            .log_written
            .then(|| log_path.map(|p| p.display().to_string()))
            .flatten(),
        error,
    }
}

/// SIGTERM the group, wait briefly, then SIGKILL. Returns the exit status.
async fn terminate(child: &mut Child, pid: Option<u32>) -> Option<std::process::ExitStatus> {
    #[cfg(unix)]
    {
        kill_group(pid, false);
        if let Ok(Ok(status)) = tokio::time::timeout(TERMINATE_GRACE, child.wait()).await {
            return Some(status);
        }
        kill_group(pid, true);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        let _ = child.start_kill();
    }
    child.wait().await.ok()
}

#[cfg(unix)]
fn kill_group(pid: Option<u32>, force: bool) {
    let Some(pid) = pid.and_then(|p| i32::try_from(p).ok()) else {
        return;
    };
    if pid <= 1 {
        return;
    }
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
    // SAFETY: killpg only sends a signal. The group id is the child's pid
    // because it was spawned with process_group(0); ESRCH (group already
    // gone) is expected and ignored.
    unsafe {
        libc::killpg(pid, signal);
    }
}

#[cfg(not(unix))]
fn kill_group(_pid: Option<u32>, _force: bool) {}

/// Read a pipe into redacted lines, preserving the stream identity.
async fn read_lines<R: AsyncRead + Unpin>(
    mut pipe: R,
    stream: Stream,
    tx: mpsc::Sender<OutputLine>,
    redactor: Redactor,
) {
    let mut buf = vec![0u8; READ_CHUNK];
    let mut pending: Vec<u8> = Vec::new();
    let send = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes);
        let text = text.strip_suffix('\r').unwrap_or(&text);
        OutputLine {
            stream,
            text: redactor.redact(text),
        }
    };
    loop {
        let n = match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        pending.extend_from_slice(&buf[..n]);
        while let Some(pos) = pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = pending.drain(..=pos).collect();
            if tx.send(send(&line[..line.len() - 1])).await.is_err() {
                return;
            }
        }
        while pending.len() >= MAX_LINE_BYTES {
            let line: Vec<u8> = pending.drain(..MAX_LINE_BYTES).collect();
            if tx.send(send(&line)).await.is_err() {
                return;
            }
        }
    }
    if !pending.is_empty() {
        let _ = tx.send(send(&pending)).await;
    }
}

#[derive(Default)]
struct Captured {
    stdout: String,
    stderr: String,
    bytes: u64,
    truncated: bool,
    log_written: bool,
}

/// Enforce the output limit, batch events for the UI and write the redacted log.
async fn collect(
    mut rx: mpsc::Receiver<OutputLine>,
    run_id: String,
    limit: usize,
    log_path: Option<PathBuf>,
    header: String,
    sink: Arc<dyn EventSink>,
) -> Captured {
    let mut captured = Captured::default();
    let mut log = match &log_path {
        Some(path) => open_log(path, &header).await,
        None => None,
    };
    captured.log_written = log.is_some();
    let mut used = 0usize;
    let mut batch: Vec<OutputLine> = Vec::new();
    let mut seq = 0u64;
    let mut ticker = tokio::time::interval(BATCH_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let flush = |batch: &mut Vec<OutputLine>, seq: &mut u64| {
        if batch.is_empty() {
            return;
        }
        *seq += 1;
        sink.emit(&ProcessEvent::Output {
            run_id: run_id.clone(),
            seq: *seq,
            lines: std::mem::take(batch),
        });
    };

    loop {
        tokio::select! {
            line = rx.recv() => {
                let Some(line) = line else { break };
                let size = line.text.len() + 1;
                captured.bytes += size as u64;
                if captured.truncated {
                    continue; // Keep draining so the child never blocks on a full pipe.
                }
                if used + size > limit {
                    captured.truncated = true;
                    let marker = format!("[output truncated: {limit} byte limit reached; further output was discarded]");
                    if let Some(f) = log.as_mut() {
                        let _ = f.write_all(format!("{marker}\n").as_bytes()).await;
                    }
                    batch.push(OutputLine { stream: Stream::Stderr, text: marker });
                    flush(&mut batch, &mut seq);
                    sink.emit(&ProcessEvent::Truncated { run_id: run_id.clone(), limit_bytes: limit as u64 });
                    continue;
                }
                used += size;
                let target = match line.stream {
                    Stream::Stdout => &mut captured.stdout,
                    Stream::Stderr => &mut captured.stderr,
                };
                target.push_str(&line.text);
                target.push('\n');
                if let Some(f) = log.as_mut() {
                    let prefix = if line.stream == Stream::Stdout { "out" } else { "err" };
                    let _ = f.write_all(format!("[{prefix}] {}\n", line.text).as_bytes()).await;
                }
                batch.push(line);
                if batch.len() >= BATCH_MAX_LINES {
                    flush(&mut batch, &mut seq);
                }
            }
            _ = ticker.tick() => flush(&mut batch, &mut seq),
        }
    }
    flush(&mut batch, &mut seq);
    if let Some(mut f) = log {
        let _ = f.flush().await;
    }
    captured
}

async fn open_log(path: &Path, header: &str) -> Option<tokio::io::BufWriter<tokio::fs::File>> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await.ok()?;
    }
    let file = tokio::fs::File::create(path).await.ok()?;
    let mut writer = tokio::io::BufWriter::new(file);
    writer.write_all(header.as_bytes()).await.ok()?;
    Some(writer)
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::{CommandCategory, CommandSpec, ExecutionPolicy, Program, TrustRequirement};
    use super::*;

    #[derive(Default)]
    struct Collect(Mutex<Vec<ProcessEvent>>);

    impl EventSink for Collect {
        fn emit(&self, event: &ProcessEvent) {
            self.0.lock().unwrap().push(event.clone());
        }
    }

    struct Fixture {
        runner: ProcessRunner,
        search: SearchPath,
        logs: tempfile::TempDir,
        db: Arc<Database>,
    }

    fn fixture(categories: &[CommandCategory]) -> Fixture {
        let logs = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open_in_memory());
        let policy = ExecutionPolicy::allowing(
            categories,
            vec![std::env::temp_dir(), logs.path().to_path_buf()],
        );
        Fixture {
            runner: ProcessRunner::new(policy, Some(logs.path().to_path_buf()), Some(db.clone())),
            search: SearchPath {
                dirs: vec!["/bin".into(), "/usr/bin".into()],
                source: "test".into(),
            },
            logs,
            db,
        }
    }

    fn sh(script: &str) -> CommandSpec {
        let mut spec = CommandSpec::new(
            "test script",
            CommandCategory::Custom,
            Program::Named("sh".into()),
            &["-c", script],
        );
        spec.timeout = Duration::from_secs(10);
        spec
    }

    async fn run(f: &Fixture, spec: CommandSpec) -> (RunResult, Vec<ProcessEvent>) {
        let sink = Arc::new(Collect::default());
        let started = f.runner.start(spec, &f.search, sink.clone()).unwrap();
        let result = started.handle.await.unwrap();
        let events = sink.0.lock().unwrap().clone();
        (result, events)
    }

    fn activity_types(db: &Database) -> Vec<String> {
        db.read(|c| activity::list(c, &Default::default()))
            .unwrap()
            .items
            .into_iter()
            .rev()
            .map(|e| e.event_type)
            .collect()
    }

    #[tokio::test]
    async fn successful_command_streams_events_and_records_activity() {
        let f = fixture(&[CommandCategory::Custom]);
        let (result, events) = run(&f, sh("echo hello; echo world")).await;
        assert_eq!(result.status, RunStatus::Succeeded);
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout, "hello\nworld\n");
        assert!(result.error.is_none());
        assert!(matches!(events.first(), Some(ProcessEvent::Started { .. })));
        assert!(matches!(
            events.last(),
            Some(ProcessEvent::Completed { .. })
        ));
        let streamed: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                ProcessEvent::Output { lines, .. } => {
                    Some(lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(streamed, ["hello", "world"]);
        assert_eq!(
            activity_types(&f.db),
            ["process.started", "process.completed"]
        );
        let log = std::fs::read_to_string(result.log_path.unwrap()).unwrap();
        assert!(log.contains("[out] hello"));
        assert!(f.runner.active().is_empty());
    }

    #[tokio::test]
    async fn stdout_and_stderr_keep_their_identity() {
        let f = fixture(&[CommandCategory::Custom]);
        let (result, events) = run(&f, sh("echo out1; echo err1 >&2; echo out2")).await;
        assert_eq!(result.stdout, "out1\nout2\n");
        assert_eq!(result.stderr, "err1\n");
        let stderr_lines: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ProcessEvent::Output { lines, .. } => Some(lines.clone()),
                _ => None,
            })
            .flatten()
            .filter(|l| l.stream == Stream::Stderr)
            .map(|l| l.text)
            .collect();
        assert_eq!(stderr_lines, ["err1"]);
    }

    #[tokio::test]
    async fn non_zero_exit_is_failed_with_stderr_details() {
        let f = fixture(&[CommandCategory::Custom]);
        let (result, _) = run(&f, sh("echo broken >&2; exit 7")).await;
        assert_eq!(result.status, RunStatus::Failed);
        assert_eq!(result.exit_code, Some(7));
        let err = result.error.unwrap();
        assert_eq!(err.code, ErrorCode::CommandFailed);
        assert_eq!(err.technical_details.as_deref(), Some("broken"));
        assert_eq!(activity_types(&f.db), ["process.started", "process.failed"]);
    }

    #[tokio::test]
    async fn missing_executable_is_rejected_before_spawn() {
        let f = fixture(&[CommandCategory::Custom]);
        let spec = CommandSpec::new(
            "missing",
            CommandCategory::Custom,
            Program::Named("definitely-not-installed-xyz".into()),
            &[],
        );
        let err = f
            .runner
            .start(spec, &f.search, Arc::new(super::super::NullSink))
            .err()
            .unwrap();
        assert_eq!(err.code, ErrorCode::ExecutableNotFound);
        let abs = CommandSpec::new(
            "missing",
            CommandCategory::Custom,
            Program::Path("/nonexistent/bin/tool".into()),
            &[],
        );
        assert_eq!(
            f.runner
                .start(abs, &f.search, Arc::new(super::super::NullSink))
                .err()
                .unwrap()
                .code,
            ErrorCode::ExecutableNotFound
        );
        assert_eq!(
            activity_types(&f.db),
            ["process.rejected", "process.rejected"]
        );
    }

    #[tokio::test]
    async fn disallowed_categories_and_trust_are_denied() {
        let f = fixture(&[CommandCategory::EnvironmentCheck]);
        for category in [
            CommandCategory::ReadonlyGit,
            CommandCategory::MutatingGit,
            CommandCategory::DependencyInstall,
            CommandCategory::Build,
            CommandCategory::Test,
            CommandCategory::Lint,
            CommandCategory::Docker,
            CommandCategory::Devin,
            CommandCategory::Custom,
        ] {
            let spec = CommandSpec::new(
                "denied",
                category,
                Program::Named("sh".into()),
                &["-c", "echo no"],
            );
            let err = f
                .runner
                .start(spec, &f.search, Arc::new(super::super::NullSink))
                .err()
                .unwrap();
            assert_eq!(err.code, ErrorCode::CommandNotAllowed, "{category:?}");
        }
        let mut trusted = CommandSpec::new(
            "repo",
            CommandCategory::EnvironmentCheck,
            Program::Named("sh".into()),
            &[],
        );
        trusted.trust = TrustRequirement::TrustedRepository;
        assert_eq!(
            f.runner
                .start(trusted, &f.search, Arc::new(super::super::NullSink))
                .err()
                .unwrap()
                .code,
            ErrorCode::CommandNotAllowed
        );
        let mut path_env = sh("true");
        path_env.category = CommandCategory::EnvironmentCheck;
        path_env.env.push(("PATH".into(), "/evil".into()));
        assert_eq!(
            f.runner
                .start(path_env, &f.search, Arc::new(super::super::NullSink))
                .err()
                .unwrap()
                .code,
            ErrorCode::ValidationFailed
        );
    }

    #[tokio::test]
    async fn invalid_working_directories_are_rejected() {
        let f = fixture(&[CommandCategory::Custom]);
        let mut missing = sh("pwd");
        missing.cwd = Some(f.logs.path().join("does-not-exist"));
        assert_eq!(
            f.runner
                .start(missing, &f.search, Arc::new(super::super::NullSink))
                .err()
                .unwrap()
                .code,
            ErrorCode::PathNotAllowed
        );

        let mut outside = sh("pwd");
        outside.cwd = Some("/".into());
        assert_eq!(
            f.runner
                .start(outside, &f.search, Arc::new(super::super::NullSink))
                .err()
                .unwrap()
                .code,
            ErrorCode::PathNotAllowed
        );

        let file = f.logs.path().join("file.txt");
        std::fs::write(&file, "x").unwrap();
        let mut not_dir = sh("pwd");
        not_dir.cwd = Some(file);
        assert_eq!(
            f.runner
                .start(not_dir, &f.search, Arc::new(super::super::NullSink))
                .err()
                .unwrap()
                .code,
            ErrorCode::PathNotAllowed
        );

        let mut ok = sh("pwd");
        ok.cwd = Some(f.logs.path().to_path_buf());
        let (result, _) = run(&f, ok).await;
        let expected = std::fs::canonicalize(f.logs.path()).unwrap();
        assert_eq!(result.stdout.trim(), expected.display().to_string());
    }

    #[tokio::test]
    async fn timeout_terminates_the_process() {
        let f = fixture(&[CommandCategory::Custom]);
        let mut spec = sh("echo started; sleep 30");
        spec.timeout = Duration::from_millis(500);
        let begin = Instant::now();
        let (result, _) = run(&f, spec).await;
        assert!(begin.elapsed() < Duration::from_secs(5));
        assert_eq!(result.status, RunStatus::TimedOut);
        assert_eq!(result.error.unwrap().code, ErrorCode::CommandTimedOut);
        assert_eq!(result.stdout, "started\n");
    }

    fn alive(pid: i32) -> bool {
        // SAFETY: signal 0 only checks for existence.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[tokio::test]
    async fn cancellation_kills_process_group_and_is_idempotent() {
        let f = fixture(&[CommandCategory::Custom]);
        let pidfile = f.logs.path().join("grandchild.pid");
        // The shell starts a grandchild, records its pid, then waits.
        let script = format!(
            "sleep 300 & echo $! > '{}'; echo ready; wait",
            pidfile.display()
        );
        let sink = Arc::new(Collect::default());
        let started = f
            .runner
            .start(sh(&script), &f.search, sink.clone())
            .unwrap();
        let run_id = started.run_id.clone();
        for _ in 0..100 {
            if pidfile.exists() && !std::fs::read_to_string(&pidfile).unwrap().trim().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let grandchild: i32 = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(alive(grandchild));
        assert_eq!(f.runner.active().len(), 1);

        assert_eq!(f.runner.cancel(&run_id), CancelOutcome::Requested);
        assert_eq!(f.runner.cancel(&run_id), CancelOutcome::Requested);
        let result = started.handle.await.unwrap();
        assert_eq!(result.status, RunStatus::Cancelled);
        assert_eq!(result.error.unwrap().code, ErrorCode::CommandCancelled);
        assert_eq!(f.runner.cancel(&run_id), CancelOutcome::NotRunning);
        assert!(f.runner.active().is_empty());

        // The grandchild in the same process group is gone too.
        let mut gone = false;
        for _ in 0..50 {
            if !alive(grandchild) {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(gone, "grandchild {grandchild} survived cancellation");
        assert_eq!(
            activity_types(&f.db),
            ["process.started", "process.cancelled"]
        );
        assert_eq!(f.runner.cancel("never-existed"), CancelOutcome::NotRunning);
    }

    #[tokio::test]
    async fn large_output_is_truncated_with_marker_and_drained() {
        let f = fixture(&[CommandCategory::Custom]);
        // ~1.9 MB of output against a 64 KiB limit; must not block or blow memory.
        let mut spec = sh("i=0; while [ $i -lt 20000 ]; do echo \"line $i aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"; i=$((i+1)); done; echo done >&2");
        spec.max_output_bytes = 64 * 1024;
        let (result, events) = run(&f, spec).await;
        assert_eq!(result.status, RunStatus::Succeeded);
        assert!(result.output_truncated);
        assert!(result.stdout.len() <= 64 * 1024);
        assert!(result.output_bytes > 1_000_000);
        assert!(events
            .iter()
            .any(|e| matches!(e, ProcessEvent::Truncated { .. })));
        let marker = events.iter().any(|e| match e {
            ProcessEvent::Output { lines, .. } => lines
                .iter()
                .any(|l| l.text.starts_with("[output truncated")),
            _ => false,
        });
        assert!(marker);
        let log = std::fs::read_to_string(result.log_path.unwrap()).unwrap();
        assert!(log.contains("[output truncated"));
        assert!(log.len() < 100 * 1024);
    }

    #[tokio::test]
    async fn secrets_are_redacted_in_events_results_logs_and_activity() {
        let f = fixture(&[CommandCategory::Custom]);
        let mut spec = sh("echo \"token=ghp_abcdefghijklmnopqrstuvwxyz0123456789\"; echo \"Authorization: Bearer s3cr3tBearerValue\" >&2; echo \"$MY_SECRET\"");
        spec.env
            .push(("MY_SECRET".into(), "literal-secret-from-env".into()));
        spec.secrets.push("literal-secret-from-env".into());
        let (result, events) = run(&f, spec).await;
        let everything = format!(
            "{}{}{}{}",
            serde_json::to_string(&result).unwrap(),
            serde_json::to_string(&events).unwrap(),
            std::fs::read_to_string(result.log_path.as_ref().unwrap()).unwrap(),
            serde_json::to_string(
                &f.db
                    .read(|c| activity::list(c, &Default::default()))
                    .unwrap()
            )
            .unwrap(),
        );
        for secret in [
            "ghp_abcdefghij",
            "s3cr3tBearerValue",
            "literal-secret-from-env",
        ] {
            assert!(!everything.contains(secret), "{secret} leaked");
        }
        assert!(result.stdout.contains("[REDACTED"));
    }

    #[tokio::test]
    async fn environment_is_cleared_except_allow_list() {
        let f = fixture(&[CommandCategory::Custom]);
        std::env::set_var("SHIFTOPS_TEST_LEAK", "should-not-pass");
        let (result, _) = run(&f, sh("env")).await;
        assert!(!result.stdout.contains("SHIFTOPS_TEST_LEAK"));
        assert!(result.stdout.contains("PATH=/bin:/usr/bin"));
    }
}
