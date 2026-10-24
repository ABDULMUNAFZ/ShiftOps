# Instructions for AI coding agents

Read `PROJECT_STATE.md` first. It records the current step, decisions and the exact next step. Do not start work beyond the step it names.

## Hard rules

1. **No fake product data.** Do not add sample repositories, campaigns, agent runs, metrics, charts or terminal output to the application. Unimplemented areas show an empty state that says so.
2. **No simulated success.** If a check or operation cannot run, report `unavailable` or `error` with the real reason.
3. **Secrets stay in Rust and the OS keychain.** Never send a credential value to the web view, SQLite, logs or browser storage.
4. **Do not widen the web view's powers.** Do not add Tauri shell, fs, http or sql plugins or broaden `capabilities/default.json` without recording the reason in `PROJECT_STATE.md`.
5. **Never pass user input to a shell.** Execute programs directly with argument arrays.
6. **Verify before claiming done.** Run `pnpm check` and `pnpm fmt:rust`, and paste the actual results into `PROJECT_STATE.md`.

## Where things go

| Change                                   | Location                                   |
| ---------------------------------------- | ------------------------------------------ |
| Domain schema or pure algorithm          | `packages/core/src`                        |
| Shared component or design token         | `packages/ui/src`                          |
| New native command                       | `apps/desktop/src-tauri/src/commands.rs`, register in `lib.rs`, add schema and wrapper in `apps/desktop/src/lib/native.ts`, add an IPC test in `lib.rs` |
| SQLite schema change                     | Append a migration to `MIGRATIONS` in `store.rs`. Never edit an existing migration once released. |
| New page                                 | `apps/desktop/src/pages`, route in `app/App.tsx`, nav entry in `app/navigation.ts` |

## Toolchain notes

- TypeScript is pinned to 6.0.x because `typescript-eslint` 8.72 supports `<6.1.0`. Do not upgrade to TypeScript 7 until it does.
- `rusqlite` 0.41 does not implement `ToSql`/`FromSql` for `usize`; use `u32` or `i64`.
- Tauri IPC tests must use origin `tauri://localhost` (`http://tauri.localhost` on Windows) and `generate_context!(test = true)`.
