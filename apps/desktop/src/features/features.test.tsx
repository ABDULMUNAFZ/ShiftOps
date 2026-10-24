import type { ActivityEvent, CredentialMetadata, Page } from "@shiftops/core";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createProcessStore, MAX_LINES_PER_RUN } from "../lib/processStore";
import { renderWithClient } from "../test/render";

const native = vi.hoisted(() => ({
  databaseHealth: vi.fn(),
  keychainStatus: vi.fn(),
  listCredentials: vi.fn(),
  storeCredential: vi.fn(),
  deleteCredential: vi.fn(),
  listActivity: vi.fn(),
  activityEntityTypes: vi.fn(),
  startDiagnostic: vi.fn(),
  cancelProcess: vi.fn(),
  openExternalUrl: vi.fn(),
}));
vi.mock("../lib/native", () => ({ native }));

const { DatabaseHealthCard } = await import("./health/DatabaseHealthCard");
const { CredentialSecurity } = await import("./credentials/CredentialSecurity");
const { ActivityList } = await import("./activity/ActivityCentre");
const { NativeDiagnostics } = await import("./diagnostics/NativeDiagnostics");

const SECRET = "ghp_ThisMustNeverBeRendered0123456789";

function credential(overrides: Partial<CredentialMetadata>): CredentialMetadata {
  return {
    provider: "github",
    kind: "github_personal_access_token",
    identifier: "github:github_personal_access_token",
    exists: false,
    manageable: true,
    createdAt: null,
    updatedAt: null,
    lastValidatedAt: null,
    lastValidationStatus: "not_validated",
    ...overrides,
  };
}

beforeEach(() => {
  for (const fn of Object.values(native)) fn.mockReset();
  native.keychainStatus.mockResolvedValue({ available: true, backend: "macOS Keychain", service: "test", error: null });
  native.activityEntityTypes.mockResolvedValue(["process"]);
});

describe("DatabaseHealthCard", () => {
  it("renders real health values and flags problems", async () => {
    native.databaseHealth.mockResolvedValue({
      path: "/data/shiftops.db",
      sizeBytes: 8192,
      schemaVersion: 3,
      latestSchemaVersion: 4,
      upToDate: false,
      appliedMigrations: [],
      foreignKeysEnabled: true,
      journalMode: "wal",
      quickCheck: "ok",
      activityEventCount: 12,
    });
    renderWithClient(<DatabaseHealthCard />);
    expect(await screen.findByText("3 of 4 (upgrade pending)")).toBeTruthy();
    expect(screen.getByText("Needs attention")).toBeTruthy();
    expect(screen.getByText("12")).toBeTruthy();
    expect(screen.getByText("enforced")).toBeTruthy();
  });
});

describe("CredentialSecurity", () => {
  it("renders metadata only and never shows a saved value", async () => {
    native.listCredentials.mockResolvedValue([
      credential({ exists: true, createdAt: "2026-10-24T10:00:00.000Z", updatedAt: "2026-10-24T11:00:00.000Z" }),
      credential({ provider: "devin", kind: "devin_api_token", identifier: "devin:devin_api_token" }),
    ]);
    renderWithClient(<CredentialSecurity />);
    await screen.findByText("GitHub personal access token");
    const row = document.querySelector('[data-credential="github:github_personal_access_token"]') as HTMLElement;
    expect(within(row).getByText("Stored")).toBeTruthy();
    expect(within(row).getByText("Not validated yet")).toBeTruthy();
    expect(within(row).queryByRole("textbox")).toBeNull();
    expect(screen.getByText("macOS Keychain available")).toBeTruthy();
    const devinRow = document.querySelector('[data-credential="devin:devin_api_token"]') as HTMLElement;
    expect(within(devinRow).getByText("Not stored")).toBeTruthy();
  });

  it("stores through the bridge and clears the field afterwards", async () => {
    native.listCredentials.mockResolvedValue([credential({})]);
    native.storeCredential.mockResolvedValue(credential({ exists: true }));
    renderWithClient(<CredentialSecurity />);
    const input = (await screen.findByLabelText("GitHub personal access token")) as HTMLInputElement;
    expect(input.type).toBe("password");
    fireEvent.change(input, { target: { value: SECRET } });
    fireEvent.click(screen.getByRole("button", { name: "Save to keychain" }));
    await waitFor(() =>
      expect(native.storeCredential).toHaveBeenCalledWith("github", "github_personal_access_token", SECRET),
    );
    await waitFor(() => expect(input.value).toBe(""));
    expect(document.body.textContent).not.toContain(SECRET);
  });

  it("rejects whitespace before calling the bridge", async () => {
    native.listCredentials.mockResolvedValue([credential({})]);
    renderWithClient(<CredentialSecurity />);
    const input = await screen.findByLabelText("GitHub personal access token");
    fireEvent.change(input, { target: { value: "two words" } });
    fireEvent.click(screen.getByRole("button", { name: "Save to keychain" }));
    expect(await screen.findByText(/without spaces/)).toBeTruthy();
    expect(native.storeCredential).not.toHaveBeenCalled();
  });

  it("requires confirmation before deleting", async () => {
    native.listCredentials.mockResolvedValue([credential({ exists: true })]);
    native.deleteCredential.mockResolvedValue(credential({ exists: false }));
    renderWithClient(<CredentialSecurity />);
    fireEvent.click(await screen.findByRole("button", { name: "Delete…" }));
    expect(native.deleteCredential).not.toHaveBeenCalled();
    const dialog = screen.getByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete credential" }));
    await waitFor(() => expect(native.deleteCredential).toHaveBeenCalledWith("github", "github_personal_access_token"));
  });
});

describe("ActivityList", () => {
  function page(offset: number, total: number, size = 25): Page<ActivityEvent> {
    const count = Math.max(0, Math.min(size, total - offset));
    return {
      total,
      limit: size,
      offset,
      items: Array.from({ length: count }, (_, i) => ({
        id: `e${offset + i}`,
        entityType: "process",
        entityId: `run-${offset + i}`,
        eventType: "process.completed",
        severity: "info",
        summary: `event ${offset + i}`,
        metadata: { exitCode: 0 },
        createdAt: "2026-10-24T10:00:00.000Z",
      })),
    };
  }

  it("paginates with the native offset and shows range", async () => {
    native.listActivity.mockImplementation(async (q: { offset: number }) => page(q.offset, 30));
    renderWithClient(<ActivityList />);
    expect(await screen.findByText("event 0")).toBeTruthy();
    expect(screen.getByText("1–25 of 30")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("event 25")).toBeTruthy();
    expect(screen.getByText("26–30 of 30")).toBeTruthy();
    expect(native.listActivity).toHaveBeenLastCalledWith({ limit: 25, offset: 25 });
    expect((screen.getByRole("button", { name: "Next" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("sends filters and resets to the first page", async () => {
    native.listActivity.mockImplementation(async (q: { offset: number }) => page(q.offset, 30));
    renderWithClient(<ActivityList />);
    await screen.findByText("event 0");
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByText("event 25");
    fireEvent.change(screen.getByLabelText("Severity"), { target: { value: "error" } });
    await waitFor(() =>
      expect(native.listActivity).toHaveBeenLastCalledWith({ limit: 25, offset: 0, statuses: ["error"] }),
    );
  });

  it("shows an empty state when nothing is recorded", async () => {
    native.listActivity.mockResolvedValue(page(0, 0));
    renderWithClient(<ActivityList />);
    expect(await screen.findByText("No activity yet")).toBeTruthy();
  });
});

describe("NativeDiagnostics", () => {
  it("streams output, cancels by run id and shows the cancelled result", async () => {
    const store = createProcessStore();
    native.cancelProcess.mockResolvedValue({ runId: "run-1", outcome: "requested" });
    renderWithClient(<NativeDiagnostics store={store} />);
    expect(screen.getByText("No diagnostics run in this session")).toBeTruthy();
    expect(screen.queryByRole("textbox")).toBeNull();

    act(() => {
      store.apply({
        type: "started",
        runId: "run-1",
        label: "Diagnostic ticker (60 s)",
        category: "environment_check",
        program: "/app/shiftops-desktop",
        args: ["--shiftops-diagnostic-ticker"],
        cwd: "/tmp",
        pid: 4242,
        startedAt: "2026-10-24T10:00:00.000Z",
      });
      store.apply({
        type: "output",
        runId: "run-1",
        seq: 1,
        lines: [
          { stream: "stdout", text: "[001/120] stdout tick" },
          { stream: "stderr", text: "[005/120] stderr sample line" },
        ],
      });
    });
    expect(screen.getByText("running")).toBeTruthy();
    expect(screen.getByLabelText("Output of Diagnostic ticker (60 s)").textContent).toContain("stderr sample line");

    fireEvent.click(screen.getByRole("button", { name: "Cancel Diagnostic ticker (60 s)" }));
    await waitFor(() => expect(native.cancelProcess).toHaveBeenCalledWith("run-1"));

    act(() => {
      store.apply({
        type: "completed",
        result: {
          runId: "run-1",
          label: "Diagnostic ticker (60 s)",
          category: "environment_check",
          status: "cancelled",
          exitCode: null,
          durationMs: 1500,
          startedAt: "2026-10-24T10:00:00.000Z",
          completedAt: "2026-10-24T10:00:01.500Z",
          stdout: "[001/120] stdout tick\n",
          stderr: "",
          outputBytes: 50,
          outputLimitBytes: 1048576,
          outputTruncated: false,
          logPath: "/logs/runs/run-1.log",
          error: {
            code: "COMMAND_CANCELLED",
            message: "Diagnostic ticker (60 s) was cancelled.",
            technicalDetails: null,
            suggestedAction: null,
            recoverable: true,
          },
        },
      });
    });
    expect(screen.getByText("cancelled")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Cancel Diagnostic/ })).toBeNull();
    expect(screen.getByText(/Structured result: exit none · 1500 ms/)).toBeTruthy();
  });

  it("starts only the fixed diagnostic kinds", async () => {
    native.startDiagnostic.mockResolvedValue({ runId: "r" });
    renderWithClient(<NativeDiagnostics store={createProcessStore()} />);
    fireEvent.click(screen.getByRole("button", { name: "git --version" }));
    await waitFor(() => expect(native.startDiagnostic).toHaveBeenCalledWith("git_version"));
  });
});

describe("processStore", () => {
  it("caps retained lines per run and counts what was dropped", () => {
    const store = createProcessStore();
    store.apply({
      type: "started",
      runId: "r",
      label: "x",
      category: "environment_check",
      program: "p",
      args: [],
      cwd: "/",
      pid: null,
      startedAt: "t",
    });
    const lines = Array.from({ length: MAX_LINES_PER_RUN + 10 }, (_, i) => ({
      stream: "stdout" as const,
      text: `${i}`,
    }));
    store.apply({ type: "output", runId: "r", seq: 1, lines });
    const run = store.get("r");
    expect(run?.lines).toHaveLength(MAX_LINES_PER_RUN);
    expect(run?.droppedLines).toBe(10);
    expect(run?.lines[0]?.text).toBe("10");
  });

  it("ignores events for runs it never saw start", () => {
    const store = createProcessStore();
    store.apply({ type: "output", runId: "ghost", seq: 1, lines: [] });
    expect(store.getSnapshot()).toEqual([]);
  });
});
