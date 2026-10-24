# Project state

Last updated: 2026-10-24 (IST). ShiftOps is built in six tasks.

- **Task 1** (foundation) is complete: commit `cca3cf6`.
- **Task 2** (persistence, credentials, process execution, activity) is complete: commit `4701627`, followed by the Windows CI fix `5c6ea42`.
- **Task 3** (product profile, local repository intelligence, GitHub integration) is **in progress**.
  - Done: milestone 1, product profile and documentation (`d388ff4`).
  - Done: milestone 2, local repository discovery (schema 5). See `CHANGELOG.md`.
  - Done: milestone 3, repository intelligence scanner (schema 6).
  - Next: milestone 4, GitHub integration. Its import-mode spec and the specs for milestones 5–6 were cut off and have not been received.
- **Tasks 4–6** are not started. Devin integration does not exist.

Remote: `https://github.com/ABDULMUNAFZ/ShiftOps.git`. `main` tracks `origin/main`. CI runs on GitHub Actions (Linux, macOS, Windows). Sections below were written at the end of Task 2; the Task 3 additions are in `CHANGELOG.md`.

## 1. Product architecture

ShiftOps is a Tauri 2 desktop app with a React 19 web view and a Rust core, plus a static Next.js website, in one pnpm monorepo. Full detail is in [ARCHITECTURE.md](ARCHITECTURE.md); the threat model is in [SECURITY.md](SECURITY.md).

**Rust core** (`apps/desktop/src-tauri/src`):

| Module                            | Responsibility                                                                         |
| --------------------------------- | -------------------------------------------------------------------------------------- |
| `db/`                             | SQLite connection, migrations (`migrations.rs`, `sql/*.sql`), typed services per table |
| `credentials.rs`                  | OS keychain                                                                            |
| `process/`                        | Safe process runner and execution policy                                               |
| `paths.rs`, `urls.rs`             | Path and URL validation                                                                |
| `redact.rs`, `logging.rs`         | Secret redaction and the redacted log                                                  |
| `error.rs`                        | Structured error contract                                                              |
| `environment.rs`, `shell_path.rs` | First-run checks and login-shell PATH                                                  |
| `commands.rs`                     | 24 Tauri commands                                                                      |

**Web view** (`apps/desktop/src`):

| File                                                  | Responsibility                                  |
| ----------------------------------------------------- | ----------------------------------------------- |
| `lib/native.ts`                                       | Zod-validated bridge; throws `ShiftOpsError`    |
| `lib/queryKeys.ts`                                    | Query-key factory                               |
| `lib/invalidation.ts`                                 | Central invalidation map                        |
| `lib/processStore.ts`                                 | Process event stream                            |
| `components/ErrorDisplay`, `components/ErrorBoundary` | Structured error display and render-error catch |
| `features/activity/ActivityCentre`                    | Activity Centre                                 |
| `features/credentials/CredentialSecurity`             | Credential management                           |
| `features/diagnostics/NativeDiagnostics`              | Native diagnostics                              |
| `features/health/DatabaseHealthCard`                  | Database health                                 |

**Shared contracts** (`packages/core`): `domain.ts` (every native model), `errors.ts` (error codes and `ShiftOpsError`), `settings.ts`, `environment.ts` and `planning.ts`.

## 2. Completed functionality

### Step 1 (unchanged, still passing)

- Shell with 7 sections, collapsible sidebar and command palette.
- Light, dark and system themes, persisted in SQLite.
- First-run environment checks.
- Website: landing, docs, security, privacy and download pages.

### Step 2

**Persistence**

- Schema version **4**. Migrations are ordered, transactional and recorded in `schema_migrations`.
- Each migration is checked for foreign-key violations before commit. A failure rolls back and stops.
- Re-running migrations is idempotent. A v1 database is backfilled, then upgraded in place.
- An automatic `VACUUM INTO` backup is written before upgrading an existing database.
- Foreign keys are enforced and verified on every connection.

**Domain tables**

- 12 domain tables, each with constraints and indexes: settings, repositories, scans, dependencies, campaigns, campaign repositories, agent tasks, agent runs, verification runs, verification checks, activity events and migration patterns.
- Plus `credential_metadata` and `schema_migrations`.

**Typed services**

- Every table has a typed Rust service with create, get, update, delete (where appropriate), paginated lists, status filters and search.
- Campaign creation and repository attachment run in one transaction, so a failure leaves no partial campaign.

**Structured errors**

- One error contract with 16 stable codes. It is redacted, logged when unexpected and validated in TypeScript.
- Error display has expandable, copyable details. Error boundaries cover the app root and router.

**Credentials**

- Real OS keychain storage for the GitHub personal access token and the Devin API token: store, check existence, replace, delete.
- The GitHub OAuth token slot is reported but not manually storable.
- Secrets never return to React. Only metadata is stored in SQLite.
- Deleting requires confirmation in the UI and `confirm: true` in the command.
- Integrations and Settings include a Credential Security section with keychain availability.

**Process runner**

- No shell. Argument arrays only.
- Environment is an allow-list. The working directory is validated.
- Timeout and output limit apply to every run.
- Stdout and stderr stream as separate, redacted, batched events.
- Cancellation and timeout terminate the whole process group. Cancellation is idempotent.
- Redacted run logs are written. Run IDs are UUIDs held in an in-memory registry.
- Policy: only `environment_check` may run. Every other category, and any command that needs a trusted repository, is denied.
- Environment checks now use the same runner.

**Native diagnostics** (Settings)

- Actions: re-run environment checks, `git --version`, `node --version`, `pnpm --version`, and a bundled 60-second ticker (the app binary itself with `--shiftops-diagnostic-ticker`).
- Streamed output, Cancel button and structured final result.
- No arbitrary command entry.

**Path and URL validation**

- Paths: canonicalised; null-byte, relative and malformed input rejected; files, directories and missing paths distinguished; symlink escapes refused unless explicitly approved.
- URLs: `https` allowed, plus loopback `http`. Everything else is rejected.
- `open_external_url` is the only way the UI opens links, used by the GitHub token help link.

**Activity Centre**

- Persisted events for:
  - Database opened.
  - Migration applied.
  - Environment checks completed.
  - Credential stored, replaced or deleted.
  - Process started, completed, cancelled, failed or timed out, and rejected before starting.
- No per-line events.
- Header button (with count), `⌘J` and a palette entry open it.
- Severity, entity and text filters; 25 events per page; refresh; expandable metadata; copy diagnostic details; empty state.

**UI integration**

- Overview: environment readiness, database health, keychain health with credential presence, and campaign count from the database.
- Integrations:
  - GitHub shows "Not configured" unless a credential exists.
  - Devin shows CLI detected or not detected, and API credential stored or not stored.
  - Neither is described as connected.
- Settings: data directory, database path, log directory, schema version, database health with applied migrations, execution timeout and output-limit form, credential security, native diagnostics, environment checks.
- Repositories, Campaigns, Agent Runs and Verification read their real (empty) tables.

**Tooling**

- Prettier (`pnpm format`, `pnpm format:check`). `pnpm check` now also runs the format check and rustfmt.

## 3. Current work

None in progress. Step 2 is closed, pending your manual UI pass (§7) and a commit.

## 4. Decisions and reasons

| Decision                                                                                                                                 | Reason                                                                                                                                                       |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| One `rusqlite` connection behind a mutex                                                                                                 | Single-user desktop app with short operations; simplest correct concurrency. `Database::write` gives every multi-row operation a transaction.                |
| One transaction per migration                                                                                                            | A failure leaves the database at the last good version. Each step is recorded in `schema_migrations` with `user_version`.                                    |
| v1 bookkeeping backfilled, never re-run                                                                                                  | Step 1 databases have no tracking table. `user_version = 1` identifies them exactly.                                                                         |
| `settings` rebuilt rather than altered                                                                                                   | SQLite cannot add a NOT NULL `created_at` without a constant default. Legacy rows use their `updated_at` as `created_at`, the best available truth.          |
| `VACUUM INTO` backup before upgrading                                                                                                    | Cheap, consistent copy, so a bad future migration is recoverable. Backups are written only for existing databases with pending migrations.                   |
| No `STRICT`; JSON validated in Rust, not `json_valid()`                                                                                  | macOS sqlite3 3.32 must read the file. CHECK constraints with JSON functions would depend on the reader's build.                                             |
| Fixed-width millisecond UTC timestamps                                                                                                   | Lexicographic order equals chronological order, which the indexes rely on. Matches v1's `strftime` format.                                                   |
| `rowid` as the activity ordering tiebreak                                                                                                | Several events can share a millisecond (migration events do). UUIDs are random.                                                                              |
| `RESTRICT` repository removal while in a campaign; agent history `RESTRICT`s campaign deletion; `SET NULL` for optional provenance links | Audit history must not disappear as a side effect.                                                                                                           |
| IPC payloads are camelCase, including the error contract (`technicalDetails`, `suggestedAction`)                                         | Consistent with every other model crossing IPC. Field meaning is unchanged from the specification.                                                           |
| `keyring` 3.6.3 (kept)                                                                                                                   | Maintained, already in use, supports macOS, Windows and Secret Service. 4.x restructures its API without adding anything needed here.                        |
| Credential metadata in SQLite, existence from the keychain                                                                               | The keychain is the source of truth. Stale metadata (entry deleted outside ShiftOps) is ignored.                                                             |
| OAuth slot not manually storable                                                                                                         | It will be written by the device flow. Accepting pasted OAuth tokens would bypass scope checks.                                                              |
| Process group per child and a final group SIGKILL                                                                                        | Cancelling `sh` alone leaves grandchildren such as `sleep` running. The test proves the grandchild dies.                                                     |
| Combined stdout+stderr byte limit, with draining                                                                                         | Bounded memory, and the child never blocks on a full pipe.                                                                                                   |
| Batched output events (50 ms or 256 lines); UI keeps 2000 lines and renders 500                                                          | Keeps the web view responsive under heavy output.                                                                                                            |
| Bundled diagnostic is the app binary with a flag                                                                                         | "Bundled" without shipping another executable. Cannot run arbitrary code.                                                                                    |
| Step 2 policy allows only `environment_check`                                                                                            | Repository-executed commands need repository trust, which does not exist yet.                                                                                |
| `tauri-plugin-opener` used from Rust only                                                                                                | Gives a maintained cross-platform opener without granting the web view any opener permission.                                                                |
| Read-only domain commands in this step                                                                                                   | Nothing in the UI legitimately creates repositories or campaigns yet. Exposing mutations would invite fake data. The service functions exist and are tested. |
| `#![cfg_attr(not(test), allow(dead_code))]` on service modules                                                                           | Operations needed by Steps 3–6 exist and are tested, but are not called from production code yet. Clippy stays at `-D warnings`.                             |
| Prettier at `printWidth` 120                                                                                                             | Matches existing code. Adds the requested formatting check.                                                                                                  |
| Timeout and output limit as settings (1–3600 s, 4 KiB–16 MiB)                                                                            | Range-checked in Rust because native code reads them; also validated in the form.                                                                            |

## 5. Commands run (this step)

```sh
# Pre-flight
cat AGENTS.md README.md ARCHITECTURE.md SECURITY.md PROJECT_STATE.md \
    src-tauri/src/{store,commands,error,lib}.rs        # read current implementation
git status --short; git log                             # untracked tree, no commits
pgrep -fl "shiftops-desktop|tauri dev|vite"             # Step 1 dev app from my earlier task (28815/29048/29074)
sqlite3 -readonly <real db> "pragma user_version; select sql from sqlite_master; select * from settings"  # v1, 0 rows
TaskStop <earlier dev task>                             # stopped only those PIDs, before editing Rust
pnpm check; pnpm fmt:rust; website build; desktop build # baseline: 35 JS, 16 Rust, builds OK
git add -A && git commit                                # cca3cf6 chore: establish ShiftOps foundation

# Versions
cargo info uuid@1 regex@1 url@2 libc@0.2 tauri-plugin-opener@2 tokio@1
npm view prettier version   # with npm_config_cache in the scratchpad (root-owned ~/.npm untouched)

# Development loop
cargo build --tests; cargo test; cargo clippy --all-targets -- -D warnings; cargo fmt
pnpm install; pnpm typecheck; pnpm lint; pnpm test; pnpm format

# Verification
pnpm check                                  # format:check, typecheck, lint, test, fmt:rust, test:rust, lint:rust
pnpm --filter @shiftops/website build
pnpm --filter @shiftops/desktop build
SHIFTOPS_UPGRADE_DB=<scratch copy> cargo test upgrade_rehearsal -- --ignored --nocapture
pnpm dev:desktop                            # launch, then stop, then relaunch (restart persistence)
sqlite3 -readonly <real db> "...schema_migrations / settings / activity_events..."
./target/debug/shiftops-desktop --shiftops-diagnostic-ticker   # then SIGTERM by its own PID
pnpm dev:website + curl of 5 routes         # then stopped by its own PIDs
```

## 6. Test and build results (final run)

| Check                                    | Result                                                                                              |
| ---------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `pnpm format:check`                      | All matched files use Prettier code style                                                           |
| `pnpm typecheck`                         | 7/7 workspaces pass                                                                                 |
| `pnpm lint`                              | 0 problems                                                                                          |
| `pnpm test`                              | **62 passed** in 14 files: core 13, github 10, devin 2, verification 4, ui 3, website 1, desktop 29 |
| `cargo fmt --check`                      | clean                                                                                               |
| `cargo clippy --all-targets -D warnings` | clean                                                                                               |
| `cargo test`                             | **79 passed, 0 failed, 1 ignored** (`upgrade_rehearsal`, manual, needs `SHIFTOPS_UPGRADE_DB`)       |
| Website `next build`                     | 6 static routes                                                                                     |
| Desktop `vite build`                     | OK (593 kB single-chunk warning, as in Step 1)                                                      |
| `tauri dev`                              | Compiles and launches. Real database upgraded 1 → 4.                                                |

Rust tests by module:

| Module             | Tests |
| ------------------ | ----- |
| `process::runner`  | 11    |
| `db::campaigns`    | 8     |
| `ipc_tests`        | 8     |
| `db::repositories` | 7     |
| `paths`            | 7     |
| `environment`      | 6     |
| `redact`           | 6     |
| `db::migrations`   | 5     |
| `credentials`      | 3     |
| `db::agents`       | 3     |
| `db::settings`     | 3     |
| `db::activity`     | 2     |
| `db::verification` | 2     |
| `error`            | 2     |
| `shell_path`       | 2     |
| `urls`             | 2     |
| `db::patterns`     | 1     |
| `time_util`        | 1     |

Required coverage, mapped to tests:

- **Database**
  - Empty database to latest: `empty_database_reaches_latest_version`.
  - v1 upgrade without data loss, plus a readable backup: `version_one_database_upgrades_without_data_loss`.
  - Rollback after intentional failure: `failed_migration_rolls_back_and_stops`.
  - Foreign keys: `scan_requires_existing_repository`, `requires_existing_membership`.
  - Campaign transaction: `failed_attachment_leaves_no_partial_campaign`.
  - Unique constraints: `unique_local_path_and_github_identity`, `one_membership_per_campaign_and_repository`.
  - Pagination and filtering: `list_filters_searches_and_paginates`, `records_redacted_events_and_lists_newest_first_with_pagination`.
  - Cascade and restrict: `repository_in_campaign_cannot_be_removed_but_campaign_delete_cascades`, `task_history_blocks_campaign_deletion`, `runs_finish_once_and_cascade_with_task`, `run_with_checks_redacts_output_and_cascades` (also covers SET NULL).
- **Credentials**
  - Store, check, replace and delete against the **real macOS Keychain** under `dev.shiftops.desktop.test.<uuid>`, cleaned up by a `Drop` guard: `store_check_replace_delete_against_real_keychain`.
  - Secret never returned over IPC, and absent from logs, activity and the database file: `credential_secret_never_crosses_back_over_ipc`.
  - Redaction: 6 tests in `redact`.
- **Process runner**
  - Success: `successful_command_streams_events_and_records_activity`.
  - Missing executable: `missing_executable_is_rejected_before_spawn`.
  - Non-zero exit: `non_zero_exit_is_failed_with_stderr_details`.
  - Timeout: `timeout_terminates_the_process`.
  - Cancellation, including a grandchild in the process group: `cancellation_kills_process_group_and_is_idempotent`.
  - Truncation of about 1.9 MB against a 64 KiB limit: `large_output_is_truncated_with_marker_and_drained`.
  - Stdout and stderr separation: `stdout_and_stderr_keep_their_identity`.
  - Disallowed categories: `disallowed_categories_and_trust_are_denied`.
  - Invalid working directory: `invalid_working_directories_are_rejected`.
  - Redaction: `secrets_are_redacted_in_events_results_logs_and_activity`.
  - Environment clearing: `environment_is_cleared_except_allow_list`.
- **Paths and URLs**
  - Allowed, missing, escaping symlink, dangling symlink, malformed: 7 tests in `paths`.
  - https allowed; file, javascript, data and others rejected: 2 in `urls`, plus the IPC test.
- **Frontend**
  - Tauri response validation: `native.test.ts`, 7 tests.
  - Query invalidation: `invalidation.test.ts`, 3 tests.
  - Database health rendering, credential metadata rendering, activity pagination and diagnostic cancellation UI: `features.test.tsx`.
  - Structured error display: `ErrorDisplay.test.tsx`.

## 7. Manual verification

| #   | Item                                                              | Result                                                                                                                                                                                                                                                                                                                  |
| --- | ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Existing onboarding still works                                   | **Partly verified.** After each launch the first-run screen called `run_environment_checks` through real IPC: `environment.checks_completed` events at 15:46:17Z and after the relaunch. I could not click **Continue** (see the note below the table).                                                                 |
| 2   | Theme persists                                                    | **Not re-verified in the UI.** Persistence is covered by the `settings` service and IPC tests and by the v1-upgrade test (theme row preserved).                                                                                                                                                                         |
| 3   | Database upgrades without deleting state                          | **Verified.** The real database (v1, 0 setting rows) was upgraded in place to v4 on launch. A backup was written to `backups/shiftops-v1-20261024T154611Z.db`, and `schema_migrations` was backfilled for v1. Upgrade rehearsal on a copy with v1 rows: both rows preserved, with `created_at` taken from `updated_at`. |
| 4   | Settings shows actual schema version and paths                    | **Data verified via IPC** (`get_app_info`, `get_database_health` tests). Not seen on screen.                                                                                                                                                                                                                            |
| 5   | Test GitHub credential stored and deleted through the keychain UI | **Not done through the UI.** The real macOS Keychain was exercised by automated tests under a test service name (store, check, replace, delete, cleanup). The UI flow is covered by component tests against a mocked bridge.                                                                                            |
| 6   | Secret never shown after saving                                   | Covered by tests: the IPC response, activity, log and database file are free of the secret; the component clears the field and the body text never contains it. **Not observed on screen.**                                                                                                                             |
| 7   | Environment checks return real values                             | **Verified.** The real app recorded 8 available and 2 unavailable (Docker daemon not running, Devin CLI not installed).                                                                                                                                                                                                 |
| 8   | Long-running diagnostic streams output                            | **Binary verified**: the ticker printed stdout ticks, stderr lines and the redaction self-test line, then exited on SIGTERM (status 143). Streaming through the runner is covered by tests. Not observed in the UI.                                                                                                     |
| 9   | Cancellation stops it                                             | Verified by tests: the grandchild process is dead after cancel, and cancel via IPC returns `requested` then `not_running`. Not clicked in the UI.                                                                                                                                                                       |
| 10  | Activity records start and cancellation                           | Verified by the IPC test (`process.started`, `process.cancelled` for the run ID). Not observed in the UI.                                                                                                                                                                                                               |
| 11  | Restart preserves activity                                        | **Verified.** Stopped and relaunched the app: activity went from 5 to 6 events. The second `database.opened` shows `fromVersion 4`, no migrations and no backup.                                                                                                                                                        |
| 12  | Website still loads                                               | **Verified.** All 5 routes return HTTP 200 from `next dev`; the static build passes.                                                                                                                                                                                                                                    |

Why the UI items are unchecked: screen capture worked, but osascript has no accessibility access (`osascript is not allowed assistive access`), so I could not click in the window. The one screenshot showed your System Settings, not ShiftOps, so I stopped capturing and deleted the images.

Remaining manual checklist (desktop app is running from `pnpm dev:desktop`):

1. First run: click **Continue**.
2. Settings: choose Dark and save. Quit and relaunch; confirm it starts dark.
3. Settings → Storage: check the schema version is "4 (latest 4)" and the data, database and log paths.
4. Integrations → Credential security: paste a throwaway GitHub token (for example `ghp_` plus 36 characters) and save.
   - It shows **Stored**, with no value visible.
   - **Replace** asks for a new value.
   - **Delete…** then **Delete credential** removes it.
   - In Keychain Access, look for service `dev.shiftops.desktop`, account `github:github_personal_access_token`.
5. Settings → Native diagnostics: click **Start long-running diagnostic**.
   - Lines stream, and stderr lines are amber.
   - The "redaction self-test" line shows `Authorization: [REDACTED]`.
   - Click **Cancel**: status becomes "cancelled" and the structured result is expandable.
6. `⌘J`: the Activity Centre shows the started and cancelled events, the credential events and the migration events. Quit and relaunch; they are still there.

## 8. Native commands exposed (24)

| Group              | Commands                                                                                                                                                                       |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| App and settings   | `get_app_info`, `get_settings`, `set_setting`, `get_execution_limits`                                                                                                          |
| Health             | `run_environment_checks`, `get_database_health`, `get_keychain_status`                                                                                                         |
| Credentials        | `list_credentials`, `store_credential`, `delete_credential` (metadata only)                                                                                                    |
| Diagnostics        | `start_diagnostic` (kind: `git_version`, `node_version`, `pnpm_version`, `long_running`), `cancel_process`, `list_active_processes`                                            |
| Activity           | `list_activity`, `list_activity_entity_types`                                                                                                                                  |
| Domain (read-only) | `list_repositories`, `get_repository`, `list_campaigns`, `get_campaign`, `list_campaign_repositories`, `list_agent_tasks`, `list_verification_runs`, `list_migration_patterns` |
| Links              | `open_external_url`                                                                                                                                                            |

Removed: `credential_status`, replaced by `list_credentials`.

Migrations: 1 `settings` (Step 1, unchanged), 2 `settings_value_json_and_created_at`, 3 `domain_tables`, 4 `credential_metadata`.

## 9. Known limitations

- Step 2 is uncommitted, by design: you asked only for the baseline commit.
- UI click-through items in §7 still need your manual pass.
- Linux and Windows are untested:
  - Process-group termination is Unix-only. Windows uses `start_kill` on the direct child, so grandchildren can survive there.
  - Keychain tests skip, with a printed reason, on runners without a usable keychain (for example headless Linux CI without Secret Service).
- Agent runs and verification checks have services but nothing creates them yet.
- Domain mutations are not exposed over IPC yet.
- Credential validation status is always `not_validated`: no provider API calls exist yet.
- Activity is never pruned. Retention policy is future work.
- Run logs in `~/Library/Logs/dev.shiftops.desktop/runs/` are never rotated.
- The redaction patterns over-redact generic `token`/`secret` key-value text by design.
- The in-memory process store is per window and cleared on reload. Completed results remain in the activity log and run logs.
- The desktop JS bundle is a single 593 kB chunk.
- `~/.npm` still has root-owned cache files (outside this task). `npm view` was run with a scratch cache.

## 10. Step 3 readiness and exact next step

**Ready:**

- `repositories`, `repository_scans` and `repository_dependencies` tables and services, with uniqueness, cascade and the scan state machine.
- `paths::validate` for root and symlink policy.
- `ProcessRunner` with categories, a policy hook, timeouts, cancellation and redacted logs.
- Activity recording.
- Repositories page wired to `list_repositories`.

**Next step: Step 3, local repository discovery.** Do not start until it is requested.

1. Add a `repository_roots` migration (5) and service: user-chosen folders, stored canonicalised.
2. Add `tauri-plugin-dialog` for the folder picker. Record the capability change here. Validate the result with `paths::validate`.
3. Extend `ExecutionPolicy` to allow `readonly_git` with `cwd` inside a registered root. Keep every other category denied.
4. Add a command `scan_repository_root(root_id)`. It walks for `.git` (depth limit; skips `node_modules`/`target`; refuses symlink escapes), then runs `git rev-parse` and `git remote get-url origin` through the runner. It records a `repository_scans` row per repository and upserts `repositories` (`untrusted`). It records activity.
5. Parse remotes with `parseGitRemote` from `packages/github` to fill the GitHub coordinates.
6. Write tests that copy `fixtures/repositories/*` to a temp dir, `git init` and commit, then scan and assert the rows.
