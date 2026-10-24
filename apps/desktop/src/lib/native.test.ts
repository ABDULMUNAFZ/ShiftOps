import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
let tauri = true;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
  isTauri: () => tauri,
}));

const { native, NativeUnavailableError } = await import("./native");

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

  it("surfaces Rust error strings verbatim", async () => {
    invoke.mockRejectedValue("unknown setting key: nope");
    await expect(native.setSetting("nope", 1)).rejects.toThrow("unknown setting key: nope");
  });

  it("rejects responses that do not match the contract", async () => {
    invoke.mockResolvedValue({ collectedAt: "x", checks: [{ id: "git" }] });
    await expect(native.environment()).rejects.toThrow(/unexpected shape/);
  });
});
