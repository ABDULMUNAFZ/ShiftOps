import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithClient } from "../test/render";

const native = vi.hoisted(() => ({
  getSubmissionReadiness: vi.fn(),
  openSubmissionFile: vi.fn(),
  openExternalUrl: vi.fn(),
}));
vi.mock("../lib/native", () => ({ native }));

const { SubmissionReadinessPage } = await import("./SubmissionReadinessPage");

beforeEach(() => {
  for (const fn of Object.values(native)) fn.mockReset();
  native.getSubmissionReadiness.mockResolvedValue({
    status: "needs_participant_input",
    sourceRoot: "/work/shiftops",
    requiredFiles: { "README.md": true, "CHANGELOG.md": true, "AI_USAGE.md": true, "SUBMISSION.md": true },
    remoteUrl: "https://github.com/ABDULMUNAFZ/ShiftOps.git",
    headSha: "abcdef0123456789",
    originMainSha: "abcdef0123456789",
    workingTreeClean: true,
    commitsAhead: 0,
    pushed: true,
    todoCount: 3,
    secretFindings: [],
    trackedFilesScanned: 120,
    validation: null,
    finalSha: null,
    reasons: ["3 participant TODOs remain."],
    warnings: ["Push state is compared with the last fetched origin/main; run `git fetch` for a fresh comparison."],
  });
  native.openSubmissionFile.mockResolvedValue(undefined);
  native.openExternalUrl.mockResolvedValue(undefined);
  Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
});

describe("SubmissionReadinessPage", () => {
  it("shows evidence, copies a checklist and never claims submission", async () => {
    renderWithClient(<SubmissionReadinessPage />);
    expect(await screen.findByText("Needs Participant Input")).toBeTruthy();
    expect(screen.getByText("Unset until Task 6")).toBeTruthy();
    expect(screen.getByText(/last fetched origin\/main/)).toBeTruthy();
    expect(screen.getByText("Disabled")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Copy checklist/ }));
    await waitFor(() =>
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(expect.stringContaining("[ ] Submitted manually")),
    );
    fireEvent.click(screen.getByRole("button", { name: /Open GitHub/ }));
    expect(native.openExternalUrl).toHaveBeenCalledWith("https://github.com/ABDULMUNAFZ/ShiftOps");
  });

  it("shows only secret paths and rule names", async () => {
    native.getSubmissionReadiness.mockResolvedValueOnce({
      ...(await native.getSubmissionReadiness()),
      status: "not_ready",
      secretFindings: [{ path: "config.txt", rule: "bearer_token" }],
      reasons: ["1 potential tracked-secret findings need review."],
    });
    renderWithClient(<SubmissionReadinessPage />);
    expect(await screen.findByText("config.txt")).toBeTruthy();
    expect(screen.getByText("bearer_token")).toBeTruthy();
    expect(document.body.textContent).not.toContain("productionvalue");
  });
});
