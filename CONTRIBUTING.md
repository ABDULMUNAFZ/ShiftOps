# Contributing

## Setup

```sh
pnpm install
pnpm dev:desktop
```

See the requirements table in [README.md](README.md).

## Before opening a pull request

```sh
pnpm check     # typecheck, lint, JS tests, Rust tests, clippy
pnpm fmt:rust  # rustfmt --check
```

CI runs the same steps on Linux, macOS and Windows.

## Standards

- **No fake data.** Every value shown in the app must come from application state, the filesystem, Git, GitHub or Devin. Empty states say what is missing and why. Test doubles belong only in tests.
- **Truthful failure.** If something cannot be determined, show `unavailable` or `error` with the underlying message. Do not guess or hide errors.
- **Native work in Rust.** Process execution, filesystem access, SQLite and keychain access stay behind Tauri commands. Validate every IPC response with Zod.
- **Strict TypeScript.** `strict`, `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes` are on. Avoid `any` and non-null assertions.
- **Accessible UI.** Every control must be keyboard-reachable and have an accessible name. Use the shared components in `packages/ui`.
- **Pinned versions.** Dependencies use exact versions. Check compatibility (peer dependencies, engines) before upgrading.
- **Update `PROJECT_STATE.md`** when you finish a piece of work: what changed, commands run, results, and the next step.

## Commits

Use Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`, `test:`, `refactor:`).
