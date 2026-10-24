import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithClient } from "../../test/render";

const native = vi.hoisted(() => ({
  previewVerification: vi.fn(),
  startVerification: vi.fn(),
  cancelVerification: vi.fn(),
  listVerificationChecks: vi.fn(),
  createRepairTask: vi.fn(),
}));
vi.mock("../../lib/native", () => ({ native, onVerificationEvent: vi.fn(async () => () => undefined) }));

const { VerificationPanel } = await import("./VerificationPanel");

const plan = {
  supported: true,
  packageManager: "pnpm",
  lockfile: "pnpm-lock.yaml",
  steps: [
    {
      name: "install",
      category: "install",
      program: "pnpm",
      args: ["install", "--frozen-lockfile", "--ignore-scripts"],
      script: null,
    },
    { name: "test", category: "test", program: "pnpm", args: ["run", "test"], script: "vitest run" },
  ],
  hash: "a1b2c3d4e5f6a7b8",
  warnings: [],
};
const failedRun = {
  id: "vr1",
  campaignRepositoryId: "m1",
  agentTaskId: null,
  profile: "node:pnpm",
  verifiedCommitSha: "0123456789abcdef",
  status: "failed",
  startedAt: "2026-10-25T00:00:00.000Z",
  completedAt: "2026-10-25T00:01:00.000Z",
  summary: { actionableFailures: ["test"] },
  planHash: "a1b2c3d4e5f6a7b8",
  trigger: "manual",
};
const preview = (over: object = {}) => ({
  memberId: "m1",
  repositoryId: "r1",
  repositoryName: "billing",
  checkout: "/w/billing",
  plan,
  problems: [],
  running: false,
  latestRun: null,
  repairAttempts: 0,
  maxRepairAttempts: 2,
  ...over,
});

beforeEach(() => {
  for (const fn of Object.values(native)) fn.mockReset();
  native.listVerificationChecks.mockResolvedValue([
    {
      id: "c1",
      verificationRunId: "vr1",
      name: "test",
      category: "test",
      command: ["pnpm", "run", "test"],
      status: "failed",
      exitCode: 1,
      durationMs: 1200,
      redactedOutput: "src/a.ts(1,5): error TS2322",
      evidence: { actionable: true, reason: "1 file(s) named in the failure output.", affectedFiles: ["src/a.ts"] },
      suggestedAction: "1 file(s) named in the failure output.",
      startedAt: "2026-10-25T00:00:00.000Z",
      completedAt: "2026-10-25T00:01:00.000Z",
    },
  ]);
});

describe("VerificationPanel", () => {
  it("shows blockers and offers no run for an untrusted repository", async () => {
    native.previewVerification.mockResolvedValue(
      preview({
        plan: null,
        problems: ["Verification runs the repository's own scripts, so the repository must be trusted first."],
      }),
    );
    renderWithClient(<VerificationPanel memberId="m1" />);
    expect(await screen.findByText(/must be trusted first/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Run verification/ })).toBeNull();
  });

  it("shows the exact commands and script bodies and runs only after confirmation", async () => {
    native.previewVerification.mockResolvedValue(preview());
    native.startVerification.mockResolvedValue({ ...failedRun, status: "running" });
    renderWithClient(<VerificationPanel memberId="m1" />);
    expect(await screen.findByText("pnpm run test")).toBeTruthy();
    expect(screen.getByText("vitest run")).toBeTruthy();
    expect(screen.getByText("pnpm install --frozen-lockfile --ignore-scripts")).toBeTruthy();
    const run = screen.getByRole("button", { name: /Run verification/ });
    expect((run as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByLabelText(/I reviewed these commands/));
    fireEvent.click(run);
    await waitFor(() => expect(native.startVerification).toHaveBeenCalledWith("m1", "a1b2c3d4e5f6a7b8"));
  });

  it("reports real failures and drafts a bounded repair task", async () => {
    native.previewVerification.mockResolvedValue(preview({ latestRun: failedRun, repairAttempts: 1 }));
    native.createRepairTask.mockResolvedValue({});
    renderWithClient(<VerificationPanel memberId="m1" />);
    expect(await screen.findByText("exit 1")).toBeTruthy();
    expect(screen.getByText("repairable")).toBeTruthy();
    expect(screen.getByText(/Files: src\/a.ts/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Draft repair task \(2 of 2\)/ }));
    await waitFor(() => expect(native.createRepairTask).toHaveBeenCalledWith("m1", "vr1"));
    expect(await screen.findByText(/approve and dispatch it on Agent Runs/)).toBeTruthy();
  });

  it("stops offering repairs at the limit", async () => {
    native.previewVerification.mockResolvedValue(preview({ latestRun: failedRun, repairAttempts: 2 }));
    renderWithClient(<VerificationPanel memberId="m1" />);
    expect(await screen.findByText(/Repair limit reached/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Draft repair task/ })).toBeNull();
  });

  it("shows a retryable error when the plan cannot be read", async () => {
    native.previewVerification.mockRejectedValue({
      code: "INTERNAL_ERROR",
      message: "boom",
      technicalDetails: null,
      suggestedAction: null,
      recoverable: true,
    });
    renderWithClient(<VerificationPanel memberId="m1" />);
    expect(await screen.findByText("Verification plan could not be read")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  });
});
