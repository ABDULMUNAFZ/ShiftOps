# Architecture

## Product model

A **campaign** is one codebase-wide change. It moves through these stages:

1. **Define**: the change specification, implementer instructions and verification policy.
2. **Discover**: find affected repositories (local checkouts, GitHub organisations).
3. **Plan**: order repositories by dependency into parallel **waves** (`planWaves` in `packages/core`).
4. **Delegate**: one Devin session per repository (`packages/devin` contract).
5. **Verify**: ShiftOps re-runs the policy's commands against the agent's branch (`packages/verification`).
6. **Track**: campaign state is persisted locally and shown per repository.

The persistence for every stage exists (schema version 4). Discovery, planning, delegation and verification logic do not yet exist. See `PROJECT_STATE.md`.

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
