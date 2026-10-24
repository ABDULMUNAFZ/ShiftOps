# Change log

Every meaningful change, newest first. Each entry is written in the same commit as the change. Timestamps are real, in IST (UTC+05:30). Entries marked **retrospective** were reconstructed from Git history after the fact; their timestamps are the commit times.

## 2026-10-25T02:29:57+05:30 — Campaigns, rollout planning and approval-gated Devin delegation (Task 4)

- Changed:
  - Campaigns page creates campaigns from real repository records with objective, migration type, runtime source/target majors, deadline and parallelism; campaign detail shows definition, members, status transitions, exclusion with reason, include, and cancel. All writes record activity.
  - `planCampaign` (pure, deterministic) classifies members against the goal, scores 0–1 risk with cited evidence, blocks no-checkout / unscanned / blocked-trust / cycle members in wave 0, orders the rest dependencies-first, reports edges outside the campaign as unknown and flags high-risk, conflicted or unclassifiable members for review.
  - `refresh_dependency_edges` records `inferred` edges from declared dependency names that match registered repositories, with the declaring fact as evidence.
  - Migration 9: `agent_tasks.instructions_hash`, `agent_tasks.approved_at`, `campaign_repositories.review_required`.
  - `devin.rs`: CLI detection and `devin auth status`; deterministic task instructions (identity, goal, evidence, scope, acceptance criteria, Task 5 verification commands, prohibitions); exact dispatch vector `devin -p <task> --permission-mode smart --sandbox --respect-workspace-trust false`.
  - New `devin` execution policies (`devin_cli`, `devin_dispatch`) and error code `DEVIN_NOT_CONFIGURED`.
  - 19 new commands (84 total): campaign lifecycle and membership, plan evidence/apply, dependency edges, Devin status, task draft/approve/dispatch/cancel, run list.
  - Agent Runs page lists tasks with instructions, hash, approval and runs; Approve and Dispatch are separate steps. Integrations shows Devin installed / connected / not signed in / error.
- Why: Task 4 turns repository intelligence into a planned, auditable rollout and delegates only what a human approved, inside Devin's own sandbox.
- Validation:
  - `pnpm check`: 92 JavaScript tests (core 21, desktop 58); 183 Rust tests passed, 3 ignored; Prettier, types, ESLint, rustfmt and Clippy `-D warnings` clean.
  - Website and desktop production builds pass.
  - IPC test drives create → evidence → apply → draft → approve and proves dispatch refuses without confirm, without approval, with a review flag and for an untrusted repository, starting no process.
  - Manual `devin::status_rehearsal` against the installed CLI: installed `devin 3000.11.205`, authenticated.
- Limitations:
  - No live Devin dispatch was run: it would modify a real checkout and spend the owner's Devin usage, so it needs the owner's explicit approval.
  - Devin cloud sessions are not used: the CLI documents no remote task-creation command beyond the interactive REPL.
  - Dependency edges are name-match heuristics only.

## 2026-10-25T00:59:50+05:30 — Repository management and Submission Readiness (Task 3, milestone 5)

- Changed:
  - Repository list now distinguishes metadata-only, external and ShiftOps-managed sources and shows GitHub identity, technology, runtime, package manager, branch/commit, working tree, trust, scan state and last scan time.
  - Repository detail supports explicit trust changes and confirmed record-only removal. Both actions create audit events; removing a record never deletes checkout files.
  - Devin remains detection-only and the Integrations page states `Full Devin workflow arrives in Task 4.`
  - Added native Submission Readiness evidence: required documents, remote, HEAD and origin/main, dirty/push state, participant TODO count, last recorded validation, bounded tracked-file secret rules and an intentionally unset final SHA.
  - Added a desktop Submission Readiness page with deterministic Not Ready / Needs Participant Input / Ready for Final Verification states, Copy Checklist, Open Files, Open GitHub and Refresh. `Submitted` is never inferred.
  - Added `pnpm validate:record`, which runs the complete check before writing a gitignored, mode-0600 `.shiftops/validation.json` for the current commit.
  - Fixed Windows CI's Unix-only test import from run `40125030556` by using `cfg(all(test, unix))`.
- Why: Task 3 must finish as an auditable repository-management product, not a collection of scanners, and submission status must be derived from evidence rather than manually claimed.
- Validation:
  - `pnpm check`: 88 JavaScript tests (desktop 54, including trust/removal and Submission Readiness UI); 179 Rust tests passed, 2 ignored; Prettier, types, ESLint, rustfmt and Clippy `-D warnings` clean.
  - Native tests cover deterministic status, redacted secret findings, real-checkout Git collection and record removal preserving files.
  - Website and desktop production builds pass.
  - Landed as `e1cd62d` plus `7666608` (Windows-only test fix: `Path::is_absolute` rejects the Unix-style destination used in the clone policy test). CI run `40128969708` passed on Linux, macOS and Windows: https://github.com/ABDULMUNAFZ/ShiftOps/actions/runs/40128969708
  - `pnpm validate:record` reran the full check and wrote `.shiftops/validation.json`; the dev app opened, applied migration 8 to the real database with a backup, and was closed.
- Limitations:
  - Live GitHub OAuth remains unverified because no real client ID or PAT was supplied.
  - `Submitted` and final SHA remain intentionally unavailable until the human submission flow in Task 6.

## 2026-10-25T00:49:13+05:30 — Managed clone and local linking (Task 3, milestone 4 completion)

- Changed:
  - Migration 8 adds explicitly approved managed workspaces, durable clone attempts, repository ownership and GitHub link method. Upgrades from every earlier schema are tested and preserve existing records.
  - Added the sole mutating Git operation permitted in Task 3: a policy-built initial HTTPS clone into `<approved workspace>/<owner>/<repository>`. It uses an argument vector, disables hooks, templates, submodules, interactive credentials and LFS smudging, and rejects credentials or non-HTTPS transports.
  - Clone jobs stream redacted progress, support cancellation, persist their outcome and start the read-only scanner after a verified success. Failed, cancelled and interrupted clones retain their folder for review.
  - Cleanup requires the exact recorded path and removes only a failed/cancelled clone directory after canonical-path, workspace, repository, root, symlink and home-directory checks.
  - Existing clones are inspected with the read-only Git policy. Matching remotes link as external; mismatches show both identities and require confirmation of the exact actual identity. External checkouts are never modified or deleted.
  - GitHub Browse now offers Metadata only, Clone into managed workspace and Link existing clone, with workspace approval, destination preview, progress/cancel, actionable errors and guarded cleanup.
- Why: GitHub metadata is useful for inventory, but deep analysis needs a checkout. These modes complete that workflow without broadening ShiftOps into a generic Git or filesystem client.
- Validation:
  - Unit coverage includes policy tampering, path traversal and collisions, retained failures, cancellation, cleanup boundaries, redaction, metadata-to-local transitions, real local HTTP clone, scan-after-clone and remote mismatch override.
  - Native IPC covers workspace approval, state, clone listing/cancellation and missing-record errors. Desktop component tests cover metadata import, reviewed clone destination and mismatch confirmation.
  - `pnpm check` passed: Prettier, typecheck and ESLint clean; 85 JavaScript tests passed (desktop 51); rustfmt and Clippy clean; 175 Rust tests passed, 2 manual rehearsals ignored.
- Limitations:
  - Private HTTPS clones rely on the user's existing Git credential helper or GitHub CLI setup; credentials are never injected into clone arguments.
  - ShiftOps never automatically deletes an incomplete clone.

## 2026-10-24T23:57:06+05:30 — Real GitHub integration (Task 3, milestone 4)

- Changed:
  - Migration 7 `github`: `github_accounts` (metadata and keychain credential kind only; one connected account), `github_organizations`, `github_repositories` (stable GitHub ids, case-insensitive owner/name, link to the ShiftOps repository) and `github_pull_requests` (head/base refs and SHAs, state, draft, mergeability, check summary, sync times).
  - Rust `github/`: an HTTP client (reqwest with native TLS) that sends the API version and User-Agent headers, validates every response into typed structs, follows `Link` pagination, and maps 401/403/404/422/429/5xx, primary and secondary rate limits, and network failures to structured errors.
    - OAuth Device Authorization Flow with no client secret. It handles pending, slow_down, expiry, denial, invalid codes and cancellation; the device code never leaves Rust.
    - Fine-grained PAT fallback, validated against `GET /user` before storage.
    - Account refresh, organisations, rate limit and disconnect.
    - Repository browsing for the user, an organisation or search, with visibility and archived filters, persisted on each fetch.
    - Repository details (languages, branches), open pull requests with check summaries, one pull request with mergeability, explicitly requested config files (secret names refused, redacted), and cancellable requests.
  - Configuration comes from `VITE_GITHUB_OAUTH_CLIENT_ID`, `VITE_GITHUB_HOST` and `VITE_GITHUB_API_BASE_URL`, read in Rust (runtime environment, then `apps/desktop/.env` at build time); see `apps/desktop/.env.example`. GitHub Enterprise endpoints are supported.
  - Seven new error codes: `NETWORK_FAILED`, `GITHUB_NOT_CONFIGURED`, `GITHUB_AUTHENTICATION_FAILED`, `GITHUB_PERMISSION_DENIED`, `GITHUB_RATE_LIMITED`, `GITHUB_AUTHORIZATION_EXPIRED`, `GITHUB_AUTHORIZATION_DENIED`.
  - Import: a browsed repository can be imported as metadata only (untrusted, no clone). Discovering a local checkout with the same origin turns that record into a local-linked one instead of creating a duplicate.
  - The generic Credential Security form no longer stores GitHub tokens. They go only through the validated connection. The Task 2 credential tests moved to the Devin slot with the same coverage.
  - UI:
    - Integrations has a GitHub connection: Not configured / Connected state, device sign-in with code, countdown, Copy Code, Open GitHub (validated URL) and Cancel; a PAT form with minimum permissions; account metadata, scopes, rate limit, organisations; Refresh, Replace token, and Disconnect with confirmation.
    - Repositories has Browse GitHub, with owner, visibility and archived filters, search, pagination, cancel, retry and import.
    - The repository detail Pull Requests tab shows real pull requests when connected.
  - CI installs `libssl-dev` for native TLS on Linux.
- Why: Task 3 milestone 4 requires real GitHub OAuth device sign-in and a PAT fallback, with tokens only in the keychain and no fake data.
- Validation:
  - `pnpm check` passed: JS 83 tests (desktop 49, including 8 new GitHub UI tests); Rust 154 passed, 2 ignored; Prettier, lint, rustfmt and clippy clean.
  - 14 Rust GitHub tests against a local mock server (mockito) cover:
    - PAT connect, with the token present only in the real macOS Keychain under a test service and absent from status and activity;
    - a rejected PAT storing nothing;
    - device flow pending, then slow_down with the interval raised to 3, then success with scopes `repo`/`read:org`;
    - denied, expired, invalid code, cancellation, a missing client ID, and Device Flow disabled;
    - primary and secondary rate limits with reset and retry-after, permission denied, not found, 502, a malformed body, and an unreachable host;
    - pagination via `Link`, archived filtering, search qualifiers and owner validation;
    - metadata import, pull requests with checks, single-PR mergeability;
    - config-file decoding with redaction, refusal of `.env` and path traversal;
    - 401 marking the account invalid;
    - request cancellation.
  - New DB tests: one active account, renames by stable id, metadata-only import becoming local-linked through discovery, PR mergeability invalidated by a new head SHA, closing missing PRs, organisation replacement. One IPC test covers the GitHub commands without an account.
  - Not verified against live GitHub: no OAuth client ID is configured in this environment, and no personal token was provided, so device sign-in and the real API were exercised only against the mock server.
  - Migration 7 was applied to the real database while the dev app was still running from milestone 3: its file watcher rebuilt the app 17 times during development. The applied schema matches the committed SQL (file last changed before it was applied), a `shiftops-v6-…` backup was written, and the extra restarts added 15 `database.opened` and 15 environment-check events. AGENTS.md now requires stopping the dev app before Rust or migration changes.
- Limitations:
  - One connected GitHub account at a time.
  - Pull-request check summaries are fetched for the first 30 open pull requests.
  - Import mode is metadata-only; the specification for other import modes was not received (the prompt was cut off).
  - OAuth requests the `repo` scope, which GitHub does not split into read and write; fine-grained tokens can be limited to read-only.

## 2026-10-24T23:35:48+05:30 — Repository intelligence scanner (Task 3, milestone 3)

- Changed:
  - Migration 6 `repository_intelligence`: `repository_facts` (value, evidence file, line, pointer, method, confidence) and `repository_conflicts`. `repository_scans` gains `scanner_version` and `cache_key`.
  - Rust `scanner/`:
    - Bounded traversal that never follows symlinks, never reads secret files (`.env*`, `.npmrc`, keys, credentials, …) and skips dependency, build, cache and fixture folders.
    - Detectors for Node/TypeScript and Python, Java, .NET, Go and Rust metadata, plus Docker and CI.
    - npm-style range checking, and scoped runtime and package-manager conflicts.
    - Cache keyed by scanner version, HEAD commit and the content fingerprint of the relevant files.
    - One scan per repository, with progress events, cancellation, a retention window of 3 scans, and activity events.
  - Commands: `start_repository_scan`, `cancel_repository_scan`, `get_repository_intelligence`, `list_repository_scans`. Repository rows now carry the latest scan status and summary.
  - UI: the repository list shows technology, runtime, package manager, scan state and conflicts. A new repository detail page has tabs Overview, Evidence, Runtimes, Dependencies, Scripts, Docker, CI, Git, Branches, Pull Requests (honest "not connected" state until milestone 4) and Activity.
  - Eleven new deterministic fixtures in `fixtures/repositories`.
- Why: Task 3 milestone 3 requires a read-only scanner whose every fact has evidence and whose conflicts are explicit.
- Validation:
  - `pnpm check` passed: JS 75 tests (desktop 41, including 5 new detail-page tests); Rust 134 passed, 2 ignored (manual rehearsals); Prettier, lint, rustfmt and clippy clean.
  - Fixture scans assert real values with evidence: Express and Jest; Next.js 14.2.15 on pnpm 9.12.0; Node range `>=18 <19` at `package.json:6 /engines/node`; FastAPI with Python 3.12.7; Spring Boot 3.3.4 on Java 21; `net8.0` with SDK 8.0.403; Go 1.22 with Gin; Cargo workspace members and toolchain 1.82.0; Yarn monorepo with nested NestJS.
  - The runtime-conflict fixture reports disagreeing versions, ranges that exclude declared versions, and npm plus yarn lockfiles.
  - The malformed fixture completes with a warning. A worktree `.git` file is handled. A symlink to a file outside the repository is not read, and a planted `.env` value never appears in facts.
  - Cache: an unchanged rescan reuses the stored scan; an uncommitted `package.json` edit invalidates it; an irrelevant file does not; Rescan forces a new scan.
  - Rehearsal scan of this repository: TypeScript primary; Node `>=22.22.0`; Rust 1.85; pnpm and Cargo; Next.js, React, Vite, Vitest, Tauri, Tokio and Serde; 0 conflicts. The first rehearsal exposed two defects, both fixed in this entry: fixture folders were scanned as the project, and conflicts were compared across unrelated packages.
  - `pnpm dev:desktop` upgraded the real database from 5 to 6 in place, with backup `shiftops-v5-20261024T180542Z.db`.
  - Scanning was not clicked through in the UI by the agent; it is covered by component tests, IPC and fixture tests.
- Limitations:
  - Detection is declarative (manifests and config). It does not resolve dependency trees or lockfile contents.
  - Python version conflicts compare only exact `major.minor` declarations, not ranges.
  - YAML and XML are read line-based rather than with full parsers; unusual formatting can be missed.
  - Scanner and walk tests are Unix-only.

## 2026-10-24T23:15:41+05:30 — Local repository discovery (Task 3, milestone 2)

- Changed:
  - Migration 5 `repository_roots_and_git_state`: new `repository_roots` table; `repositories` gains `root_id` (SET NULL), Git kind, detached flag, dirty flag, staged/modified/untracked counts, last commit time, collection time and warnings.
  - Rust `git/` (remote parsing with credential stripping, exact read-only command policy, repository config risk scan, metadata collection) and `discovery/` (filesystem walk, jobs with progress events and cancellation, transactional import).
  - Execution policy `readonly_git` for discovery only.
  - Native folder picker via `tauri-plugin-dialog`, called from Rust only.
  - Eight new commands: `pick_directory`, `start_discovery`, `cancel_discovery`, `get_discovery`, `import_discovered`, `list_repository_roots`, `remove_repository_root`, `list_repository_branches`.
  - Repositories page rebuilt: Open Local Repository, Discover Folder with depth and symlink options, live progress and Cancel, warnings, review table with selection, import, tracked folders with confirmed removal, and a searchable registered-repository table.
- Why: Task 3 milestone 2 requires real local discovery with read-only Git, imported repositories starting untrusted, and no file ever deleted.
- Validation:
  - `pnpm check` passed: Prettier, typecheck, lint; JS 70 tests (desktop 36, including 6 new discovery UI tests; github 11); Rust 106 passed, 1 ignored; rustfmt and clippy clean.
  - New Rust tests:
    - every schema version 1–4 upgrades to 5 without data loss;
    - walker: nesting, depth, skipped folders, worktree and submodule `.git` files, invalid markers, symlink escape, symlink loop, unreadable folder, entry limit, cancellation;
    - real temporary Git repositories (per-command `-c user.*`, `GIT_CONFIG_GLOBAL=/dev/null`): branch, commit, origin, dirty counts, worktree, non-GitHub remote, no origin, credential stripping, duplicate import refresh, root removal keeping repositories and files, repository mode, cancellation, and a planted `filter.clean` that is proven never to run;
    - the git policy rejects mutations and injected options;
    - an IPC test of the discovery commands.
  - Launched `pnpm dev:desktop`: the real database upgraded from 4 to 5 in place, with backup `backups/shiftops-v4-20261024T174534Z.db`; existing activity events preserved.
  - Discovery was not clicked through in the UI by the agent (no accessibility access); the flow is covered by component tests and real-Git Rust tests.
- Limitations:
  - A second checkout of an already-linked GitHub repository keeps its remote URL but is not linked (one checkout per GitHub identity, by the unique index).
  - Git metadata is refreshed only by re-running discovery.
  - Windows discovery is not exercised by tests (the walker and real-Git tests are Unix-only).

## 2026-10-24T22:33:06+05:30 — Product profile and submission documents (Task 3, milestone 1)

- Changed:
  - README introduction rewritten: tagline, one-line description, the event credit, the independence statement, Problem, Solution, the ShiftOps/Devin division of responsibility, a CI badge, and a task status table.
  - Added `CHANGELOG.md`, `AI_USAGE.md` and `SUBMISSION.md`.
  - Desktop: new About page (route `#/about`, sidebar link, command-palette entry). It shows identity, the event credit, independence and real build facts (version, schema, paths).
  - Website: footer now carries the event credit, the independence note and the source link. The hero uses the new description. The "Current status" card matches what exists.
  - `brand` in `packages/ui` gained `description`, `repositoryUrl` and `event`.
  - `PROJECT_STATE.md` header now tracks Tasks 1–6.
- Why: Task 3 asks for a restrained product identity. The hackathon credit appears only in the README introduction, website footer, desktop About page and `SUBMISSION.md`. Devin integration must not be presented as complete.
- Validation:
  - `pnpm check` passed: Prettier clean; typecheck 7/7; lint 0 problems; JS tests 63 passed (desktop 30, including the new `AboutPage.test.tsx`; core 13; github 10; verification 4; ui 3; devin 2; website 1); rustfmt clean; Rust 79 passed, 1 ignored; clippy clean.
  - Website and desktop production builds passed.
  - CI is green on all platforms for the preceding commit `824691f` (run `40114232654`: TypeScript, Linux, macOS and Windows all succeeded). This commit's own CI run is recorded on GitHub Actions.
  - The About page was not viewed in the running app (no accessibility access for UI automation); it is covered by its component test.
- Limitations: `SUBMISSION.md` items that depend on later tasks are marked Pending. The specifications for the rest of Task 3 (discovery, scanner, GitHub, repository management) have not been received in full.

## 2026-10-24T22:31:27+05:30 — Windows test executables load (CI repair 2)

- Changed: `apps/desktop/src-tauri/build.rs` now disables tauri-build's own manifest on MSVC targets. It embeds the same Common-Controls v6 manifest (`windows-app-manifest.xml`) through `/MANIFEST:EMBED` and `/MANIFESTINPUT`, for every linked target.
- Why: CI run `40112409827` passed clippy on `windows-latest`, but the test executable exited with `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`. tauri-build attaches the manifest only to binaries, so test executables could not resolve comctl32 v6 entry points.
- Validation: on macOS, `cargo clippy --all-targets -D warnings` passed and `cargo test` passed 79 with 1 ignored; the macOS build path is unchanged. CI run `40114232654` succeeded on all four jobs, including Windows clippy and the Windows test suite.
- Limitations: cannot be verified locally (no Windows toolchain here); CI is the verifier.

## 2026-10-24T22:07:20+05:30 — Gate Unix-only code for Windows (CI repair 1)

- Changed: `shell_path.rs` and `process/runner.rs` gate their Unix-only items with `cfg`: the login-shell markers, the `Duration` import and the termination grace period. The `/bin/sh` diagnostic IPC test in `lib.rs` now runs on Unix only.
- Why: the repository's first CI run, `40111900364`, failed `cargo clippy -D warnings` on `windows-latest` with 6 unused-item errors. The Linux, macOS and TypeScript jobs passed.
- Validation: local clippy clean and 79 Rust tests passed. CI run `40112409827`: Linux, macOS and TypeScript passed; Windows clippy passed but tests failed to load (fixed in the entry above).
- Limitations: on Windows, cancellation still terminates only the direct child process, not its process group.

## 2026-10-24T22:00:31+05:30 — Persistence, credentials, process runner, Activity Centre (Task 2) — retrospective

- Changed:
  - Versioned SQLite migrations (schema 1 → 4) with automatic backups, and typed services for every domain table.
  - Structured error contract with 16 codes.
  - OS keychain credential storage with redaction.
  - Policy-checked process runner with streaming, timeout and cancellation.
  - Path and URL validation.
  - Activity Centre, native diagnostics and credential management UI.
  - Prettier added.
- Why: give later tasks safe persistence, secret handling and command execution.
- Validation: 62 JS and 79 Rust tests passed; format, lint and clippy clean; both builds passed. The real database was upgraded from schema 1 to 4 in place with a backup; activity persisted across a restart. Commit `4701627`, pushed to `origin/main`.
- Limitations: UI click-through not performed by the agent. Windows untested at the time (see the CI repairs above).

## 2026-10-24T20:41:34+05:30 — Foundation (Task 1) — retrospective

- Changed: pnpm monorepo, Tauri 2 desktop shell, first-run environment checks, Next.js website, shared packages, CI workflow, project documentation.
- Why: establish the product shell and a verified toolchain.
- Validation: 35 JS and 16 Rust tests passed; lint and builds passed; the desktop app launched. Commit `cca3cf6`.
- Limitations: no repository features; Windows and Linux untested at the time.
