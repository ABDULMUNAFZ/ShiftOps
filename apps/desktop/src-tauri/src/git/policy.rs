//! The Git policy. ShiftOps may run only the exact read-only argument vectors
//! listed here, each preceded by `SAFE_PREFIX`, plus one mutating operation:
//! an initial `git clone` of a GitHub repository into a ShiftOps-managed
//! workspace, built by `clone_args` and checked by `validate_clone`. Anything
//! else, including checkout, reset, clean, commit, push, pull, fetch, merge,
//! rebase, submodule commands and any config change, is rejected with
//! `COMMAND_NOT_ALLOWED`.

use std::path::{Component, Path};

use url::Url;

use crate::error::{AppError, AppResult, ErrorCode};

/// Global options prepended to every Git call. They disable features that
/// can execute programs named in a repository's own configuration
/// (`core.fsmonitor`, signature verification), and stop Git from taking
/// optional locks or writing the index during `status`.
pub const SAFE_PREFIX: &[&str] = &[
    "--no-pager",
    "--no-optional-locks",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.untrackedCache=false",
    "-c",
    "log.showSignature=false",
    "-c",
    "color.ui=false",
];

pub const BRANCH_FORMAT: &str =
    "--format=%(refname:short)%00%(objectname)%00%(committerdate:iso-strict)%00%(upstream:short)";

/// Every permitted read-only command after the prefix, exactly.
pub const READONLY_COMMANDS: &[&[&str]] = &[
    &["rev-parse", "--abbrev-ref", "HEAD"],
    &["rev-parse", "--verify", "--quiet", "HEAD"],
    &[
        "symbolic-ref",
        "--quiet",
        "--short",
        "refs/remotes/origin/HEAD",
    ],
    &["remote", "get-url", "origin"],
    &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
    &["log", "-1", "--format=%cI", "--no-show-signature"],
    &[
        "for-each-ref",
        BRANCH_FORMAT,
        "--sort=-committerdate",
        "--count=200",
        "refs/heads",
    ],
];

/// Environment for every Git call: no prompts, no pager, no optional locks,
/// stable C-locale output.
pub const GIT_ENV: &[(&str, &str)] = &[
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_PAGER", "cat"),
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("LC_ALL", "C"),
];

pub fn command_args(command: &[&str]) -> Vec<String> {
    SAFE_PREFIX
        .iter()
        .chain(command.iter())
        .map(|s| (*s).to_string())
        .collect()
}

/// Accept only `SAFE_PREFIX` followed by one entry of `READONLY_COMMANDS`.
pub fn validate(args: &[String]) -> AppResult<()> {
    let deny = |why: &str| {
        Err(AppError::new(
            ErrorCode::CommandNotAllowed,
            format!("Git command not permitted: {why}."),
        )
        .with_context("args", args.join(" ")))
    };
    if args.len() <= SAFE_PREFIX.len()
        || args[..SAFE_PREFIX.len()]
            .iter()
            .map(String::as_str)
            .ne(SAFE_PREFIX.iter().copied())
    {
        return deny("missing the read-only safety prefix");
    }
    let rest = &args[SAFE_PREFIX.len()..];
    if READONLY_COMMANDS
        .iter()
        .any(|allowed| rest.iter().map(String::as_str).eq(allowed.iter().copied()))
    {
        Ok(())
    } else {
        deny("only the documented read-only commands may run")
    }
}

/// Where Git looks for hooks during a managed clone: a path that can never
/// hold a hook, so neither the user's `core.hooksPath` nor a template runs.
pub const NO_HOOKS_PATH: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// Configuration for the managed clone, after `SAFE_PREFIX`: no hooks, no
/// submodule recursion, no interactive credential prompts, and HTTPS as the
/// only transport (`file`, `ssh`, `ext` and the rest are refused).
const CLONE_CONFIG: &[&str] = &[
    "-c",
    "protocol.allow=never",
    "-c",
    "protocol.https.allow=always",
    "-c",
    "submodule.recurse=false",
    "-c",
    "credential.interactive=false",
];
/// Added only when the configured GitHub host is plain HTTP on loopback
/// (local test servers); `urls::validate` refuses plain HTTP elsewhere.
const LOOPBACK_HTTP_CONFIG: &[&str] = &["-c", "protocol.http.allow=always"];
const CLONE_COMMAND: &[&str] = &[
    "clone",
    "--progress",
    "--no-recurse-submodules",
    // An empty template directory: no sample hooks are copied into `.git`.
    "--template=",
    "--",
];

/// Environment for the managed clone on top of `GIT_ENV`: never prompt, and
/// leave Git LFS pointer files unsmudged so no LFS filter process runs.
pub const CLONE_ENV: &[(&str, &str)] =
    &[("GIT_LFS_SKIP_SMUDGE", "1"), ("GCM_INTERACTIVE", "never")];

fn is_loopback_http(url: &Url) -> bool {
    url.scheme() == "http"
        && matches!(
            url.host(),
            Some(url::Host::Domain("localhost"))
                | Some(url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST))
                | Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
        )
}

fn hooks_config() -> String {
    format!("core.hooksPath={NO_HOOKS_PATH}")
}

/// The one mutating Git command: `git clone <url> <destination>`.
pub fn clone_args(url: &Url, destination: &Path) -> Vec<String> {
    let mut args: Vec<String> = SAFE_PREFIX.iter().map(|s| (*s).to_string()).collect();
    args.push("-c".into());
    args.push(hooks_config());
    args.extend(CLONE_CONFIG.iter().map(|s| (*s).to_string()));
    if is_loopback_http(url) {
        args.extend(LOOPBACK_HTTP_CONFIG.iter().map(|s| (*s).to_string()));
    }
    args.extend(CLONE_COMMAND.iter().map(|s| (*s).to_string()));
    args.push(url.to_string());
    args.push(destination.display().to_string());
    args
}

/// A GitHub owner or repository name usable as a single path segment.
pub fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s != "."
        && s != ".."
        && !s.eq_ignore_ascii_case(".git")
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// A clone source: `https://host/owner/repo.git` (or loopback `http`), with
/// no credentials, query or fragment.
pub fn validate_clone_url(raw: &str) -> AppResult<Url> {
    let deny = |why: &str| {
        Err(AppError::new(
            ErrorCode::CommandNotAllowed,
            format!("Clone source not permitted: {why}."),
        )
        .with_context("url", raw))
    };
    let Ok(url) = Url::parse(raw) else {
        return deny("not a URL");
    };
    if url.scheme() != "https" && !is_loopback_http(&url) {
        return deny("only HTTPS is allowed");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return deny("credentials must not appear in the URL");
    }
    if url.query().is_some() || url.fragment().is_some() || url.host().is_none() {
        return deny("unexpected URL parts");
    }
    let segments: Vec<&str> = url.path().trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        [owner, repo]
            if valid_segment(owner) && repo.strip_suffix(".git").is_some_and(valid_segment) => {}
        _ => return deny("the path must be /owner/repository.git"),
    }
    if url.as_str() != raw {
        return deny("the URL is not in normal form");
    }
    Ok(url)
}

/// Accept only the exact vector `clone_args` builds for a valid source and an
/// absolute, normalised destination. Where the destination may be is checked
/// by `ExecutionPolicy` against its approved roots.
pub fn validate_clone(args: &[String]) -> AppResult<()> {
    let deny = |why: &str| {
        Err(AppError::new(
            ErrorCode::CommandNotAllowed,
            format!("Git command not permitted: {why}."),
        )
        .with_context("args", args.join(" ")))
    };
    let (Some(dest), Some(url)) = (args.last(), args.len().checked_sub(2).map(|i| &args[i])) else {
        return deny("not a clone");
    };
    let url = validate_clone_url(url)?;
    let dest_path = Path::new(dest);
    let normal = dest_path.is_absolute()
        && !dest.starts_with('-')
        && dest_path
            .components()
            .all(|c| !matches!(c, Component::ParentDir | Component::CurDir));
    if !normal {
        return deny("the clone destination must be an absolute, normalised path");
    }
    if clone_args(&url, dest_path).as_slice() != args {
        return deny("only the documented managed clone may run");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn accepts_every_documented_command() {
        for cmd in READONLY_COMMANDS {
            validate(&command_args(cmd)).unwrap();
        }
    }

    #[test]
    fn rejects_mutations_extra_arguments_and_missing_prefix() {
        for cmd in [
            &["checkout", "main"][..],
            &["reset", "--hard"],
            &["clean", "-fdx"],
            &["commit", "-m", "x"],
            &["push"],
            &["pull"],
            &["fetch"],
            &["merge", "x"],
            &["rebase", "x"],
            &["config", "user.name", "x"],
            &["remote", "set-url", "origin", "x"],
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=normal",
                "--",
                "x",
            ],
            &["rev-parse", "--abbrev-ref", "HEAD", "--output=/tmp/x"],
        ] {
            let err = validate(&command_args(cmd)).unwrap_err();
            assert_eq!(err.code, ErrorCode::CommandNotAllowed, "{cmd:?}");
        }
        assert!(validate(&v(&["rev-parse", "HEAD"])).is_err());
        let mut injected = command_args(&["rev-parse", "--abbrev-ref", "HEAD"]);
        injected.insert(0, "-c".into());
        injected.insert(1, "core.pager=sh".into());
        assert!(validate(&injected).is_err());
    }

    #[test]
    fn clone_accepts_only_the_built_vector() {
        let url = validate_clone_url("https://github.com/acme/api.git").unwrap();
        let args = clone_args(&url, Path::new("/work/acme/api"));
        validate_clone(&args).unwrap();
        assert!(args.contains(&"--no-recurse-submodules".to_string()));
        assert!(args.contains(&"--template=".to_string()));
        assert!(args.contains(&format!("core.hooksPath={NO_HOOKS_PATH}")));
        assert!(!args.iter().any(|a| a == "protocol.http.allow=always"));
        // Read-only validation never accepts a clone, and vice versa.
        assert!(validate(&args).is_err());
        assert!(validate_clone(&command_args(READONLY_COMMANDS[0])).is_err());

        let loopback = validate_clone_url("http://127.0.0.1:9000/acme/api.git").unwrap();
        let args = clone_args(&loopback, Path::new("/work/acme/api"));
        assert!(args.iter().any(|a| a == "protocol.http.allow=always"));
        validate_clone(&args).unwrap();

        let tampered = |f: &dyn Fn(&mut Vec<String>)| {
            let mut a = clone_args(&url, Path::new("/work/acme/api"));
            f(&mut a);
            validate_clone(&a).unwrap_err().code
        };
        for code in [
            tampered(&|a| a.insert(a.len() - 3, "--recurse-submodules".into())),
            tampered(&|a| a.insert(a.len() - 3, "--config=core.hooksPath=/x".into())),
            tampered(&|a| a.retain(|x| x != "--template=")),
            tampered(&|a| a.insert(0, "-c".into())),
            tampered(&|a| *a.last_mut().unwrap() = "relative/dir".into()),
            tampered(&|a| *a.last_mut().unwrap() = "/work/acme/../../etc".into()),
            tampered(&|a| a.push("extra".into())),
            tampered(&|a| {
                let i = a.iter().position(|x| x == "clone").unwrap();
                a[i] = "fetch".into();
            }),
        ] {
            assert_eq!(code, ErrorCode::CommandNotAllowed);
        }
    }

    #[test]
    fn clone_sources_must_be_plain_https_github_paths() {
        for bad in [
            "http://github.com/acme/api.git",
            "ssh://git@github.com/acme/api.git",
            "git@github.com:acme/api.git",
            "file:///tmp/acme/api.git",
            "ext::sh -c touch% /tmp/pwned",
            "https://x-access-token:ghp_secret@github.com/acme/api.git",
            "https://token@github.com/acme/api.git",
            "https://github.com/acme/api.git?x=1",
            "https://github.com/acme/api",
            "https://github.com/acme/team/api.git",
            "https://github.com/../api.git",
            "https://github.com/-acme/api.git",
            "https://github.com/acme/.git",
            "http://10.0.0.1/acme/api.git",
        ] {
            let err = validate_clone_url(bad).unwrap_err();
            assert_eq!(err.code, ErrorCode::CommandNotAllowed, "{bad}");
            assert!(!err.to_string().contains("ghp_secret"), "{bad}");
        }
        validate_clone_url("https://github.com/acme/my.repo_name-1.git").unwrap();
        validate_clone_url("https://acme.ghe.com/acme/api.git").unwrap();
        assert!(valid_segment(".github"));
        assert!(!valid_segment("a/b") && !valid_segment("..") && !valid_segment(""));
    }
}
