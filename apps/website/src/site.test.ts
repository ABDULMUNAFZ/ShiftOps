import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const appDir = join(import.meta.dirname, "app");
const chrome = readFileSync(join(import.meta.dirname, "components", "site-chrome.tsx"), "utf8");

describe("website routes", () => {
  it("has a page for every link in the site header", () => {
    const hrefs = [...chrome.matchAll(/href: "\/([a-z-]+)\/"/g)].map((m) => m[1]);
    expect(hrefs).toEqual(["docs", "security", "privacy", "download"]);
    for (const href of hrefs) {
      expect(existsSync(join(appDir, String(href), "page.tsx")), href).toBe(true);
    }
  });
});
