# Change log

Every meaningful change, newest first. Each entry is written in the same commit as the change. Timestamps are real, in IST (UTC+05:30). Entries marked **retrospective** were reconstructed from Git history after the fact; their timestamps are the commit times.

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
