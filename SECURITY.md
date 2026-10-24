# Security

## Reporting a vulnerability

Report vulnerabilities privately using GitHub's **Report a vulnerability** (security advisory) form on this repository. Do not open a public issue. Include reproduction steps and the affected version or commit.

## Threat model

ShiftOps will hold credentials that can push code to many repositories and start agent sessions. The design assumes the web view is the most exposed component and keeps it powerless.

### Credentials

- Stored only in the OS credential store via the `keyring` crate: macOS Keychain, Windows Credential Manager, Secret Service on Linux. Service name: `dev.shiftops.desktop`.
- Read only in Rust. The `credential_status` command returns whether a credential exists, never its value.
- Never written to SQLite, logs, `localStorage` or the frontend bundle.

### Web view

- Only five application commands are registered. No Tauri shell, fs, http or sql plugins are enabled.
- The capability file grants `core:default` only.
- Content Security Policy: `default-src 'self'; script-src 'self'; connect-src ipc: http://ipc.localhost` (see `tauri.conf.json`). The web view cannot load remote scripts or make network requests.

### Native commands

- Environment checks execute fixed programs with fixed arguments. No user-controlled string reaches a shell or process argument list.
- The login shell is invoked once with a fixed script to read `PATH`, with a 4 second timeout and stdin closed.
- `set_setting` accepts only allow-listed keys, requires valid JSON and caps values at 4 KiB.
- `credential_status` accepts only `github` or `devin`.

### Agent output

Agent-produced changes are untrusted. A repository is marked verified only after ShiftOps itself re-runs the policy's commands against the branch (not yet implemented; see `PROJECT_STATE.md`).

## Supported versions

No release has been published. Security fixes apply to `main`.
