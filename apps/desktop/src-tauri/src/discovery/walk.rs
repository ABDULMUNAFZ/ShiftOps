//! Filesystem walk that finds Git repositories under an approved root.
//!
//! - A directory containing `.git` (directory or file) is a repository; the
//!   walk records it and does not descend into it.
//! - High-noise directories (dependencies, build output, caches, VCS
//!   internals) and every other dot-directory are skipped.
//! - Depth and visited-entry limits bound the work.
//! - Symlinked directories are never followed outside the root. Inside the
//!   root they are followed only when the root allows it, with loop
//!   protection.
//! - Unreadable directories become warnings; the walk continues.
//! - Cancellation is checked before every directory.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::git::inspect::{detect_marker, GitKind};

/// Directory names never descended into.
pub const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "bower_components",
    "jspm_packages",
    "vendor",
    "target",
    "dist",
    "build",
    "out",
    "coverage",
    "bin",
    "obj",
    "Pods",
    "DerivedData",
    "__pycache__",
    "venv",
    "site-packages",
];

#[derive(Debug, Clone, Copy)]
pub struct WalkOptions {
    pub max_depth: u32,
    pub follow_internal_symlinks: bool,
    pub max_entries: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    Inaccessible,
    SymlinkEscape,
    BrokenSymlink,
    InvalidGitMarker,
    EntryLimit,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalkWarning {
    pub path: String,
    pub code: WarningCode,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct FoundRepository {
    pub path: PathBuf,
    pub git_kind: GitKind,
    pub git_dir: PathBuf,
    pub depth: u32,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalkStats {
    pub directories_visited: u64,
    pub entries_seen: u64,
    pub skipped_directories: u64,
    pub depth_limited_directories: u64,
}

#[derive(Debug, Default)]
pub struct WalkOutcome {
    pub repositories: Vec<FoundRepository>,
    pub warnings: Vec<WalkWarning>,
    pub stats: WalkStats,
    pub cancelled: bool,
    pub truncated: bool,
}

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// `root` must already be canonical. `progress` receives the current
/// directory and running statistics, at most every 100 ms.
pub fn walk(
    root: &Path,
    options: WalkOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&Path, &WalkStats, usize),
) -> WalkOutcome {
    let mut out = WalkOutcome::default();
    let mut stack: Vec<(PathBuf, u32)> = vec![(root.to_path_buf(), 0)];
    let mut visited: HashSet<PathBuf> = HashSet::from([root.to_path_buf()]);
    let mut last_progress = Instant::now() - PROGRESS_INTERVAL;
    let warn = |out: &mut WalkOutcome, path: &Path, code: WarningCode, message: String| {
        out.warnings.push(WalkWarning {
            path: path.display().to_string(),
            code,
            message,
        });
    };

    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        out.stats.directories_visited += 1;
        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            progress(&dir, &out.stats, out.repositories.len());
            last_progress = Instant::now();
        }

        match detect_marker(&dir) {
            Some(Ok((git_kind, git_dir))) => {
                out.repositories.push(FoundRepository {
                    path: dir,
                    git_kind,
                    git_dir,
                    depth,
                });
                continue;
            }
            Some(Err(message)) => {
                warn(&mut out, &dir, WarningCode::InvalidGitMarker, message);
                continue;
            }
            None => {}
        }

        if depth >= options.max_depth {
            out.stats.depth_limited_directories += 1;
            continue;
        }

        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                warn(
                    &mut out,
                    &dir,
                    WarningCode::Inaccessible,
                    format!("Directory could not be read: {e}"),
                );
                continue;
            }
        };
        let mut children = Vec::new();
        for entry in entries {
            out.stats.entries_seen += 1;
            if out.stats.entries_seen as usize > options.max_entries {
                out.truncated = true;
                warn(
                    &mut out,
                    &dir,
                    WarningCode::EntryLimit,
                    format!(
                        "Stopped after {} entries; results are partial. Choose a narrower folder.",
                        options.max_entries
                    ),
                );
                return out;
            }
            let Ok(entry) = entry else { continue };
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = dir.join(&name);

            if file_type.is_symlink() {
                let target = match std::fs::canonicalize(&path) {
                    Ok(t) => t,
                    Err(_) => {
                        warn(
                            &mut out,
                            &path,
                            WarningCode::BrokenSymlink,
                            "Symbolic link target does not exist.".into(),
                        );
                        continue;
                    }
                };
                if !target.is_dir() {
                    continue;
                }
                if !target.starts_with(root) {
                    warn(
                        &mut out,
                        &path,
                        WarningCode::SymlinkEscape,
                        format!(
                            "Symbolic link leads outside the root (to {}); not followed.",
                            target.display()
                        ),
                    );
                    continue;
                }
                if !options.follow_internal_symlinks || skip_name(&name_str) {
                    out.stats.skipped_directories += 1;
                    continue;
                }
                if visited.insert(target.clone()) {
                    children.push((target, depth + 1));
                }
                continue;
            }
            if !file_type.is_dir() {
                continue;
            }
            if skip_name(&name_str) {
                out.stats.skipped_directories += 1;
                continue;
            }
            if visited.insert(path.clone()) {
                children.push((path, depth + 1));
            }
        }
        // Reverse so the stack visits children in directory-listing order.
        children.sort();
        stack.extend(children.into_iter().rev());
    }
    progress(root, &out.stats, out.repositories.len());
    out
}

/// `.git` internals, every other dot-directory and known noise folders.
fn skip_name(name: &str) -> bool {
    name.starts_with('.') || SKIPPED_DIRS.contains(&name)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;

    fn opts(max_depth: u32) -> WalkOptions {
        WalkOptions {
            max_depth,
            follow_internal_symlinks: false,
            max_entries: 10_000,
        }
    }

    fn run(root: &Path, options: WalkOptions) -> WalkOutcome {
        let root = fs::canonicalize(root).unwrap();
        walk(&root, options, &AtomicBool::new(false), &mut |_, _, _| {})
    }

    fn names(out: &WalkOutcome) -> Vec<String> {
        let mut n: Vec<String> = out
            .repositories
            .iter()
            .map(|r| r.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        n.sort();
        n
    }

    fn repo(path: &Path) {
        fs::create_dir_all(path.join(".git")).unwrap();
    }

    #[test]
    fn finds_nested_repositories_and_stops_at_each() {
        let t = tempfile::tempdir().unwrap();
        repo(&t.path().join("a"));
        repo(&t.path().join("a/vendored-inside")); // inside a repo: not reported
        repo(&t.path().join("group/b"));
        repo(&t.path().join("group/deeper/c"));
        let out = run(t.path(), opts(4));
        assert_eq!(names(&out), ["a", "b", "c"]);
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn root_that_is_a_repository_returns_only_itself() {
        let t = tempfile::tempdir().unwrap();
        repo(t.path());
        repo(&t.path().join("nested"));
        let out = run(t.path(), opts(4));
        assert_eq!(out.repositories.len(), 1);
        assert_eq!(out.repositories[0].depth, 0);
    }

    #[test]
    fn respects_depth_limit() {
        let t = tempfile::tempdir().unwrap();
        repo(&t.path().join("l1"));
        repo(&t.path().join("x/l2"));
        repo(&t.path().join("x/y/l3"));
        assert_eq!(names(&run(t.path(), opts(2))), ["l1", "l2"]);
        assert_eq!(names(&run(t.path(), opts(1))), ["l1"]);
        assert!(run(t.path(), opts(1)).stats.depth_limited_directories > 0);
        assert_eq!(names(&run(t.path(), opts(3))), ["l1", "l2", "l3"]);
    }

    #[test]
    fn skips_noise_and_hidden_directories() {
        let t = tempfile::tempdir().unwrap();
        for noisy in [
            "node_modules/pkg",
            "target/debug",
            "dist/x",
            ".cache/y",
            ".hidden/z",
            "vendor/v",
            "__pycache__/p",
        ] {
            repo(&t.path().join(noisy));
        }
        repo(&t.path().join("real"));
        let out = run(t.path(), opts(4));
        assert_eq!(names(&out), ["real"]);
        assert!(out.stats.skipped_directories >= 7);
    }

    #[test]
    fn detects_worktree_and_submodule_git_files() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("wt")).unwrap();
        fs::write(
            t.path().join("wt/.git"),
            "gitdir: /src/main/.git/worktrees/wt\n",
        )
        .unwrap();
        fs::create_dir_all(t.path().join("bad")).unwrap();
        fs::write(t.path().join("bad/.git"), "garbage").unwrap();
        let out = run(t.path(), opts(2));
        assert_eq!(names(&out), ["wt"]);
        assert_eq!(out.repositories[0].git_kind, GitKind::Worktree);
        assert_eq!(out.warnings.len(), 1);
        assert_eq!(out.warnings[0].code, WarningCode::InvalidGitMarker);
    }

    #[test]
    fn refuses_symlink_escapes_and_follows_internal_links_only_when_allowed() {
        let t = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        repo(&outside.path().join("secret-repo"));
        std::os::unix::fs::symlink(outside.path(), t.path().join("escape")).unwrap();
        repo(&t.path().join("inner/real"));
        std::os::unix::fs::symlink(t.path().join("inner"), t.path().join("alias")).unwrap();
        std::os::unix::fs::symlink(t.path().join("missing"), t.path().join("dangling")).unwrap();

        let out = run(t.path(), opts(4));
        assert_eq!(names(&out), ["real"]);
        let codes: Vec<_> = out.warnings.iter().map(|w| w.code).collect();
        assert!(codes.contains(&WarningCode::SymlinkEscape));
        assert!(codes.contains(&WarningCode::BrokenSymlink));

        // Following internal links does not double-count already visited directories.
        let follow = run(
            t.path(),
            WalkOptions {
                follow_internal_symlinks: true,
                ..opts(4)
            },
        );
        assert_eq!(names(&follow), ["real"]);
    }

    #[test]
    fn symlink_loops_terminate() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("a")).unwrap();
        std::os::unix::fs::symlink(t.path(), t.path().join("a/loop")).unwrap();
        repo(&t.path().join("a/r"));
        let out = run(
            t.path(),
            WalkOptions {
                follow_internal_symlinks: true,
                ..opts(10)
            },
        );
        assert_eq!(names(&out), ["r"]);
    }

    #[test]
    fn unreadable_directories_warn_without_stopping() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let locked = t.path().join("locked");
        fs::create_dir(&locked).unwrap();
        repo(&t.path().join("open"));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let out = run(t.path(), opts(3));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(names(&out), ["open"]);
        assert!(out
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::Inaccessible));
    }

    #[test]
    fn entry_limit_stops_with_partial_results() {
        let t = tempfile::tempdir().unwrap();
        for i in 0..30 {
            fs::create_dir(t.path().join(format!("d{i:02}"))).unwrap();
        }
        let out = run(
            t.path(),
            WalkOptions {
                max_entries: 10,
                ..opts(3)
            },
        );
        assert!(out.truncated);
        assert_eq!(out.warnings.last().unwrap().code, WarningCode::EntryLimit);
    }

    #[test]
    fn cancellation_stops_the_walk() {
        let t = tempfile::tempdir().unwrap();
        repo(&t.path().join("a"));
        let root = fs::canonicalize(t.path()).unwrap();
        let out = walk(&root, opts(3), &AtomicBool::new(true), &mut |_, _, _| {});
        assert!(out.cancelled);
        assert!(out.repositories.is_empty());
    }
}
