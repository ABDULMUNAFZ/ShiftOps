/**
 * The desktop release the website links to. Set only after the release
 * exists on GitHub with these exact assets; `site.test.ts` checks the shape.
 * `null` means no installers are published, and the download page says so.
 */
export interface DesktopRelease {
  tag: string;
  publishedAt: string;
  signed: false;
  assets: { platform: "macOS" | "Windows" | "Linux"; label: string; file: string }[];
}

export const SITE_URL = "https://onlyhacks.tech";
export const RELEASES_URL = "https://github.com/ABDULMUNAFZ/ShiftOps/releases";

export const DESKTOP_RELEASE: DesktopRelease | null = null;

export function assetUrl(release: DesktopRelease, file: string): string {
  return `${RELEASES_URL}/download/${release.tag}/${file}`;
}
