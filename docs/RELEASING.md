# Releasing ShiftOps

## Desktop installers

`.github/workflows/release.yml` runs when a version tag is pushed:

```sh
# Version lives in apps/desktop/src-tauri/tauri.conf.json, Cargo.toml and apps/desktop/package.json.
git tag v0.1.0 && git push origin v0.1.0
```

It builds on GitHub-hosted runners with `tauri-apps/tauri-action` (pinned to action-v1.0.0 by commit):

| Platform            | Runner           | Artifacts                                            |
| ------------------- | ---------------- | ---------------------------------------------------- |
| macOS Apple Silicon | `macos-latest`   | `ShiftOps_<version>_aarch64.dmg`, `.app.tar.gz`      |
| macOS Intel         | `macos-latest`   | `ShiftOps_<version>_x64.dmg`, `.app.tar.gz`          |
| Windows x64         | `windows-latest` | `ShiftOps_<version>_x64_en-US.msi`, `_x64-setup.exe` |
| Linux x64           | `ubuntu-22.04`   | `ShiftOps_<version>_amd64.AppImage`, `_amd64.deb`    |

Assets go to a **draft** release first. The `publish` job turns it into a pre-release only after every platform built, so a partial release is never published. Each job also keeps its files as a workflow artifact.

After a release is published, record it in `apps/website/src/release.ts` (`DESKTOP_RELEASE`) with the exact asset file names, and redeploy the website. The download page shows links only for a recorded release.

Locally, `pnpm build:desktop` builds the installers for the current machine into `apps/desktop/src-tauri/target/release/bundle/`.

## Signing status

No signing credentials exist for this project, so every installer is **unsigned**:

- **macOS**: not signed with a Developer ID certificate and not notarized. Gatekeeper blocks downloaded copies; the website recommends building from source on macOS. Signing needs an Apple Developer Program membership (USD 99 per year), a Developer ID Application certificate, and notarization credentials stored as repository secrets. Tauri reads `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, and `APPLE_ID`/`APPLE_PASSWORD`/`APPLE_TEAM_ID` or an App Store Connect API key.
- **Windows**: not Authenticode-signed. SmartScreen warns about an unknown publisher. Signing needs a code-signing certificate (OV/EV, or Azure Trusted Signing) and the matching `bundle.windows` signing configuration.
- **Linux**: AppImage and .deb are not GPG-signed.

Never describe these builds as signed, and never tell users to disable Gatekeeper or remove quarantine attributes.

## Website

`.github/workflows/website.yml` builds `apps/website` and publishes `apps/website/out` to the `gh-pages` branch on pushes to `main` that touch the website, or when run manually. GitHub Pages serves that branch at the domain in `apps/website/public/CNAME` (`onlyhacks.tech`) at no cost; HTTPS comes from GitHub's Let's Encrypt certificate once DNS resolves.

DNS for `onlyhacks.tech` (registrar: Namify; nameservers `*.orderbox-dns.com`):

| Type  | Host  | Value                   |
| ----- | ----- | ----------------------- |
| A     | `@`   | `185.199.108.153`       |
| A     | `@`   | `185.199.109.153`       |
| A     | `@`   | `185.199.110.153`       |
| A     | `@`   | `185.199.111.153`       |
| AAAA  | `@`   | `2606:50c0:8000::153`   |
| AAAA  | `@`   | `2606:50c0:8001::153`   |
| AAAA  | `@`   | `2606:50c0:8002::153`   |
| AAAA  | `@`   | `2606:50c0:8003::153`   |
| CNAME | `www` | `abdulmunafz.github.io` |

After DNS resolves, enable **Enforce HTTPS** in the repository's Pages settings (or `gh api -X PUT repos/ABDULMUNAFZ/ShiftOps/pages -F https_enforced=true`).
