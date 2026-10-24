//! Evidence-backed readiness for the ShiftOps submission repository.
//! It uses the same exact read-only Git policy as discovery and reports only
//! secret rule names and file paths—never matched values.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::git::policy::{command_args, GIT_ENV};
use crate::process::{CommandCategory, CommandSpec, ExecutionPolicy, NullSink, Program, RunStatus};
use crate::shell_path;

const REQUIRED: &[&str] = &["README.md", "CHANGELOG.md", "AI_USAGE.md", "SUBMISSION.md"];
const MAX_TRACKED_FILES: usize = 20_000;
const MAX_FILE_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessStatus {
    NotReady,
    NeedsParticipantInput,
    ReadyForFinalVerification,
    #[allow(dead_code)] // Task 6 adds the explicit human-controlled transition.
    Submitted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretFinding {
    pub path: String,
    pub rule: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationEvidence {
    pub commit_sha: String,
    pub recorded_at: String,
    pub passed: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionReadiness {
    pub status: ReadinessStatus,
    pub source_root: String,
    pub required_files: BTreeMap<String, bool>,
    pub remote_url: Option<String>,
    pub head_sha: Option<String>,
    pub origin_main_sha: Option<String>,
    pub working_tree_clean: bool,
    pub commits_ahead: Option<u32>,
    pub pushed: bool,
    pub todo_count: u32,
    pub secret_findings: Vec<SecretFinding>,
    pub tracked_files_scanned: usize,
    pub validation: Option<ValidationEvidence>,
    pub final_sha: Option<String>,
    pub reasons: Vec<String>,
}

pub(crate) fn source_root() -> AppResult<PathBuf> {
    let configured = std::env::var_os("SHIFTOPS_SOURCE_ROOT").map(PathBuf::from);
    let compiled = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf);
    let candidate = configured
        .or(compiled)
        .ok_or_else(|| AppError::internal("ShiftOps source root is unavailable."))?;
    let canonical = std::fs::canonicalize(&candidate).map_err(|e| {
        AppError::internal(format!("ShiftOps source root cannot be opened: {e}"))
            .with_context("path", candidate.display().to_string())
    })?;
    if !canonical.join(".git").exists() {
        return Err(AppError::validation(
            "Submission Readiness requires the ShiftOps Git checkout.",
        )
        .with_context("path", canonical.display().to_string())
        .with_action("Set SHIFTOPS_SOURCE_ROOT to the ShiftOps repository checkout."));
    }
    Ok(canonical)
}

async fn git(root: &Path, command: &[&str]) -> AppResult<Option<String>> {
    let runner = crate::process::ProcessRunner::new(
        ExecutionPolicy::readonly_git(vec![root.to_path_buf()]),
        None,
        None,
    );
    let mut spec = CommandSpec::new(
        format!("git {}", command.first().copied().unwrap_or("read")),
        CommandCategory::ReadonlyGit,
        Program::Named("git".into()),
        &[],
    );
    spec.args = command_args(command);
    spec.cwd = Some(root.to_path_buf());
    spec.env = GIT_ENV
        .iter()
        .map(|(k, v)| ((*k).into(), (*v).into()))
        .collect();
    spec.timeout = Duration::from_secs(30);
    spec.max_output_bytes = 2 * 1024 * 1024;
    spec.record_activity = false;
    let started = runner.start(spec, shell_path::search_path().await, Arc::new(NullSink))?;
    let result = started
        .handle
        .await
        .map_err(|e| AppError::internal(format!("Git readiness task failed: {e}")))?;
    if result.status == RunStatus::Succeeded {
        Ok(Some(
            result
                .stdout
                .trim_matches(['\0', '\r', '\n', ' '])
                .to_string(),
        ))
    } else if result.exit_code == Some(1) {
        Ok(None)
    } else {
        Err(result
            .error
            .unwrap_or_else(|| AppError::internal(format!("{} failed.", result.label))))
    }
}

fn secret_findings(root: &Path, tracked: &str) -> (Vec<SecretFinding>, usize) {
    let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut findings = Vec::new();
    let mut scanned = 0;
    for relative in tracked
        .split('\0')
        .filter(|p| !p.is_empty())
        .take(MAX_TRACKED_FILES)
    {
        let path = canonical_root.join(relative);
        let Ok(canonical) = std::fs::canonicalize(&path) else {
            continue;
        };
        if !canonical.starts_with(&canonical_root) {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&canonical) else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(&canonical) else {
            continue;
        };
        if bytes.contains(&0) {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        scanned += 1;
        for line in text.lines() {
            let lower = line.to_ascii_lowercase();
            if [
                "test",
                "mock",
                "placeholder",
                "redact",
                "neverread",
                "not-a-real",
            ]
            .iter()
            .any(|word| lower.contains(word))
            {
                continue;
            }
            let rule = if lower.contains("-----begin private key-----") {
                Some("private_key")
            } else if lower.contains("github_pat_")
                || lower.contains("ghp_")
                || lower.contains("gho_")
            {
                Some("github_token")
            } else if lower.contains("authorization: bearer ") {
                Some("bearer_token")
            } else {
                None
            };
            if let Some(rule) = rule {
                findings.push(SecretFinding {
                    path: relative.to_string(),
                    rule: rule.into(),
                });
                break;
            }
        }
    }
    (findings, scanned)
}

fn validation(root: &Path) -> Option<ValidationEvidence> {
    let raw = std::fs::read_to_string(root.join(".shiftops/validation.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some(ValidationEvidence {
        commit_sha: value.get("commitSha")?.as_str()?.to_string(),
        recorded_at: value.get("recordedAt")?.as_str()?.to_string(),
        passed: value.get("passed")?.as_bool()?,
        summary: value.get("summary")?.as_str()?.chars().take(500).collect(),
    })
}

pub fn compute_status(readiness: &SubmissionReadiness) -> ReadinessStatus {
    if readiness.required_files.values().any(|present| !present)
        || readiness.remote_url.is_none()
        || readiness.head_sha.is_none()
        || !readiness.working_tree_clean
        || !readiness.pushed
        || !readiness.secret_findings.is_empty()
    {
        ReadinessStatus::NotReady
    } else if readiness.todo_count > 0 {
        ReadinessStatus::NeedsParticipantInput
    } else {
        // Submitted is intentionally never inferred. Task 6 must explicitly
        // record the final SHA after human submission.
        ReadinessStatus::ReadyForFinalVerification
    }
}

pub async fn collect(_db: &Database) -> AppResult<SubmissionReadiness> {
    let root = source_root()?;
    let required_files = REQUIRED
        .iter()
        .map(|name| ((*name).into(), root.join(name).is_file()))
        .collect();
    let remote_url = git(&root, &["remote", "get-url", "origin"])
        .await?
        .filter(|v| !v.is_empty());
    let head_sha = git(&root, &["rev-parse", "--verify", "--quiet", "HEAD"])
        .await?
        .filter(|v| !v.is_empty());
    let origin_main_sha = git(
        &root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/remotes/origin/main",
        ],
    )
    .await?
    .filter(|v| !v.is_empty());
    let dirty = git(
        &root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
    )
    .await?
    .unwrap_or_default();
    let commits_ahead = git(
        &root,
        &["rev-list", "--count", "refs/remotes/origin/main..HEAD"],
    )
    .await?
    .and_then(|v| v.parse::<u32>().ok());
    let tracked = git(&root, &["ls-files", "-z"]).await?.unwrap_or_default();
    let (secret_findings, tracked_files_scanned) = secret_findings(&root, &tracked);
    let todo_count = std::fs::read_to_string(root.join("SUBMISSION.md"))
        .map(|s| s.matches("TODO").count().min(u32::MAX as usize) as u32)
        .unwrap_or(0);
    let pushed = head_sha.is_some() && head_sha == origin_main_sha && commits_ahead == Some(0);
    let mut readiness = SubmissionReadiness {
        status: ReadinessStatus::NotReady,
        source_root: root.display().to_string(),
        required_files,
        remote_url,
        head_sha,
        origin_main_sha,
        working_tree_clean: dirty.is_empty(),
        commits_ahead,
        pushed,
        todo_count,
        secret_findings,
        tracked_files_scanned,
        validation: validation(&root),
        final_sha: None,
        reasons: Vec::new(),
    };
    if !readiness.working_tree_clean {
        readiness
            .reasons
            .push("The working tree has uncommitted changes.".into());
    }
    if !readiness.pushed {
        readiness
            .reasons
            .push("HEAD is not equal to origin/main.".into());
    }
    if readiness.todo_count > 0 {
        readiness.reasons.push(format!(
            "{} participant TODOs remain.",
            readiness.todo_count
        ));
    }
    if !readiness.secret_findings.is_empty() {
        readiness.reasons.push(format!(
            "{} potential tracked-secret findings need review.",
            readiness.secret_findings.len()
        ));
    }
    readiness.status = compute_status(&readiness);
    Ok(readiness)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> SubmissionReadiness {
        SubmissionReadiness {
            status: ReadinessStatus::NotReady,
            source_root: "/repo".into(),
            required_files: REQUIRED.iter().map(|v| ((*v).into(), true)).collect(),
            remote_url: Some("https://github.com/acme/x.git".into()),
            head_sha: Some("abc".into()),
            origin_main_sha: Some("abc".into()),
            working_tree_clean: true,
            commits_ahead: Some(0),
            pushed: true,
            todo_count: 0,
            secret_findings: vec![],
            tracked_files_scanned: 1,
            validation: None,
            final_sha: None,
            reasons: vec![],
        }
    }

    #[test]
    fn status_is_deterministic_and_never_auto_submitted() {
        let mut r = base();
        assert_eq!(
            compute_status(&r),
            ReadinessStatus::ReadyForFinalVerification
        );
        r.todo_count = 2;
        assert_eq!(compute_status(&r), ReadinessStatus::NeedsParticipantInput);
        r.todo_count = 0;
        r.working_tree_clean = false;
        assert_eq!(compute_status(&r), ReadinessStatus::NotReady);
    }

    #[test]
    fn secret_scan_reports_rule_and_path_not_value() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(
            t.path().join("config.txt"),
            "authorization: bearer productionvalue123456",
        )
        .unwrap();
        let (findings, scanned) = secret_findings(t.path(), "config.txt\0");
        assert_eq!(scanned, 1);
        assert_eq!(
            findings,
            vec![SecretFinding {
                path: "config.txt".into(),
                rule: "bearer_token".into()
            }]
        );
        assert!(!serde_json::to_string(&findings)
            .unwrap()
            .contains("productionvalue"));
    }

    #[tokio::test]
    async fn collector_reads_the_real_checkout_without_exposing_values() {
        let readiness = collect(&Database::open_in_memory()).await.unwrap();
        assert!(readiness.required_files.values().all(|present| *present));
        assert!(readiness
            .remote_url
            .as_deref()
            .is_some_and(|url| url.contains("ShiftOps")));
        assert!(readiness
            .head_sha
            .as_deref()
            .is_some_and(|sha| sha.len() >= 7));
        assert!(readiness.tracked_files_scanned > 20);
        let json = serde_json::to_string(&readiness).unwrap();
        assert!(!json.contains("productionvalue"));
        assert!(readiness.final_sha.is_none());
        assert_ne!(readiness.status, ReadinessStatus::Submitted);
    }
}
