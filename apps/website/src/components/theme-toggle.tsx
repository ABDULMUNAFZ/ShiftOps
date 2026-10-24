"use client";

import { Button } from "@shiftops/ui";
import { Moon, Sun } from "lucide-react";

const KEY = "shiftops.site.theme";

export function ThemeToggle() {
  const toggle = () => {
    const dark = !document.documentElement.classList.contains("dark");
    document.documentElement.classList.toggle("dark", dark);
    try {
      localStorage.setItem(KEY, dark ? "dark" : "light");
    } catch {
      // Storage unavailable: the choice lasts for this page view only.
    }
  };

  return (
    <Button variant="ghost" size="icon" onClick={toggle} aria-label="Toggle colour theme">
      <Sun className="hidden dark:block" />
      <Moon className="dark:hidden" />
    </Button>
  );
}
