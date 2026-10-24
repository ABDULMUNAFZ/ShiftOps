import { describe, expect, it } from "vitest";
import { parseSetting, settingDefaults } from "./settings";
import { summarizeReport } from "./environment";

describe("parseSetting", () => {
  it("returns the stored value when valid", () => {
    expect(parseSetting("theme", '"dark"')).toBe("dark");
    expect(parseSetting("sidebarCollapsed", "true")).toBe(true);
  });

  it("falls back to defaults for missing, malformed or invalid values", () => {
    expect(parseSetting("theme", null)).toBe(settingDefaults.theme);
    expect(parseSetting("theme", "{not json")).toBe(settingDefaults.theme);
    expect(parseSetting("theme", '"purple"')).toBe(settingDefaults.theme);
  });
});

describe("summarizeReport", () => {
  it("counts statuses and flags missing required checks", () => {
    const base = { version: null, detail: "", path: null, command: null, durationMs: 0 };
    const summary = summarizeReport({
      collectedAt: "2026-01-01T00:00:00Z",
      checks: [
        { ...base, id: "git", label: "Git", status: "unavailable" },
        { ...base, id: "node", label: "Node", status: "available" },
        { ...base, id: "keychain", label: "Keychain", status: "error" },
      ],
    });
    expect(summary).toEqual({ available: 1, unavailable: 1, errors: 1, missingRequired: ["git", "keychain"] });
  });
});
