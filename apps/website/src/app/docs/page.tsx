import type { Metadata } from "next";
import { DocPage } from "../../components/doc-page";

export const metadata: Metadata = { title: "Documentation" };

export default function Docs() {
  return (
    <DocPage title="Documentation" lead="How to build and run ShiftOps, and what each part of the app does today.">
      <h2 id="status">What works today</h2>
      <table>
        <thead>
          <tr>
            <th>Area</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Desktop shell, navigation, command palette, themes</td>
            <td>Available</td>
          </tr>
          <tr>
            <td>First-run environment checks</td>
            <td>Available</td>
          </tr>
          <tr>
            <td>Local database (SQLite, versioned migrations with automatic backup)</td>
            <td>Available</td>
          </tr>
          <tr>
            <td>Credential storage in the OS keychain</td>
            <td>Available</td>
          </tr>
          <tr>
            <td>Native diagnostics with streaming output and cancellation</td>
            <td>Available</td>
          </tr>
          <tr>
            <td>Activity Centre (persisted audit log)</td>
            <td>Available</td>
          </tr>
          <tr>
            <td>Repository discovery</td>
            <td>Not yet implemented</td>
          </tr>
          <tr>
            <td>Campaign planning, Devin delegation, verification</td>
            <td>Not yet implemented</td>
          </tr>
          <tr>
            <td>GitHub and Devin account connection</td>
            <td>Not yet implemented</td>
          </tr>
        </tbody>
      </table>

      <h2 id="build">Build from source</h2>
      <p>
        Requirements: Node.js 22.22 or newer, pnpm 10, a stable Rust toolchain and the Tauri 2 system prerequisites for
        your OS.
      </p>
      <pre>
        <code>{`git clone <repository-url> shiftops
cd shiftops
pnpm install
pnpm dev:desktop`}</code>
      </pre>

      <h2 id="first-run">First-run environment checks</h2>
      <p>
        On first launch ShiftOps runs these checks natively through its Rust backend and shows the raw result. A tool
        that is missing is reported as unavailable with the reason; nothing is assumed.
      </p>
      <table>
        <thead>
          <tr>
            <th>Check</th>
            <th>How it is performed</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Operating system</td>
            <td>
              Compile-time OS and architecture, plus <code>sw_vers</code> on macOS, <code>/etc/os-release</code> on
              Linux or <code>ver</code> on Windows.
            </td>
          </tr>
          <tr>
            <td>Application data directory</td>
            <td>Resolves the per-user data directory and writes then deletes a probe file.</td>
          </tr>
          <tr>
            <td>Git, Node.js, pnpm, npm, Yarn, Devin CLI</td>
            <td>
              Finds the executable on the PATH from your login shell, then runs it with <code>--version</code>.
            </td>
          </tr>
          <tr>
            <td>Docker</td>
            <td>
              Runs <code>docker --version</code>, then <code>docker info</code> to confirm the daemon is reachable.
            </td>
          </tr>
          <tr>
            <td>OS keychain</td>
            <td>Writes, reads back and deletes a probe credential.</td>
          </tr>
        </tbody>
      </table>
      <p>
        Desktop apps started from the Dock or a launcher do not inherit your shell PATH, so ShiftOps reads it once from
        your login shell and adds common install locations such as <code>/opt/homebrew/bin</code>.
      </p>

      <h2 id="shortcuts">Keyboard shortcuts</h2>
      <ul>
        <li>
          <code>⌘K</code> / <code>Ctrl+K</code>: command palette
        </li>
        <li>
          <code>⌘1</code>–<code>⌘7</code>: go to Overview, Repositories, Campaigns, Agent Runs, Verification,
          Integrations, Settings
        </li>
        <li>
          <code>⌘B</code> / <code>Ctrl+B</code>: collapse or expand the sidebar
        </li>
        <li>
          <code>⌘J</code> / <code>Ctrl+J</code>: open the Activity Centre
        </li>
      </ul>
    </DocPage>
  );
}
