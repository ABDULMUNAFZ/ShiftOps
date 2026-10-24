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

export const DESKTOP_RELEASE: DesktopRelease | null = {
  tag: "v0.1.0",
  publishedAt: "2026-10-24T23:33:53Z",
  signed: false,
  assets: [
    { platform: "macOS", label: "macOS, Apple Silicon (.dmg)", file: "ShiftOps_0.1.0_aarch64.dmg" },
    { platform: "macOS", label: "macOS, Intel (.dmg)", file: "ShiftOps_0.1.0_x64.dmg" },
    { platform: "Windows", label: "Windows x64 installer (.exe)", file: "ShiftOps_0.1.0_x64-setup.exe" },
    { platform: "Windows", label: "Windows x64 (.msi)", file: "ShiftOps_0.1.0_x64_en-US.msi" },
    { platform: "Linux", label: "Linux x64 (.AppImage)", file: "ShiftOps_0.1.0_amd64.AppImage" },
    { platform: "Linux", label: "Debian / Ubuntu x64 (.deb)", file: "ShiftOps_0.1.0_amd64.deb" },
  ],
};

export function assetUrl(release: DesktopRelease, file: string): string {
  return `${RELEASES_URL}/download/${release.tag}/${file}`;
}
