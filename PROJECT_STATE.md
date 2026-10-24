# Project state

Last updated: 2026-10-25 (IST). This file is the hand-off record: anyone (human or agent) resuming ShiftOps reads this first, then `AGENTS.md`. Detailed per-change history, with the validation for each change, is in `CHANGELOG.md`; design detail is in `ARCHITECTURE.md`; the threat model is in `SECURITY.md`.

## 0. Status at a glance

| Task                                                                  | State                                                                                         | Commits                                          |
| --------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| 1. Foundation (shell, environment checks, website)                    | Complete                                                                                      | `cca3cf6`                                        |
| 2. Persistence, credentials, safe execution, activity                 | Complete                                                                                      | `4701627`, Windows CI fixes `5c6ea42`, `824691f` |
| 3. Product profile, local repository intelligence, GitHub integration | **In progress**: milestones 1–4 done; milestones 5–6 not started (specification not received) | `d388ff4`, `0e10fa1`, `43f264b`, `62a1b4b`       |
| 4. Campaign planning, dependency intelligence, Devin integration      | Not started. Devin integration does not exist.                                                |                                                  |
| 5. Migration execution, verification, Devin repair loop               | Not started                                                                                   |                                                  |
| 6. End-to-end workflow, packaging, security audit, submission         | Not started                                                                                   |                                                  |

- **Repository:** `https://github.com/ABDULMUNAFZ/ShiftOps` (public). Branch `main` tracks `origin/main`. The last code commit is `62a1b4b`; the only commit after it is the docs-only hand-off commit that rewrote this file.
- **CI:** GitHub Actions `ci.yml` runs TypeScript plus Rust on Linux, macOS and Windows. Last run for `62a1b4b` (40120833159) passed all four jobs.
- **Database:** schema version **7**.
- **Hackathon:** built for Devin Hacks Kochi 2026. The credit appears only in the README introduction, the website footer, the desktop About page and `SUBMISSION.md`. ShiftOps is independent and not an official Cognition product.

## 1. How to resume

1. `cd ~/shiftops && git status && git log --oneline -10`, then confirm it matches §0.
2. Read `AGENTS.md` (hard rules), this file, and the newest entries of `CHANGELOG.md`.
3. Baseline: `source ~/.cargo/env && pnpm check && pnpm --filter @shiftops/website build && pnpm --filter @shiftops/desktop build`. Expected: 83 JS tests and 154 Rust tests passing, 2 ignored.
4. Continue with §9. Do not start Task 4.

Environment on the development Mac:

- Node 26.7.0, pnpm 10.33.0, Rust 1.99.1 (installed via rustup at `~/.cargo`), macOS 26.7.1 x86_64. `gh` is authenticated as ABDULMUNAFZ through the keychain.
- `~/.npm` has root-owned cache files. Never fix them with `sudo`; use pnpm, and pass `npm_config_cache=<scratch dir>` for `npm view`.

## 2. Architecture

A pnpm monorepo:

- **`apps/desktop`**: Tauri 2.12 with React 19, Vite 8, TanStack Query, React Hook Form + Zod, cmdk and Tailwind 4.
  - The Rust core (`src-tauri/src`) owns every native operation and exposes **52 commands** (§6).
  - The web view has no shell, fs, http, sql, dialog or opener permissions; the capability file is `core:default` only.
- **`apps/website`**: Next.js 16 static export (landing, docs, security, privacy, download).
- **`packages/core`**: Zod contracts for every native model (`domain.ts`), the error contract (`errors.ts`), settings, environment checks, and the `planWaves` planner.
- **`packages/ui`**: design tokens, components and brand constants.
- **`packages/github`**: TypeScript remote parser.
- **`packages/devin`**, **`packages/verification`**: contracts only.

Rust modules:

| Module                            | Responsibility                                                                                                                                                                                                         |
| --------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `db/`                             | SQLite, migrations in `db/sql/0002–0007` (v1 inline), services: settings, repositories (+ scans, dependencies), roots, intelligence, campaigns, agents, verification, patterns, credentials metadata, activity, github |
| `credentials.rs`                  | Keychain via `keyring` 3.6                                                                                                                                                                                             |
| `process/`                        | Policy-checked runner: argument arrays, cleared environment, process-group kill, streaming, timeouts, output limits                                                                                                    |
| `git/`                            | Remote parsing, exact read-only command allow-list, risky-config scan, metadata collection                                                                                                                             |
| `discovery/`                      | Filesystem walk, jobs, import                                                                                                                                                                                          |
| `scanner/`                        | File collection, detectors, version ranges, conflicts, cache                                                                                                                                                           |
| `github/`                         | reqwest native-tls client, device flow, PAT, provider operations                                                                                                                                                       |
| `paths.rs`, `urls.rs`             | Validation                                                                                                                                                                                                             |
| `redact.rs`, `logging.rs`         | Redaction and the redacted log                                                                                                                                                                                         |
| `error.rs`                        | 23 stable error codes                                                                                                                                                                                                  |
| `environment.rs`, `shell_path.rs` | Environment checks and login-shell PATH                                                                                                                                                                                |

## 3. Completed functionality

### Task 1

- Seven-section shell, collapsible sidebar, ⌘K palette, ⌘1–7 navigation, ⌘J Activity Centre.
- Light, dark and system themes, persisted.
- First-run environment checks (OS, data directory, Git, Node, pnpm, npm, Yarn, Docker with daemon check, Devin CLI, keychain probe), using the login-shell PATH.
- Website.

### Task 2

- **Migrations:** ordered and transactional, recorded in `schema_migrations`, idempotent, with foreign-key checks and `VACUUM INTO` backups.
- **Domain tables:** settings, repositories, scans, dependencies, campaigns, memberships, agent tasks and runs, verification runs and checks, activity events, migration patterns, credential metadata.
- **Services:** typed, with pagination, filters and search; transactional campaign creation.
- **Errors:** structured contract with expandable and copyable details, and error boundaries.
- **Credentials:** OS keychain storage. Secrets never return to React; GitHub tokens are written only through the validated GitHub connection.
- **Redaction:** GitHub tokens, Bearer and Authorization values, generic key fields and URL credentials.
- **Process runner:** only `environment_check` in the app runner; cancellation kills the process group; output is limited and truncated with a marker; run logs are redacted.
- **Validation:** paths (symlink escape, null bytes) and URLs (https, or http to loopback).
- **UI:** Activity Centre; Native Diagnostics (git, node and pnpm versions, plus a 60 s bundled ticker with cancel).

### Task 3

- **Milestone 1:** product profile and documents — README introduction, `CHANGELOG.md`, `AI_USAGE.md`, `SUBMISSION.md`, desktop About page, website footer.
- **Milestone 2:** local repository discovery (migration 5: `repository_roots`, Git state columns).
  - Native folder picker; open one repository or discover a folder (depth 0–12, internal-symlink option).
  - Skips noise and hidden folders, refuses symlink escapes, warns on unreadable folders, enforces an entry limit, streams progress and supports cancel.
  - Read-only Git inspection: branch or detached HEAD, HEAD, default branch, origin with credentials stripped and GitHub coordinates, last commit, staged/modified/untracked counts.
  - `git status` is skipped when the repository's own config defines filters or includes.
  - Imports start untrusted. Removing a root never deletes files.
  - One checkout holds each GitHub identity; other checkouts keep their remote URL and get a warning.
- **Milestone 3:** repository intelligence scanner (migration 6: `repository_facts`, `repository_conflicts`, scan cache key).
  - Facts carry file, line or JSON pointer, method and confidence.
  - Coverage: Node/TypeScript in full, plus Python, Java, .NET, Go, Rust, Docker and CI.
  - Conflicts are scoped to the package and directory they apply to.
  - Never reads secret files; never follows symlinks; skips fixture, dependency and build folders.
  - Cached by scanner version + HEAD + relevant-file content; the last 3 scans are kept.
  - UI: list columns and a repository detail page with Overview, Evidence, Runtimes, Dependencies, Scripts, Docker, CI, Git, Branches, Pull Requests and Activity tabs.
  - 11 deterministic fixtures in `fixtures/repositories`.
- **Milestone 4:** real GitHub integration (migration 7: `github_accounts`, `github_organizations`, `github_repositories`, `github_pull_requests`).
  - OAuth device flow (no secret) and a fine-grained PAT fallback, validated before storage.
  - Account, organisations, rate limit, browse and search with pagination and filters, details, open PRs with check summaries, single-PR mergeability, requested config files (secret names refused).
  - Precise error mapping (seven new codes) and cancellable requests.
  - Metadata-only import becomes local-linked when its checkout is discovered.
  - Configuration: `VITE_GITHUB_OAUTH_CLIENT_ID`, `VITE_GITHUB_HOST`, `VITE_GITHUB_API_BASE_URL` in `apps/desktop/.env` (see `.env.example`).
  - UI: GitHub connection in Integrations, Browse GitHub in Repositories, Pull Requests tab.

## 4. Key decisions

Full reasons are in `CHANGELOG.md` and `ARCHITECTURE.md`.

- **TypeScript 6.0.3**, not 7: `typescript-eslint` 8.72 requires TypeScript below 6.1.
- **Tauri 2.12.5**: 3.x is alpha.
- **Hash routing**: the Tauri asset protocol has no history fallback.
- **SQLite in Rust** (rusqlite bundled):
  - React never runs SQL.
  - No `STRICT` tables, because macOS sqlite3 3.32 must be able to read the file.
  - JSON is validated in Rust.
  - Timestamps are fixed-width UTC milliseconds.
- **keyring 3.6.3**, kept rather than upgrading to 4.x.
- **camelCase across IPC**, including the error contract.
- **Process runner:**
  - The app-level policy allows only `environment_check`.
  - Discovery and scanning use a separate runner whose policy is `readonly_git`, limited to exact argument vectors.
  - Mutating Git, install, build, test, Docker, Devin and custom commands are denied everywhere.
- **Git safety prefix:** `--no-pager --no-optional-locks -c core.fsmonitor=false -c core.untrackedCache=false -c log.showSignature=false -c color.ui=false`.
- **Windows CI:** `build.rs` embeds the Common-Controls v6 manifest through the MSVC linker, so test executables load.
- **reqwest 0.13 with `native-tls`**: the rustls default uses aws-lc-rs, which needs CMake/NASM. CI installs `libssl-dev`.
- **GitHub configuration is read in Rust**, not the web view, so tokens cannot be redirected. One connected account at a time.
- **Prettier** at printWidth 120. `fixtures/repositories` is ignored so the fixtures stay byte-exact.

## 5. Real data on the development Mac

- **Database:** `~/Library/Application Support/dev.shiftops.desktop/shiftops.db`. Schema 7, 36 activity events, 0 repositories, no settings rows (onboarding not yet clicked through).
- **Backups:** `backups/shiftops-v1-…`, `-v4-…`, `-v5-…`, `-v6-…`.
- **Logs:** `~/Library/Logs/dev.shiftops.desktop/` (`shiftops.log`, `runs/`).
- **Keychain:** service `dev.shiftops.desktop`. No real credentials stored by the agent; tests use `dev.shiftops.desktop.test.<uuid>` and clean up.
- **Incident:** during milestone 4 the agent's still-running dev app applied migration 7 to this database mid-development. The schema matches the committed SQL and a v6 backup exists. The extra restarts added 15 `database.opened` and 15 environment-check events. AGENTS.md rule 11 now forbids this.

## 6. Native commands (52)

| Area             | Commands                                                                                                                                                                                                                                                                                                                                                                                                    |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| App and settings | `get_app_info`, `get_settings`, `set_setting`, `get_execution_limits`                                                                                                                                                                                                                                                                                                                                       |
| Health           | `run_environment_checks`, `get_database_health`, `get_keychain_status`                                                                                                                                                                                                                                                                                                                                      |
| Credentials      | `list_credentials`, `store_credential`, `delete_credential`                                                                                                                                                                                                                                                                                                                                                 |
| Diagnostics      | `start_diagnostic`, `cancel_process`, `list_active_processes`                                                                                                                                                                                                                                                                                                                                               |
| Activity         | `list_activity`, `list_activity_entity_types`                                                                                                                                                                                                                                                                                                                                                               |
| Domain reads     | `list_repositories`, `get_repository`, `list_campaigns`, `get_campaign`, `list_campaign_repositories`, `list_agent_tasks`, `list_verification_runs`, `list_migration_patterns`                                                                                                                                                                                                                              |
| Links            | `open_external_url`                                                                                                                                                                                                                                                                                                                                                                                         |
| Discovery        | `pick_directory`, `start_discovery`, `cancel_discovery`, `get_discovery`, `import_discovered`, `list_repository_roots`, `remove_repository_root`, `list_repository_branches`                                                                                                                                                                                                                                |
| Scanner          | `start_repository_scan`, `cancel_repository_scan`, `get_repository_intelligence`, `list_repository_scans`                                                                                                                                                                                                                                                                                                   |
| GitHub           | `github_status`, `github_start_device_flow`, `github_cancel_device_flow`, `github_connect_pat`, `github_refresh`, `github_disconnect`, `github_list_organizations`, `github_browse_repositories`, `github_repository_details`, `github_list_pull_requests`, `github_get_pull_request`, `github_get_file`, `github_rate_limit`, `github_import_repository`, `github_cancel_request`, `github_repository_for` |

Events: `shiftops://process`, `shiftops://discovery`, `shiftops://scan`, `shiftops://github-auth`.

## 7. Tests and verification (latest)

- **JavaScript:** 83 tests — core 13, github 11, devin 2, verification 4, ui 3, website 1, desktop 49.
- **Rust:** 154 passed, 2 ignored. The ignored ones are manual rehearsals:
  - `upgrade_rehearsal` with `SHIFTOPS_UPGRADE_DB`;
  - `scan_rehearsal` with `SHIFTOPS_SCAN_PATH`.
- **Other checks:** Prettier, ESLint, rustfmt and clippy `-D warnings` clean; website and desktop production builds pass. CI passes on all platforms.
- **Verified on the real machine:**
  - database upgrades 1 → 7 in place with backups;
  - environment checks: 8 available; Docker unavailable because its daemon is stopped; Devin CLI unavailable because it is not installed;
  - activity persists across restarts;
  - the website serves every route;
  - the bundled diagnostic ticker runs and stops on SIGTERM;
  - the scanner's rehearsal of this repository is accurate.
- **Not verified by the agent:**
  - UI click-through: no macOS accessibility permission, so components are covered by tests only;
  - live GitHub: no OAuth client ID or token was provided; mock-server tests only.

## 8. Known limitations

- **Missing specification:** milestones 5 (repository management) and 6 (tests), and the remainder of milestone 4's "repository import modes". Only metadata-only import plus automatic local linking exists; there is no cloning.
- **Windows:** cancellation kills only the direct child process. Walker, scanner and real-Git tests are Unix-only.
- **Keychain:** keychain tests skip, with a printed reason, where no keychain exists (headless Linux).
- **Unbuilt data paths:** activity and run logs are never pruned or rotated; credential validation status is not refreshed automatically; agent and verification services exist but nothing creates their records yet.
- **Scanner limits:** declarative only, with no dependency-tree resolution; YAML and XML are read line-based.
- **GitHub limits:** one account at a time; OAuth asks for `repo` (GitHub has no read-only equivalent); PR checks are summarised for the first 30 PRs.
- **Build size:** the desktop bundle is a single chunk of roughly 600 kB.
- **Build Log artifact** (claude.ai, pinned) mirrors this file. If they disagree, this file wins.

## 9. Exact next step

Task 3 milestones 5 and 6 need the user's full specification, which was cut off in two pastes. Ask for it, preferably in several smaller messages:

- the "Repository import modes" section of milestone 4;
- milestone 5, repository management;
- milestone 6, tests;
- the end-of-task report format.

Until then, do not guess cloning or other mutating Git behaviour. Each milestone follows the same workflow:

1. Inspect `git status` and the diff.
2. Run the relevant tests.
3. Update `CHANGELOG.md` (real IST time) and `AI_USAGE.md`, and README if setup or behaviour changed.
4. Commit with an accurate message and push.
5. Confirm the pushed SHA and the CI result.

Do not start Task 4.
