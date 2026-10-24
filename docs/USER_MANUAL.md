# ShiftOps user manual

This guide walks through a complete migration campaign in the ShiftOps desktop app: adding repositories, planning, delegating to Devin, verifying the result and repairing failures. Every step uses real data from your machine. Nothing in the app is sample data.

## 1. Install and start

There are no signed installers yet, so build from source.

1. Install Node.js 22.22 or newer, pnpm 10, and Rust through `rustup`, plus the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform.
2. Clone the repository and run:

   ```sh
   pnpm install
   pnpm dev:desktop
   ```

3. On first launch, ShiftOps checks your workstation: OS, data directory, Git, Node.js, pnpm, npm, Yarn, Docker, the Devin CLI and the OS keychain. Missing optional tools are reported as unavailable; nothing is faked.

Keyboard: ⌘K (Ctrl+K) opens the command palette, ⌘1–8 switch sections, ⌘J opens the Activity Centre, ⌘B collapses the sidebar.

## 2. Add repositories

The **Repositories** page offers five import actions:

| Action                           | What it does                                                                                                                                                                                                                                             |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Open Local Repository**        | Registers one checkout you pick.                                                                                                                                                                                                                         |
| **Discover Folder**              | Searches a folder up to the depth you choose (0–12). It skips dependency, build and hidden folders and never follows links that leave the folder. You pick which results to import.                                                                      |
| **GitHub Metadata Only**         | Records a GitHub repository without downloading it. Deep analysis needs a checkout.                                                                                                                                                                      |
| **Clone into Managed Workspace** | Clones into `<approved workspace>/<owner>/<name>`. You approve the workspace folder once. ShiftOps shows the exact destination before cloning, streams progress and supports cancel. A failed or cancelled clone keeps its folder until you clean it up. |
| **Link Existing Clone**          | Attaches a checkout you already have. If its remote differs from the GitHub repository, both identities are shown and you must confirm an override.                                                                                                      |

The three GitHub actions need a connected account (see §4). Private clones use your own Git credentials; run `gh auth login` and then `gh auth setup-git` if a clone reports `GIT_AUTHENTICATION_REQUIRED`. ShiftOps never passes its stored token to Git.

Every imported repository starts **untrusted**.

## 3. Understand a repository

Open a repository to see:

- **Overview, Evidence, Runtimes, Dependencies, Scripts, Docker, CI**: results of the read-only scan. Every fact cites its file and line or JSON pointer. Use **Scan** or **Rescan**.
- **Git, Branches, Pull Requests, Activity**: read-only Git state, local branches, open pull requests with check summaries (GitHub connected), and the audit trail.
- **Details and removal**: display name, notes, and whether the checkout folder still exists.

**Trust** controls what ShiftOps may do with the checkout. A confirmation states the effect before anything changes:

- _Untrusted_ (default) and _Blocked_: read only.
- _Trusted_: the repository may run its own package scripts during verification and may be delegated to Devin after approval.

Lowering trust immediately cancels any live verification or Devin session in that checkout.

**Letting go of a repository** has three separate actions:

- **Unlink checkout** turns a GitHub-linked record back into metadata-only. The folder is not touched.
- **Delete managed files** is offered only for checkouts ShiftOps cloned. You must type the exact path, and the working tree must be clean. Commits that were never pushed are lost.
- **Forget record** removes the ShiftOps record only. Files stay on disk.

External checkouts are never deleted by ShiftOps.

## 4. Connect GitHub (optional)

Integrations → GitHub:

- **Sign in with GitHub** (device flow) needs `VITE_GITHUB_OAUTH_CLIENT_ID` in `apps/desktop/.env`.
- Or paste a **fine-grained personal access token** with read access to Metadata, Contents, Pull requests and Checks (plus organisation Members to list organisations).

Tokens are validated before they are stored, and they live only in the OS keychain.

## 5. Plan a campaign

1. **Campaigns → New campaign**: name, objective (for example "Upgrade Node.js 18 to Node.js 22"), migration type, runtime and source/target majors, optional deadline, parallelism, and the repositories.
2. **Refresh edges** records `inferred` dependency edges where a manifest declares a package that matches another registered repository.
3. **Compute plan** classifies every member against the goal and scores risk with cited reasons. It orders members into waves: dependencies first, and blockers (no checkout, no scan, blocked trust, cycles) in wave 0. High-risk or unclear members are flagged for review.
4. **Apply plan & mark ready** saves it. Use **Clear review** once you have reviewed a flagged member, or **Exclude** with a reason.

## 6. Delegate to Devin

1. **Draft task** on a member generates deterministic instructions: identity, goal, scan evidence, scope, acceptance criteria and prohibitions. Their SHA-256 is pinned.
2. **Agent Runs → Approve** approves exactly that text, after a confirmation.
3. **Dispatch to Devin** asks for a final confirmation, then runs `devin -p <task> --permission-mode smart --sandbox --respect-workspace-trust false` in the checkout. Dispatch is refused unless every condition holds:
   - the task is approved and unchanged since approval;
   - no review flag remains;
   - the member is classified `affected`;
   - the repository is trusted;
   - the working tree is clean;
   - no verification is running;
   - no other session is live;
   - the Devin CLI is installed and signed in.
4. **Cancel** stops the session's whole process group.

A finished session moves the member to _awaiting verification_. It is never treated as success on its own.

## 7. Verify and repair

1. Campaign detail → **Verify** on a member shows the verification plan derived from that checkout:
   - a dependency install with lifecycle scripts disabled (only when a lockfile exists);
   - then whichever of `typecheck`, `lint`, `build`, `test` and `check` the root `package.json` declares, each with its script body.
2. Tick **I reviewed these commands** and **Run verification**. The repository must be trusted. If any script or the lockfile changed since you opened the plan, nothing runs and you are asked to review again.
3. Each check records its real exit code, duration and redacted output. A run **passes** only when every check passes, which marks the member _verified_.
4. A failed check is marked **repairable** when it is a script failure whose output names files in the checkout. Install failures, timeouts and missing commands are left for you to fix.
5. **Draft repair task** (at most 2 per repository) creates a Devin task from the failures. Approve and dispatch it like any task. When the repair session succeeds, ShiftOps re-runs the same approved plan automatically, but only if it is unchanged.

Verification currently supports Node.js repositories (root `package.json`).

## 8. Submission Readiness

Submission Readiness (⌘8) checks this ShiftOps repository:

- the required documents and the GitHub remote;
- HEAD against the last fetched `origin/main`, and a clean working tree;
- participant TODOs and a bounded scan for tracked secrets (paths and rule names only);
- a passing validation recorded for the current HEAD. Run `pnpm validate:record` to record one.

Blocking reasons and non-blocking notes are listed separately. **Submitted** is never set automatically.

## 9. Where your data lives (macOS)

| What        | Location                                                         |
| ----------- | ---------------------------------------------------------------- |
| Database    | `~/Library/Application Support/dev.shiftops.desktop/shiftops.db` |
| Backups     | `~/Library/Application Support/dev.shiftops.desktop/backups/`    |
| Logs        | `~/Library/Logs/dev.shiftops.desktop/` (redacted)                |
| Credentials | Keychain, service `dev.shiftops.desktop`                         |

The database is upgraded in place with a backup first. A database written by a newer ShiftOps is refused and left untouched.

## 10. Troubleshooting

| Message or code                                                   | What to do                                                                                            |
| ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `GIT_AUTHENTICATION_REQUIRED`                                     | Run `gh auth login`, then `gh auth setup-git` (or configure another credential helper), then retry.   |
| `DEVIN_NOT_CONFIGURED`                                            | Install and sign in to the Devin CLI. Everything up to an approved draft still works.                 |
| "The checkout has uncommitted changes"                            | Commit or stash your work before delegating or deleting managed files.                                |
| "The verification plan changed since it was reviewed"             | Re-open Verify, review the new commands and script bodies, and confirm again.                         |
| "must be trusted first"                                           | Review the repository, then set trust to _Trusted_ on its detail page.                                |
| "An earlier clone attempt still holds this destination"           | Clean up the failed clone from the GitHub import panel, then clone again.                             |
| "Work interrupted by the previous session" in the Activity Centre | Sessions or runs cut off by a restart were closed as failed. Check the checkout, then run them again. |

## Safety notes

- ShiftOps never runs a shell command string, never stores tokens outside the keychain, and never deletes a checkout it did not clone.
- Devin's changes are untrusted until ShiftOps's own verification passes.
- During development, macOS XProtect reported a "Malicious Script Blocked" warning while the Devin CLI was being troubleshot. The blocked item has not been identified. Do not bypass Gatekeeper or XProtect to run the CLI. If the warning appears for you, leave the CLI blocked until it is resolved; ShiftOps works without it up to an approved task.
