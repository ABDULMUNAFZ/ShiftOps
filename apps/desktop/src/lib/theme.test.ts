import { describe, expect, it } from "vitest";
import { applyResolvedTheme, resolveTheme } from "./theme";

describe("theme", () => {
  it("resolves system against the OS preference", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
  });

  it("toggles the dark class on the root element", () => {
    applyResolvedTheme("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    applyResolvedTheme("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });
});
