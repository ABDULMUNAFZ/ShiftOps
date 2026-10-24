# Project state

Last updated: 2026-10-25 (IST). This file is the hand-off record: anyone (human or agent) resuming ShiftOps reads this first, then `AGENTS.md`. Detailed per-change history, with the validation for each change, is in `CHANGELOG.md`; design detail is in `ARCHITECTURE.md`; the threat model is in `SECURITY.md`.

## 0. Status at a glance

| Task                                                                  | State                                                                                           | Commits                                                                     |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| 1. Foundation (shell, environment checks, website)                    | Complete                                                                                        | `cca3cf6`                                                                   |
| 2. Persistence, credentials, safe execution, activity                 | Complete                                                                                        | `4701627`, Windows CI fixes `5c6ea42`, `824691f`                            |
| 3. Product profile, local repository intelligence, GitHub integration | **Complete**                                                                                    | `d388ff4`, `0e10fa1`, `43f264b`, `62a1b4b`, `9a9f265`, `e1cd62d`, `7666608` |
| 4. Campaign planning, dependency intelligence, Devin integration      | **Complete** (local Devin CLI; live dispatch not exercised)                                     | Task 4 commits (see §9)                                                     |
| 5. Migration execution, verification, Devin repair loop               | **Complete** (live Devin dispatch not exercised)                                                | branch `feat/verification-repair-loop` (`9fd1b3d`, pushed; see §8d)         |
| 6. End-to-end workflow, packaging, security audit, submission         | **In progress**: repository-management hardening done; packaging, audit, submission not started | same branch                                                                 |

- **Repository:** `https://github.com/ABDULMUNAFZ/ShiftOps` (public). Branch `main` tracks `origin/main`.
- **CI:** GitHub Actions `ci.yml` runs TypeScript plus Rust on Linux, macOS and Windows. Run `40128969708` for `7666608` passed all four jobs.
- **Database:** schema version **10**. Migration 9 adds Devin task approval/hash and member review flags; migration 10 adds verification plans, repair tasks, one-live-operation indexes and repository display name and notes. The real database is still at 8 and upgrades on next app launch with a backup; a rehearsal on a copy upgraded 8 → 10 cleanly.
- **Hackathon:** built for Devin Hacks Kochi 2026. The credit appears only in the README introduction, the website footer, the desktop About page and `SUBMISSION.md`. ShiftOps is independent and not an official Cognition product.

## 1. How to resume

1. `cd ~/shiftops && git status && git log --oneline -10`, then confirm it matches §0.
2. Read `AGENTS.md` (hard rules), this file, and the newest entries of `CHANGELOG.md`.
3. Baseline: `source ~/.cargo/env && pnpm check && pnpm --filter @shiftops/website build && pnpm --filter @shiftops/desktop build`. Expected: 112 JS tests and 208 Rust tests passing, 3 ignored.
4. Tasks 1–5 are complete; Task 6 is in progress (§8d). Do not start further Task 6 work without its specification.

Environment on the development Mac:

- Node 26.7.0, pnpm 10.33.0, Rust 1.99.1 (installed via rustup at `~/.cargo`), macOS 26.7.1 x86_64. `gh` is authenticated as ABDULMUNAFZ through the keychain.
- `~/.npm` has root-owned cache files. Never fix them with `sudo`; use pnpm, and pass `npm_config_cache=<scratch dir>` for `npm view`.

## 2. Architecture

A pnpm monorepo:

- **`apps/desktop`**: Tauri 2.12 with React 19, Vite 8, TanStack Query, React Hook Form + Zod, cmdk and Tailwind 4.
  - The Rust core (`src-tauri/src`) owns every native operation and exposes **92 commands** (§6).
  - The web view has no shell, fs, http, sql, dialog or opener permissions; the capability file is `core:default` only.
- **`apps/website`**: Next.js 16 static export (landing, docs, security, privacy, download).
- **`packages/core`**: Zod contracts for every native model (`domain.ts`), the error contract (`errors.ts`), settings, environment checks, and the `planWaves` planner.
- **`packages/ui`**: design tokens, components and brand constants.
- **`packages/github`**: TypeScript remote parser.
- **`packages/devin`**, **`packages/verification`**: contracts only.

Rust modules:

| Module                            | Responsibility                                                                                                                                                                                                                    |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `db/`                             | SQLite, migrations in `db/sql/0002–0009` (v1 inline), services: settings, repositories (+ scans, dependencies), roots, intelligence, campaigns, agents, verification, patterns, credentials metadata, activity, github, workspace |
| `credentials.rs`                  | Keychain via `keyring` 3.6                                                                                                                                                                                                        |
| `process/`                        | Policy-checked runner: argument arrays, cleared environment, process-group kill, streaming, timeouts, output limits                                                                                                               |
| `git/`                            | Remote parsing, exact read-only command allow-list, risky-config scan, metadata collection                                                                                                                                        |
| `discovery/`                      | Filesystem walk, jobs, import                                                                                                                                                                                                     |
| `scanner/`                        | File collection, detectors, version ranges, conflicts, cache                                                                                                                                                                      |
| `github/`                         | reqwest native-tls client, device flow, PAT, provider operations                                                                                                                                                                  |
| `workspace/`                      | Managed workspace approval, policy-checked clone with progress/cancel/cleanup, existing-checkout linking                                                                                                                          |
| `devin.rs`                        | Devin CLI status, deterministic task instructions, exact dispatch vector                                                                                                                                                          |
| `readiness.rs`                    | Submission Readiness evidence: read-only Git state, required documents, TODO count, bounded redacted secret scan, validation record                                                                                               |
| `paths.rs`, `urls.rs`             | Validation                                                                                                                                                                                                                        |
| `redact.rs`, `logging.rs`         | Redaction and the redacted log                                                                                                                                                                                                    |
| `error.rs`                        | 25 stable error codes                                                                                                                                                                                                             |
| `environment.rs`, `shell_path.rs` | Environment checks and login-shell PATH                                                                                                                                                                                           |

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
- **Milestone 4b:** managed clone and local linking (migration 8: `managed_workspaces`, `repository_clones`, `repositories.ownership`, `repositories.github_link_method`).
  - Three import modes converge on one repository id: metadata-only, managed clone into an approved workspace, link an existing checkout.
  - Managed clone is the single mutating Git command: HTTPS only, no hooks/templates/submodules/interactive credentials/LFS smudge, destination exactly two segments under the approved root, progress events, cancellation, exact-path-confirmed cleanup.
  - Startup closes interrupted clone attempts; link mismatches need the actual identity confirmed; ownership `external` is never modified or deleted.
- **Milestone 5:** repository management and Submission Readiness.
  - Repository list shows source/ownership, GitHub identity, technology, runtime, package manager, branch/commit, working tree, trust, scan state and last scan time.
  - Detail page changes trust explicitly and forgets only the record after confirmation — both audited; files are never deleted.
  - `readiness.rs` collects real checkout evidence (required documents, remote/HEAD/origin-main, dirty and ahead state, SUBMISSION.md TODO count, bounded redacted tracked-secret scan, `.shiftops/validation.json`) and computes a deterministic status that can never be `Submitted`.
  - Desktop Submission Readiness page (⌘8): refresh, copy checklist, open validated documents, open the GitHub repository.
  - `pnpm validate:record` reruns the whole check and writes the gitignored mode-0600 validation record for the current commit.

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
  - Managed clone uses `managed_clone` (one `git clone` vector).
  - **Task 5 (rule 5 record):** `ExecutionPolicy::verification(root, vectors)` enables `dependency_install`, `build`, `test` and `lint` only for the exact (program, arguments) vectors of one approved verification plan (`pnpm`/`npm`/`yarn` install with `--ignore-scripts`, and `run <typecheck|lint|build|test|check>`), only with `TrustRequirement::TrustedRepository`, and only inside that checkout. The Devin-only exception for `TrustedRepository` is replaced by an explicit per-policy `trusted_checkout` flag, set only by `devin_dispatch` and `verification`. Reason: independent verification must run the repository's own checks; the trusted, confirmed, hash-pinned plan is the gate.
  - **Task 4 (rule 5 record):** `devin` is enabled by `devin_cli` (exactly `version`, `auth status`, `doctor`) and `devin_dispatch` (exactly `-p <prompt> --permission-mode smart --sandbox --respect-workspace-trust false`, cwd inside one trusted checkout). `TrustedRepository` is accepted only for the `devin` category. Reason: delegation must run in the repository; every other gate is in `dispatch_agent_task`.
  - Install, build, test, lint, Docker and custom commands stay denied. No capability or plugin change in the web view (rule 4).
- **Git safety prefix:** `--no-pager --no-optional-locks -c core.fsmonitor=false -c core.untrackedCache=false -c log.showSignature=false -c color.ui=false`.
- **Windows CI:** `build.rs` embeds the Common-Controls v6 manifest through the MSVC linker, so test executables load.
- **reqwest 0.13 with `native-tls`**: the rustls default uses aws-lc-rs, which needs CMake/NASM. CI installs `libssl-dev`.
- **GitHub configuration is read in Rust**, not the web view, so tokens cannot be redirected. One connected account at a time.
- **Prettier** at printWidth 120. `fixtures/repositories` is ignored so the fixtures stay byte-exact.

## 5. Real data on the development Mac

- **Database:** `~/Library/Application Support/dev.shiftops.desktop/shiftops.db`. Schema 8 (upgraded 7 → 8 at the post-commit launch, 36+ activity events), 0 repositories, no settings rows (onboarding not yet clicked through).
- **Backups:** `backups/shiftops-v1-…`, `-v4-…`, `-v5-…`, `-v6-…`, `-v7-…`.
- **Logs:** `~/Library/Logs/dev.shiftops.desktop/` (`shiftops.log`, `runs/`).
- **Keychain:** service `dev.shiftops.desktop`. No real credentials stored by the agent; tests use `dev.shiftops.desktop.test.<uuid>` and clean up.
- **Incident:** during milestone 4 the agent's still-running dev app applied migration 7 to this database mid-development. The schema matches the committed SQL and a v6 backup exists. The extra restarts added 15 `database.opened` and 15 environment-check events. AGENTS.md rule 11 now forbids this.

## 6. Native commands (92)

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
| Repository admin | `set_repository_trust`, `remove_repository_record`                                                                                                                                                                                                                                                                                                                                                          |
| GitHub           | `github_status`, `github_start_device_flow`, `github_cancel_device_flow`, `github_connect_pat`, `github_refresh`, `github_disconnect`, `github_list_organizations`, `github_browse_repositories`, `github_repository_details`, `github_list_pull_requests`, `github_get_pull_request`, `github_get_file`, `github_rate_limit`, `github_import_repository`, `github_cancel_request`, `github_repository_for` |
| Workspace        | `get_managed_workspace`, `approve_managed_workspace`, `preview_managed_clone`, `start_managed_clone`, `cancel_managed_clone`, `list_repository_clones`, `cleanup_managed_clone`, `inspect_existing_clone`, `link_existing_clone`                                                                                                                                                                            |
| Campaigns        | `create_campaign`, `update_campaign`, `delete_campaign`, `set_campaign_status`, `add_campaign_repositories`, `remove_campaign_repository`, `exclude_campaign_repository`, `include_campaign_repository`, `resolve_member_review`, `campaign_plan_evidence`, `apply_campaign_plan`, `refresh_dependency_edges`, `list_repository_dependencies`                                                               |
| Devin            | `devin_status`, `create_agent_task`, `approve_agent_task`, `dispatch_agent_task`, `cancel_agent_task`, `list_agent_runs`                                                                                                                                                                                                                                                                                    |
| Verification     | `preview_verification`, `start_verification`, `cancel_verification`, `list_verification_checks`, `create_repair_task`                                                                                                                                                                                                                                                                                       |
| Repository edits | `update_repository_metadata`, `unlink_repository_checkout`, `delete_managed_checkout`                                                                                                                                                                                                                                                                                                                       |
| Readiness        | `get_submission_readiness`, `open_submission_file`                                                                                                                                                                                                                                                                                                                                                          |

Events: `shiftops://process`, `shiftops://discovery`, `shiftops://scan`, `shiftops://github-auth`, `shiftops://clone`, `shiftops://verification`.

## 7. Tests and verification (latest)

- **JavaScript:** 112 tests — core 21, github 11, devin 2, verification 4, ui 3, website 3, desktop 68.
- **Rust:** 208 passed, 3 ignored. The ignored ones are manual rehearsals:
  - `upgrade_rehearsal` with `SHIFTOPS_UPGRADE_DB`;
  - `scan_rehearsal` with `SHIFTOPS_SCAN_PATH`;
  - `devin::status_rehearsal` (needs the Devin CLI; verified: installed 3000.11.205, authenticated).
- **Other checks:** Prettier, ESLint, rustfmt and clippy `-D warnings` clean; website and desktop production builds pass. The latest CI result is recorded in §9.
- **Verified on the real machine:**
  - database upgrades 1 → 7 in place with backups;
  - environment checks: 8 available; Docker unavailable because its daemon is stopped; Devin CLI unavailable at that time (as of Task 4 it is installed at `~/.local/bin/devin`, 3000.11.205, and signed in);
  - activity persists across restarts;
  - the website serves every route;
  - the bundled diagnostic ticker runs and stops on SIGTERM;
  - the scanner's rehearsal of this repository is accurate.
- **Not verified by the agent:**
  - UI click-through: no macOS accessibility permission, so components are covered by tests only;
  - live GitHub: no OAuth client ID or token was provided; mock-server tests only.

## 8. Known limitations

- **Windows:** cancellation kills only the direct child process. Walker, scanner and real-Git tests are Unix-only.
- **Keychain:** keychain tests skip, with a printed reason, where no keychain exists (headless Linux).
- **Unbuilt data paths:** activity and run logs are never pruned or rotated; credential validation status is not refreshed automatically; migration patterns are still never created.
- **Scanner limits:** declarative only, with no dependency-tree resolution; YAML and XML are read line-based.
- **GitHub limits:** one account at a time; OAuth asks for `repo` (GitHub has no read-only equivalent); PR checks are summarised for the first 30 PRs.
- **Devin prompt check:** `devin::validate_prompt` (in `dispatch_spec` and the `devin_dispatch` policy) rejects empty, whitespace-only, NUL-containing, over-200,000-byte and option-like prompts (first non-whitespace character `-`). Merged in PR #1 (`30099d0`).
- **Readiness limits:** the secret scan is a bounded heuristic (paths and rule names only); `Submitted` and the final SHA stay unset until Task 6's human submission flow.
- **Verification limits:** Node.js repositories only (root `package.json`); monorepo packages are verified through root scripts only; Yarn Berry (`--immutable`) is not distinguished from Yarn 1; installs pass `--ignore-scripts`, so packages that need lifecycle scripts may fail to build; verification runs in the checkout itself, not an isolated worktree, and Git LFS or submodules are not prepared.
- **Repair limits:** at most 2 repair tasks per campaign member; affected files come from path patterns in tool output, so failures without file paths are not delegated; the automatic re-run after a repair needs an unchanged plan hash.
- **Not exercised live:** a real Devin dispatch, a real repair session and real package-manager runs from the app. Tests use a fixture package manager on a private search path and a disabled Devin CLI.
- **Devin CLI / XProtect:** the macOS "Malicious Script Blocked" item was not identified. The installed binary `~/.local/share/devin/cli/_versions/3000.11.205/bin/devin` passes `codesign --verify --strict`: Developer ID Application EXAFUNCTION, INC. (83Z2LHX6XW), hardened runtime, sha256 `edb6958dd81cf344d29019412445734527eab43471505fd3e2a5d4c3583fdf1f`. No XProtect entries appear in three days of unified log, and the `_download` staging folder is empty. Release-manifest checksum comparison was not done. The agent did not execute the CLI.
- **Build size:** the desktop bundle is a single chunk of roughly 717 kB.
- **Build Log artifact** (claude.ai, pinned) mirrors this file. If they disagree, this file wins.

## 8a. Managed clone and link milestone (2026-10-25)

The full Task 3 continuation specification was received. Managed clone and existing-clone linking are implemented and locally verified. The real database was not touched during development; no dev app was running.

Implemented across Rust, IPC and React, with `pnpm check` at **85 JavaScript tests and 175 Rust tests passed, 2 ignored**:

- Migration 8 `0008_managed_workspaces.sql`: `managed_workspaces` (with `approval_seq`), `repository_clones` (partial unique index on open destinations), `repositories.ownership` (`external` / `shiftops_managed`) and `repositories.github_link_method`, with backfill. Upgrade tests cover every earlier version.
- `git/policy.rs`: `clone_args` / `validate_clone` / `validate_clone_url`: the one mutating Git command (HTTPS only, `--template=`, `core.hooksPath=/dev/null`, `--no-recurse-submodules`, `credential.interactive=false`, no credentials in the URL). `ExecutionPolicy::managed_clone(root)` checks the destination is an existing real directory inside the root.
- `process/runner.rs`: a lone `\r` now ends a line, so clone progress streams.
- `db/workspace.rs`: workspace approval, clone attempts, `attach_checkout` (a metadata-only record becomes local without a duplicate).
- `db/repositories.rs`: ownership and link method on records; an `override` link survives rediscovery. `db/github.rs`: `import_metadata_only` reuses a checkout that already holds the identity.
- `workspace/mod.rs`: root validation (refuses home and its ancestors, repositories, overlaps with registered checkouts), destination derivation, preview, guarded cleanup (exact recorded path, confirmed by repeating the path, failed or cancelled attempts only, never roots, home, links or registered repositories).
- `workspace/clone.rs`: clone job with progress events (`shiftops://clone`), cancellation, failure classification (new code `GIT_AUTHENTICATION_REQUIRED`, also added to `errors.ts`), linking, then a scan. Tested against a local dumb-HTTP server.
- `workspace/link.rs`: inspect and link an existing clone; a mismatch needs the exact `actualDisplay` confirmed; ownership `external`.
- AppState closes interrupted clone attempts on startup and owns the live clone-job registry. Nine typed Tauri commands expose workspace approval, preview/start/cancel/list/cleanup and inspect/link.
- Shared Zod contracts validate workspace, clone, progress and link results. GitHub Browse offers all three import modes with destination review, progress, cancellation, retained-failure cleanup and mismatch confirmation.
- IPC and component tests cover the new boundary. Architecture, security, README, changelog and AI usage records describe the actual behavior.

All three follow-up items are now done: the milestone was committed as `9a9f265`; milestone 5 (repository management) and Submission Readiness are implemented in the current commit — see §3 milestone 5 and the top `CHANGELOG.md` entry.

## 8b. Repository management and Submission Readiness milestone (2026-10-25)

- `set_repository_trust` and `remove_repository_record` commands require explicit confirmation, record `repository.trust_changed` / `repository.record_removed` activity and never touch checkout files.
- `readiness.rs` collects evidence through the exact read-only Git policy (remote, HEAD, `origin/main`, porcelain status, ahead count, `ls-files`), checks the four required documents, counts SUBMISSION.md TODOs, scans up to 20,000 tracked files of at most 512 KiB for three secret rules (values never leave Rust), and reads the optional `.shiftops/validation.json`. A pure `compute_status` is unit-tested and can never return `Submitted`.
- `open_submission_file` opens only the four required document names through the Rust-side opener (no web-view permission).
- A Git inspection regression was fixed by replacing fragile `READONLY_COMMANDS` numeric indexes with named command constants; `readiness` commands were added to the same allow-list.
- The Windows CI Clippy failure in run `40125030556` (Unix-only `std::path::Path` import in `workspace/link.rs` tests) is fixed with `cfg(all(test, unix))`.
- `pnpm validate:record` (`scripts/record-validation.mjs`) reruns `pnpm check` and writes `.shiftops/validation.json` mode 0600 for the current commit.

## 8c. Task 4: campaigns, planning and Devin delegation (2026-10-25)

- Planner: `planCampaign` in `packages/core/src/planning.ts` (21 core tests incl. ordering, cycles, risk, unknown/rejected edges, determinism, no-data).
- Backend: migration 9; `commands.rs` campaign, plan, edge and Devin sections; `devin.rs`; policies in `process/mod.rs`; `AppState.devin_runs` live registry.
- UI: `CampaignsPage`, `CampaignDetailPage`, `TasksPage` (Agent Runs), Devin card in Integrations.
- Genuine Devin capability used: local CLI `devin -p` print mode, plus `devin version` / `devin auth status` for detection. The CLI documents no remote task-creation command, so cloud sessions are not used.
- Not exercised by the agent: a live dispatch (modifies a real checkout and spends the owner's Devin usage; needs explicit owner approval). Session status beyond process exit (Devin PR, branch) is not read back; Task 5 inspects the working tree.
- Dependency edges are name-match heuristics; stored as `inferred`, never confirmed automatically.

## 8d. Task 5 and Task 6 (repository hardening), 2026-10-25

The working specification is the owner's "Complete Steps 5 and 6" prompt. Task 5 is implemented; Task 6's repository-management hardening is implemented. Packaging, the security audit and the submission flow (the original Task 6 title) are not started.

- **Verification:** `src-tauri/src/verification.rs` (plan derivation, failure analysis, jobs, cancellation), `ExecutionPolicy::verification`, migration 10, and the commands `preview_verification`, `start_verification`, `cancel_verification` and `list_verification_checks`. UI: Campaign detail → Verify (`features/verification/VerificationPanel.tsx`).
- **Repair loop:** `create_repair_task` drafts from the latest failed run; repair tasks are bounded to 2 per member; `devin::build_repair_instructions` neutralises fences and redacts output. A successful repair session re-runs the pinned plan automatically only if its hash is unchanged (otherwise `verification.rerun_blocked`).
- **Dispatch hardening:** atomic `claim_for_dispatch` with re-check of the instruction hash; refuses a dirty working tree, a live verification and a second live session; releases the claim when dispatch stops early.
- **Recovery:** on start, interrupted Devin runs, tasks and members and interrupted verification runs and checks are closed as failed, error or cancelled and recorded (`app.interrupted_work_recovered`).
- **Migrations:** a database newer than the app is refused and left untouched. Migration 10 has an upgrade test from every version, and a rehearsal on a copy of the real database (8 → 10) passed integrity and foreign-key checks.
- **Repository management:** display name and notes, `checkoutAvailable`, confirmed trust changes (lowering cancels live work), unlink checkout, typed-path deletion of ShiftOps-managed clones only, and a busy guard on removal operations. The five import actions are on the Repositories page.
- **Readiness:** now requires a passing validation recorded for the current HEAD (before this, it could report Ready with no validation). Non-blocking `warnings` are separated from blocking reasons, and the page refreshes on window focus.
- **Contract fix:** `githubLinkMethod` now accepts `metadata_only` and `discovery`. Before this, any metadata-only or discovered GitHub record failed Zod validation and broke the repository list.

## 8e. Website, desktop UI and branding (2026-10-25)

- **Website:** the landing page (`apps/website/src/app/page.tsx`) is dark and full-bleed with acid-lime accents. Sections: hero, illustrative product mockup (labelled as such), problem/solution, a six-step workflow, use cases, verification, safeguards and a download CTA. The CTA says honestly that no signed installers exist yet. GSAP 3.15 ScrollTrigger drives the hero entrance, section reveals, the preview tilt and the workflow progress line; all are skipped under reduced motion. The hero glow follows the pointer for mouse users only. The site defaults to dark.
- **Brand:** the shared accent is acid lime (dark `oklch(0.91 0.2 125)`, light deep olive for contrast). The icon (`src-tauri/icons/icon.svg`) is lime, and every PNG/ICNS/ICO was regenerated with `tauri icon`. Favicons: website `app/icon.svg`, desktop `public/favicon.svg`.
- **Desktop UI:** rounded cards, pill buttons and badges in `@shiftops/ui`; a lime active navigation and larger brand block in the shell; larger page headers. Overview adds live counts and a "Get started" checklist driven by real data, and replaces a stale empty state. The Verification page shows status badges, profile, trigger and time.
- **Docs:** README features, quick start, architecture and tests; `docs/USER_MANUAL.md`.
- **Visual verification:** the website was rendered in headless Chrome at 1440 px and 500 px. The desktop UI was not rendered in Tauri: launching it would run the environment check that executes the `devin` binary (see the XProtect note) and would upgrade the real database. It is covered by component tests only.

## 9. Exact next step

Task 5 and Task 6's repository hardening are implemented on the feature branch (§8d); see the newest `CHANGELOG.md` entry. The branch is pushed to origin; a PR and merge need the owner's explicit authorisation. Next: the rest of Task 6 (end-to-end workflow, packaging, security audit, submission), when its specification arrives. A live Devin dispatch still needs the owner's explicit approval.

Task 4 final state (kept for reference): commit SHAs and the CI run are recorded in the `CHANGELOG.md` Task 4 entry.

Task 3 final state (kept for reference): commits `e1cd62d`, `7666608`, `0d86fb5`; CI run `40129274651` green on all platforms; real database at schema 8.

Before a live dispatch, the owner must approve it explicitly: it modifies a real checkout and spends Devin usage.
