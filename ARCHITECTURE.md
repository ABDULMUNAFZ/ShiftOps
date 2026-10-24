# Architecture

## Product model

A **campaign** is one codebase-wide change. It moves through these stages:

1. **Define**: the change specification, implementer instructions and verification policy.
2. **Discover**: find affected repositories (local checkouts, GitHub organisations).
3. **Plan**: order repositories by dependency into parallel **waves** (`planWaves` in `packages/core`).
4. **Delegate**: one Devin session per repository (`packages/devin` contract).
5. **Verify**: ShiftOps re-runs the policy's commands against the agent's branch (`packages/verification`).
6. **Track**: campaign state is persisted locally and shown per repository.

The persistence for every stage exists (schema version 7). Discovery, planning, delegation and verification logic do not yet exist. See `PROJECT_STATE.md`.

## Processes and boundaries

```
┌─────────────────────────────── Desktop app (Tauri 2) ───────────────────────────────┐
│  Web view (React 19, Vite)                 Rust core (tauri 2.12)                   │
│  ─────────────────────────                 ──────────────────────                   │
│  TanStack Query ── invoke() ── IPC ──▶     commands.rs (24 commands)                │
│  lib/native.ts: Zod-validated results,       ├─ db/        SQLite services          │
│    ShiftOpsError on rejection                │   ├─ migrations.rs + sql/*.sql       │
│  lib/queryKeys.ts + lib/invalidation.ts      │   └─ settings, repositories, …       │
│  lib/processStore.ts  ◀── events ──────      ├─ process/   safe process runner      │
│    ("shiftops://process")                    ├─ credentials.rs  OS keychain         │
│  Activity Centre, Settings diagnostics       ├─ paths.rs / urls.rs  validation      │
│                                              ├─ redact.rs / logging.rs              │
│                                              └─ environment.rs / shell_path.rs      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

- **All native work happens in Rust.** The web view has no shell, filesystem, HTTP, SQL or opener permission. It can only call the commands registered in `lib.rs`.
- **React never executes SQL.** Components call typed wrappers in `lib/native.ts`. Those call Tauri commands, which call service functions in `db/*`.
- **Every IPC response is validated** with Zod schemas from `packages/core` (`domain.ts`, `errors.ts`, `environment.ts`). A Rust/TypeScript mismatch becomes an `INTERNAL_ERROR`, never silent bad data.
- **Outside Tauri** (`pnpm dev:web`), `native.ts` throws `NativeUnavailableError` instead of returning anything. There is no mock mode.

## Error contract

Every command returns `Result<T, AppError>` (`src-tauri/src/error.rs`). Errors serialise as:

```json
{
  "code": "COMMAND_TIMED_OUT",
  "message": "…",
  "technicalDetails": "… (redacted)",
  "suggestedAction": "…",
  "recoverable": true,
  "context": { "…": "redacted metadata" }
}
```

IPC uses camelCase throughout, so the contract's `technical_details` and `suggested_action` appear as `technicalDetails` and `suggestedAction`. There are 16 stable codes (`ErrorCode` in Rust, `errorCodes` in `packages/core/src/errors.ts`). Messages, technical details and string context values pass through redaction on construction. Database and internal errors are written to the redacted log. In the web view, `ShiftOpsError` carries the contract. `ErrorDisplay` shows the code, message, suggested action and expandable, copyable details. `ErrorBoundary` (app root) and `RouteError` (router) catch render failures.

## Persistence

- SQLite via `rusqlite` (`bundled`), one connection behind a mutex (`db::Database`). `foreign_keys=ON`, `busy_timeout=5s`, WAL and `synchronous=NORMAL` are set when the connection opens, and foreign-key enforcement is verified.
- `Database::write` runs a closure in a transaction. Any error rolls back every write in it. Example: `campaigns::create_with_repositories` creates a campaign and its memberships together or not at all.
- **Migrations** (`db/migrations.rs`, SQL in `db/sql/`):
  - Each migration has a contiguous version and runs in its own transaction, together with its `schema_migrations` row and `PRAGMA user_version`.
  - A foreign-key check runs before commit. A failure rolls back and stops with `DATABASE_MIGRATION_FAILED`.
  - Re-running is a no-op.
  - Version-1 databases from Step 1 are detected by `user_version` and their bookkeeping is backfilled.
  - Before upgrading an existing database, `VACUUM INTO` writes `backups/shiftops-v<from>-<timestamp>.db`.
  - No `STRICT` tables, so macOS's sqlite3 3.32 can read the file.
- **Conventions**:
  - UUID v4 string ids.
  - UTC RFC 3339 timestamps with exactly three fractional digits (`2026-10-24T15:45:36.023Z`), so text order is time order.
  - `*_json` columns are written only through `json_text`, which checks shape and size.
  - Enums are enforced by `CHECK` constraints and parsed by `string_enum!` types.

| Table                                      | Notes                                                                                                                                   |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| `settings`                                 | `key`, `value_json`, `created_at`, `updated_at`. Keys are allow-listed.                                                                 |
| `repositories`                             | Unique `local_path` and case-insensitive unique (`github_host`, `github_owner`, `github_repo`). Removal never touches disk.             |
| `repository_scans`                         | State machine queued → running → completed / failed / cancelled. Cascades with its repository.                                          |
| `repository_dependencies`                  | `CHECK (source <> target)`, confidence 0–1. Confirmation is preserved when detection runs again.                                        |
| `campaigns`, `campaign_repositories`       | Status transition rules. Membership is unique per campaign and repository. A repository is `RESTRICT`ed while it belongs to a campaign. |
| `agent_tasks`, `agent_runs`                | Task history `RESTRICT`s campaign deletion. Runs cascade with their task.                                                               |
| `verification_runs`, `verification_checks` | Output is redacted and capped at 64 Ki characters. `agent_task_id` is `SET NULL`.                                                       |
| `activity_events`                          | Index `(created_at DESC)` with `rowid` as tiebreak.                                                                                     |
| `migration_patterns`                       | Editing returns the pattern to `candidate`. Source links are `SET NULL`.                                                                |
| `credential_metadata`                      | Non-secret facts only (timestamps, validation status).                                                                                  |
| `schema_migrations`                        | Applied migrations.                                                                                                                     |

## Credentials

`credentials.rs` wraps the `keyring` crate.

- **Service and account**: the service is `dev.shiftops.desktop`, and the account is `<provider>:<kind>`.
- **Slots**:
  - GitHub personal access token: manageable.
  - GitHub OAuth token: reserved for the device flow, so not manually storable.
  - Devin API token: manageable.
- **Writes**:
  - `put` writes and verifies by reading back.
  - `store_credential` and `delete_credential` update metadata and record activity, with no value.
- **What the web view gets**: only `CredentialMetadata`. `retrieve` is `pub(crate)` for later native integrations.
- **In memory**: secrets are wrapped in `Secret`, which zeroes on drop and prints `[REDACTED]`.

## Process runner

`process::ProcessRunner` executes every external program, including the environment probes.

- **Spec**: `CommandSpec` holds a label, category, trust requirement, a `Program` (name resolved on the login-shell search path, or an absolute path), an argument array, a working directory, extra env vars, literal secrets to mask, a timeout and an output limit.
- **Policy**: `ExecutionPolicy::step2` allows only `environment_check`. Every other category, and any `TrustedRepository` requirement, is denied with `COMMAND_NOT_ALLOWED`. The working directory must be an existing directory inside the temp or app-data directory, checked by `paths::validate`.
- **Spawn**:
  - No shell.
  - `env_clear()` plus an inherited allow-list (`HOME`, `USER`, `LANG`, …), with `PATH` set to the resolved search path.
  - stdin is null.
  - `kill_on_drop`.
  - On Unix, its own process group.
- **Streaming**: stdout and stderr are read in 8 KiB chunks, split into lines (max 16 KiB each) and redacted. They are batched every 50 ms or 256 lines into `Output` events, keeping stream identity.
- **Output limit**: the limit is the combined byte budget. Past it, a single truncation marker is emitted and the pipes keep draining, so the child never blocks.
- **Run log**: each run writes a redacted log to `<log dir>/runs/<run id>.log`.
- **Lifecycle**: a run ends on exit, timeout or cancellation.
  - Timeout and cancel send SIGTERM to the process group, wait 2 s, then send SIGKILL.
  - After any exit, the group gets a final SIGKILL to clean up stragglers.
  - The resulting status is `succeeded`, `failed`, `cancelled` or `timed_out`.
- **Identity and activity**: runs are identified by generated UUIDs, held in an in-memory registry and cancelled by `cancel(run_id)`. Repeated cancellation returns `not_running`. Start and finish are recorded as activity events, never per output line.
- **Bundled diagnostic**: the long-running diagnostic is the ShiftOps executable itself with `--shiftops-diagnostic-ticker`. `main.rs` handles the flag before Tauri starts.

Events are emitted on `shiftops://process`. `lib/processStore.ts` keeps the last 2000 lines per run. `NativeDiagnostics` renders the last 500.

## Local repository discovery

`discovery/` finds repositories under a folder the user picked (native picker from `tauri-plugin-dialog`, called from Rust; the web view has no dialog permission).

- **Walk** (`discovery/walk.rs`, blocking, off the async threads):
  - A directory containing `.git` (a directory, or a `gitdir:` file for worktrees and submodules) is recorded and not descended into.
  - Dependency, build, cache and all dot-directories are skipped.
  - Depth (0–12) and entry (200,000) limits apply.
  - Symlinks leaving the root are refused with a warning. Internal symlinks are followed only if the root allows it, with loop protection.
  - Unreadable folders become warnings without stopping the walk.
  - Cancellation is checked per directory, and progress is emitted at most every 100 ms.
- **Inspect** (`git/inspect.rs`): up to 4 repositories at a time, through a per-discovery `ProcessRunner` whose policy (`ExecutionPolicy::readonly_git`) allows only `readonly_git` with the working directory inside the root.
  - `git/policy.rs` accepts exactly seven argument vectors, each preceded by a fixed safety prefix: `--no-pager --no-optional-locks -c core.fsmonitor=false -c core.untrackedCache=false -c log.showSignature=false -c color.ui=false`.
  - Git runs with `GIT_TERMINAL_PROMPT=0`, `GIT_OPTIONAL_LOCKS=0` and `LC_ALL=C`.
  - Before running `git status`, the repository's own config is read. If it defines clean/smudge/process filters or includes other config files, status is skipped with a warning, because Git would execute them.
- **Collected per repository:** canonical path, name, kind (standard, worktree, submodule, git file), current branch or detached HEAD, HEAD commit, default branch (`origin/HEAD`), origin remote (credentials stripped; GitHub coordinates parsed from HTTPS, SCP-style and `ssh://` forms), last commit time, staged/modified/untracked counts.
- **Import** (`import_discovered`) takes the discovery id and the selected paths. It never takes client-supplied metadata. In one transaction it:
  - upserts the root in `repository_roots`;
  - inserts new repositories as `untrusted`, or refreshes the Git facts of existing ones without touching trust;
  - records activity.
- **GitHub identity:** the identity (host, owner, repo) stays unique. A second checkout or worktree of the same GitHub repository keeps its remote URL, gets no GitHub link, and carries a warning naming the checkout that holds the link.
- **Removing a root** (`remove_repository_root`, requires `confirm: true`) deletes only that record. Repositories keep their records with `root_id = NULL`. No file is deleted.
- **Events and results:** events go to `shiftops://discovery`. Results are also retrievable with `get_discovery`, so a discovery that finishes before the UI subscribes is not lost.

## Repository intelligence scanner

`scanner/` produces evidence-backed facts for one registered repository.

- **Traversal** (`files.rs`):
  - Depth 8, 60,000 entries, 2,000 relevant files.
  - Never follows symlinks. Never reads secret files (`.env*`, `.npmrc`, `.yarnrc*`, `.netrc`, keys, certificates, `credentials*`, `secrets*`).
  - Skips dependency, build, cache and fixture folders, and nested repositories.
  - Reads only allow-listed manifest and config names, each up to 512 KiB. Larger files are fingerprinted by size and modification time.
  - Counts source files per language.
- **Detection** (`detect.rs`): each fact carries its file, a 1-based line and/or a JSON pointer, the method and a confidence.
  - Node/TypeScript: `package.json` (engines, `packageManager`, Volta, scripts, workspaces, dependencies and frameworks), `.nvmrc`, `.node-version`, `.tool-versions`, npm/pnpm/yarn/Bun lockfiles, `pnpm-workspace.yaml`, Turbo/Nx/Lerna, `tsconfig`.
  - Python: `pyproject` (PEP 621 and Poetry), `requirements*.txt`, Pipfile, `.python-version`.
  - Java: Maven and Gradle (Java version, Spring Boot).
  - .NET: `*.csproj`, `global.json`.
  - Go: `go.mod`, `go.work`.
  - Rust: `Cargo.toml` (workspace, `rust-version`, dependencies), `rust-toolchain`.
  - Docker: `FROM` and compose `image:` lines, with the implied runtime.
  - CI: GitHub Actions `setup-*` steps (matrix values resolved), GitLab, CircleCI, Jenkins, Azure.
- **Conflicts**: exact runtime declarations in the same directory must agree. A `package.json` engines range must admit every exact declaration from its own directory or an ancestor, with CI treated as repository-wide. Two JavaScript package managers in one directory is also a conflict. Duplicate conflicts are collapsed.
- **Cache**: the key is sha256(scanner version, HEAD commit from read-only `git rev-parse`, sha256 of every relevant file's path and content). Uncommitted edits to relevant files change the key; other files do not. A cache hit returns the stored scan (`repository.scan_reused` activity). Rescan forces a new scan.
- **Storage**: facts and conflicts reference their scan and cascade with it. The last 3 scans per repository are kept. The scan's `result_json` holds the headline summary shown in lists.
- **Jobs**: one scan per repository at a time. Progress goes to `shiftops://scan`, cancellation is by repository id, and every outcome (completed, cached, cancelled, failed) is recorded as activity.

## GitHub integration

`github/` is the only code that talks to GitHub. It runs in Rust; the web view never sees a token.

- **Configuration** (`GithubConfig::from_env`):
  - Variables: `VITE_GITHUB_OAUTH_CLIENT_ID`, `VITE_GITHUB_HOST`, `VITE_GITHUB_API_BASE_URL`. Order of precedence: runtime environment, then `apps/desktop/.env` (forwarded by `build.rs` at build time), then the GitHub.com defaults.
  - URLs must be https (http only to loopback, for tests) and carry no credentials.
  - The API base defaults per host: `api.github.com`, `api.<tenant>.ghe.com`, or `<host>/api/v3`.
- **Client** (`client.rs`):
  - Sends `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`, `User-Agent: ShiftOps/<version>` and a Bearer token from the keychain.
  - Responses are deserialised into typed structs; an unexpected shape is an error. `Link` headers drive pagination.
  - Errors map to: 401 → `GITHUB_AUTHENTICATION_FAILED` (marks the account invalid); 403 or 429 with an exhausted limit or `retry-after` → `GITHUB_RATE_LIMITED` (context: secondary, `resetAt`, `retryAfterSeconds`); other 403 → `GITHUB_PERMISSION_DENIED`; 404 → `RECORD_NOT_FOUND`; 422 → `VALIDATION_FAILED`; 5xx and transport failures → `NETWORK_FAILED`.
- **Device flow** (`DeviceFlows`):
  - `POST /login/device/code` with the client ID and `repo read:org`. The UI receives only the user code, verification URL (validated), expiry and interval.
  - It polls `POST /login/oauth/access_token` at the server interval: `authorization_pending` continues, `slow_down` raises the interval, `expired_token` → expired, `access_denied` → denied, anything else → failed. Cancellation is checked every 50 ms.
  - On success the token is validated (`/user`, `/user/orgs`), stored in the keychain, and the account recorded.
  - Events go to `shiftops://github-auth`.
- **PAT:** validated against the same endpoints before anything is stored.
- **Accounts:** one connected account. SQLite holds the login, id, auth method, keychain credential kind, scopes (when GitHub reports them; fine-grained tokens do not), validation time and the last rate-limit headers.
- **Persistence:** browsed repositories and pull requests are stored on every fetch, keyed by stable GitHub ids, so renames update in place. A pull request's mergeability and checks are kept across list refreshes until its head SHA changes.
- **Import modes:** metadata-only creates an untrusted record; managed clone uses the single policy-built mutating Git command inside an explicitly approved workspace; link-existing inspects a checkout read-only and marks it external. All three converge on the same repository id.
- **Managed clone boundary:** destinations are exactly two path segments below the canonical approved root. Hooks, templates, submodules, interactive credentials, LFS smudging and non-HTTPS transports are disabled. Clone progress is emitted on `shiftops://clone`; failures and cancellations retain their folder until an exact-path confirmed cleanup passes all safety checks.
- **Local link boundary:** normalized GitHub identities are compared before linking. A mismatch override records the actual identity and survives rediscovery. External checkouts are never modified or deleted.

## Submission Readiness

`readiness.rs` collects evidence from the ShiftOps checkout through the exact read-only Git policy. It checks required files, remote/HEAD/origin state, dirty and ahead state, participant TODOs, a bounded tracked-file secret scan and the optional gitignored validation record. Secret matches return only a path and rule name. A pure status function is covered independently; it never produces `Submitted`. The desktop view exposes refresh, checklist copy, validated document opening and the GitHub repository link.

- **Cancellation:** browse, details and pull-request listing accept a `requestId`, cancelled through `github_cancel_request`.

## Validation

- `paths::validate`: rejects relative paths, null bytes and paths over 4096 bytes. It normalises `..` and canonicalises existing paths, or for a missing path, its deepest existing ancestor. It reports file, directory, missing or other, and rejects broken symlinks. Paths outside the allowed roots are rejected. A path that is inside a root by name but resolves outside it through a symlink is rejected unless `allowing_symlink_escape()` is set explicitly.
- `urls::validate_external_url`: allows `https` with a host and no credentials, and `http` to loopback hosts only (documented for local development servers). It requires a literal `scheme://host` and rejects everything else. `open_external_url` is the only way the web view opens links, through `tauri-plugin-opener`'s Rust API. The plugin's JS commands are not permitted by the capability file.

## Activity Centre

Events are recorded for:

- Database opened and each migration applied.
- Environment checks completed.
- Credential stored, replaced or deleted.
- Process started, completed, cancelled, failed or timed out (`process.failed`), and processes rejected before starting.

The Activity Centre (header button, `⌘J`, command palette) lists events newest first. It has severity, entity-type and text filters, 25 per page, refresh, expandable metadata and "Copy diagnostic details". Pages that depend on activity are refreshed through `invalidateAfter(kind)` in `lib/invalidation.ts`, the single map from change kind to stale query keys.

## Environment checks

`environment.rs` runs each probe through the process runner (`environment_check` category, activity off, 64 KiB limit, 10 s timeout and 5 s for `docker info`). `shell_path.rs` reads PATH once from `$SHELL -ilc` (falling back to `-lc`), merges the process PATH and well-known install directories, and does its own executable lookup. GUI apps do not inherit the shell PATH.

## Theme

The theme setting lives in SQLite. `useApplyTheme` toggles the `dark` class and follows the OS in `system` mode. A `localStorage` copy of the resolved theme prevents a flash before SQLite loads.

## Shared UI and website

`packages/ui` exports design tokens and components shared by both apps. The website is Next.js 16 with `output: "export"`.

## Testing

| Layer         | Tool                                                                   | What it covers                                                                                                                                                              |
| ------------- | ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Packages      | Vitest                                                                 | Planner, settings parsing, error and domain contracts, remote parsing, policy verdicts                                                                                      |
| Desktop UI    | Vitest + Testing Library                                               | IPC validation, structured errors, invalidation, database health, credential metadata, activity pagination, diagnostic cancellation                                         |
| Rust units    | `cargo test`                                                           | Migrations (fresh, v1 upgrade, rollback, idempotence), constraints, transactions, services, redaction, paths, URLs, process runner, real keychain under a test service name |
| Rust IPC      | `cargo test` + `tauri::test`                                           | Commands through Tauri's IPC and ACL. The secret never returns over IPC, logs or activity.                                                                                  |
| Upgrade drill | `cargo test upgrade_rehearsal -- --ignored` with `SHIFTOPS_UPGRADE_DB` | Migrate a copy of a real database                                                                                                                                           |
