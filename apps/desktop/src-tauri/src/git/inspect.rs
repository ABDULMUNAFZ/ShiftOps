//! Read-only inspection of a local Git repository. Reads files under `.git`
//! directly where possible and runs only `policy::READONLY_COMMANDS`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::policy::{
    command_args, BRANCHES_COMMAND, CURRENT_BRANCH_COMMAND, DEFAULT_BRANCH_COMMAND, GIT_ENV,
    HEAD_COMMAND, LAST_COMMIT_COMMAND, ORIGIN_URL_COMMAND, STATUS_COMMAND,
};
use super::remote::{parse_remote, RemoteInfo};
use crate::process::{CommandCategory, CommandSpec, ProcessRunner, Program, RunStatus};
use crate::shell_path::SearchPath;

const MAX_CONFIG_BYTES: u64 = 256 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(20);
const GIT_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitKind {
    /// `.git` is a directory.
    Standard,
    /// `.git` is a file pointing into `<main>/.git/worktrees/<name>`.
    Worktree,
    /// `.git` is a file pointing into `<superproject>/.git/modules/<name>`.
    Submodule,
    /// `.git` is a file pointing somewhere else.
    GitFile,
}

impl GitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Worktree => "worktree",
            Self::Submodule => "submodule",
            Self::GitFile => "git_file",
        }
    }
}

/// Identify a repository marker in `dir`. Returns the kind and the resolved
/// git directory, or `None` when `dir/.git` is absent or unreadable.
pub fn detect_marker(dir: &Path) -> Option<Result<(GitKind, PathBuf), String>> {
    let marker = dir.join(".git");
    let meta = std::fs::symlink_metadata(&marker).ok()?;
    if meta.is_dir() {
        return Some(Ok((GitKind::Standard, marker)));
    }
    if !meta.is_file() {
        return Some(Err("`.git` is neither a directory nor a file".into()));
    }
    if meta.len() > 4096 {
        return Some(Err("`.git` file is unexpectedly large".into()));
    }
    let text = match std::fs::read_to_string(&marker) {
        Ok(t) => t,
        Err(e) => return Some(Err(format!("`.git` file is unreadable: {e}"))),
    };
    let Some(target) = text
        .lines()
        .find_map(|l| l.strip_prefix("gitdir:"))
        .map(str::trim)
    else {
        return Some(Err("`.git` file has no `gitdir:` line".into()));
    };
    let target = Path::new(target);
    let git_dir = if target.is_absolute() {
        target.to_path_buf()
    } else {
        dir.join(target)
    };
    let normalized = git_dir.to_string_lossy().replace('\\', "/");
    let kind = if normalized.contains("/worktrees/") {
        GitKind::Worktree
    } else if normalized.contains("/modules/") {
        GitKind::Submodule
    } else {
        GitKind::GitFile
    };
    Some(Ok((kind, git_dir)))
}

/// Config files that apply to a repository: its own, the worktree's, and,
/// for linked worktrees, the shared one named by `commondir`.
fn config_files(git_dir: &Path) -> Vec<PathBuf> {
    let mut files = vec![git_dir.join("config"), git_dir.join("config.worktree")];
    if let Ok(common) = std::fs::read_to_string(git_dir.join("commondir")) {
        let common = common.trim();
        let common_dir = if Path::new(common).is_absolute() {
            PathBuf::from(common)
        } else {
            git_dir.join(common)
        };
        files.push(common_dir.join("config"));
    }
    files
}

/// Keys in a repository's own config that make `git status` run external
/// programs (clean/smudge/process filters) or pull in configuration from
/// elsewhere. When any are present, status is not collected.
pub fn risky_config(git_dir: &Path) -> Option<String> {
    for file in config_files(git_dir) {
        let Ok(meta) = std::fs::metadata(&file) else {
            continue;
        };
        if meta.len() > MAX_CONFIG_BYTES {
            return Some(format!(
                "{} is larger than {} KiB",
                file.display(),
                MAX_CONFIG_BYTES / 1024
            ));
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with(';') || line.is_empty() {
                continue;
            }
            if let Some(header) = line.strip_prefix('[') {
                section = header
                    .split(']')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                if section == "include" || section.starts_with("includeif") {
                    return Some(format!(
                        "{} includes other configuration files",
                        file.display()
                    ));
                }
                continue;
            }
            let key = line
                .split('=')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if section.starts_with("filter")
                && matches!(key.as_str(), "clean" | "smudge" | "process")
            {
                return Some(format!(
                    "{} defines a `{section}` {key} command",
                    file.display()
                ));
            }
            if section == "core" && key == "fsmonitor" {
                // Overridden by SAFE_PREFIX; noted only because it signals intent.
                continue;
            }
        }
    }
    None
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingTreeCounts {
    pub staged: u32,
    pub modified: u32,
    pub untracked: u32,
}

impl WorkingTreeCounts {
    pub fn dirty(&self) -> bool {
        self.staged + self.modified + self.untracked > 0
    }
}

/// Parse `git status --porcelain=v1 -z` output.
pub fn parse_status(output: &str) -> WorkingTreeCounts {
    let mut counts = WorkingTreeCounts::default();
    let mut entries = output.split('\0').filter(|e| !e.is_empty());
    while let Some(entry) = entries.next() {
        let bytes = entry.as_bytes();
        if bytes.len() < 3 {
            continue;
        }
        let (x, y) = (bytes[0] as char, bytes[1] as char);
        if x == '?' && y == '?' {
            counts.untracked += 1;
            continue;
        }
        if x == '!' {
            continue;
        }
        if x != ' ' {
            counts.staged += 1;
        }
        if y != ' ' {
            counts.modified += 1;
        }
        // Renames and copies carry the original path as a separate entry.
        if matches!(x, 'R' | 'C') {
            entries.next();
        }
    }
    counts
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitMetadata {
    pub current_branch: Option<String>,
    pub detached: bool,
    pub head_commit: Option<String>,
    pub default_branch: Option<String>,
    pub origin: Option<RemoteInfo>,
    pub last_commit_at: Option<String>,
    /// `None` when status was not collected (see `warnings`).
    pub working_tree: Option<WorkingTreeCounts>,
    pub warnings: Vec<String>,
}

struct Git<'a> {
    runner: &'a ProcessRunner,
    search: &'a SearchPath,
    cwd: &'a Path,
}

impl Git<'_> {
    /// `Ok(Some(stdout))` on exit 0, `Ok(None)` on a non-zero exit (for
    /// example no `origin`), `Err` when Git could not run at all.
    async fn run(&self, command: &[&str]) -> Result<Option<String>, String> {
        let mut spec = CommandSpec::new(
            format!("git {}", command.first().unwrap_or(&"")),
            CommandCategory::ReadonlyGit,
            Program::Named("git".into()),
            &[],
        );
        spec.args = command_args(command);
        spec.cwd = Some(self.cwd.to_path_buf());
        spec.env = GIT_ENV
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        spec.timeout = GIT_TIMEOUT;
        spec.max_output_bytes = GIT_OUTPUT_LIMIT;
        spec.record_activity = false;
        let result = self
            .runner
            .run_captured(spec, self.search)
            .await
            .map_err(|e| e.technical_details.unwrap_or(e.message))?;
        match result.status {
            RunStatus::Succeeded if result.output_truncated => {
                Err(format!("`git {}` output exceeded the limit", command[0]))
            }
            RunStatus::Succeeded => Ok(Some(result.stdout)),
            RunStatus::Failed => Ok(None),
            RunStatus::TimedOut => Err(format!("`git {}` timed out", command[0])),
            RunStatus::Cancelled => Err("cancelled".into()),
        }
    }
}

fn first_line(s: Option<String>) -> Option<String> {
    s.and_then(|v| {
        v.lines()
            .next()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
    })
}

/// Collect metadata for one repository through read-only Git commands.
pub async fn collect(
    runner: &ProcessRunner,
    search: &SearchPath,
    repo: &Path,
    git_dir: &Path,
) -> GitMetadata {
    let git = Git {
        runner,
        search,
        cwd: repo,
    };
    let mut warnings = Vec::new();
    let mut take = |r: Result<Option<String>, String>, what: &str| match r {
        Ok(v) => v,
        Err(e) => {
            warnings.push(format!("{what}: {e}"));
            None
        }
    };

    let branch = first_line(take(
        git.run(CURRENT_BRANCH_COMMAND).await,
        "current branch",
    ));
    let head = first_line(take(git.run(HEAD_COMMAND).await, "HEAD commit"));
    let default_branch = first_line(take(
        git.run(DEFAULT_BRANCH_COMMAND).await,
        "default branch",
    ))
    .map(|b| b.strip_prefix("origin/").unwrap_or(&b).to_string());
    let origin =
        first_line(take(git.run(ORIGIN_URL_COMMAND).await, "origin")).map(|u| parse_remote(&u));
    let last_commit_at = first_line(take(git.run(LAST_COMMIT_COMMAND).await, "last commit"));

    let risky = risky_config(git_dir);
    let working_tree = if risky.is_none() {
        take(git.run(STATUS_COMMAND).await, "status").map(|s| parse_status(&s))
    } else {
        None
    };
    if let Some(reason) = risky {
        warnings.push(format!(
            "Working-tree status not collected: {reason}. Running `git status` could execute it."
        ));
    }

    let detached = branch.as_deref() == Some("HEAD");
    GitMetadata {
        current_branch: branch.filter(|b| b != "HEAD"),
        detached,
        head_commit: head,
        default_branch,
        origin,
        last_commit_at,
        working_tree,
        warnings,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    pub commit: String,
    pub committed_at: Option<String>,
    pub upstream: Option<String>,
}

pub fn parse_branches(output: &str) -> Vec<BranchInfo> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\0');
            let name = parts.next()?.trim().to_string();
            let commit = parts.next()?.trim().to_string();
            let committed_at = parts
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let upstream = parts
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            (!name.is_empty()).then_some(BranchInfo {
                name,
                commit,
                committed_at,
                upstream,
            })
        })
        .collect()
}

/// Local branches, newest first (at most 200).
pub async fn branches(
    runner: &ProcessRunner,
    search: &SearchPath,
    repo: &Path,
) -> Result<Vec<BranchInfo>, String> {
    let git = Git {
        runner,
        search,
        cwd: repo,
    };
    match git.run(BRANCHES_COMMAND).await? {
        Some(out) => Ok(parse_branches(&out)),
        None => Err("git for-each-ref failed".into()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_status() {
        let out = "M  staged.txt\0 M modified.txt\0MM both.txt\0?? new.txt\0R  new-name.txt\0old-name.txt\0!! ignored\0";
        let c = parse_status(out);
        assert_eq!((c.staged, c.modified, c.untracked), (3, 2, 1));
        assert!(c.dirty());
        assert!(!parse_status("").dirty());
    }

    #[test]
    fn detects_marker_kinds() {
        let dir = tempfile::tempdir().unwrap();
        assert!(detect_marker(dir.path()).is_none());
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert_eq!(
            detect_marker(dir.path()).unwrap().unwrap().0,
            GitKind::Standard
        );

        let wt = tempfile::tempdir().unwrap();
        std::fs::write(
            wt.path().join(".git"),
            "gitdir: /src/main/.git/worktrees/feature\n",
        )
        .unwrap();
        assert_eq!(
            detect_marker(wt.path()).unwrap().unwrap().0,
            GitKind::Worktree
        );

        let sm = tempfile::tempdir().unwrap();
        std::fs::write(sm.path().join(".git"), "gitdir: ../.git/modules/lib\n").unwrap();
        let (kind, path) = detect_marker(sm.path()).unwrap().unwrap();
        assert_eq!(kind, GitKind::Submodule);
        assert!(path.starts_with(sm.path()));

        let bad = tempfile::tempdir().unwrap();
        std::fs::write(bad.path().join(".git"), "not a gitdir file").unwrap();
        assert!(detect_marker(bad.path()).unwrap().is_err());
    }

    #[test]
    fn flags_config_that_would_execute_programs() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config");
        std::fs::write(
            &cfg,
            "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = x\n",
        )
        .unwrap();
        assert_eq!(risky_config(dir.path()), None);
        std::fs::write(&cfg, "[filter \"lfs\"]\n\tclean = git-lfs clean -- %f\n").unwrap();
        assert!(risky_config(dir.path()).unwrap().contains("clean"));
        std::fs::write(&cfg, "[includeIf \"gitdir:~/\"]\n\tpath = other\n").unwrap();
        assert!(risky_config(dir.path()).unwrap().contains("includes"));
        std::fs::write(&cfg, "[core]\n\tfsmonitor = /tmp/evil\n").unwrap();
        assert_eq!(
            risky_config(dir.path()),
            None,
            "fsmonitor is overridden by SAFE_PREFIX"
        );
    }

    #[test]
    fn parses_branch_listing() {
        let out = "main\0abc123\x002026-10-20T10:00:00+05:30\0origin/main\nfeature\0def456\x002026-10-21T10:00:00+05:30\0\n";
        let b = parse_branches(out);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].upstream.as_deref(), Some("origin/main"));
        assert_eq!(b[1].upstream, None);
    }
}
