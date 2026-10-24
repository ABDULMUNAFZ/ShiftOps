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
- **Category policy**: the application runner allows only `environment_check`; discovery uses a separate runner that allows only `readonly_git`. A dedicated managed-clone runner permits exactly one policy-built initial `git clone` into an approved workspace. Devin runners permit only the vectors listed under Devin delegation. A verification runner permits only the exact (program, arguments) vectors of one approved verification plan, inside one trusted checkout (see Verification). Other mutating Git, Docker and custom commands remain denied, as do install, build, test and lint commands outside an approved plan.
- **Environment**: cleared, plus an allow-list of inherited variables. `PATH` cannot be overridden.
- **Working directory**: validated, and must be an existing directory under an allowed root (OS temp or app data).
- **Bounded runs**: every run has a timeout and an output-size limit. Output past the limit is discarded, and the pipe is drained so the child cannot block.
- **Cancellation and timeout**: SIGTERM, then SIGKILL, sent to the whole process group (Unix), followed by a final group SIGKILL after exit, so grandchildren do not survive.

### GitHub

- Tokens (OAuth from device flow, or a fine-grained PAT) are stored only in the OS keychain, read only by `github/`, and sent only to the configured API base over https. The web view cannot change that base URL: it is read from the environment or build-time configuration in Rust.
- The device code never leaves Rust. The UI shows only the user code and GitHub's verification URL, which passes the external-URL validator.
- A token is validated before it is stored; a rejected token stores nothing. A 401 later marks the account invalid.
- Device codes, tokens and `Authorization` headers are never logged. Error details contain only status, path and GitHub's message, redacted.
- Disconnect requires confirmation and deletes the keychain entry.
- Requested repository files go through the secret-file deny-list (no `.env`, keys or credentials) and redaction, with a 512 KiB limit.

### Repository discovery and Git

- Imported repositories are untrusted. Discovery never executes repository code, installs dependencies or writes to a repository.
- Managed clone disables hooks, templates, submodule recursion, interactive credential prompts and LFS smudging. Sources must be credential-free HTTPS GitHub URLs (loopback HTTP exists only for tests), and destinations must be canonical children of a user-approved dedicated workspace.
- Failed or cancelled clone folders are retained. Cleanup is a separate exact-path-confirmed operation that refuses roots, home, links, registered repositories and external checkouts.
- Repository removal deletes only the SQLite record after confirmation. It never calls a filesystem deletion path. Trust changes and removals are recorded as activity.
- Submission Readiness lists tracked files with the read-only Git policy and reads at most 20,000 files of 512 KiB each. It refuses path escapes and binary files and returns only potential-secret rule names and paths, never matched values.
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
- A database whose schema version is newer than the running ShiftOps supports is refused with `DATABASE_MIGRATION_FAILED` and left untouched; nothing reads or writes an unknown schema.
- The schema enforces enums, ranges, uniqueness, foreign keys and self-reference rules, independently of application validation.

### Devin delegation

- The Devin CLI owns its credentials (`devin auth login`, stored by the CLI). ShiftOps stores none, never reads the CLI's credential file and passes no token in arguments or environment. Only the account email from `devin auth status` is shown.
- The `devin` category is enabled only through two policies: `devin_cli` (exactly `version`, `auth status`, `doctor`) and `devin_dispatch` (exactly `-p <prompt> --permission-mode smart --sandbox --respect-workspace-trust false`, working directory inside one trusted checkout). Any other argument, program or category is `COMMAND_NOT_ALLOWED`.
- The prompt is the only variable argument. `devin::validate_prompt` rejects a prompt that is empty or whitespace-only, whose first non-whitespace character is `-` (the CLI could read it as an option such as `--help` or `-p`), that contains a NUL byte, or that exceeds 200,000 bytes. Hyphens later in the text are allowed. The check runs when the dispatch spec is built and again in the execution policy, so a hand-built argument vector cannot bypass it.
- Dispatch requires, in order: explicit confirmation, a recorded approval of the task's stored content hash, no outstanding planning review flag, classification `affected`, repository trust `trusted`, a canonical local checkout, and an installed, authenticated CLI. Every gate is tested over IPC.
- `--sandbox` runs Devin's exec tool under macOS seatbelt / Linux bwrap, limited to writes inside the workspace. `--permission-mode smart` auto-runs only actions Devin's safety model judges safe. `--respect-workspace-trust false` is required because print mode cannot answer Devin's own trust prompt; ShiftOps' repository trust gate replaces it.
- Task text contains scan evidence, which the scanner already redacts, and no credentials. Run output goes through the redacted run log like every other process.
- Devin is told not to push, open pull requests or change remotes. ShiftOps cannot enforce that beyond the sandbox; verification checks the resulting working tree independently.
- Dispatch claims the task atomically (`pending` to `queued`) before any process starts, and re-checks that the stored instructions still match the approved hash. A database index allows at most one live session per campaign member. Dispatch refuses a checkout with uncommitted changes, so Devin's work never mixes with the user's.
- Sessions still live when ShiftOps stops are closed on the next launch as `failed`, never `succeeded`, and recorded.

- **Open item:** during development, macOS XProtect reported "Malicious Script Blocked" while the Devin CLI was being troubleshot. The blocked item has not been identified. The installed `devin` binary passes `codesign --verify --strict` (Developer ID Application EXAFUNCTION, INC., 83Z2LHX6XW). That shows a valid signature, not that the binary is safe. The checksum has not yet been compared with a published release manifest. Never bypass Gatekeeper or XProtect to run it; live dispatch stays unverified until this is resolved.

### Verification

- Verification executes the repository's own scripts, so it requires trust `trusted` (checked in Rust, not only in the UI), an explicit confirmation, and the plan hash the user reviewed.
- The plan is derived only from the checkout's root `package.json` and lockfile (both must be regular files, not links, at most 512 KiB). It contains at most one install, with lifecycle scripts disabled, plus the scripts named `typecheck`, `lint`, `build`, `test` and `check`. Nothing comes from model output or tool output.
- The plan hash covers the package manager, the lockfile name, every argument vector and every script body. A Devin session that edits a script changes the hash, so the automatic re-run after a repair is refused and a human must review the new plan.
- `ExecutionPolicy::verification` accepts only those exact vectors, only with `TrustRequirement::TrustedRepository`, and only inside that checkout. Processes get the usual cleared environment plus `CI=1`, `FORCE_COLOR=0` and `NO_COLOR=1`, with a 20-minute timeout and a 2 MiB output limit per step.
- Check output is redacted before it is stored or shown. Affected files are only paths that exist as regular files inside the checkout.
- Repair tasks are bounded to 2 per campaign member. They contain at most 80 lines and 6,000 characters of redacted output per check. Code fences inside the output are neutralised, and the task tells Devin that the output is data, not instructions. Repair tasks still need the normal approval and dispatch. A run cut short by a restart is closed as `error`, never `passed`.

### Agent output

Agent-produced changes are untrusted. A campaign member is marked `verified` only after ShiftOps itself has run the approved plan against the checkout and every check passed. A finished Devin session alone never marks anything verified.

### Repository files

- Removing a record and unlinking a checkout never touch files. Deleting files is limited to a checkout ShiftOps cloned. It must have ownership `shiftops_managed` and a succeeded clone record for the exact path, sit at `<approved workspace>/<owner>/<name>`, be a real folder that resolves to itself, have no other registered repository or tracked folder inside it, and have a clean working tree. The user must type the exact path. External checkouts are never deleted.
- Unlinking, deleting and removing are refused while a scan, verification or Devin session is live for the repository.

## Supported versions

No release has been published. Security fixes apply to `main`.
