//! Bounded, secret-aware traversal of a repository working tree. Collects
//! the contents of allow-listed manifest files and counts source files by
//! language. Never follows symbolic links and never reads secret files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};

pub const MAX_DEPTH: u32 = 8;
pub const MAX_ENTRIES: u64 = 60_000;
pub const MAX_RELEVANT_FILES: usize = 2_000;
/// Larger relevant files are fingerprinted by size and modification time and not parsed.
pub const MAX_FILE_BYTES: u64 = 512 * 1024;

const SKIPPED_DIRS: &[&str] = &[
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
    ".next",
    ".nuxt",
    ".turbo",
    ".gradle",
    ".mvn",
    ".venv",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".cache",
    ".idea",
    ".vscode",
    ".terraform",
    // Test fixtures describe other projects, not this one.
    "fixtures",
    "__fixtures__",
    "testdata",
];

/// Dot-directories that hold configuration the scanner needs.
const ALLOWED_DOT_DIRS: &[&str] = &[".github", ".circleci", ".gitlab"];

/// Exact file names worth reading.
const RELEVANT_NAMES: &[&str] = &[
    "package.json",
    ".nvmrc",
    ".node-version",
    ".tool-versions",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "bun.lock",
    "pnpm-workspace.yaml",
    "lerna.json",
    "turbo.json",
    "nx.json",
    "tsconfig.json",
    "Dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
    ".gitlab-ci.yml",
    "Jenkinsfile",
    "azure-pipelines.yml",
    "pyproject.toml",
    "requirements.txt",
    "setup.py",
    "setup.cfg",
    "Pipfile",
    "Pipfile.lock",
    "poetry.lock",
    "uv.lock",
    ".python-version",
    "runtime.txt",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "gradle-wrapper.properties",
    ".java-version",
    "global.json",
    "Directory.Build.props",
    "go.mod",
    "go.work",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
];

/// Names never read, even when they look relevant. Checked first.
pub fn is_secret_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == ".env"
        || lower.starts_with(".env.")
        || lower.ends_with(".env")
        || matches!(
            lower.as_str(),
            ".npmrc" | ".yarnrc" | ".yarnrc.yml" | ".netrc" | ".pypirc" | ".git-credentials"
        )
        || lower.starts_with("id_rsa")
        || lower.starts_with("id_ed25519")
        || lower.starts_with("id_ecdsa")
        || lower.starts_with("credentials")
        || lower.starts_with("secrets")
        || [".pem", ".key", ".p12", ".pfx", ".keystore", ".jks", ".kdbx"]
            .iter()
            .any(|ext| lower.ends_with(ext))
}

fn is_relevant(rel: &str, name: &str) -> bool {
    if RELEVANT_NAMES.contains(&name) {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    lower.starts_with("dockerfile")
        || lower.starts_with("requirements") && lower.ends_with(".txt")
        || lower.starts_with("tsconfig") && lower.ends_with(".json")
        || lower.ends_with(".csproj")
        || lower.ends_with(".fsproj")
        || lower.ends_with(".sln")
        || (rel.starts_with(".github/workflows/")
            && (lower.ends_with(".yml") || lower.ends_with(".yaml")))
        || rel == ".circleci/config.yml"
}

fn language_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "cs" => "C#",
        "fs" => "F#",
        "go" => "Go",
        "rs" => "Rust",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "scala" => "Scala",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" => "C++",
        "sh" | "bash" | "zsh" => "Shell",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "css" | "scss" | "sass" | "less" => "CSS",
        "html" | "htm" => "HTML",
        "sql" => "SQL",
        _ => return None,
    })
}

#[derive(Debug, Clone)]
pub struct RelevantFile {
    /// Path relative to the repository root, `/`-separated.
    pub rel: String,
    /// `None` when the file is larger than `MAX_FILE_BYTES` or not UTF-8.
    pub text: Option<String>,
    pub len: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanWarning {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Collected {
    pub files: Vec<RelevantFile>,
    pub languages: BTreeMap<&'static str, u64>,
    pub entries_seen: u64,
    pub warnings: Vec<ScanWarning>,
    pub cancelled: bool,
    pub truncated: bool,
    /// sha256 over every relevant file's path and content (or size and mtime).
    pub fingerprint: String,
}

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

pub fn collect(root: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, &str)) -> Collected {
    let mut out = Collected::default();
    let mut hasher = Sha256::new();
    let mut stack: Vec<(PathBuf, String, u32)> = vec![(root.to_path_buf(), String::new(), 0)];
    let mut last = Instant::now() - PROGRESS_INTERVAL;
    let mut fingerprint_parts: Vec<(String, Vec<u8>)> = Vec::new();

    'walk: while let Some((dir, rel_dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                out.warnings.push(ScanWarning {
                    path: rel_dir.clone(),
                    message: format!("Directory could not be read: {e}"),
                });
                continue;
            }
        };
        let mut children = Vec::new();
        for entry in entries.flatten() {
            out.entries_seen += 1;
            if out.entries_seen > MAX_ENTRIES {
                out.truncated = true;
                out.warnings.push(ScanWarning {
                    path: rel_dir.clone(),
                    message: format!("Stopped after {MAX_ENTRIES} entries; results are partial."),
                });
                break 'walk;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if rel_dir.is_empty() {
                name.clone()
            } else {
                format!("{rel_dir}/{name}")
            };
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                if is_relevant(&rel, &name) && !is_secret_file(&name) {
                    out.warnings.push(ScanWarning {
                        path: rel,
                        message: "Symbolic link not followed; the scanner reads only regular files inside the repository.".into(),
                    });
                }
                continue;
            }
            if ft.is_dir() {
                let hidden = name.starts_with('.') && !ALLOWED_DOT_DIRS.contains(&name.as_str());
                if name == ".git"
                    || hidden
                    || SKIPPED_DIRS.contains(&name.as_str())
                    || depth + 1 > MAX_DEPTH
                {
                    continue;
                }
                // A nested repository is its own project; do not scan into it.
                if std::fs::symlink_metadata(entry.path().join(".git")).is_ok() {
                    continue;
                }
                children.push((entry.path(), rel, depth + 1));
                continue;
            }
            if !ft.is_file() || is_secret_file(&name) {
                continue;
            }
            if let Some(lang) = language_of(&name) {
                *out.languages.entry(lang).or_default() += 1;
            }
            if !is_relevant(&rel, &name) {
                continue;
            }
            if out.files.len() >= MAX_RELEVANT_FILES {
                out.truncated = true;
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let len = meta.len();
            let text = if len <= MAX_FILE_BYTES {
                std::fs::read(entry.path()).ok().and_then(|bytes| {
                    fingerprint_parts.push((rel.clone(), bytes.clone()));
                    String::from_utf8(bytes).ok()
                })
            } else {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                fingerprint_parts
                    .push((rel.clone(), format!("len={len};mtime={mtime}").into_bytes()));
                None
            };
            out.files.push(RelevantFile { rel, text, len });
        }
        if last.elapsed() >= PROGRESS_INTERVAL {
            progress(out.entries_seen, &rel_dir);
            last = Instant::now();
        }
        children.sort();
        stack.extend(children.into_iter().rev());
    }
    out.files.sort_by(|a, b| a.rel.cmp(&b.rel));
    fingerprint_parts.sort();
    for (rel, bytes) in &fingerprint_parts {
        hasher.update(rel.as_bytes());
        hasher.update([0u8]);
        hasher.update(Sha256::digest(bytes));
    }
    out.fingerprint = hex(&hasher.finalize());
    progress(out.entries_seen, "");
    out
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_files_are_recognised() {
        for n in [
            ".env",
            ".env.local",
            "prod.env",
            ".npmrc",
            ".yarnrc.yml",
            "id_rsa",
            "id_ed25519.pub",
            "server.pem",
            "tls.key",
            "credentials.json",
            "secrets.yaml",
            "app.keystore",
        ] {
            assert!(is_secret_file(n), "{n}");
        }
        for n in [
            "package.json",
            "Dockerfile",
            "envoy.yaml",
            "keys.ts",
            "environment.ts",
        ] {
            assert!(!is_secret_file(n), "{n}");
        }
    }

    #[test]
    fn collects_relevant_files_and_languages_but_not_secrets() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path();
        std::fs::write(p.join("package.json"), "{}").unwrap();
        std::fs::write(p.join(".env"), "TOKEN=ghp_shouldNeverBeRead").unwrap();
        std::fs::write(p.join(".npmrc"), "//registry.npmjs.org/:_authToken=x").unwrap();
        std::fs::create_dir_all(p.join("src")).unwrap();
        std::fs::write(p.join("src/a.ts"), "").unwrap();
        std::fs::write(p.join("src/b.tsx"), "").unwrap();
        std::fs::create_dir_all(p.join(".github/workflows")).unwrap();
        std::fs::write(p.join(".github/workflows/ci.yml"), "on: push").unwrap();
        std::fs::create_dir_all(p.join("node_modules/x")).unwrap();
        std::fs::write(p.join("node_modules/x/package.json"), "{}").unwrap();
        std::fs::create_dir_all(p.join("nested-repo/.git")).unwrap();
        std::fs::write(p.join("nested-repo/package.json"), "{}").unwrap();
        let c = collect(p, &AtomicBool::new(false), &mut |_, _| {});
        let rels: Vec<_> = c.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, [".github/workflows/ci.yml", "package.json"]);
        assert_eq!(c.languages.get("TypeScript"), Some(&2));
        assert!(c
            .files
            .iter()
            .all(|f| !f.text.as_deref().unwrap_or("").contains("ghp_")));
        assert_eq!(c.fingerprint.len(), 64);
    }

    #[test]
    fn fingerprint_changes_with_uncommitted_edits() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("package.json"), r#"{"name":"a"}"#).unwrap();
        let a = collect(t.path(), &AtomicBool::new(false), &mut |_, _| {}).fingerprint;
        let same = collect(t.path(), &AtomicBool::new(false), &mut |_, _| {}).fingerprint;
        std::fs::write(t.path().join("package.json"), r#"{"name":"b"}"#).unwrap();
        let b = collect(t.path(), &AtomicBool::new(false), &mut |_, _| {}).fingerprint;
        assert_eq!(a, same);
        assert_ne!(a, b);
        // Irrelevant files do not change the fingerprint.
        std::fs::write(t.path().join("notes.md"), "x").unwrap();
        assert_eq!(
            collect(t.path(), &AtomicBool::new(false), &mut |_, _| {}).fingerprint,
            b
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_never_followed() {
        let t = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("package.json"), r#"{"name":"outside"}"#).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("package.json"),
            t.path().join("package.json"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), t.path().join("linked-dir")).unwrap();
        let c = collect(t.path(), &AtomicBool::new(false), &mut |_, _| {});
        assert!(c.files.is_empty());
        assert_eq!(c.warnings.len(), 1);
    }

    #[test]
    fn large_files_are_fingerprinted_not_read() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(
            t.path().join("pnpm-lock.yaml"),
            "x".repeat((MAX_FILE_BYTES + 1) as usize),
        )
        .unwrap();
        let c = collect(t.path(), &AtomicBool::new(false), &mut |_, _| {});
        assert_eq!(c.files.len(), 1);
        assert!(c.files[0].text.is_none());
    }

    #[test]
    fn cancellation() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("package.json"), "{}").unwrap();
        let c = collect(t.path(), &AtomicBool::new(true), &mut |_, _| {});
        assert!(c.cancelled);
    }
}
