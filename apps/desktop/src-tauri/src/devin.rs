//! Devin CLI integration: detection, authentication status and
//! approval-gated task dispatch.
//!
//! Only documented local CLI capabilities are used: `devin version`,
//! `devin auth status` and `devin --print <prompt>` for a non-interactive
//! local session. The CLI keeps its own credentials; ShiftOps stores none.
//! Dispatched sessions run with `--sandbox` and `--permission-mode smart`,
//! so Devin's own seatbelt sandbox constrains it to the repository working
//! directory.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::process::{
    CommandCategory, CommandSpec, ExecutionPolicy, NullSink, ProcessRunner, Program, RunStatus,
};
use crate::shell_path;

/// Argument vectors the status check may run, exactly.
const STATUS_COMMANDS: &[&[&str]] = &[&["version"], &["auth", "status"], &["doctor"]];

/// Fixed flags every dispatched Devin task carries. The prompt is the only
/// variable argument and always sits directly after `-p`.
const DISPATCH_FLAGS: &[&str] = &[
    "--permission-mode",
    "smart",
    "--sandbox",
    "--respect-workspace-trust",
    "false",
];

/// Largest prompt accepted as one argument; matches the stored-instruction
/// limit and stays well below the platform argument-size limits.
const MAX_PROMPT_BYTES: usize = 200_000;

/// Reject a prompt the CLI could parse as something other than the value of
/// `-p`: empty or whitespace-only text, a first non-whitespace character of
/// `-` (an option such as `--help` or `-p`), NUL bytes, or oversize text.
/// Hyphens anywhere after the first meaningful character are fine.
pub fn validate_prompt(prompt: &str) -> AppResult<()> {
    let deny = |why: &str| {
        Err(AppError::new(
            ErrorCode::CommandNotAllowed,
            format!("Devin prompt not permitted: {why}."),
        )
        .with_context("promptBytes", prompt.len().to_string()))
    };
    let meaningful = prompt.trim_start();
    if meaningful.is_empty() {
        return deny("the prompt is empty");
    }
    if meaningful.starts_with('-') {
        return deny("the prompt must not begin with '-', which the CLI would read as an option");
    }
    if prompt.contains('\0') {
        return deny("the prompt contains a NUL byte");
    }
    if prompt.len() > MAX_PROMPT_BYTES {
        return deny("the prompt is too long");
    }
    Ok(())
}

/// Build the exact `devin` argument vector for a dispatch.
pub fn dispatch_args(prompt: &str) -> Vec<String> {
    let mut args = vec!["-p".to_string(), prompt.to_string()];
    args.extend(DISPATCH_FLAGS.iter().map(|s| (*s).to_string()));
    args
}

/// Accept only the status allow-list or the exact dispatch vector.
pub fn validate_args(args: &[String]) -> AppResult<()> {
    let deny = |why: &str| {
        Err(AppError::new(
            ErrorCode::CommandNotAllowed,
            format!("Devin command not permitted: {why}."),
        )
        .with_context("args", args.join(" ")))
    };
    if STATUS_COMMANDS
        .iter()
        .any(|allowed| args.iter().map(String::as_str).eq(allowed.iter().copied()))
    {
        return Ok(());
    }
    let shape_ok = args.len() == 2 + DISPATCH_FLAGS.len()
        && args[0] == "-p"
        && args[2..]
            .iter()
            .map(String::as_str)
            .eq(DISPATCH_FLAGS.iter().copied());
    if shape_ok {
        return validate_prompt(&args[1]);
    }
    deny("only version/auth-status/doctor or the documented dispatch vector may run")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevinStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub authenticated: bool,
    pub account_email: Option<String>,
    pub detail: String,
}

impl DevinStatus {
    fn not_installed() -> Self {
        Self {
            installed: false,
            version: None,
            authenticated: false,
            account_email: None,
            detail: "Devin CLI was not found on the login-shell PATH.".into(),
        }
    }
}

async fn run(args: &[&str]) -> AppResult<Option<String>> {
    let runner = ProcessRunner::new(
        ExecutionPolicy::devin_cli(vec![std::env::temp_dir()]),
        None,
        None,
    );
    let mut spec = CommandSpec::new(
        format!("devin {}", args.join(" ")),
        CommandCategory::Devin,
        Program::Named("devin".into()),
        &[],
    );
    spec.args = args.iter().map(|s| (*s).to_string()).collect();
    spec.timeout = Duration::from_secs(30);
    spec.max_output_bytes = 64 * 1024;
    spec.record_activity = false;
    let search = shell_path::search_path().await;
    let started = runner.start(spec, search, Arc::new(NullSink))?;
    let result = started
        .handle
        .await
        .map_err(|e| AppError::internal(format!("Devin status task failed: {e}")))?;
    if result.status == RunStatus::Succeeded {
        Ok(Some(result.stdout))
    } else {
        Ok(None)
    }
}

/// Detect the CLI and, when present, whether `devin auth status` reports a
/// logged-in account. Never reports or persists credentials.
pub async fn status() -> AppResult<DevinStatus> {
    let version = match run(&["version"]).await? {
        Some(v) => v.trim().to_string(),
        None => return Ok(DevinStatus::not_installed()),
    };
    let auth = run(&["auth", "status"]).await?.unwrap_or_default();
    let authenticated = auth.contains("Logged in");
    let account_email = auth
        .lines()
        .find_map(|l| l.trim().strip_prefix("Email:").map(str::trim))
        .map(|s| s.to_string());
    Ok(DevinStatus {
        installed: true,
        version: Some(version),
        authenticated,
        account_email,
        detail: if authenticated {
            "Devin CLI is installed and authenticated.".into()
        } else {
            "Devin CLI is installed but not authenticated; run `devin auth login`.".into()
        },
    })
}

/// Everything an instruction needs from the campaign, member and evidence.
pub struct TaskContext<'a> {
    pub campaign_name: &'a str,
    pub objective: &'a str,
    pub migration_type: &'a str,
    pub repository_name: &'a str,
    pub local_path: &'a str,
    pub github: Option<&'a str>,
    pub current_branch: Option<&'a str>,
    pub evidence: &'a [String],
    pub verification_commands: &'a [String],
    pub constraints: &'a [String],
}

/// The deterministic Devin task body. Approval and the stored hash refer to
/// this exact text; verification commands run in Task 5, not here.
pub fn build_instructions(ctx: &TaskContext) -> String {
    let mut s = String::with_capacity(4096);
    s.push_str(&format!(
        "# ShiftOps task: {}\n\n## Repository\n- Name: {}\n- Checkout: {}\n",
        ctx.campaign_name, ctx.repository_name, ctx.local_path
    ));
    if let Some(g) = ctx.github {
        s.push_str(&format!("- GitHub: {g}\n"));
    }
    if let Some(b) = ctx.current_branch {
        s.push_str(&format!("- Branch at plan time: {b}\n"));
    }
    s.push_str(&format!(
        "\n## Approved goal\n{}\n\nMigration type: {}\n",
        ctx.objective, ctx.migration_type
    ));
    s.push_str("\n## Evidence from the ShiftOps scan\n");
    for e in ctx.evidence {
        s.push_str(&format!("- {e}\n"));
    }
    s.push_str(
        "\n## Scope\n- Make only the change described by the approved goal, in this checkout.\n",
    );
    for c in ctx.constraints {
        s.push_str(&format!("- {c}\n"));
    }
    s.push_str(
        "\n## Acceptance criteria\n- The migration goal is met inside this repository only.\n- No unrelated refactors, formatting sweeps or dependency changes.\n- No secrets, credentials or tokens are added, read from unrelated files or committed.\n",
    );
    if !ctx.verification_commands.is_empty() {
        s.push_str("\n## Verification (run later by ShiftOps)\nLeave the working tree so these commands can be run and reported on:\n");
        for v in ctx.verification_commands {
            s.push_str(&format!("- `{v}`\n"));
        }
    }
    s.push_str("\n## Prohibitions\n- Do not change repositories outside this checkout.\n- Do not push, open pull requests or change remotes.\n- Do not modify CI secrets, keychain entries or ShiftOps files.\n");
    s
}

/// Dispatch spec for a task. `cwd` is the repository checkout and trust is
/// `TrustedRepository`; the command layer verifies the repository's trust
/// flag before building this. The prompt is validated here and again by the
/// execution policy when the runner starts the process.
pub fn dispatch_spec(prompt: &str, checkout: &Path) -> AppResult<CommandSpec> {
    validate_prompt(prompt)?;
    let mut spec = CommandSpec::new(
        "devin task",
        CommandCategory::Devin,
        Program::Named("devin".into()),
        &[],
    );
    spec.args = dispatch_args(prompt);
    spec.trust = crate::process::TrustRequirement::TrustedRepository;
    spec.cwd = Some(checkout.to_path_buf());
    spec.timeout = Duration::from_secs(2 * 60 * 60);
    spec.max_output_bytes = 4 * 1024 * 1024;
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn status_commands_and_dispatch_are_the_only_allowed_vectors() {
        for args in STATUS_COMMANDS {
            validate_args(&args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>()).unwrap();
        }
        validate_args(&dispatch_args("do the thing")).unwrap();
        for bad in [
            v(&["-p"]),
            v(&["-p", "x", "--dangerous"]),
            v(&["cloud", "drs"]),
            v(&["rm", "x"]),
            v(&["mcp"]),
            v(&[
                "-p",
                "",
                "--permission-mode",
                "smart",
                "--sandbox",
                "--respect-workspace-trust",
                "false",
            ]),
        ] {
            assert_eq!(
                validate_args(&bad).unwrap_err().code,
                ErrorCode::CommandNotAllowed,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn dispatch_policy_rejects_other_programs_flags_and_categories() {
        let dir = tempfile::tempdir().unwrap();
        let checkout = std::fs::canonicalize(dir.path()).unwrap();
        let policy = ExecutionPolicy::devin_dispatch(checkout.clone());
        policy
            .check(&dispatch_spec("goal", &checkout).unwrap())
            .unwrap();

        let mut sh = dispatch_spec("goal", &checkout).unwrap();
        sh.program = Program::Named("sh".into());
        assert_eq!(
            policy.check(&sh).unwrap_err().code,
            ErrorCode::CommandNotAllowed
        );

        let mut extra = dispatch_spec("goal", &checkout).unwrap();
        extra.args.push("--permission-mode".into());
        extra.args.push("dangerous".into());
        assert_eq!(
            policy.check(&extra).unwrap_err().code,
            ErrorCode::CommandNotAllowed
        );

        // A non-Devin category stays denied under the Devin policy.
        let mut git = dispatch_spec("goal", &checkout).unwrap();
        git.category = CommandCategory::MutatingGit;
        assert_eq!(
            policy.check(&git).unwrap_err().code,
            ErrorCode::CommandNotAllowed
        );
    }

    fn dispatch_vector(prompt: &str) -> Vec<String> {
        let mut a = v(&["-p", prompt]);
        a.extend(DISPATCH_FLAGS.iter().map(|s| (*s).to_string()));
        a
    }

    #[test]
    fn option_like_prompts_are_rejected_everywhere() {
        let dir = tempfile::tempdir().unwrap();
        let checkout = std::fs::canonicalize(dir.path()).unwrap();
        let policy = ExecutionPolicy::devin_dispatch(checkout.clone());
        for bad in [
            "-p",
            "--help",
            "--permission-mode=dangerous",
            "-",
            "--",
            "   -p",
            "\t--help",
            "\n\n- list item first",
            " \r\n --version",
            "",
            "   ",
            "\t\n ",
            "goal\0--dangerous",
        ] {
            assert_eq!(
                validate_prompt(bad).unwrap_err().code,
                ErrorCode::CommandNotAllowed,
                "validate_prompt {bad:?}"
            );
            // Hand-built vector: the policy (the runner's only gate) refuses it.
            assert_eq!(
                validate_args(&dispatch_vector(bad)).unwrap_err().code,
                ErrorCode::CommandNotAllowed,
                "validate_args {bad:?}"
            );
            // The constructor refuses it before a spec exists.
            assert!(
                dispatch_spec(bad, &checkout).is_err(),
                "dispatch_spec {bad:?}"
            );
            // A spec built some other way is still refused by the policy.
            let mut spec = dispatch_spec("goal", &checkout).unwrap();
            spec.args = dispatch_vector(bad);
            assert_eq!(
                policy.check(&spec).unwrap_err().code,
                ErrorCode::CommandNotAllowed,
                "policy {bad:?}"
            );
        }
        assert!(validate_prompt(&"x".repeat(MAX_PROMPT_BYTES + 1)).is_err());
    }

    #[test]
    fn prompts_with_inner_hyphens_are_accepted() {
        for ok in [
            "Do the follow-up work",
            "Ship the AI-powered planner",
            "Lower the threshold to -5 degrees",
            "Use --sandbox semantics described in the docs",
            "  Upgrade Node.js 18 to Node.js 22 - carefully",
            "x",
        ] {
            validate_prompt(ok).unwrap();
            validate_args(&dispatch_args(ok)).unwrap();
        }
    }

    #[test]
    fn generated_shiftops_instructions_are_valid_prompts() {
        let ctx = TaskContext {
            campaign_name: "-leading-dash-campaign",
            objective: "--help",
            migration_type: "runtime_upgrade",
            repository_name: "-p",
            local_path: "/work/x",
            github: None,
            current_branch: None,
            evidence: &["node=18.x (package.json/engines/node)".into()],
            verification_commands: &["pnpm run test".into()],
            constraints: &[],
        };
        let prompt = build_instructions(&ctx);
        assert!(prompt.starts_with("# ShiftOps task"));
        validate_prompt(&prompt).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let checkout = std::fs::canonicalize(dir.path()).unwrap();
        let spec = dispatch_spec(&prompt, &checkout).unwrap();
        ExecutionPolicy::devin_dispatch(checkout)
            .check(&spec)
            .unwrap();
    }

    /// Manual: probes the real local Devin CLI (read-only `version` and
    /// `auth status`). Never dispatches.
    #[tokio::test]
    #[ignore = "manual: needs the Devin CLI on PATH"]
    async fn status_rehearsal() {
        let s = status().await.unwrap();
        println!("{s:?}");
        assert!(s.installed);
    }

    #[test]
    fn instructions_carry_goal_evidence_scope_and_prohibitions() {
        let ctx = TaskContext {
            campaign_name: "Node 22",
            objective: "Upgrade Node.js 18 to Node.js 22",
            migration_type: "runtime_upgrade",
            repository_name: "orders-api",
            local_path: "/work/orders-api",
            github: Some("acme/orders-api"),
            current_branch: Some("main"),
            evidence: &["node=18.x (package.json/engines/node)".into()],
            verification_commands: &["pnpm test".into()],
            constraints: &["Keep pnpm lockfile".into()],
        };
        let s = build_instructions(&ctx);
        assert!(s.contains("orders-api"));
        assert!(s.contains("Upgrade Node.js 18 to Node.js 22"));
        assert!(s.contains("node=18.x"));
        assert!(s.contains("pnpm test"));
        assert!(s.contains("No unrelated refactors"));
        assert!(s.contains("Do not push"));
    }
}
