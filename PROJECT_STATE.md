# Project state

Last updated: 2026-10-24. Step 1 of the build plan is complete. Step 2 has **not** been started.

## 1. Product architecture

ShiftOps is a Tauri 2 desktop app with a React 19 web view and a Rust core, plus a static Next.js website, in one pnpm monorepo.

- **Web view** (`apps/desktop/src`): React Router (hash routing), TanStack Query for all native calls, React Hook Form + Zod for forms, cmdk command palette, Tailwind v4 with shared tokens from `packages/ui`.
- **Rust core** (`apps/desktop/src-tauri/src`): five Tauri commands. It handles process execution (`environment.rs`), PATH resolution (`shell_path.rs`), SQLite (`store.rs`, rusqlite bundled) and the OS keychain (`keychain.rs`, keyring crate).
- **Shared packages**:
  - `core`: Zod schemas for environment reports and settings, and the `planWaves` dependency planner.
  - `ui`: design tokens and components.
  - `github`: provider contract and Git remote parsing.
  - `devin`: provider contract and session schema.
  - `verification`: policy schema and verdict evaluation.
- **Website** (`apps/website`): Next.js 16, `output: "export"`. Pages: landing, docs, security, privacy and download.

Full detail: [ARCHITECTURE.md](ARCHITECTURE.md).

## 2. Completed functionality

- pnpm monorepo with the requested layout, strict TypeScript, ESLint flat config and CI workflow (Linux/macOS/Windows Rust matrix).
- **Desktop shell**:
  - Seven-section navigation: Overview, Repositories, Campaigns, Agent Runs, Verification, Integrations and Settings.
  - Collapsible sidebar, persisted in SQLite (`⌘B` / `Ctrl+B`).
  - Command palette (`⌘K` / `Ctrl+K`) for navigation, theme, sidebar and re-running checks.
  - `⌘1`–`⌘7` jump straight to each section.
- **Themes**: light, dark and system. The choice is persisted in SQLite and applied before first paint from a local hint. In system mode it follows OS changes live.
- **First-run screen**: runs the real native environment checks and shows each result's status, version, raw output, executable path, exact command and duration. "Continue" stores `onboardingCompletedAt` in SQLite.
- **Environment checks** (Rust): OS name/version/arch, app data directory (write probe), Git, Node.js, pnpm, npm, Yarn, Docker (CLI and daemon reachability), Devin CLI and OS keychain (write/read/delete probe). Missing tools report `unavailable` with the reason. Failing tools report `error` with exit code and stderr.
- **Login-shell PATH resolution**, so tools installed via Homebrew, nvm, Corepack or `~/.local/bin` are found when the app is launched outside a terminal.
- **Overview**: real environment summary, keychain credential status for GitHub and Devin, and local state (version, DB path, schema version).
- **Integrations**: real keychain status per provider and real Git and Devin CLI detection. Connect buttons are disabled and labelled as not implemented.
- **Settings**: preferences form (React Hook Form + Zod), storage paths and full environment panel with re-run.
- **Repositories, Campaigns, Agent Runs and Verification**: truthful empty states. No sample data.
- **Website**: landing, docs (status table, build steps, the exact method of each check, shortcuts), security, privacy and download. The download page states that no builds have been published. Light/dark toggle. Same tokens and components as the desktop app.
- Test fixtures: two minimal Node repositories (`fixtures/repositories`), for future discovery tests.

## 3. Current work

None in progress. Step 1 is closed.

## 4. Decisions and reasons

| Decision | Reason |
| --- | --- |
| Tauri 2.12.5 (crate) / CLI and `@tauri-apps/api` 2.12.4 | Latest stable 2.x. `cargo search` shows 3.0.0-alpha, which is not stable. |
| React 19.3.1, Vite 8.3.5, `@vitejs/plugin-react` 6.1.2 | Current stable. plugin-react 6 requires Vite ^8. |
| TypeScript **6.0.3**, not 7.0.2 | `typescript-eslint` 8.72.0 has peer `typescript >=4.8.4 <6.1.0`. |
| React Router 8.5.0 | Current stable. Requires Node ≥ 22.22, so the root `engines` field is set to that. |
| Hash routing | The production bundle is served by Tauri's asset protocol, which has no history-API fallback for deep links. |
| SQLite through `rusqlite` in Rust, not `tauri-plugin-sql` | Keeps SQL out of the web view. The web view cannot run arbitrary queries. `bundled` avoids a system SQLite dependency. |
| No `STRICT` tables | macOS ships sqlite3 3.32.3, which cannot open `STRICT` tables. Inspecting the DB with system tools matters more than STRICT on a key/value table. |
| `keyring` 3.6.3, not 4.2.0 | 4.x restructured into separate store crates. 3.6 is stable with a well-known API and supports macOS, Windows and Secret Service. |
| Settings key allow-list in Rust and Zod parsing in TypeScript | Defence in depth. Corrupt values fall back to defaults instead of crashing. |
| No mock mode for the web view in a browser | It would be a source of fake data. `native.ts` throws `NativeUnavailableError` instead. |
| Checks run with cwd = OS temp dir | Running in the repo made Corepack's `yarn` refuse ("This project is configured to use pnpm"). A neutral directory gives the machine-level answer. |
| Docker is `unavailable` when the daemon is unreachable | The CLI being installed is not enough to use Docker. The CLI version is still reported. |
| Rust IPC tests via `tauri::test` and `generate_context!(test = true)` | They exercise the same IPC and ACL path as `invoke()`, not just the functions. |
| Packages consumed as TS source | No build step for internal packages. Vite and Next `transpilePackages` compile them. |
| Next.js static export | The website has no server behaviour. Static output can be hosted anywhere. |

## 5. Commands run

```sh
# Tooling inspection
node -v                 # v26.7.0
pnpm -v                 # 10.33.0
npm -v                  # 11.19.0
yarn -v                 # 1.22.22
git --version           # 2.50.1 (Apple Git-155)
docker --version        # 29.8.1
rustc -V                # not installed
which devin             # not found
uname -a                # Darwin 25.6.0 x86_64

# Rust install (approved by the user)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal -c clippy -c rustfmt
rustc -V                # 1.99.1 (2026-10-13)

# Version and compatibility checks (npm view with a temporary cache, because ~/.npm has root-owned files)
npm view <pkg> version / peerDependencies / engines    # for every dependency
cargo search / cargo info tauri@2 tauri-build@2 keyring@3 rusqlite tokio

# Build, test and run
pnpm install
pnpm tauri icon src-tauri/icons/icon.svg -o src-tauri/icons
pnpm typecheck && pnpm lint && pnpm test
cargo test / cargo clippy --all-targets -- -D warnings / cargo fmt --check
SHIFTOPS_PRINT_ENV_REPORT=1 cargo test environment_checks -- --nocapture
pnpm --filter @shiftops/website build
pnpm --filter @shiftops/desktop build
pnpm dev:desktop        # app launched; process target/debug/shiftops-desktop running
pnpm dev:website        # / /docs/ /security/ /privacy/ /download/ all HTTP 200
sqlite3 "~/Library/Application Support/dev.shiftops.desktop/shiftops.db" "pragma user_version"   # 1
```

## 6. Test and build results

| Check | Result |
| --- | --- |
| `pnpm install` | OK |
| `pnpm typecheck` | 7/7 workspaces pass |
| `pnpm lint` | 0 problems |
| `pnpm test` | 35 tests in 10 files pass: core 8, github 10, devin 2, verification 4, ui 3, desktop 7, website 1 |
| `cargo test` | 16 pass: store 4, environment 6, shell_path 2, IPC 4 |
| `cargo clippy --all-targets -D warnings` | clean |
| `cargo fmt --check` | clean |
| `next build` | 6 static routes |
| `vite build` (desktop) | OK. One 593 kB JS chunk (warning only). |
| `tauri dev` | App compiles and launches. SQLite created at schema version 1. |

Real environment report from this machine, through Tauri IPC (2026-10-24):

| Check | Status | Detail |
| --- | --- | --- |
| OS | available | macOS 26.7.1 (macos, x86_64) |
| App data dir | available | writable |
| Git | available | 2.50.1, `/usr/bin/git` |
| Node.js | available | 26.7.0, `~/.local/bin/node` |
| pnpm | available | 10.33.0, `/usr/local/bin/pnpm` |
| npm | available | 11.19.0, `~/.local/bin/npm` |
| Yarn | available | 1.22.22, `/usr/local/bin/yarn` |
| Docker | unavailable | CLI 29.8.1 installed. Daemon not reachable (`Cannot connect to the Docker daemon at unix:///Users/zahirhussain/.docker/run/docker.sock`). |
| Devin CLI | unavailable | `devin` not found in 24 directories searched |
| OS keychain | available | macOS Keychain probe write/read/delete succeeded |

## 7. Known limitations

- **UI flows not observed on screen.** `screencapture` failed ("could not create image from display") because the terminal has no Screen Recording permission. Rust↔IPC is proven by tests and the React components by tests. The click-through (first run, navigation, theme persistence across restart) still needs a manual pass. See the checklist below.
- **Devin CLI detection assumes the executable is named `devin`.** If Devin's CLI uses a different binary name, the check reports it as not found.
- **Integrations cannot connect yet.** There is no UI to store GitHub or Devin credentials. `credential_status` only reads.
- **Repository scanning, campaigns, agent runs and verification are not implemented.** By design for Step 1, those pages show empty states.
- **No signed or notarised builds.** `tauri build` produces an unsigned local bundle.
- **Desktop bundle is a single 593 kB chunk.** Acceptable for a locally loaded app. Code-split if startup time becomes measurable.
- **Windows and Linux are untested locally.** The code paths exist (`cfg` branches, CI matrix), but the CI workflow has not run yet because there is no remote repository.
- **The login-shell PATH probe runs `$SHELL -ilc`**, which executes the user's interactive rc files once per app launch (4 s timeout, stdin closed).
- **`git init` was run, but nothing is committed.**

### Manual verification checklist

1. `pnpm dev:desktop`: the first-run screen lists 10 checks with real values. Click **Continue**.
2. Navigate with the sidebar and with `⌘1`–`⌘7`. Open the palette with `⌘K`.
3. Settings: choose Dark and save. Quit the app, run `pnpm dev:desktop` again and confirm it starts dark. `sqlite3 ~/Library/Application\ Support/dev.shiftops.desktop/shiftops.db "select * from settings"` shows `theme|"dark"`.
4. `⌘B` collapses the sidebar. Restart and confirm it stays collapsed.
5. Start Docker Desktop and re-run checks from the palette. Docker turns `available`, with the server version.
6. `pnpm dev:website` and open http://localhost:3000.

## 8. Exact next step

**Step 2: local repository discovery.** Do not start until it is requested.

1. Append migration 2 in `store.rs` with tables `repository_roots(id, path, added_at)` and `repositories(id, root_id, path, name, default_branch, head_sha, remote_url, detected_at)`.
2. Add Rust command `add_repository_root(path)`, using the native folder picker via `tauri-plugin-dialog`. Record the capability change here.
3. Add Rust command `scan_repository_root(root_id)`. It walks for `.git` directories (depth limit, skips `node_modules`/`target`) and reads branch, HEAD and remote with `git` as a direct argv call.
4. Parse remotes with `parseGitRemote` from `packages/github`. Show results on the Repositories page.
5. Write tests against `fixtures/repositories`: copy each fixture to a temp dir, `git init`, commit, then scan.
