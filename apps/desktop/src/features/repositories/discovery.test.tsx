import type { DiscoveryEvent, DiscoveryResult, ImportSummary, Repository } from "@shiftops/core";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithClient } from "../../test/render";

const native = vi.hoisted(() => ({
  pickDirectory: vi.fn(),
  startDiscovery: vi.fn(),
  getDiscovery: vi.fn(),
  cancelDiscovery: vi.fn(),
  importDiscovered: vi.fn(),
  listRepositoryRoots: vi.fn(),
  removeRepositoryRoot: vi.fn(),
  listRepositories: vi.fn(),
}));
const events = vi.hoisted(() => ({ handler: null as null | ((e: DiscoveryEvent) => void) }));
vi.mock("../../lib/native", () => ({
  native,
  onDiscoveryEvent: async (h: (e: DiscoveryEvent) => void) => {
    events.handler = h;
    return () => undefined;
  },
}));

const { RepositoriesPage } = await import("../../pages/RepositoriesPage");

const metadata = {
  currentBranch: "main",
  detached: false,
  headCommit: "0123456789abcdef0123456789abcdef01234567",
  defaultBranch: "main",
  origin: { sanitizedUrl: "https://github.com/acme/api.git", host: "github.com", owner: "acme", repo: "api" },
  lastCommitAt: "2026-10-24T10:00:00+05:30",
  workingTree: { staged: 0, modified: 1, untracked: 2 },
  warnings: [],
};

function result(overrides: Partial<DiscoveryResult> = {}): DiscoveryResult {
  return {
    discoveryId: "d1",
    rootPath: "/work",
    mode: "directory",
    maxDepth: 4,
    followInternalSymlinks: false,
    status: "completed",
    candidates: [
      { path: "/work/api", name: "api", gitKind: "standard", depth: 1, alreadyImported: false, metadata },
      {
        path: "/work/api-feature",
        name: "api-feature",
        gitKind: "worktree",
        depth: 1,
        alreadyImported: true,
        metadata: { ...metadata, workingTree: null, warnings: ["Working-tree status not collected: filter"] },
      },
    ],
    warnings: [{ path: "/work/link", code: "symlink_escape", message: "Symbolic link leads outside the root." }],
    stats: { directoriesVisited: 12, entriesSeen: 40, skippedDirectories: 3, depthLimitedDirectories: 0 },
    truncated: false,
    startedAt: "2026-10-24T10:00:00.000Z",
    completedAt: "2026-10-24T10:00:01.000Z",
    durationMs: 1000,
    error: null,
    ...overrides,
  };
}

const emptyPage = { items: [] as Repository[], total: 0, limit: 200, offset: 0 };

beforeEach(() => {
  for (const fn of Object.values(native)) fn.mockReset();
  native.listRepositoryRoots.mockResolvedValue([]);
  native.listRepositories.mockResolvedValue(emptyPage);
  native.getDiscovery.mockResolvedValue(null);
  native.startDiscovery.mockResolvedValue({ discoveryId: "d1" });
});

describe("Repositories discovery", () => {
  it("discovers a folder with options, streams progress, then imports the selection", async () => {
    native.pickDirectory.mockResolvedValue("/work");
    native.importDiscovered.mockResolvedValue({
      root: {
        id: "r1",
        canonicalPath: "/work",
        displayName: "work",
        rootKind: "directory",
        maxDepth: 2,
        followInternalSymlinks: true,
        createdAt: "t",
        updatedAt: "t",
        lastDiscoveredAt: "t",
        repositoryCount: 1,
      },
      imported: 1,
      updated: 0,
      repositoryIds: ["x"],
    } satisfies ImportSummary);
    renderWithClient(<RepositoriesPage />);
    expect(await screen.findByText("No repositories yet")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /Discover Folder/ }));
    const options = await screen.findByRole("region", { name: "Discovery options" });
    fireEvent.change(within(options).getByLabelText("Maximum depth"), { target: { value: "2" } });
    fireEvent.click(within(options).getByLabelText(/Follow symbolic links/));
    fireEvent.click(within(options).getByRole("button", { name: "Start discovery" }));
    await waitFor(() =>
      expect(native.startDiscovery).toHaveBeenCalledWith({
        path: "/work",
        mode: "directory",
        maxDepth: 2,
        followInternalSymlinks: true,
      }),
    );

    await screen.findByRole("button", { name: /Cancel/ });
    act(() =>
      events.handler?.({
        type: "progress",
        discoveryId: "d1",
        phase: "walking",
        currentPath: "/work/group",
        stats: { directoriesVisited: 7, entriesSeen: 20, skippedDirectories: 1, depthLimitedDirectories: 0 },
        repositoriesFound: 2,
        inspected: 0,
      }),
    );
    expect(screen.getByText(/7 folders, 2 repositories found/)).toBeTruthy();

    act(() => events.handler?.({ type: "completed", result: result({ maxDepth: 2 }) }));
    const table = await screen.findByRole("table", { name: "Discovered repositories" });
    expect(within(table).getByText("already imported")).toBeTruthy();
    expect(within(table).getByText("not collected")).toBeTruthy();
    expect(screen.getByText(/1 warning/)).toBeTruthy();
    // Already-imported candidates start unselected.
    expect((within(table).getByLabelText("Select api") as HTMLInputElement).checked).toBe(true);
    expect((within(table).getByLabelText("Select api-feature") as HTMLInputElement).checked).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Import 1 selected" }));
    await waitFor(() => expect(native.importDiscovered).toHaveBeenCalledWith("d1", ["/work/api"]));
    expect(await screen.findByText(/Imported 1, refreshed 0/)).toBeTruthy();
  });

  it("opens a single repository and handles a result that finished before events arrived", async () => {
    native.pickDirectory.mockResolvedValue("/work/api");
    native.getDiscovery.mockResolvedValue(result({ mode: "repository", maxDepth: 0, rootPath: "/work/api" }));
    renderWithClient(<RepositoriesPage />);
    fireEvent.click(await screen.findByRole("button", { name: /Open Local Repository/ }));
    await waitFor(() =>
      expect(native.startDiscovery).toHaveBeenCalledWith({
        path: "/work/api",
        mode: "repository",
        maxDepth: 0,
        followInternalSymlinks: false,
      }),
    );
    expect(await screen.findByRole("table", { name: "Discovered repositories" })).toBeTruthy();
  });

  it("does nothing when the picker is dismissed", async () => {
    native.pickDirectory.mockResolvedValue(null);
    renderWithClient(<RepositoriesPage />);
    fireEvent.click(await screen.findByRole("button", { name: /Discover Folder/ }));
    await waitFor(() => expect(native.pickDirectory).toHaveBeenCalled());
    expect(screen.queryByRole("region", { name: "Discovery options" })).toBeNull();
    expect(native.startDiscovery).not.toHaveBeenCalled();
  });

  it("cancels a running discovery and reports it", async () => {
    native.pickDirectory.mockResolvedValue("/work/api");
    native.cancelDiscovery.mockResolvedValue({ discoveryId: "d1", outcome: "requested" });
    renderWithClient(<RepositoriesPage />);
    fireEvent.click(await screen.findByRole("button", { name: /Open Local Repository/ }));
    fireEvent.click(await screen.findByRole("button", { name: /Cancel/ }));
    await waitFor(() => expect(native.cancelDiscovery).toHaveBeenCalledWith("d1"));
    act(() => events.handler?.({ type: "completed", result: result({ status: "cancelled", candidates: [] }) }));
    expect(await screen.findByText(/Cancelled. Nothing was imported/)).toBeTruthy();
  });

  it("shows structured errors when discovery cannot start", async () => {
    native.pickDirectory.mockResolvedValue("/work/not-a-repo");
    native.startDiscovery.mockRejectedValue({
      code: "VALIDATION_FAILED",
      message: "This folder is not a Git repository (no `.git`).",
      technicalDetails: null,
      suggestedAction: "Correct the highlighted input and try again.",
      recoverable: true,
    });
    renderWithClient(<RepositoriesPage />);
    fireEvent.click(await screen.findByRole("button", { name: /Open Local Repository/ }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("VALIDATION_FAILED");
    expect(alert.textContent).toContain("not a Git repository");
  });

  it("removes a tracked folder only after confirmation", async () => {
    native.listRepositoryRoots.mockResolvedValue([
      {
        id: "r1",
        canonicalPath: "/work",
        displayName: "work",
        rootKind: "directory",
        maxDepth: 4,
        followInternalSymlinks: false,
        createdAt: "t",
        updatedAt: "t",
        lastDiscoveredAt: null,
        repositoryCount: 3,
      },
    ]);
    native.removeRepositoryRoot.mockResolvedValue({});
    renderWithClient(<RepositoriesPage />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove…" }));
    expect(native.removeRepositoryRoot).not.toHaveBeenCalled();
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Stop tracking" }));
    await waitFor(() => expect(native.removeRepositoryRoot).toHaveBeenCalledWith("r1"));
  });
});
