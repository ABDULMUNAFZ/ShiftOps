//! First-run environment checks. Every result comes from executing a real
//! command, reading the filesystem or calling the OS credential store. A
//! check that cannot run reports `unavailable` or `error` with the reason.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::keychain;
use crate::shell_path::{self, SearchPath};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const DOCKER_DAEMON_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_DETAIL_CHARS: usize = 600;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Available,
    Unavailable,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: &'static str,
    pub label: &'static str,
    pub status: Status,
    pub version: Option<String>,
    pub detail: String,
    pub path: Option<String>,
    pub command: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub collected_at: String,
    pub checks: Vec<Check>,
}

pub async fn run_all(data_dir: &Path) -> Report {
    let search = shell_path::search_path().await;
    let (git, node, pnpm, npm, yarn, docker, devin) = tokio::join!(
        tool(search, "git", "Git", &["--version"]),
        tool(search, "node", "Node.js", &["--version"]),
        tool(search, "pnpm", "pnpm", &["--version"]),
        tool(search, "npm", "npm", &["--version"]),
        tool(search, "yarn", "Yarn", &["--version"]),
        docker_check(search),
        tool(search, "devin", "Devin CLI", &["--version"]),
    );
    let os = os_check(search).await;
    let data = data_dir_check(data_dir);
    let keychain = keychain_check().await;

    Report {
        collected_at: now_rfc3339(),
        checks: vec![
            os, data, git, node, pnpm, npm, yarn, docker, devin, keychain,
        ],
    }
}

struct Output {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

async fn exec(
    search: &SearchPath,
    exe: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, String> {
    let mut cmd = tokio::process::Command::new(exe);
    // Neutral working directory: tools such as Corepack-managed yarn change
    // behaviour based on the nearest package.json.
    cmd.args(args)
        .current_dir(std::env::temp_dir())
        .env("PATH", search.joined())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    match tokio::time::timeout(timeout, cmd.output()).await {
        Err(_) => Err(format!("timed out after {}s", timeout.as_secs())),
        Ok(Err(e)) => Err(format!("failed to start: {e}")),
        Ok(Ok(out)) => Ok(Output {
            success: out.status.success(),
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        }),
    }
}

fn not_found(id: &'static str, label: &'static str, program: &str, search: &SearchPath) -> Check {
    Check {
        id,
        label,
        status: Status::Unavailable,
        version: None,
        detail: format!(
            "`{program}` was not found in {} directories searched ({}).",
            search.dirs.len(),
            search.source
        ),
        path: None,
        command: None,
        duration_ms: 0,
    }
}

async fn tool(
    search: &SearchPath,
    program: &'static str,
    label: &'static str,
    args: &[&str],
) -> Check {
    let id = program;
    let Some(exe) = search.find(program) else {
        return not_found(id, label, program, search);
    };
    let started = Instant::now();
    let command = format!("{} {}", exe.display(), args.join(" "));
    let result = exec(search, &exe, args, COMMAND_TIMEOUT).await;
    let duration_ms = elapsed_ms(started);
    let (status, version, detail) = match result {
        Ok(out) if out.success => {
            let version = parse_version(&out.stdout).or_else(|| parse_version(&out.stderr));
            let shown = if out.stdout.is_empty() {
                out.stderr
            } else {
                out.stdout
            };
            (Status::Available, version, truncate(&shown))
        }
        Ok(out) => (
            Status::Error,
            None,
            truncate(&format!(
                "exited with {}: {}",
                out.code.map_or("signal".into(), |c| c.to_string()),
                if out.stderr.is_empty() {
                    out.stdout
                } else {
                    out.stderr
                }
            )),
        ),
        Err(e) => (Status::Error, None, e),
    };
    Check {
        id,
        label,
        status,
        version,
        detail,
        path: Some(exe.display().to_string()),
        command: Some(command),
        duration_ms,
    }
}

async fn docker_check(search: &SearchPath) -> Check {
    let mut check = tool(search, "docker", "Docker", &["--version"]).await;
    if check.status != Status::Available {
        return check;
    }
    let Some(exe) = search.find("docker") else {
        return check;
    };
    let started = Instant::now();
    let args = ["info", "--format", "{{.ServerVersion}}"];
    let daemon = exec(search, &exe, &args, DOCKER_DAEMON_TIMEOUT).await;
    check.duration_ms += elapsed_ms(started);
    check.command = Some(format!(
        "{} --version && {} {}",
        exe.display(),
        exe.display(),
        args.join(" ")
    ));
    match daemon {
        Ok(out) if out.success && !out.stdout.is_empty() => {
            check.detail = format!(
                "{}\nDaemon reachable, server version {}",
                check.detail, out.stdout
            );
        }
        Ok(out) => {
            check.status = Status::Unavailable;
            let reason = if out.stderr.is_empty() {
                out.stdout
            } else {
                out.stderr
            };
            check.detail = truncate(&format!(
                "{}\nCLI installed, daemon not reachable: {reason}",
                check.detail
            ));
        }
        Err(e) => {
            check.status = Status::Unavailable;
            check.detail = format!("{}\nCLI installed, daemon check {e}", check.detail);
        }
    }
    check
}

async fn os_check(search: &SearchPath) -> Check {
    let started = Instant::now();
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let (release, command) = os_release(search).await;
    let detail = match &release {
        Some(r) => format!("{r} ({os}, {arch})"),
        None => format!("{os}, {arch}; OS release could not be determined"),
    };
    Check {
        id: "os",
        label: "Operating system",
        status: Status::Available,
        version: release,
        detail,
        path: None,
        command,
        duration_ms: elapsed_ms(started),
    }
}

#[cfg(target_os = "macos")]
async fn os_release(search: &SearchPath) -> (Option<String>, Option<String>) {
    let exe = Path::new("/usr/bin/sw_vers");
    let name = exec(search, exe, &["-productName"], COMMAND_TIMEOUT).await;
    let ver = exec(search, exe, &["-productVersion"], COMMAND_TIMEOUT).await;
    let release = match (name, ver) {
        (Ok(n), Ok(v)) if n.success && v.success => Some(format!("{} {}", n.stdout, v.stdout)),
        _ => None,
    };
    (
        release,
        Some("/usr/bin/sw_vers -productName; -productVersion".into()),
    )
}

#[cfg(target_os = "linux")]
async fn os_release(_search: &SearchPath) -> (Option<String>, Option<String>) {
    let release = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_string())
        });
    (release, Some("read /etc/os-release".into()))
}

#[cfg(target_os = "windows")]
async fn os_release(search: &SearchPath) -> (Option<String>, Option<String>) {
    let out = exec(search, Path::new("cmd"), &["/C", "ver"], COMMAND_TIMEOUT).await;
    let release = out.ok().filter(|o| o.success).map(|o| o.stdout);
    (release, Some("cmd /C ver".into()))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
async fn os_release(_search: &SearchPath) -> (Option<String>, Option<String>) {
    (None, None)
}

fn data_dir_check(dir: &Path) -> Check {
    let started = Instant::now();
    let probe = dir.join(".shiftops-write-probe");
    let result = std::fs::write(&probe, b"ok").and_then(|_| std::fs::remove_file(&probe));
    let (status, detail) = match result {
        Ok(()) => (
            Status::Available,
            "Directory exists and is writable.".to_string(),
        ),
        Err(e) => (Status::Error, format!("Directory is not writable: {e}")),
    };
    Check {
        id: "app_data_dir",
        label: "Application data directory",
        status,
        version: None,
        detail,
        path: Some(dir.display().to_string()),
        command: None,
        duration_ms: elapsed_ms(started),
    }
}

async fn keychain_check() -> Check {
    let started = Instant::now();
    // Keychain APIs block; keep them off the async worker threads.
    let result = tokio::task::spawn_blocking(keychain::probe)
        .await
        .unwrap_or_else(|e| Err(format!("probe task failed: {e}")));
    let (status, version, detail) = match result {
        Ok(backend) => (
            Status::Available,
            None,
            format!(
                "{backend}: wrote, read back and deleted a probe entry under service {}.",
                keychain::SERVICE
            ),
        ),
        Err(e) => (Status::Unavailable, None, e),
    };
    Check {
        id: "keychain",
        label: "OS keychain",
        status,
        version,
        detail,
        path: None,
        command: None,
        duration_ms: elapsed_ms(started),
    }
}

/// Extract the first version-looking token: optional `v`, then digits and dots.
pub fn parse_version(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .map(|t| t.strip_prefix('v').unwrap_or(t))
        .find(|t| {
            t.chars().next().is_some_and(|c| c.is_ascii_digit())
                && t.contains('.')
                && t.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
        })
        .map(str::to_string)
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_DETAIL_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX_DETAIL_CHARS).collect();
    format!("{cut}…")
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_version_outputs() {
        assert_eq!(
            parse_version("git version 2.50.1 (Apple Git-155)").as_deref(),
            Some("2.50.1")
        );
        assert_eq!(parse_version("v26.7.0").as_deref(), Some("26.7.0"));
        assert_eq!(parse_version("10.33.0").as_deref(), Some("10.33.0"));
        assert_eq!(
            parse_version("Docker version 29.8.1, build 4a63305").as_deref(),
            Some("29.8.1")
        );
        assert_eq!(
            parse_version("devin 1.2.0-beta.3").as_deref(),
            Some("1.2.0-beta.3")
        );
        assert_eq!(parse_version("no version here"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn truncates_long_detail() {
        let long = "x".repeat(MAX_DETAIL_CHARS + 10);
        assert_eq!(truncate(&long).chars().count(), MAX_DETAIL_CHARS + 1);
        assert_eq!(truncate("short"), "short");
    }

    #[test]
    fn data_dir_check_reports_writable_directory() {
        let dir = tempfile::tempdir().unwrap();
        let check = data_dir_check(dir.path());
        assert_eq!(check.status, Status::Available);
        assert!(!dir.path().join(".shiftops-write-probe").exists());
    }

    #[test]
    fn data_dir_check_reports_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let check = data_dir_check(&dir.path().join("does-not-exist"));
        assert_eq!(check.status, Status::Error);
    }

    #[tokio::test]
    async fn missing_tool_is_reported_unavailable_not_failed() {
        let search = SearchPath {
            dirs: vec![tempfile::tempdir().unwrap().path().to_path_buf()],
            source: "test".into(),
        };
        let check = tool(&search, "devin", "Devin CLI", &["--version"]).await;
        assert_eq!(check.status, Status::Unavailable);
        assert!(check.detail.contains("not found"));
        assert!(check.path.is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_real_executable_and_parses_its_output() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("fakegit");
        std::fs::write(&exe, "#!/bin/sh\necho \"fakegit version 9.8.7\"\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let search = SearchPath {
            dirs: vec![dir.path().to_path_buf(), "/bin".into(), "/usr/bin".into()],
            source: "test".into(),
        };
        let check = tool(&search, "fakegit", "Fake", &["--version"]).await;
        assert_eq!(check.status, Status::Available);
        assert_eq!(check.version.as_deref(), Some("9.8.7"));

        std::fs::write(&exe, "#!/bin/sh\necho boom >&2\nexit 3\n").unwrap();
        let failed = tool(&search, "fakegit", "Fake", &["--version"]).await;
        assert_eq!(failed.status, Status::Error);
        assert!(failed.detail.contains("exited with 3: boom"));
    }
}
