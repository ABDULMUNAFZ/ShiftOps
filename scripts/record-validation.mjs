import { mkdirSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const pnpm = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
const check = spawnSync(pnpm, ["check"], { cwd: root, stdio: "inherit", env: process.env });
if (check.status !== 0) process.exit(check.status ?? 1);

const git = spawnSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" });
if (git.status !== 0) {
  process.stderr.write(git.stderr || "Could not resolve HEAD.\n");
  process.exit(git.status ?? 1);
}

const directory = join(root, ".shiftops");
mkdirSync(directory, { recursive: true });
writeFileSync(
  join(directory, "validation.json"),
  `${JSON.stringify(
    {
      commitSha: git.stdout.trim(),
      recordedAt: new Date().toISOString(),
      passed: true,
      summary: "pnpm check passed: format, types, lint, JavaScript tests, rustfmt, Rust tests and Clippy.",
    },
    null,
    2,
  )}\n`,
  { mode: 0o600 },
);
process.stdout.write("Recorded .shiftops/validation.json for the current commit.\n");
