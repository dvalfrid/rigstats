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
- runs the pinned LibreHardwareMonitor prepare step through `npm run build`
- runs `npm run build`
- uploads generated NSIS and MSI installers as GitHub Actions artifacts

Use it by either:

1. pushing to `main`, or
2. running it manually from GitHub Actions -> Build -> Run workflow

After completion, download installers from the workflow run's Artifacts section.

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
- builds installers
- uploads `.exe` and `.msi` to that published release

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
4. GitHub will create the new tag/release and the release workflow will attach installers.

The bundled LHM version is pinned in `build/prepare-lhm.ps1` (currently `v0.9.6`).
