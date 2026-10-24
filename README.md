# ShiftOps

**Define the change once. Ship it everywhere.**

ShiftOps plans, coordinates, and verifies code migrations across every repository from one place.

`Built for Devin Hacks Kochi 2026`

Repository: https://github.com/ABDULMUNAFZ/ShiftOps

[![CI](https://github.com/ABDULMUNAFZ/ShiftOps/actions/workflows/ci.yml/badge.svg)](https://github.com/ABDULMUNAFZ/ShiftOps/actions/workflows/ci.yml)

ShiftOps is an independent project built for Devin Hacks Kochi 2026. It is not an official Cognition product.

## Problem

Engineering teams repeat the same change across many repositories: runtime upgrades, dependency replacements, vulnerability remediation and platform standardisation. Each repository is handled separately. Ordering mistakes break consumers, progress is tracked in spreadsheets, and "done" often means an agent or engineer said so.

## Solution

ShiftOps treats all repositories as one connected engineering system. It discovers affected projects, analyses their relationships, plans the correct change order, prepares repository-specific work for Devin and independently verifies every result.

```text
ShiftOps
- Discovers affected repositories
- Understands dependencies and risk
- Plans migration waves
- Prepares repository-specific Devin tasks
- Independently verifies results
- Tracks the complete transformation

Devin
- Understands an individual repository
- Implements the requested code changes
- Repairs build and test failures
- Produces a branch or pull request
```

## Status

ShiftOps is built in six tasks. This table records what exists today.

| Task                                                                | State                                                                                          |
| ------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| 1. Desktop foundation, environment checks, website                  | Complete                                                                                       |
| 2. SQLite persistence, OS keychain, safe process runner, activity   | Complete                                                                                       |
| 3. Product profile, local repository intelligence, GitHub           | Complete                                                                                       |
| 4. Campaign planning, dependency intelligence, Devin integration    | Complete (local Devin CLI; see below)                                                          |
| 5. Migration execution, independent verification, Devin repair loop | Complete (live Devin dispatch not exercised; see below)                                        |
| 6. End-to-end workflow, packaging, security audit, submission       | In progress: repository-management hardening done; packaging, audit and submission not started |

Details, decisions and test results: [PROJECT_STATE.md](PROJECT_STATE.md). Change history: [CHANGELOG.md](CHANGELOG.md).

## Repository layout

```
apps/
  desktop/        Tauri 2 + React desktop app (Rust backend in src-tauri/)
  website/        Next.js static site: landing, docs, security, privacy, download
packages/
  core/           Domain and error contracts (Zod), settings model, migration wave planner
  ui/             Shared design tokens and React components
  github/         GitHub provider contract and remote URL parsing
  devin/          Devin provider contract and session schema
  verification/   Verification policy schema and verdict evaluation
fixtures/
  repositories/   Minimal repositories used by tests
docs/             Design notes
```

## Requirements

| Tool    | Version used in development                                                                                                   | Minimum                              |
| ------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| Node.js | 26.7.0                                                                                                                        | 22.22.0 (required by React Router 8) |
| pnpm    | 10.33.0                                                                                                                       | 10                                   |
| Rust    | 1.99.1 stable                                                                                                                 | 1.85                                 |
| OS deps | Xcode Command Line Tools on macOS; see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for Linux and Windows |                                      |

## Using local repository discovery

1. Run `pnpm dev:desktop` and open **Repositories**.
2. Choose **Open Local Repository** to register one repository, or **Discover Folder** to search a folder. Pick a maximum depth (default 4) and whether to follow symbolic links that stay inside the folder.
3. Progress streams while ShiftOps searches and reads Git metadata. **Cancel** stops the search.
4. Review the results (branch, commit, origin, working-tree counts, warnings), select repositories and click **Import**.

Discovery reads the filesystem and runs only a fixed list of read-only Git commands. It never runs repository code, installs dependencies or changes files. Imported repositories start as **untrusted**.

## Repository intelligence

Open a repository and click **Run first scan** (or **Scan** / **Rescan**). The scanner reads manifests and configuration and records each fact with its file, line or JSON pointer, detection method and confidence:

- languages, runtimes, package managers, workspaces, lockfiles;
- frameworks, dependencies and declared scripts;
- Docker and CI configuration.

Conflicting runtime or package-manager declarations are listed explicitly.

The scanner never runs repository code and never reads secret files such as `.env` or `.npmrc`. Results are cached by commit and relevant-file content, so an unchanged repository is not rescanned.

To check what the scanner would report for any checkout, without the UI or the database:

```sh
SHIFTOPS_SCAN_PATH=/path/to/repo cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml scan_rehearsal -- --ignored --nocapture
```

## Connecting GitHub

ShiftOps talks to GitHub from its Rust core. Tokens are stored only in the OS keychain and are never shown again.

**Device sign-in (recommended):**

1. Create a GitHub OAuth app (GitHub → Settings → Developer settings → OAuth Apps) and enable **Device Flow**. No client secret is needed.
2. Copy `apps/desktop/.env.example` to `apps/desktop/.env` and set `VITE_GITHUB_OAUTH_CLIENT_ID`. For GitHub Enterprise, also set `VITE_GITHUB_HOST` and `VITE_GITHUB_API_BASE_URL`.
3. Restart `pnpm dev:desktop`.
4. Open Integrations → GitHub → **Sign in with GitHub**, then enter the code on GitHub.

**Fine-grained personal access token (fallback):** grant access to the repositories you need, with Metadata, Contents, Pull requests and Checks set to read-only, plus organisation Members (read) to list organisations. Paste it in Integrations → GitHub. ShiftOps validates it before storing it.

Then use **Repositories → Browse GitHub** and choose one of three modes:

- **Metadata only** inventories the repository without downloading it; deep analysis remains unavailable.
- **Clone into managed workspace** previews a new destination under a folder you explicitly approve, streams clone progress and scans the checkout after success.
- **Link existing clone** compares its normalized remote with the GitHub repository and requires an explicit override when they differ. Existing clones remain external and are never modified or deleted.

Repository detail lets you explicitly change trust or forget only the ShiftOps record. Forgetting a record never deletes checkout files. **Submission Readiness** derives the hackathon checklist from the current Git state, required documents, participant TODOs, validation evidence and a bounded tracked-secret review; it never marks the project Submitted automatically.

## Campaigns and Devin delegation

1. **Campaigns → New campaign**: name, objective (for example "Upgrade Node.js 18 to Node.js 22"), migration type, runtime key and source/target majors, optional deadline, parallelism, and repositories picked from your real records.
2. **Refresh edges** records inferred dependency edges where one repository's scanned manifest declares a package whose name matches another registered repository. Edges are labelled `inferred` with the matching fact as evidence; they are never presented as certain.
3. **Compute plan** runs the pure planner (`packages/core/src/planning.ts`) over the latest scan evidence: classification against the goal, a 0–1 risk score with cited reasons (uncommitted changes, scan conflicts, missing lockfile, no CI, many dependents), and deterministic waves. Wave 0 holds blockers (blocked trust, no checkout, no completed scan, dependency cycles). Later waves put dependencies before dependents. High-risk, conflicted, unclassifiable or externally-dependent repositories are flagged for human review.
4. **Apply plan & mark ready** persists the plan per member and moves the campaign to `ready`.
5. **Draft task** on a member builds a deterministic Devin task: repository identity, approved goal, scan evidence, scope, acceptance criteria, verification commands for Task 5, and explicit prohibitions. Its SHA-256 is stored.
6. **Agent Runs → Approve**, then **Dispatch to Devin**. Dispatch requires: approval, a cleared review flag, classification `affected`, a trusted repository with a local checkout, and an installed, signed-in Devin CLI. It runs `devin -p <task> --permission-mode smart --sandbox --respect-workspace-trust false` in that checkout. Cancel stops the process group.

Without a signed-in Devin CLI every step up to an approved draft still works; dispatch reports `DEVIN_NOT_CONFIGURED`.

Dispatch also refuses a checkout with uncommitted changes, a member whose verification is running, and a second live session for the same repository.

## Verification and repair

1. **Campaign detail → Verify** on a member shows the verification plan derived from that checkout's own root `package.json` and lockfile: one dependency install with lifecycle scripts disabled (`pnpm install --frozen-lockfile --ignore-scripts`, `yarn install --frozen-lockfile --ignore-scripts` or `npm ci --ignore-scripts`, only when a lockfile exists), then whichever of the `typecheck`, `lint`, `build`, `test` and `check` scripts exist, with each script body shown.
2. Verification runs the repository's own scripts, so it requires a **trusted** repository and a ticked confirmation. The plan hash covers every command and script body; if anything changed since you reviewed it, nothing runs.
3. Each check records its real exit code, duration and redacted output. Failures are marked repairable only when they are a failed script (not the install, a timeout or a missing command) whose output names files inside the checkout.
4. **Draft repair task** creates a bounded Devin task from the latest failed run (at most 2 per repository). It goes through the same Approve and Dispatch steps. When the repair session succeeds, ShiftOps re-runs the same approved plan automatically, but only if its hash is unchanged. A passed run marks the member `verified`; a Devin session alone never does.
5. This version verifies Node.js repositories only. Other ecosystems report "no verification plan".

## Managing repositories

- The repository detail page edits a display name and notes (discovery never overwrites them), shows whether the checkout folder still exists, and changes trust only after a confirmation that states what the new level allows. Lowering trust cancels live verification and Devin work in that checkout.
- There are three separate ways to let go of a repository. **Unlink checkout** turns a GitHub-linked record back into metadata-only and leaves the folder alone. **Delete managed files** is offered only for checkouts ShiftOps cloned; it needs the exact path typed and a clean working tree. **Forget record** removes the ShiftOps record only. External checkouts are never deleted.
- The five import actions are on the Repositories page: Open Local Repository, Discover Folder, GitHub Metadata Only, Clone into Managed Workspace, and Link Existing Clone.

## Commands

```sh
pnpm install            # install all workspaces
pnpm dev:desktop        # launch the desktop app (Vite + Tauri, hot reload)
pnpm dev:website        # website at http://localhost:3000
pnpm dev:web            # desktop frontend only, in a browser (native calls report unavailable)

pnpm format:check       # Prettier
pnpm typecheck          # tsc across all workspaces
pnpm lint               # ESLint across the repository
pnpm test               # Vitest across all workspaces
pnpm test:rust          # cargo test (includes IPC tests through Tauri's mock runtime)
pnpm lint:rust          # cargo clippy -D warnings
pnpm fmt:rust           # rustfmt --check
pnpm check              # all of the above
```

Print the real environment report for this machine as JSON:

```sh
SHIFTOPS_PRINT_ENV_REPORT=1 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml environment_checks -- --nocapture
```

Rehearse a schema upgrade on a **copy** of a database:

```sh
SHIFTOPS_UPGRADE_DB=/path/to/copy.db cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml upgrade_rehearsal -- --ignored --nocapture
```

## Where ShiftOps keeps data (macOS)

| What                | Location                                                         |
| ------------------- | ---------------------------------------------------------------- |
| Database            | `~/Library/Application Support/dev.shiftops.desktop/shiftops.db` |
| Pre-upgrade backups | `~/Library/Application Support/dev.shiftops.desktop/backups/`    |
| Application log     | `~/Library/Logs/dev.shiftops.desktop/shiftops.log` (redacted)    |
| Command run logs    | `~/Library/Logs/dev.shiftops.desktop/runs/<run id>.log`          |
| Credentials         | macOS Keychain, service `dev.shiftops.desktop`                   |

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md): how the pieces fit together
- [SECURITY.md](SECURITY.md): threat model and reporting
- [CONTRIBUTING.md](CONTRIBUTING.md): workflow and standards
- [AGENTS.md](AGENTS.md): rules for AI coding agents working in this repository
- [PROJECT_STATE.md](PROJECT_STATE.md): what is done, what is next, and why
- [CHANGELOG.md](CHANGELOG.md): dated record of every meaningful change
- [AI_USAGE.md](AI_USAGE.md): which AI tools contributed what
- [SUBMISSION.md](SUBMISSION.md): hackathon submission details
