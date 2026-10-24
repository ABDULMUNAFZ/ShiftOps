//! Central filesystem path validation. Every path that a native operation
//! will read, write or use as a working directory goes through `validate`.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult, ErrorCode};

const MAX_PATH_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PathKind {
    File,
    Directory,
    Missing,
    Other,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedPath {
    /// Fully resolved path (symlinks followed) for existing paths; for missing
    /// paths, the resolved nearest existing ancestor joined with the rest.
    pub canonical: PathBuf,
    pub kind: PathKind,
    /// The path itself is a symbolic link.
    pub is_symlink: bool,
    /// Resolution passed through at least one symlink below the allowed root.
    pub resolved_through_symlink: bool,
}

#[derive(Debug, Clone)]
pub struct PathPolicy {
    /// Allowed roots as given and canonicalised.
    roots: Vec<(PathBuf, PathBuf)>,
    /// Explicit approval to follow a symlink that leaves every allowed root.
    allow_symlink_escape: bool,
}

impl PathPolicy {
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        let roots = roots
            .into_iter()
            .map(|r| {
                let canonical = std::fs::canonicalize(&r).unwrap_or_else(|_| r.clone());
                (r, canonical)
            })
            .collect();
        Self {
            roots,
            allow_symlink_escape: false,
        }
    }

    pub fn allowing_symlink_escape(mut self) -> Self {
        self.allow_symlink_escape = true;
        self
    }

    fn contains_canonical(&self, path: &Path) -> bool {
        self.roots.iter().any(|(_, c)| path.starts_with(c))
    }

    fn contains_lexical(&self, path: &Path) -> Option<&Path> {
        self.roots
            .iter()
            .find(|(given, canonical)| path.starts_with(given) || path.starts_with(canonical))
            .map(|(given, _)| given.as_path())
    }
}

fn reject(message: impl Into<String>, input: &str) -> AppError {
    AppError::new(ErrorCode::PathNotAllowed, message).with_context("path", input)
}

/// Remove `.` and resolve `..` without touching the filesystem.
fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() || out.as_os_str().is_empty() {
                    return None;
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

pub fn validate(input: &str, policy: &PathPolicy) -> AppResult<ValidatedPath> {
    if input.is_empty() {
        return Err(AppError::validation("Path is empty."));
    }
    if input.contains('\0') {
        return Err(AppError::validation("Path contains a null byte.")
            .with_context("path", input.replace('\0', "\\0")));
    }
    if input.len() > MAX_PATH_BYTES {
        return Err(AppError::validation(format!(
            "Path exceeds {MAX_PATH_BYTES} bytes."
        )));
    }
    let raw = Path::new(input);
    if !raw.is_absolute() {
        return Err(AppError::validation("Path must be absolute.").with_context("path", input));
    }
    let lexical = normalize(raw)
        .ok_or_else(|| AppError::validation("Path is malformed.").with_context("path", input))?;

    let link_meta = std::fs::symlink_metadata(&lexical);
    let is_symlink = link_meta
        .as_ref()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);

    let (canonical, kind) = match std::fs::canonicalize(&lexical) {
        Ok(canonical) => {
            let meta = std::fs::metadata(&canonical)
                .map_err(|e| reject(format!("Path is not accessible: {e}"), input))?;
            let kind = if meta.is_dir() {
                PathKind::Directory
            } else if meta.is_file() {
                PathKind::File
            } else {
                PathKind::Other
            };
            (canonical, kind)
        }
        Err(_) if link_meta.is_ok() => {
            // A dangling symlink: refuse rather than guess where it points.
            return Err(reject("Path is a broken symbolic link.", input));
        }
        Err(_) => (resolve_missing(&lexical), PathKind::Missing),
    };

    let inside_by_name = policy.contains_lexical(&lexical);
    let inside_resolved = policy.contains_canonical(&canonical);
    let resolved_through_symlink = inside_by_name.is_some_and(|root| {
        let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        lexical
            .strip_prefix(root)
            .or_else(|_| lexical.strip_prefix(&canonical_root))
            .map(|rel| canonical_root.join(rel) != canonical)
            .unwrap_or(false)
    });

    if !inside_resolved {
        if inside_by_name.is_some() && !policy.allow_symlink_escape {
            return Err(reject(
                "Path resolves outside the allowed directory through a symbolic link.",
                input,
            )
            .with_context("resolved", canonical.display().to_string()));
        }
        if inside_by_name.is_none() {
            return Err(reject("Path is outside the allowed directories.", input));
        }
    }

    Ok(ValidatedPath {
        canonical,
        kind,
        is_symlink,
        resolved_through_symlink,
    })
}

/// Canonicalise the deepest existing ancestor and append the remainder.
fn resolve_missing(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (
            existing.file_name().map(|n| n.to_os_string()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut resolved = std::fs::canonicalize(&existing).unwrap_or(existing);
    for name in rest.into_iter().rev() {
        resolved.push(name);
    }
    resolved
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, PathPolicy) {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let policy = PathPolicy::new([root.path().to_path_buf()]);
        (root, outside, policy)
    }

    fn s(p: &Path) -> String {
        p.display().to_string()
    }

    #[test]
    fn allows_existing_paths_inside_root_and_reports_kind() {
        let (root, _o, policy) = setup();
        std::fs::create_dir(root.path().join("dir")).unwrap();
        std::fs::write(root.path().join("dir/file.txt"), "x").unwrap();
        assert_eq!(
            validate(&s(&root.path().join("dir")), &policy)
                .unwrap()
                .kind,
            PathKind::Directory
        );
        let file = validate(&s(&root.path().join("dir/./file.txt")), &policy).unwrap();
        assert_eq!(file.kind, PathKind::File);
        assert!(!file.is_symlink);
    }

    #[test]
    fn reports_missing_paths_without_error() {
        let (root, _o, policy) = setup();
        let v = validate(&s(&root.path().join("new/child")), &policy).unwrap();
        assert_eq!(v.kind, PathKind::Missing);
        assert!(v.canonical.ends_with("new/child"));
    }

    #[test]
    fn rejects_malformed_relative_and_null_paths() {
        let (_r, _o, policy) = setup();
        assert_eq!(
            validate("", &policy).unwrap_err().code,
            ErrorCode::ValidationFailed
        );
        assert_eq!(
            validate("relative/path", &policy).unwrap_err().code,
            ErrorCode::ValidationFailed
        );
        assert_eq!(
            validate("/tmp/a\0b", &policy).unwrap_err().code,
            ErrorCode::ValidationFailed
        );
        assert_eq!(
            validate("/../..", &policy).unwrap_err().code,
            ErrorCode::ValidationFailed
        );
    }

    #[test]
    fn rejects_paths_outside_roots_including_dot_dot_escapes() {
        let (root, outside, policy) = setup();
        assert_eq!(
            validate(&s(outside.path()), &policy).unwrap_err().code,
            ErrorCode::PathNotAllowed
        );
        let sneaky = format!(
            "{}/../{}",
            s(root.path()),
            outside.path().file_name().unwrap().to_string_lossy()
        );
        assert_eq!(
            validate(&sneaky, &policy).unwrap_err().code,
            ErrorCode::PathNotAllowed
        );
    }

    #[test]
    fn rejects_symlink_escaping_root_unless_approved() {
        let (root, outside, policy) = setup();
        let link = root.path().join("escape");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        let err = validate(&s(&link), &policy).unwrap_err();
        assert_eq!(err.code, ErrorCode::PathNotAllowed);
        assert!(err.message.contains("symbolic link"));

        let approved = validate(&s(&link), &policy.clone().allowing_symlink_escape()).unwrap();
        assert!(approved.is_symlink);
        assert!(approved.resolved_through_symlink);
    }

    #[test]
    fn allows_symlink_that_stays_inside_root() {
        let (root, _o, policy) = setup();
        std::fs::create_dir(root.path().join("real")).unwrap();
        let link = root.path().join("alias");
        std::os::unix::fs::symlink(root.path().join("real"), &link).unwrap();
        let v = validate(&s(&link), &policy).unwrap();
        assert!(v.is_symlink);
        assert!(v.resolved_through_symlink);
        assert_eq!(v.kind, PathKind::Directory);
    }

    #[test]
    fn rejects_dangling_symlink() {
        let (root, _o, policy) = setup();
        let link = root.path().join("dangling");
        std::os::unix::fs::symlink(root.path().join("nope"), &link).unwrap();
        assert_eq!(
            validate(&s(&link), &policy).unwrap_err().code,
            ErrorCode::PathNotAllowed
        );
    }
}
