import { ShiftOpsError } from "@shiftops/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
let tauri = true;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
  isTauri: () => tauri,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const { native, NativeUnavailableError } = await import("./native");

const health = {
  path: "/data/shiftops.db",
  sizeBytes: 4096,
  schemaVersion: 4,
  latestSchemaVersion: 4,
  upToDate: true,
  appliedMigrations: [{ version: 1, name: "settings", appliedAt: "2026-10-24T10:00:00.000Z" }],
  foreignKeysEnabled: true,
  journalMode: "wal",
  quickCheck: "ok",
  activityEventCount: 3,
};

describe("native bridge", () => {
  beforeEach(() => {
    invoke.mockReset();
    tauri = true;
  });

  it("refuses to fake results outside the Tauri runtime", async () => {
    tauri = false;
    await expect(native.environment()).rejects.toBeInstanceOf(NativeUnavailableError);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("JSON-encodes setting values before sending them to Rust", async () => {
    invoke.mockResolvedValue(null);
    await native.setSetting("theme", "dark");
    expect(invoke).toHaveBeenCalledWith("set_setting", { key: "theme", value: '"dark"' });
  });

  it("converts structured Rust errors into ShiftOpsError", async () => {
    invoke.mockRejectedValue({
      code: "VALIDATION_FAILED",
      message: "Unknown setting key: nope",
      technicalDetails: null,
      suggestedAction: "Correct the highlighted input and try again.",
      recoverable: true,
      context: { key: "nope" },
    });
    const error = await native.setSetting("nope", 1).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ShiftOpsError);
    expect(error).toMatchObject({ code: "VALIDATION_FAILED", message: "Unknown setting key: nope", recoverable: true });
  });

  it("wraps Tauri's plain-string rejections as INTERNAL_ERROR with details", async () => {
    invoke.mockRejectedValue("invalid args `kind` for command `start_diagnostic`");
    await expect(native.startDiagnostic("git_version")).rejects.toMatchObject({
      code: "INTERNAL_ERROR",
      technicalDetails: "invalid args `kind` for command `start_diagnostic`",
    });
  });

  it("validates responses against the contract", async () => {
    invoke.mockResolvedValue(health);
    await expect(native.databaseHealth()).resolves.toEqual(health);

    invoke.mockResolvedValue({ ...health, quickCheck: undefined });
    await expect(native.databaseHealth()).rejects.toMatchObject({ code: "INTERNAL_ERROR" });

    invoke.mockResolvedValue({ collectedAt: "x", checks: [{ id: "git" }] });
    await expect(native.environment()).rejects.toThrow(/expected contract/);
  });

  it("rejects credential metadata that includes a secret value", async () => {
    invoke.mockResolvedValue({
      provider: "github",
      kind: "github_personal_access_token",
      identifier: "github:github_personal_access_token",
      exists: true,
      manageable: true,
      createdAt: null,
      updatedAt: null,
      lastValidatedAt: null,
      lastValidationStatus: "not_validated",
      secret: "ghp_leaked",
    });
    await expect(native.storeCredential("github", "github_personal_access_token", "ghp_x")).rejects.toMatchObject({
      code: "INTERNAL_ERROR",
    });
  });

  it("always sends confirm: true when deleting a credential", async () => {
    invoke.mockResolvedValue({
      provider: "devin",
      kind: "devin_api_token",
      identifier: "devin:devin_api_token",
      exists: false,
      manageable: true,
      createdAt: null,
      updatedAt: null,
      lastValidatedAt: null,
      lastValidationStatus: "not_validated",
    });
    await native.deleteCredential("devin", "devin_api_token");
    expect(invoke).toHaveBeenCalledWith("delete_credential", {
      provider: "devin",
      kind: "devin_api_token",
      confirm: true,
    });
  });
});
