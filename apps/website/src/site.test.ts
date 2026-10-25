import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { assetUrl, DESKTOP_RELEASE, SITE_URL } from "./release";

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

  it("download links exist only for a recorded release, and say they are unsigned", () => {
    const download = readFileSync(join(appDir, "download", "page.tsx"), "utf8");
    expect(download).toMatch(/No signed release builds have been published yet/);
    expect(download).toMatch(/unsigned/);
    if (DESKTOP_RELEASE) {
      expect(DESKTOP_RELEASE.tag).toMatch(/^v\d+\.\d+\.\d+/);
      expect(DESKTOP_RELEASE.signed).toBe(false);
      expect(DESKTOP_RELEASE.assets.length).toBeGreaterThan(0);
      for (const a of DESKTOP_RELEASE.assets) {
        expect(assetUrl(DESKTOP_RELEASE, a.file)).toBe(
          `https://github.com/ABDULMUNAFZ/ShiftOps/releases/download/${DESKTOP_RELEASE.tag}/${a.file}`,
        );
      }
    }
  });

  it("serves the production domain", () => {
    expect(readFileSync(join(import.meta.dirname, "..", "public", "CNAME"), "utf8").trim()).toBe("onlyhacks.tech");
    expect(existsSync(join(import.meta.dirname, "..", "public", ".nojekyll"))).toBe(true);
    expect(SITE_URL).toBe("https://onlyhacks.tech");
  });
});
