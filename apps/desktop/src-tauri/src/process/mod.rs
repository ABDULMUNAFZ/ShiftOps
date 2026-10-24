//! Safe native process execution.
//!
//! Every process ShiftOps starts is described by a `CommandSpec`: a resolved
//! executable, an argument array (never a shell string), a validated working
//! directory, an allow-listed environment, a timeout, an output limit, a
//! category and a trust requirement. `ExecutionPolicy` decides which
//! categories may run; in Step 2 only `environment_check` is permitted.

mod runner;

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use runner::{ActiveProcess, CancelOutcome, ProcessRunner};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::paths::PathPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandCategory {
    EnvironmentCheck,
    ReadonlyGit,
    MutatingGit,
    DependencyInstall,
    Build,
    Test,
    Lint,
    Docker,
    Devin,
    Custom,
}

impl CommandCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EnvironmentCheck => "environment_check",
            Self::ReadonlyGit => "readonly_git",
            Self::MutatingGit => "mutating_git",
            Self::DependencyInstall => "dependency_install",
            Self::Build => "build",
            Self::Test => "test",
            Self::Lint => "lint",
            Self::Docker => "docker",
            Self::Devin => "devin",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustRequirement {
    /// Runs without reference to a repository (environment checks).
    None,
    /// Runs inside a repository the user has marked trusted.
    TrustedRepository,
}

#[derive(Debug, Clone)]
pub enum Program {
    /// Resolved on the login-shell search path.
    Named(String),
    /// An absolute path, e.g. the ShiftOps executable for bundled diagnostics.
    Path(PathBuf),
}

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub label: String,
    pub category: CommandCategory,
    pub trust: TrustRequirement,
    pub program: Program,
    pub args: Vec<String>,
    /// Defaults to the OS temp directory: a neutral location, so tools such
    /// as Corepack shims are not affected by a nearby `package.json`.
    pub cwd: Option<PathBuf>,
    /// Extra variables on top of the inherited allow-list.
    pub env: Vec<(String, String)>,
    /// Literal secrets passed to the process that must be masked in output.
    pub secrets: Vec<String>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
    /// Record start/finish in the activity log.
    pub record_activity: bool,
}

impl CommandSpec {
    pub fn new(
        label: impl Into<String>,
        category: CommandCategory,
        program: Program,
        args: &[&str],
    ) -> Self {
        Self {
            label: label.into(),
            category,
            trust: TrustRequirement::None,
            program,
            args: args.iter().map(|s| (*s).to_string()).collect(),
            cwd: None,
            env: Vec::new(),
            secrets: Vec::new(),
            timeout: Duration::from_secs(120),
            max_output_bytes: 1024 * 1024,
            record_activity: true,
        }
    }
}

/// Environment variables inherited from the ShiftOps process. Everything
/// else is cleared; `PATH` is replaced with the resolved search path.
pub const INHERITED_ENV: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TMPDIR",
    "TZ",
    // Windows essentials.
    "SYSTEMROOT",
    "WINDIR",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "PATHEXT",
    "COMSPEC",
    "TEMP",
    "TMP",
];

#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    allowed: HashSet<CommandCategory>,
    /// Directories a working directory may be inside.
    cwd_roots: PathPolicy,
}

impl ExecutionPolicy {
    /// Step 2 policy: environment checks only, run from the temp directory or
    /// the application data directory. Repository commands, installs, builds,
    /// tests, Docker, Devin and custom commands are denied until later steps
    /// add explicit, trust-aware policies.
    pub fn step2(data_dir: PathBuf) -> Self {
        Self {
            allowed: HashSet::from([CommandCategory::EnvironmentCheck]),
            cwd_roots: PathPolicy::new([std::env::temp_dir(), data_dir]),
        }
    }

    /// Discovery and scanning policy: read-only Git (exact commands from
    /// `git::policy`) with the working directory inside one of `roots`.
    /// Everything else stays denied.
    pub fn readonly_git(roots: Vec<PathBuf>) -> Self {
        Self {
            allowed: HashSet::from([CommandCategory::ReadonlyGit]),
            cwd_roots: PathPolicy::new(roots),
        }
    }

    #[cfg(test)]
    pub fn allowing(categories: &[CommandCategory], roots: Vec<PathBuf>) -> Self {
        Self {
            allowed: categories.iter().copied().collect(),
            cwd_roots: PathPolicy::new(roots),
        }
    }

    pub fn check(&self, spec: &CommandSpec) -> AppResult<()> {
        if !self.allowed.contains(&spec.category) {
            return Err(AppError::new(
                ErrorCode::CommandNotAllowed,
                format!(
                    "{} commands are not enabled in this version of ShiftOps.",
                    spec.category.as_str()
                ),
            )
            .with_context("category", spec.category.as_str())
            .with_context("label", spec.label.as_str()));
        }
        if spec.trust == TrustRequirement::TrustedRepository {
            return Err(AppError::new(
                ErrorCode::CommandNotAllowed,
                "Commands inside repositories are not enabled until repository trust is implemented.",
            )
            .with_context("category", spec.category.as_str()));
        }
        if spec.category == CommandCategory::ReadonlyGit {
            if !matches!(&spec.program, Program::Named(name) if name == "git") {
                return Err(AppError::new(
                    ErrorCode::CommandNotAllowed,
                    "readonly_git commands must run `git`.",
                ));
            }
            crate::git::policy::validate(&spec.args)?;
        }
        for (name, _) in &spec.env {
            let valid = !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.starts_with(|c: char| c.is_ascii_digit());
            if !valid || name.eq_ignore_ascii_case("PATH") {
                return Err(AppError::validation(format!(
                    "Environment variable name not allowed: {name}"
                )));
            }
        }
        if spec.timeout.is_zero() || spec.max_output_bytes == 0 {
            return Err(AppError::validation(
                "Timeout and output limit must be positive.",
            ));
        }
        Ok(())
    }

    pub fn cwd_roots(&self) -> &PathPolicy {
        &self.cwd_roots
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputLine {
    pub stream: Stream,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub run_id: String,
    pub label: String,
    pub category: CommandCategory,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub started_at: String,
    pub completed_at: String,
    /// Redacted captured output, up to the output limit.
    pub stdout: String,
    pub stderr: String,
    pub output_bytes: u64,
    pub output_limit_bytes: u64,
    pub output_truncated: bool,
    pub log_path: Option<String>,
    pub error: Option<AppError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProcessEvent {
    Started {
        run_id: String,
        label: String,
        category: CommandCategory,
        program: String,
        args: Vec<String>,
        cwd: String,
        pid: Option<u32>,
        started_at: String,
    },
    Output {
        run_id: String,
        /// Monotonic per run, so the UI can detect gaps.
        seq: u64,
        lines: Vec<OutputLine>,
    },
    Truncated {
        run_id: String,
        limit_bytes: u64,
    },
    Completed {
        result: RunResult,
    },
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: &ProcessEvent);
}

pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: &ProcessEvent) {}
}

pub const TAURI_EVENT: &str = "shiftops://process";

pub struct TauriSink<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> EventSink for TauriSink<R> {
    fn emit(&self, event: &ProcessEvent) {
        use tauri::Emitter;
        if let Err(e) = self.0.emit(TAURI_EVENT, event) {
            crate::logging::warn("process", &format!("failed to emit process event: {e}"));
        }
    }
}

/// Bundled harmless long-running diagnostic: `shiftops-desktop --shiftops-diagnostic-ticker`.
pub const DIAGNOSTIC_TICKER_FLAG: &str = "--shiftops-diagnostic-ticker";

/// Body of the bundled diagnostic. Prints one line every 500 ms for 60 s,
/// with every fifth line on stderr and one line that exercises redaction.
pub fn run_diagnostic_ticker() {
    use std::io::Write;
    let total = 120;
    let pid = std::process::id();
    println!(
        "ShiftOps diagnostic ticker started (pid {pid}). It prints {total} lines over 60 seconds."
    );
    for i in 1..=total {
        std::thread::sleep(Duration::from_millis(500));
        if i % 5 == 0 {
            eprintln!("[{i:03}/{total}] stderr sample line");
        } else {
            println!("[{i:03}/{total}] stdout tick");
        }
        if i == 3 {
            // A synthetic credential to prove redaction happens before display and persistence.
            println!("redaction self-test: Authorization: Bearer shiftops-diagnostic-{pid}-not-a-real-token");
        }
        let _ = std::io::stdout().flush();
    }
    println!("ShiftOps diagnostic ticker finished.");
}
