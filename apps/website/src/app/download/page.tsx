import type { Metadata } from "next";
import Link from "next/link";
import { DocPage } from "../../components/doc-page";

export const metadata: Metadata = { title: "Download" };

export default function Download() {
  return (
    <DocPage title="Download" lead="No signed release builds have been published yet.">
      <p>
        Installers for macOS, Windows and Linux will be listed here once release signing and notarisation are in place.
        Until then, build ShiftOps from source.
      </p>

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
        <code>{`pnpm install
pnpm dev:desktop        # development build with hot reload
pnpm --filter @shiftops/desktop tauri build   # local installer, unsigned`}</code>
      </pre>
      <p>
        See the <Link href="/docs/">documentation</Link> for what the current build does.
      </p>
    </DocPage>
  );
}
