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

## GitHub Releases

The repository includes `.github/workflows/release.yml`.

It runs when you push a tag matching `v*`, for example `v1.0.0`.

The workflow:

- runs verification
- downloads and bundles the pinned LibreHardwareMonitor release as part of `npm run build`
- builds the installers
- creates a GitHub Release
- attaches `.exe` and `.msi` files to the release

The bundled LHM version is currently pinned to `v0.9.6` via `build/prepare-lhm.ps1`.
`vendor/` stays ignored in git, so GitHub builds remain deterministic without checking the binaries into the repository.

Recommended release flow:

1. Update versions in these files so they all match:
   - `package.json`
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
2. Commit and push to `main`
3. Create and push a tag:

   ```powershell
   git tag v1.0.0
   git push origin v1.0.0
   ```

4. Wait for the Release workflow to complete
5. Edit release notes on GitHub if you want to expand on the generated notes
