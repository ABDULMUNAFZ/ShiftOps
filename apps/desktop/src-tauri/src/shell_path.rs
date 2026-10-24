//! Executable discovery.
//!
//! Desktop apps launched from Finder, the Dock or a Linux launcher do not
//! inherit the PATH configured in the user's shell profile, so tools installed
//! with Homebrew, nvm, Volta or Corepack would appear missing. ShiftOps
//! resolves PATH once from the user's login shell, merges it with the process
//! PATH and a short list of well-known install locations, and searches it
//! itself. Every check reports the absolute path it actually executed.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

const MARKER_START: &str = "__SHIFTOPS_PATH_START__";
const MARKER_END: &str = "__SHIFTOPS_PATH_END__";

#[derive(Debug, Clone)]
pub struct SearchPath {
    pub dirs: Vec<PathBuf>,
    /// Where the login-shell PATH came from, for display.
    pub source: String,
}

static SEARCH_PATH: OnceLock<SearchPath> = OnceLock::new();

pub async fn search_path() -> &'static SearchPath {
    if let Some(path) = SEARCH_PATH.get() {
        return path;
    }
    let resolved = resolve().await;
    SEARCH_PATH.get_or_init(|| resolved)
}

impl SearchPath {
    pub fn joined(&self) -> OsString {
        std::env::join_paths(&self.dirs).unwrap_or_default()
    }

    pub fn find(&self, program: &str) -> Option<PathBuf> {
        self.dirs.iter().find_map(|dir| executable_in(dir, program))
    }
}

async fn resolve() -> SearchPath {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut source = String::from("process PATH");

    if let Some(shell_dirs) = login_shell_path().await {
        dirs.extend(shell_dirs);
        source = String::from("login shell PATH + process PATH");
    }
    if let Some(process) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&process));
    }
    dirs.extend(well_known_dirs());

    let mut seen = HashSet::new();
    dirs.retain(|d| !d.as_os_str().is_empty() && seen.insert(d.clone()));
    SearchPath { dirs, source }
}

#[cfg(unix)]
async fn login_shell_path() -> Option<Vec<PathBuf>> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let script = format!("printf '%s%s%s' '{MARKER_START}' \"$PATH\" '{MARKER_END}'");
    // Interactive login first (picks up nvm in .zshrc/.bashrc), then plain login.
    for flags in ["-ilc", "-lc"] {
        let mut cmd = tokio::process::Command::new(&shell);
        cmd.arg(flags)
            .arg(&script)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(4), cmd.output()).await
        else {
            continue;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(path) = extract_marked(&text) {
            return Some(std::env::split_paths(path).collect());
        }
    }
    None
}

#[cfg(not(unix))]
async fn login_shell_path() -> Option<Vec<PathBuf>> {
    None
}

fn extract_marked(text: &str) -> Option<&str> {
    let start = text.rfind(MARKER_START)? + MARKER_START.len();
    let end = start + text[start..].find(MARKER_END)?;
    let value = &text[start..end];
    (!value.is_empty()).then_some(value)
}

fn well_known_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(unix)]
    {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            for rel in [
                ".local/bin",
                ".cargo/bin",
                ".volta/bin",
                "Library/pnpm",
                ".bun/bin",
            ] {
                dirs.push(home.join(rel));
            }
        }
    }
    dirs
}

#[cfg(unix)]
fn executable_in(dir: &Path, program: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let candidate = dir.join(program);
    let meta = std::fs::metadata(&candidate).ok()?;
    (meta.is_file() && meta.permissions().mode() & 0o111 != 0).then_some(candidate)
}

#[cfg(windows)]
fn executable_in(dir: &Path, program: &str) -> Option<PathBuf> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into());
    exts.split(';')
        .filter(|e| !e.is_empty())
        .map(|ext| dir.join(format!("{program}{}", ext.to_ascii_lowercase())))
        .find(|c| c.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_path_between_markers_ignoring_shell_noise() {
        let out = format!("Welcome!\n{MARKER_START}/usr/bin:/bin{MARKER_END}");
        assert_eq!(extract_marked(&out), Some("/usr/bin:/bin"));
        assert_eq!(extract_marked("no markers"), None);
        assert_eq!(extract_marked(&format!("{MARKER_START}{MARKER_END}")), None);
    }

    #[cfg(unix)]
    #[test]
    fn finds_only_executable_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("tool");
        let plain = dir.path().join("data");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::write(&plain, "x").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();

        let search = SearchPath {
            dirs: vec![dir.path().to_path_buf()],
            source: "test".into(),
        };
        assert_eq!(search.find("tool"), Some(exe));
        assert_eq!(search.find("data"), None);
        assert_eq!(search.find("missing"), None);
    }
}
