//! The read-only Git policy. ShiftOps may run only the exact argument vectors
//! listed here, each preceded by `SAFE_PREFIX`. Anything else, including
//! checkout, reset, clean, commit, push, pull, fetch, merge, rebase and any
//! config change, is rejected with `COMMAND_NOT_ALLOWED`.

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

/// Every permitted command after the prefix, exactly.
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
}
