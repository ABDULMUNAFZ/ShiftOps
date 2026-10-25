import type { Metadata } from "next";
import Link from "next/link";
import { DocPage } from "../../components/doc-page";
import { assetUrl, DESKTOP_RELEASE, RELEASES_URL } from "../../release";

export const metadata: Metadata = { title: "Download for Desktop" };

const PLATFORM_NOTES = {
  macOS:
    "Not signed or notarized. macOS Gatekeeper blocks unsigned downloads, so on macOS building from source (below) is the dependable route until signed builds exist.",
  Windows:
    "Not code-signed. Windows SmartScreen shows an “unknown publisher” warning; choose More info, then Run anyway, only if you trust this download.",
  Linux: "AppImage runs on most distributions (make it executable first); the .deb installs on Debian and Ubuntu.",
} as const;

export default function Download() {
  const release = DESKTOP_RELEASE;
  return (
    <DocPage
      title="Download for Desktop"
      lead={
        release
          ? `Pre-release ${release.tag} for macOS, Windows and Linux. These installers are unsigned.`
          : "No signed release builds have been published yet."
      }
    >
      {release ? (
        <>
          <h2>Installers ({release.tag})</h2>
          {(["macOS", "Windows", "Linux"] as const).map((platform) => {
            const assets = release.assets.filter((a) => a.platform === platform);
            if (assets.length === 0) return null;
            return (
              <section key={platform} aria-label={platform}>
                <h3>{platform}</h3>
                <ul>
                  {assets.map((a) => (
                    <li key={a.file}>
                      <a href={assetUrl(release, a.file)}>{a.label}</a> <code>{a.file}</code>
                    </li>
                  ))}
                </ul>
                <p>{PLATFORM_NOTES[platform]}</p>
              </section>
            );
          })}
          <p>
            All files and checksums are on the <a href={`${RELEASES_URL}/tag/${release.tag}`}>GitHub release page</a>.
          </p>
        </>
      ) : (
        <p>
          Installers for macOS, Windows and Linux will be listed here once a release is built and published on{" "}
          <a href={RELEASES_URL}>GitHub Releases</a>. Until then, build ShiftOps from source.
        </p>
      )}

      <h2>Build from source</h2>
      <ol>
        <li>Install Node.js 22.22 or newer and pnpm 10.</li>
        <li>
          Install Rust with <code>rustup</code> and the{" "}
          <a href="https://v2.tauri.app/start/prerequisites/">Tauri 2 prerequisites</a> for your platform.
        </li>
        <li>Clone the repository and run:</li>
      </ol>
      <pre>
        <code>{`git clone https://github.com/ABDULMUNAFZ/ShiftOps.git
cd ShiftOps
pnpm install
pnpm dev:desktop        # development build with hot reload
pnpm build:desktop      # local installer for this machine (unsigned)`}</code>
      </pre>
      <p>
        A locally built app is not quarantined, so macOS runs it without the Gatekeeper prompt that downloaded unsigned
        builds trigger. See the <Link href="/docs/">documentation</Link> for what the current build does.
      </p>
    </DocPage>
  );
}
