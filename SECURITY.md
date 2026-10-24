# Security

## Reporting a vulnerability

Report vulnerabilities privately using GitHub's **Report a vulnerability** (security advisory) form on this repository. Do not open a public issue. Include reproduction steps and the affected version or commit.

## Threat model

ShiftOps will hold credentials that can push code to many repositories and start agent sessions, and it will run commands inside those repositories. The design assumes the web view is the most exposed component and keeps it powerless. It treats every external program's output as untrusted text that may contain secrets.

### Credentials

- Stored only in the OS credential store via the `keyring` crate: macOS Keychain, Windows Credential Manager or Secret Service on Linux. Service: `dev.shiftops.desktop`. Account: `<provider>:<kind>`.
- The value crosses IPC exactly once, inbound, in `store_credential`. It is wrapped in a `Secret` that zeroes its memory on drop, validated, written, verified by reading back, and dropped.
- No command returns a secret. `list_credentials`, `store_credential` and `delete_credential` return `CredentialMetadata`: provider, kind, identifier, exists, timestamps and validation status. The TypeScript schema is `.strict()`, so a response carrying any extra field is rejected.
- `retrieve` is `pub(crate)` and reserved for native integration code.
- SQLite stores only non-secret metadata (`credential_metadata`). Activity events and logs record the provider and kind, never the value.
- The UI never re-displays a stored value. Replacing one requires entering a new value. Deleting requires an explicit confirmation step, and the command itself rejects `confirm: false`.
- Tests use a unique `dev.shiftops.desktop.test.<uuid>` service and remove their entries in a `Drop` guard, so cleanup runs even if an assertion fails.

### Redaction

`redact.rs` is applied to log lines, error messages, technical details and context, activity summaries and metadata, process output (events, captured results and run logs), and verification output. It masks:

- GitHub tokens (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`)
- `Authorization` and `Proxy-Authorization` header values
- `Bearer` and `token` schemes
- Credentials in URLs (`scheme://user:pass@` and `scheme://token@`)
- Generic key/value secrets: `api_key`, `access_token`, `client_secret`, `password`, `secret`, `token` and similar, in env, JSON, YAML, query string or CLI form

Callers can also register literal secrets, for example a token passed in a child environment, which are masked wherever they appear.

### Web view

- Commands are limited to those registered in `lib.rs`. The capability file grants `core:default` only. No shell, fs, http, sql or opener permissions are granted. `tauri-plugin-opener` is used from Rust only.
- Content Security Policy: `default-src 'self'; script-src 'self'; connect-src ipc: http://ipc.localhost`. The web view cannot load remote scripts or make network requests.
- Domain data (repositories, campaigns, agent tasks, verification and patterns) is read-only from the web view in this version.

### Process execution

- **No shell, no arbitrary commands**: programs are executed directly with argument arrays. The web view cannot supply a program or arguments. `start_diagnostic` accepts one of four fixed kinds.
- **Category policy**: the application runner allows only `environment_check`; discovery uses a separate runner that allows only `readonly_git` (see above). `mutating_git`, `dependency_install`, `build`, `test`, `lint`, `docker`, `devin` and `custom` are denied everywhere, and so is any command that requires a trusted repository.
- **Environment**: cleared, plus an allow-list of inherited variables. `PATH` cannot be overridden.
- **Working directory**: validated, and must be an existing directory under an allowed root (OS temp or app data).
- **Bounded runs**: every run has a timeout and an output-size limit. Output past the limit is discarded, and the pipe is drained so the child cannot block.
- **Cancellation and timeout**: SIGTERM, then SIGKILL, sent to the whole process group (Unix), followed by a final group SIGKILL after exit, so grandchildren do not survive.

### Repository discovery and Git

- Imported repositories are untrusted. Discovery never executes repository code, installs dependencies or writes to a repository.
- Only `readonly_git` is enabled, and only for the exact argument vectors in `git/policy.rs`, run with the working directory inside the chosen root. Checkout, reset, clean, commit, push, pull, fetch, merge, rebase and config changes are rejected with `COMMAND_NOT_ALLOWED`.
- Every Git call carries a fixed prefix that disables `core.fsmonitor` and signature verification and avoids optional locks. A repository's own config can still make `git status` run clean/smudge filters, or include other files. ShiftOps reads that config first and skips `status` (with a warning) when such keys are present. A test proves a planted filter never runs.
- Credentials in remote URLs (`https://token@host/...`) are stripped before the URL is stored or shown.
- Symbolic links that leave the chosen folder are never followed.

### Repository scanning

- The scanner reads only allow-listed manifest and configuration files from the repository's own tree. It never follows symbolic links, never executes repository code and runs one read-only Git command (`rev-parse HEAD`).
- Secret files (`.env*`, `.npmrc`, `.yarnrc*`, `.netrc`, `.pypirc`, `.git-credentials`, private keys and certificates, `credentials*`, `secrets*`, keystores) are never opened. A test plants a `.env` value and proves it never reaches stored facts.
- Values that are stored (scripts, image names, versions) pass through redaction.
- File size, depth, entry and file-count limits bound memory and time.

### Paths and URLs

- Paths are canonicalised and checked against allowed roots. Symlinks that escape a root are refused unless explicitly approved. Null bytes, relative and malformed paths are rejected.
- External URLs: `https` only, plus `http` to `localhost`, `127.0.0.1` or `[::1]` for local development. `file:`, `javascript:`, `data:` and other schemes, embedded credentials and malformed URLs are rejected.

### Database

- Migrations are transactional and verified with a foreign-key check before commit. An existing database is copied to `backups/` before an upgrade.
- The schema enforces enums, ranges, uniqueness, foreign keys and self-reference rules, independently of application validation.

### Agent output

Agent-produced changes are untrusted. A repository will be marked verified only after ShiftOps itself re-runs the policy's commands against the branch. This is not yet implemented; see `PROJECT_STATE.md`.

## Supported versions

No release has been published. Security fixes apply to `main`.
