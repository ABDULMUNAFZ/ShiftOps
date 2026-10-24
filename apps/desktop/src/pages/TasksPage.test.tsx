import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithClient } from "../test/render";

const native = vi.hoisted(() => ({
  listAgentTasks: vi.fn(),
  devinStatus: vi.fn(),
  listAgentRuns: vi.fn(),
  approveAgentTask: vi.fn(),
  dispatchAgentTask: vi.fn(),
  cancelAgentTask: vi.fn(),
}));
vi.mock("../lib/native", () => ({ native }));

const { AgentRunsPage } = await import("./TasksPage");

const task = {
  id: "task-0001-abcd",
  campaignRepositoryId: "m1",
  provider: "devin",
  executionMode: "local_cli",
  instructionsMarkdown: "# ShiftOps task: Node 22\n\n## Prohibitions\n- Do not push",
  externalSessionId: null,
  externalSessionUrl: null,
  worktreePath: null,
  branchName: null,
  commitSha: null,
  pullRequestUrl: null,
  status: "pending",
  instructionsHash: "abcdef0123456789",
  approvedAt: null,
  startedAt: null,
  completedAt: null,
  createdAt: "2026-10-25T00:00:00.000Z",
  updatedAt: "2026-10-25T00:00:00.000Z",
};

beforeEach(() => {
  for (const fn of Object.values(native)) fn.mockReset();
  native.listAgentTasks.mockResolvedValue({ items: [task], total: 1, limit: 200, offset: 0 });
  native.devinStatus.mockResolvedValue({
    installed: false,
    version: null,
    authenticated: false,
    accountEmail: null,
    detail: "Devin CLI was not found on the login-shell PATH.",
  });
  native.listAgentRuns.mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
  native.approveAgentTask.mockResolvedValue({ ...task, approvedAt: "2026-10-25T00:01:00.000Z" });
});

describe("AgentRunsPage", () => {
  it("shows the not-installed state and still lists drafts", async () => {
    renderWithClient(<AgentRunsPage />);
    expect(await screen.findByText(/dispatch is disabled/)).toBeTruthy();
    expect(screen.getByText("unapproved")).toBeTruthy();
  });

  it("requires approval before dispatch is offered", async () => {
    renderWithClient(<AgentRunsPage />);
    fireEvent.click(await screen.findByText("task-000"));
    expect(screen.queryByRole("button", { name: /Dispatch/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Approve/ }));
    await waitFor(() => expect(native.approveAgentTask).toHaveBeenCalledWith(task.id));
    expect(native.dispatchAgentTask).not.toHaveBeenCalled();
    expect(screen.getByText(/Do not push/)).toBeTruthy();
  });
});
