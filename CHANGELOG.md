# Change log

Every meaningful change, newest first. Each entry is written in the same commit as the change. Timestamps are real, in IST (UTC+05:30). Entries marked **retrospective** were reconstructed from Git history after the fact; their timestamps are the commit times.

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
