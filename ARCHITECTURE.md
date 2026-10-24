# Architecture

## Product model

A **campaign** is one codebase-wide change. It moves through these stages:

1. **Define**: the change specification, implementer instructions and verification policy.
2. **Discover**: find affected repositories (local checkouts, GitHub organisations).
3. **Plan**: order repositories by dependency into parallel **waves** (`planWaves` in `packages/core`).
4. **Delegate**: one Devin session per repository (`packages/devin` contract).
5. **Verify**: ShiftOps re-runs the policy's commands against the agent's branch (`packages/verification`).
6. **Track**: campaign state is persisted locally and shown per repository.

Only stage 3's algorithm and the contracts for 4–5 exist today. See `PROJECT_STATE.md`.

## Processes and boundaries

```
┌──────────────────────── Desktop app (Tauri 2) ────────────────────────┐
│  Web view (React 19, Vite)                Rust core (tauri 2.12)      │
│  ─────────────────────────                ───────────────────────     │
│  TanStack Query ── invoke() ── IPC ──▶    commands.rs                 │
│  Zod-validated responses                    ├─ environment.rs (probes)│
│  React Router (hash), cmdk palette          ├─ store.rs (SQLite)      │
│  React Hook Form + Zod (forms)              ├─ keychain.rs (keyring)  │
│                                             └─ shell_path.rs (PATH)   │
└───────────────────────────────────────────────────────────────────────┘
```

- **All native work happens in Rust.** The web view has no shell, filesystem or SQL plugin. It can only call the five commands registered in `lib.rs`.
- **Every IPC response is validated** in `apps/desktop/src/lib/native.ts` with Zod schemas from `packages/core`. A Rust/TypeScript mismatch becomes a visible error, never silent bad data.
- **Outside Tauri** (`pnpm dev:web`), `native.ts` throws `NativeUnavailableError` instead of returning anything. There is no mock mode.

## Native commands

| Command                  | Purpose                                                                  |
| ------------------------ | ------------------------------------------------------------------------ |
| `get_app_info`           | App version, data directory, database path, schema version              |
| `get_settings`           | All stored settings as raw JSON strings                                  |
| `set_setting`            | Upsert one allow-listed setting (JSON value, ≤ 4 KiB)                    |
| `run_environment_checks` | Run every environment probe concurrently and return a report             |
| `credential_status`      | Whether a `github` or `devin` credential exists in the keychain (never the value) |

## Environment checks

`environment.rs` runs each probe with a timeout (10 s, 5 s for `docker info`), a neutral working directory (the OS temp dir, so Corepack shims are not affected by a nearby `package.json`) and the resolved PATH. Each result carries status, parsed version, raw detail, executable path, exact command and duration.

`shell_path.rs` exists because GUI apps on macOS and Linux do not inherit the shell's PATH. It reads PATH once from `$SHELL -ilc` (falling back to `-lc`), merges the process PATH and well-known install directories, and does its own executable lookup.

## Local state

- SQLite via `rusqlite` with the `bundled` feature (no system SQLite dependency), WAL mode.
- Path: `<app data dir>/shiftops.db`. On macOS: `~/Library/Application Support/dev.shiftops.desktop/shiftops.db`.
- Migrations are an ordered list in `store.rs`, tracked with `PRAGMA user_version`.
- Settings are schema-checked twice: Rust allow-lists keys and requires valid JSON; TypeScript parses each value with its Zod schema and falls back to the default on invalid data.

## Theme

The theme setting (`light` | `dark` | `system`) lives in SQLite. `useApplyTheme` toggles the `dark` class on `<html>` and follows OS changes when set to `system`. A copy of the last resolved theme in `localStorage` is applied before React mounts to prevent a flash; SQLite remains authoritative.

## Shared UI

`packages/ui` exports design tokens (`styles.css`, Tailwind v4 `@theme`) and components (Button, Card, Badge, StatusDot, EmptyState, ErrorState, Spinner, Kbd, Logo). Both apps import the same CSS and components, so the desktop app and website share one visual identity. Packages are consumed as TypeScript source (no build step); Vite and Next (`transpilePackages`) compile them.

## Website

Next.js 16 App Router with `output: "export"`: the site is fully static and has no server behaviour.

## Testing

| Layer          | Tool                          | What it covers                                           |
| -------------- | ----------------------------- | -------------------------------------------------------- |
| Packages       | Vitest                        | Planner, settings parsing, remote parsing, policy verdicts |
| Desktop UI     | Vitest + Testing Library      | Check rendering, IPC bridge validation, theme resolution |
| Rust units     | `cargo test`                  | Store, migrations, version parsing, PATH lookup, probes  |
| Rust IPC       | `cargo test` + `tauri::test`  | Commands invoked through Tauri's IPC with the real ACL   |
