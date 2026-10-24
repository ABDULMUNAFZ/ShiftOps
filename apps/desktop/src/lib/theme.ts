import type { Theme } from "@shiftops/core";
import { useEffect } from "react";

const BOOT_HINT_KEY = "shiftops.resolvedTheme";

export function resolveTheme(theme: Theme, prefersDark: boolean): "light" | "dark" {
  if (theme === "system") return prefersDark ? "dark" : "light";
  return theme;
}

export function applyResolvedTheme(resolved: "light" | "dark"): void {
  document.documentElement.classList.toggle("dark", resolved === "dark");
  try {
    localStorage.setItem(BOOT_HINT_KEY, resolved);
  } catch {
    // Storage can be unavailable; the SQLite setting remains authoritative.
  }
}

/**
 * Apply the last resolved theme before React renders, so the window does not
 * flash light while settings load from SQLite. SQLite is the source of truth;
 * this hint only covers the first frames.
 */
export function applyBootThemeHint(): void {
  let hint: string | null;
  try {
    hint = localStorage.getItem(BOOT_HINT_KEY);
  } catch {
    hint = null;
  }
  const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  applyResolvedTheme(hint === "light" || hint === "dark" ? hint : prefersDark ? "dark" : "light");
}

/** Keep the document class in sync with the persisted theme and the OS. */
export function useApplyTheme(theme: Theme | undefined): void {
  useEffect(() => {
    if (!theme) return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => applyResolvedTheme(resolveTheme(theme, media.matches));
    update();
    if (theme !== "system") return;
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [theme]);
}
