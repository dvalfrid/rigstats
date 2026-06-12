# Release And CI

## Verify Workflow

The repository includes `.github/workflows/verify.yml`.

It runs on Windows for every push and pull request and executes:

- `npm run prepare:sidecar` (publishes the .NET sensor sidecar as a self-contained exe)
- `cargo test --manifest-path rigstats-backend/Cargo.toml`
- `cargo clippy --manifest-path src-egui/Cargo.toml -- -D warnings`
- `cargo fmt --check`
- `npm run lint` (ESLint)
- `npm run lint:md` (markdownlint)
- `vitest run`

To require it before merge:

1. Open GitHub repository Settings → Branches
2. Add a branch protection rule for `main`
3. Enable pull requests before merging
4. Enable required status checks
5. Select `Verify (Windows)`

## Build Workflow

The repository includes `.github/workflows/build.yml`.

It runs on every push to `main` (and can be triggered manually) and:

- runs `npm run verify`
- runs `npm run build` (publishes sidecar + builds release egui binary)
- builds the NSIS installer
- uploads the installer as a GitHub Actions artifact (`rigstats-nsis`)

To download the built installer without triggering a release:

1. Open GitHub → Actions → Build
2. Click the latest run
3. Download `rigstats-nsis` from the Artifacts section

## Automated Changelog + Versioning

The repository uses [Release Please](https://github.com/googleapis/release-please):

- Workflow: `.github/workflows/release-please.yml`
- Config: `release-please-config.json`
- Version manifest: `.release-please-manifest.json`

What it does:

- Reads Conventional Commits on `main`
- Opens/updates a release PR
- Updates `CHANGELOG.md`
- Bumps versions in `package.json` and `src-egui/Cargo.toml` (marked with `# x-release-please-version`)
- When the release PR is merged, creates tag + GitHub Release and triggers `release.yml`

## Release Assets

Installer publishing is handled by `.github/workflows/release.yml`.

It runs when a GitHub Release is published (or manually via `workflow_dispatch` with an existing tag) and:

- runs `npm run verify`
- builds the NSIS installer
- **signs the installer with Azure Trusted Signing** (Authenticode / SmartScreen)
- **generates `latest.json`** — version, installer URL, SHA256 checksum, and the current version's changelog section embedded in the `notes` field
- uploads the `.exe` and `latest.json` to the GitHub Release

### Signing

| Key | Purpose | Where stored |
| --- | --- | --- |
| Azure Trusted Signing | Authenticode (SmartScreen trust, Windows installer) | GitHub Actions secrets: `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` |

### Manual re-run

If the release build fails or needs to be re-run for an existing tag:

1. Open GitHub → Actions → Release
2. Click `Run workflow`
3. Enter the existing tag (e.g. `v1.27.0`)
4. Run

## Day-To-Day Process

1. Develop normally and merge PRs to `main`.
2. Release Please keeps one release PR updated automatically.
3. When ready to release, merge the release PR.
4. GitHub creates the tag/release and `release.yml` attaches the signed installer.

## Commit Style

Use [Conventional Commits](https://www.conventionalcommits.org/) for best changelog quality:

- `feat: add manual GPU sensor override`
- `fix: handle missing LHM network throughput`
- `docs: update release instructions`
- `chore: bump vitest`

---

## Testing an Installer Before Release

Before merging the release PR, test the full installer flow using the artifact
from `build.yml` and a test `latest.json` to verify the in-app updater
end-to-end without publishing a real release.

### Step 1 — Get the installer artifact

Run `build.yml` on `main` (or let it run automatically after merge),
then download the `rigstats-nsis` artifact from the Actions run.

### Step 2 — Upload to a GitHub pre-release

Create a GitHub Release marked as **pre-release** (not published) and upload the
installer `.exe`. Copy the direct download URL — you will need it in step 3.
Pre-releases are never returned by `/releases/latest/`, so the live update
endpoint is unaffected.

### Step 3 — Create a test `latest.json`

Create a file with version set **one patch higher** than the build (e.g. `1.27.1`
if the app is `1.27.0`) so `is_newer` returns `true` against the running build:

```json
{
  "version": "1.27.1",
  "notes": "## [1.27.1] — Update flow test\n\n### Bug Fixes\n* verify end-to-end update works",
  "pub_date": "2026-06-12T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "sha256": "test",
      "url": "https://github.com/dvalfrid/rigstats/releases/download/v1.27.0-test/RIGStats_1.27.0_x64-setup.exe"
    }
  }
}
```

Upload this file as a **GitHub Gist** and copy the Raw URL.

### Step 4 — Build a test binary pointing at the Gist

Set `RIGSTATS_UPDATE_URL` before building. The `option_env!` macro bakes the
URL in at compile time; production builds (where the variable is not set) always
use the real GitHub URL.

```powershell
$env:RIGSTATS_UPDATE_URL = "https://gist.githubusercontent.com/dvalfrid/<id>/raw/latest.json"
cargo build --manifest-path src-egui/Cargo.toml
.\target\debug\rigstats.exe
```

### Step 5 — Run through the update flow

1. Open the test app → tray icon → **Check for Updates**
   - ✅ Dialog shows "v1.27.1 available" with the changelog text from `notes`
2. Click **Update Now**
   - ✅ Download progress bar advances
   - ✅ Windows UAC prompt appears for the installer
   - ✅ NSIS installer runs (accept UAC to proceed)
3. After install completes:
   - ✅ `rigstats.exe` updated on disk
   - ✅ `rigstats-sensor` Windows Service is running
   - ✅ App starts and previous settings are intact

### Step 6 — Clean up

- Delete the pre-release and the Gist.
- Remove the env var: `Remove-Item Env:\RIGSTATS_UPDATE_URL`
