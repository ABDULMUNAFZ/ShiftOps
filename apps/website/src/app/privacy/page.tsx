import type { Metadata } from "next";
import { DocPage } from "../../components/doc-page";

export const metadata: Metadata = { title: "Privacy" };

export default function Privacy() {
  return (
    <DocPage title="Privacy" lead="ShiftOps is a local desktop application. It does not operate a backend service.">
      <h2>What stays on your machine</h2>
      <ul>
        <li>Application state is stored in a SQLite database in your user data directory.</li>
        <li>Environment check results are computed locally and are not transmitted.</li>
        <li>Credentials are stored in your operating system keychain.</li>
      </ul>

      <h2>What leaves your machine</h2>
      <p>
        ShiftOps contacts third parties only for features you connect. When those features are enabled, requests go
        directly from your machine to GitHub and to Devin under your own accounts and their respective terms. The
        current build does not connect to either.
      </p>

      <h2>Telemetry</h2>
      <p>ShiftOps collects no telemetry, analytics or crash reports.</p>

      <h2>This website</h2>
      <p>
        This site is static. It sets no cookies. Your theme choice is saved in your browser&apos;s local storage and
        never sent anywhere.
      </p>
    </DocPage>
  );
}
