# Project submission

**Built for Devin Hacks Kochi 2026.** ShiftOps is an independent project. It is not an official Cognition product and does not claim a partnership or endorsement.

This file is completed progressively. Items that are not finished yet are marked **Pending** with the task that will finish them.

## Project

- **Project name:** ShiftOps
- **Tagline:** Define the change once. Ship it everywhere.
- **Short description:** ShiftOps plans, coordinates, and verifies code migrations across every repository from one place. Engineering teams repeat the same change (runtime upgrades, dependency replacements, vulnerability fixes, platform standards) across many repositories. ShiftOps discovers the affected repositories, plans the order, prepares repository-specific work for Devin, and independently verifies each result.
- **Code repository URL:** https://github.com/ABDULMUNAFZ/ShiftOps
- **Final submitted implementation commit SHA:** Pending (Task 6)
- **Working demo URL or local demo steps:** Local desktop app; steps below. Packaged builds: Pending (Task 6)
- **Demo access requirements:** macOS with the toolchain listed below. GitHub and Devin credentials are entered by the person running the demo and are stored in their OS keychain; none are included in this repository.

## Participants

| Participant name | GitHub handle | Contribution                                                                 |
| ---------------- | ------------- | ---------------------------------------------------------------------------- |
| Abdul Munaf Z    | ABDULMUNAFZ   | Product owner; direction, review, decisions on what is committed and pushed. |

AI assistance is recorded in [AI_USAGE.md](AI_USAGE.md).

## Run and demonstrate

- **Prerequisites and versions:** Node.js ≥ 22.22 (26.7.0 used), pnpm 10 (10.33.0 used), Rust stable (1.99.1 used), Xcode Command Line Tools on macOS.
- **Install command:** `pnpm install`
- **Required environment variable names:** None. Credentials are entered in the app and stored in the OS keychain.
- **Start command:** `pnpm dev:desktop` (website: `pnpm dev:website`)
- **Demo walkthrough (current state):**
  1. Run the first-run environment checks: they report real tool versions.
  2. Settings → Native diagnostics: run `git --version`, or start the long-running diagnostic and cancel it.
  3. Integrations → Credential security: store and delete a token in the OS keychain.
  4. Open the Activity Centre with ⌘J to see the persisted audit trail.
  5. Repositories → Open folder or Discover: add a local checkout, inspect its read-only Git state and run the intelligence scan.
  6. Integrations → GitHub: connect with the device flow or a fine-grained PAT, then Repositories → Browse GitHub to import metadata-only, clone into an approved managed workspace, or link an existing checkout.
  7. Open a repository's detail page to review evidence, runtimes, dependencies, scripts, Docker, CI, Git, branches and pull requests; change its trust or forget only the record.
  8. Submission Readiness (⌘8): inspect the live evidence checklist, copy it, open the required documents, or open the GitHub repository.
  9. Campaigns → New campaign: pick scanned repositories, set "Upgrade Node.js 18 to Node.js 22" (node, 18 → 22), create.
  10. Campaign detail: Refresh edges, Compute plan (waves, risk and cited reasons), Apply plan & mark ready, then Draft task on a member.
  11. Agent Runs: open the task, read the generated instructions and hash, Approve. Dispatch to Devin runs only for a trusted, review-cleared, affected repository with a signed-in Devin CLI; otherwise it reports why.
  12. Campaign detail → Verify on a trusted member: review the plan (exact commands and script bodies), confirm, run; read each check's exit code and output. A failed run with repairable failures offers Draft repair task, which then goes through Approve and Dispatch on Agent Runs. A successful repair session re-runs the same approved plan automatically if it is unchanged. Live Devin dispatch was not exercised by the agent.
- **Checks performed and results:** See the latest entry in [CHANGELOG.md](CHANGELOG.md) and section 6 of [PROJECT_STATE.md](PROJECT_STATE.md).
- **Known limitations or incomplete features:** See [PROJECT_STATE.md](PROJECT_STATE.md) §9. Devin delegation uses the local Devin CLI; live dispatch was not exercised by the agent.
- **Libraries, APIs, and public datasets used:** Tauri 2, React 19, Vite 8, TanStack Query, React Hook Form, Zod, cmdk, lucide-react, Tailwind CSS 4, Next.js 16, rusqlite (bundled SQLite), keyring, tokio, regex, url, uuid, tauri-plugin-opener. Exact versions are in `package.json` files and `Cargo.lock`.
- **AI usage record:** [AI_USAGE.md](AI_USAGE.md)
- **Development change log:** [CHANGELOG.md](CHANGELOG.md)

## Submission checklist

- [ ] Code repository URL, short description and working demo are provided. (The demo is complete in Task 6.)
- [x] README setup instructions match the code at the current commit.
- [x] Actual checks and known limitations are documented.
- [x] Meaningful changes are documented and committed throughout development.
- [ ] The final implementation is committed and pushed, and its SHA is recorded above. (Task 6)
- [x] No credentials or private data are included in the repository or demo instructions.
- [ ] The organisers' announced submission method and timing have been followed. (Confirm at submission.)
