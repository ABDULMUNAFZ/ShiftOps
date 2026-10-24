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

  it("landing page links resolve to real routes and in-page sections", () => {
    const landing = readFileSync(join(appDir, "page.tsx"), "utf8");
    const routes = [...landing.matchAll(/href="\/([a-z-]+)\/"/g)].map((m) => m[1]);
    expect(routes.length).toBeGreaterThan(0);
    for (const route of routes) expect(existsSync(join(appDir, String(route), "page.tsx")), route).toBe(true);
    for (const anchor of landing.matchAll(/href="#([a-z-]+)"/g)) {
      expect(landing).toContain(`id="${anchor[1]}"`);
    }
  });

  it("is honest that no signed installers are published", () => {
    const download = readFileSync(join(appDir, "download", "page.tsx"), "utf8");
    expect(download).toMatch(/No signed release builds have been published yet/);
  });
});
