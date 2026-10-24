import type { Repository, RepositoryIntelligence, ScanEvent } from "@shiftops/core";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithClient } from "../test/render";

const native = vi.hoisted(() => ({
  getRepository: vi.fn(),
  getRepositoryIntelligence: vi.fn(),
  listRepositoryBranches: vi.fn(),
  startRepositoryScan: vi.fn(),
  cancelRepositoryScan: vi.fn(),
  listActivity: vi.fn(),
  activityEntityTypes: vi.fn(),
}));
const events = vi.hoisted(() => ({ handler: null as null | ((e: ScanEvent) => void) }));
vi.mock("../lib/native", () => ({
  native,
  onScanEvent: async (h: (e: ScanEvent) => void) => {
    events.handler = h;
    return () => undefined;
  },
}));

const { RepositoryDetailPage } = await import("./RepositoryDetailPage");

const repo: Repository = {
  id: "r1",
  sourceType: "local",
  name: "orders-api",
  localPath: "/work/orders-api",
  remoteUrl: "https://github.com/acme/orders-api.git",
  githubHost: "github.com",
  githubOwner: "acme",
  githubRepo: "orders-api",
  defaultBranch: "main",
  currentBranch: "main",
  currentCommit: "0123456789abcdef0123456789abcdef01234567",
  trustStatus: "untrusted",
  createdAt: "t",
  updatedAt: "t",
  rootId: "root",
  gitKind: "standard",
  isDetached: false,
  isDirty: true,
  stagedCount: 0,
  modifiedCount: 2,
  untrackedCount: 1,
  lastCommitAt: "2026-10-24T10:00:00+05:30",
  gitCollectedAt: "2026-10-24T10:00:00.000Z",
  gitWarnings: [],
  lastScanStatus: "completed",
  lastScannedAt: "2026-10-24T11:00:00.000Z",
  scanSummary: null,
};

const scanned: RepositoryIntelligence = {
  scan: {
    id: "s1",
    repositoryId: "r1",
    scannedCommit: "0123456789abcdef",
    relevantFilesHash: "h",
    status: "completed",
    result: {
      scannerVersion: "1",
      cacheKey: "k",
      primaryLanguage: "JavaScript",
      languages: [{ name: "JavaScript", files: 12 }],
      runtimes: { node: [">=18 <19"] },
      packageManagers: ["npm"],
      frameworks: ["Express", "Jest"],
      hasDocker: true,
      ci: [],
      facts: 3,
      conflicts: 1,
      relevantFiles: 4,
      entriesSeen: 30,
      truncated: false,
      warnings: [],
    },
    errorCode: null,
    errorMessage: null,
    startedAt: "t",
    completedAt: "2026-10-24T11:00:00.000Z",
  },
  latestAttempt: null,
  facts: [
    {
      id: "f1",
      category: "runtime",
      key: "node",
      value: ">=18 <19",
      file: "package.json",
      line: 6,
      pointer: "/engines/node",
      method: "package.json engines",
      confidence: 0.9,
    },
    {
      id: "f2",
      category: "dependency",
      key: "express",
      value: "^4.19.2",
      file: "package.json",
      line: 14,
      pointer: "/dependencies/express",
      method: "package.json dependencies",
      confidence: 1,
    },
    {
      id: "f3",
      category: "dependency",
      key: "jest",
      value: "^29.7.0",
      file: "package.json",
      line: 17,
      pointer: "/devDependencies/jest",
      method: "package.json devDependencies",
      confidence: 1,
    },
  ],
  conflicts: [
    {
      id: "c1",
      subject: "runtime.node",
      message: 'engines.node ">=20" in package.json excludes Node 18 declared by .nvmrc',
      values: [{ value: "18", file: ".nvmrc", line: 1, method: ".nvmrc" }],
    },
  ],
  scanRunning: false,
};

function renderPage() {
  return renderWithClient(
    <MemoryRouter initialEntries={["/repositories/r1"]}>
      <Routes>
        <Route path="/repositories/:id" element={<RepositoryDetailPage />} />
      </Routes>
    </MemoryRouter>,
  );
}

beforeEach(() => {
  for (const fn of Object.values(native)) fn.mockReset();
  native.getRepository.mockResolvedValue(repo);
  native.getRepositoryIntelligence.mockResolvedValue(scanned);
  native.startRepositoryScan.mockResolvedValue({ repositoryId: "r1" });
  native.cancelRepositoryScan.mockResolvedValue({ discoveryId: "r1", outcome: "requested" });
  native.activityEntityTypes.mockResolvedValue([]);
});

describe("RepositoryDetailPage", () => {
  it("offers a first scan when the repository has never been scanned", async () => {
    native.getRepositoryIntelligence.mockResolvedValue({ ...scanned, scan: null, facts: [], conflicts: [] });
    renderPage();
    expect(await screen.findByText("Not scanned yet")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Run first scan/ }));
    await waitFor(() => expect(native.startRepositoryScan).toHaveBeenCalledWith("r1", false));
  });

  it("shows the overview with conflicts and evidence with file, line and pointer", async () => {
    renderPage();
    expect(await screen.findByText("orders-api")).toBeTruthy();
    expect(screen.getByText("1 conflicts")).toBeTruthy();
    expect(screen.getAllByText(/excludes Node 18/).length).toBeGreaterThan(0);
    expect(screen.getByText("node >=18 <19")).toBeTruthy();

    // Keyboard navigation across tabs.
    const overview = screen.getByRole("tab", { name: "Overview" });
    overview.focus();
    fireEvent.keyDown(overview, { key: "ArrowRight" });
    const evidenceTab = screen.getByRole("tab", { name: /Evidence/ });
    expect(evidenceTab.getAttribute("aria-selected")).toBe("true");
    const table = screen.getByRole("table", { name: "Evidence" });
    expect(within(table).getByText("package.json:6")).toBeTruthy();
    expect(within(table).getByText("/engines/node")).toBeTruthy();
    expect(within(table).getByText("90%")).toBeTruthy();
  });

  it("filters dependencies and reports GitHub pull requests honestly", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("tab", { name: /Dependencies/ }));
    fireEvent.change(screen.getByLabelText("Filter dependencies"), { target: { value: "jes" } });
    const table = screen.getByRole("table", { name: "Dependencies" });
    expect(within(table).queryByText("express")).toBeNull();
    expect(within(table).getByText("jest")).toBeTruthy();

    fireEvent.click(screen.getByRole("tab", { name: "Pull Requests" }));
    expect(screen.getByText("GitHub is not connected")).toBeTruthy();
  });

  it("retries branch loading after a structured error", async () => {
    native.listRepositoryBranches.mockRejectedValueOnce({
      code: "COMMAND_FAILED",
      message: "Branches could not be read.",
      technicalDetails: "git for-each-ref failed",
      suggestedAction: "Inspect the command output for the cause.",
      recoverable: true,
    });
    native.listRepositoryBranches.mockResolvedValueOnce([
      { name: "main", commit: "0123456789abcdef", committedAt: null, upstream: "origin/main" },
    ]);
    renderPage();
    fireEvent.click(await screen.findByRole("tab", { name: "Branches" }));
    expect((await screen.findByRole("alert")).textContent).toContain("COMMAND_FAILED");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("table", { name: "Branches" })).toBeTruthy();
  });

  it("streams scan progress, cancels, and explains a cached result", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: /^Scan$/ }));
    await waitFor(() => expect(native.startRepositoryScan).toHaveBeenCalledWith("r1", false));
    act(() => events.handler?.({ type: "progress", repositoryId: "r1", entriesSeen: 420, currentPath: "src" }));
    expect(screen.getByText(/420 entries/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Cancel scan/ }));
    await waitFor(() => expect(native.cancelRepositoryScan).toHaveBeenCalledWith("r1"));
    act(() =>
      events.handler?.({
        type: "completed",
        outcome: { repositoryId: "r1", status: "cached", scan: scanned.scan, error: null },
      }),
    );
    expect(await screen.findByText(/reused it/)).toBeTruthy();
  });
});
