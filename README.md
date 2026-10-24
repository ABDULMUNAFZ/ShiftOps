# ShiftOps

**Define the change once. Ship it everywhere.**

ShiftOps is a desktop application for engineering organisations. You define one codebase-wide change, ShiftOps discovers every affected repository, plans the migration order, delegates repository-level implementation to Devin, verifies each result independently and tracks the whole campaign.

> **Status:** early development. Step 2 is complete: versioned SQLite persistence (schema 4), OS keychain credential storage, a policy-checked native process runner, and a persisted Activity Centre. Repository discovery, planning, delegation and verification are not implemented yet. See [PROJECT_STATE.md](PROJECT_STATE.md).

## Repository layout

```
apps/
  desktop/        Tauri 2 + React desktop app (Rust backend in src-tauri/)
  website/        Next.js static site: landing, docs, security, privacy, download
packages/
  core/           Domain schemas (Zod), settings model, migration wave planner
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
| OS deps | Xcode Command Line Tools on macOS; see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for Linux and Windows |

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
