# Release And CI

## Verify Workflow

The repository includes `.github/workflows/verify.yml`.

It runs on Windows for push and pull requests and executes:

- `npm run prepare:lhm` (pinned LHM download if needed)
- `cargo test`
- `cargo check`
- `vitest run`

To require it before merge:

1. Open GitHub repository Settings -> Branches
2. Add a branch protection rule for `main`
3. Enable pull requests before merging
4. Enable required status checks
5. Select `Verify (Windows)`

## Build Workflow

The repository includes `.github/workflows/build.yml`.

It:

- installs dependencies
- runs `npm run verify`
- runs `npm run build`
- uploads generated NSIS installer as a GitHub Actions artifact

Use it by either:

1. pushing to `main`, or
2. running it manually from GitHub Actions -> Build -> Run workflow

After completion, download the installer from the workflow run's Artifacts section.

## Automated Changelog + Versioning

The repository now uses Release Please:

- Workflow: `.github/workflows/release-please.yml`
- Config: `release-please-config.json`
- Version manifest: `.release-please-manifest.json`

What it does:

- reads commits on `main`
- opens/updates a release PR
- updates `CHANGELOG.md`
- bumps versions in:
  - `package.json`
  - `src-tauri/Cargo.toml`
  - `src-tauri/tauri.conf.json`
- when the release PR is merged, it creates tag + GitHub Release automatically

## Release Assets

Installer publishing is handled by `.github/workflows/release.yml`.

It runs when a GitHub Release is published and:

- runs verification
- downloads and bundles pinned LibreHardwareMonitor via build scripts
- builds the NSIS installer
- **signs the installer with Azure Trusted Signing** (Authenticode / SmartScreen)
- **signs the installer with the Tauri minisign key** (`@tauri-apps/cli signer sign`) — must happen after Azure signing so the signature covers the final PE
- **generates `latest.json`** — extracts the current version's section from `CHANGELOG.md` and embeds it in the `notes` field; the updater dialog uses this to show release notes before installing
- uploads the `.exe` and `latest.json` to the published GitHub Release

The `latest.json` endpoint (`https://github.com/dvalfrid/rigstats/releases/latest/download/latest.json`) is polled by `tauri-plugin-updater` on every running instance.

### Signing keys

Two separate signing mechanisms are used:

| Key | Purpose | Where stored |
| --- | --- | --- |
| Azure Trusted Signing | Authenticode (SmartScreen trust, Windows installer) | GitHub Actions secrets: `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` |
| Tauri minisign keypair | Update integrity (Tauri updater verifies before install) | GitHub Actions secrets: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |

To regenerate the Tauri keypair:

```bash
npx @tauri-apps/cli signer generate -w ./rigstats-update.key
```

Copy the printed public key to `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`.

Manual run option:

1. Open GitHub -> Actions -> `Release`
2. Click `Run workflow`
3. Enter the existing tag (for example `v1.0.0`)
4. Run

This rebuilds the installer for that tag and attaches it to the same release.

## Commit Style (Important)

For best changelog quality, use Conventional Commits, for example:

- `feat: add manual GPU sensor override`
- `fix: handle missing LHM network throughput`
- `docs: update release instructions`
- `chore: bump vitest`

## Day-To-Day Process (Simple)

1. Develop normally and merge PRs to `main`.
2. Release Please keeps one release PR updated automatically.
3. When you want to release, merge that release PR.
4. GitHub will create the new tag/release and the release workflow will attach the installer.

The bundled LHM version is pinned in `build/prepare-lhm.ps1` (currently `v0.9.6`).
