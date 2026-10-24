import type { Metadata } from "next";
import { DocPage } from "../../components/doc-page";

export const metadata: Metadata = { title: "Security" };

export default function Security() {
  return (
    <DocPage title="Security" lead="ShiftOps runs code-changing work across many repositories. This is how it limits what it can touch.">
      <h2>Credentials</h2>
      <ul>
        <li>
          Tokens are stored only in the operating system credential store: macOS Keychain, Windows Credential Manager or
          the Secret Service on Linux.
        </li>
        <li>Secrets are read by the native Rust layer. The web view can ask whether a credential exists, never its value.</li>
        <li>Credentials are never written to the SQLite database, log files or browser storage.</li>
      </ul>

      <h2>Native boundary</h2>
      <ul>
        <li>The web view can call only the commands ShiftOps registers. No shell or filesystem plugin is exposed to it.</li>
        <li>Settings writes are restricted to a fixed list of keys and size-limited.</li>
        <li>A strict Content Security Policy blocks remote scripts and network requests from the web view.</li>
        <li>Environment checks run fixed commands with fixed arguments. No user input is passed to a shell.</li>
      </ul>

      <h2>Agents and verification</h2>
      <p>
        An agent&apos;s report that a change succeeded is not trusted on its own. ShiftOps re-runs each repository&apos;s
        verification commands against the resulting branch before marking it verified.
      </p>

      <h2>Reporting a vulnerability</h2>
      <p>
        Report vulnerabilities privately through the repository&apos;s GitHub security advisory form. Please do not open
        a public issue.
      </p>
    </DocPage>
  );
}
