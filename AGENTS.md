# Instructions for AI coding agents

Read `PROJECT_STATE.md` first. It records the current step, decisions and the exact next step. Do not start work beyond the step it names.

## Hard rules

1. **No fake product data.** Do not add sample repositories, campaigns, agent runs, metrics, charts or terminal output to the application. Unimplemented areas show an empty state that says so.
2. **No simulated success.** If a check or operation cannot run, report `unavailable` or `error` with the real reason.
3. **Secrets stay in Rust and the OS keychain.** Never send a credential value to the web view, SQLite, logs or browser storage.
4. **Do not widen the web view's powers.** Do not add Tauri shell, fs, http or sql plugins or broaden `capabilities/default.json` without recording the reason in `PROJECT_STATE.md`.
5. **Never pass user input to a shell.** Every external program runs through `process::ProcessRunner` with an argument array. Do not use `std::process::Command` or `tokio::process::Command` elsewhere. New command categories need an explicit `ExecutionPolicy` change, recorded in `PROJECT_STATE.md`.
6. **Verify before claiming done.** Run `pnpm check` (format, types, lint, JS tests, rustfmt, Rust tests, clippy), and paste the actual results into `PROJECT_STATE.md`.
7. **No raw string errors.** Commands return `AppResult<T>` with a stable `ErrorCode`. Add new codes in both `error.rs` and `packages/core/src/errors.ts`.
8. **Redact before you persist or display.** Logs, activity, process output and error details go through `redact.rs`. Never log a `Secret` or a token.
9. **React never runs SQL.** Add a service function in `db/`, a command, a Zod schema in `packages/core/src/domain.ts`, a wrapper in `lib/native.ts`, a key in `lib/queryKeys.ts` and, if it writes, an entry in `lib/invalidation.ts`.
10. **Tests use temporary databases and the test keychain service** (`credentials::tests::TestKeychain`). Never touch the real app data directory or `dev.shiftops.desktop` keychain entries from tests.

11. **Stop the dev app before changing Rust or migrations.** `pnpm dev:desktop` rebuilds on every Rust change and applies new migrations to the real database in `~/Library/Application Support/dev.shiftops.desktop/`. Stop it (only the PIDs you started) before editing, and restart it only to apply a finished migration.

## Where things go

| Change                           | Location                                                                                                                                                                                 |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Domain schema or pure algorithm  | `packages/core/src`                                                                                                                                                                      |
| Shared component or design token | `packages/ui/src`                                                                                                                                                                        |
| New native command               | `apps/desktop/src-tauri/src/commands.rs`, register in `lib.rs`, add schema and wrapper in `apps/desktop/src/lib/native.ts`, add an IPC test in `lib.rs`                                  |
| SQLite schema change             | Add `src-tauri/src/db/sql/000N_name.sql` and append it to `MIGRATIONS` in `db/migrations.rs`. Never edit a released migration. No `STRICT` tables (macOS sqlite3 3.32 cannot open them). |
| New page                         | `apps/desktop/src/pages`, route in `app/App.tsx`, nav entry in `app/navigation.ts`                                                                                                       |

## Toolchain notes

- TypeScript is pinned to 6.0.x because `typescript-eslint` 8.72 supports `<6.1.0`. Do not upgrade to TypeScript 7 until it does.
- `rusqlite` 0.41 does not implement `ToSql`/`FromSql` for `usize`; use `u32` or `i64`.
- Clippy is run with `--all-targets -D warnings`. Service modules allow dead code outside test builds only (`#![cfg_attr(not(test), allow(dead_code))]`), because later steps call those operations.
- Tauri IPC tests must use origin `tauri://localhost` (`http://tauri.localhost` on Windows) and `generate_context!(test = true)`.
