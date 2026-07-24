# Release And CI

## Verify Workflow

The repository includes `.github/workflows/verify.yml`.

It runs on Windows for every push and pull request and executes `cargo xtask verify`:

- Validates `website/pad.xml` (well-formed XML; `Program_Version` and `Primary_Download_URL` match `src-egui/Cargo.toml`'s version)
- Publishes the .NET sensor sidecar
- `cargo test` on `rigstats-backend` and `src-egui`
- `cargo clippy -- -D warnings`
- `cargo fmt --check`

To require it before merge:

1. Open GitHub repository Settings → Branches
2. Add a branch protection rule for `main`
3. Enable pull requests before merging
4. Enable required status checks
5. Select `Verify (Windows)`

## Build Workflow

The repository includes `.github/workflows/build.yml`.

It runs on every push to `main` (and can be triggered manually) and:

- runs `cargo xtask verify`
- runs `cargo xtask build` (publishes sidecar + builds release egui binary)
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
- Bumps versions in `src-egui/Cargo.toml` (marked with `# x-release-please-version`)
- When the release PR is merged, creates tag + GitHub Release and triggers `release.yml`

## PAD File (Software Directory Listings)

`website/pad.xml` is a [PAD](https://www.asp-shareware.org/pad/) (Portable
Application Description) file — a standardized XML format that software
directories like Softpedia read to list and auto-refresh an app's page. It's
published at `https://rigstats.app/pad.xml` by `deploy-website.yml` whenever
`website/**` changes on `main`. Directories that support PAD periodically
re-crawl that URL on their own, so once a listing exists there's normally
nothing to do per release — see the sections below for what keeps the file
itself in sync.

Everything below happens inside release-please's own release PR, before
merge, so no step needs to push to `main` directly (`main` requires a PR +
review + the `Verify (Windows)` check; the default `GITHUB_TOKEN` can't
bypass that, only the repo's Admin role can).

**Version + download URL** — `Program_Version` and `Primary_Download_URL` in
`website/pad.xml` are marked with the same `x-release-please-version`
comment used in `src-egui/Cargo.toml`, and the file is listed in
`release-please-config.json`'s `extra-files`. Release-please bumps both the
moment it opens/updates the release PR — same mechanism as the Cargo.toml
version bump.

**Release date + changelog text** — `Program_Release_Month/Day/Year` and
`Program_Change_Info` aren't simple version substrings, so
`release-please.yml` has an extra step (`Sync PAD release date and
changelog`) that runs right after release-please itself: it checks out
release-please's PR branch (`release-please--branches--main`, not `main`),
reads the newest entry release-please just wrote into `CHANGELOG.md`, and
pushes a follow-up commit with the parsed date and a plain-text summary of
the bug-fix/feature bullets (markdown links stripped, joined with `; `).
That branch isn't protected the way `main` is, so this works with the
default `GITHUB_TOKEN`. It's safe to re-run on every push while the PR is
open — release-please recomputes the whole PR each time, so this step just
reapplies on top.

**Not automated** — `File_Size_Bytes/K/MB` stay static from the initial
submission. Getting the exact figure needs the compiled installer, which
only exists after the release PR is merged and `release.yml` builds it —
by then we're back to the `main` branch-protection problem, and it's not
worth a privileged PAT just for a size figure that drifts by a few KB per
release. Revisit only if a directory actually rejects updates over it.

## Uptodown

RIGStats is also listed on [Uptodown](https://rigstats.en.uptodown.com/windows).
Unlike the PAD directories above, Uptodown mirrors the installer itself onto
its own CDN rather than linking straight to the GitHub asset, and renames it
to its own convention (`rigstats-<version>.exe`, lowercase) when it does.
That rename happens on their end — release assets stay
`RIGStats_<version>_x64-setup.exe` here, nothing to adjust per release.

To publish a new version there: open the app's Uptodown page, use their
update/submit flow, and point it at (or upload) the signed installer from the
GitHub Release. They re-crawl/re-approve on their own schedule after that.

## Release Assets

Installer publishing is handled by `.github/workflows/release.yml`.

It runs when a GitHub Release is published (or manually via `workflow_dispatch` with an existing tag) and:

- runs `cargo xtask verify`
- runs `cargo xtask build`
- builds the NSIS installer
- **signs the installer with Azure Trusted Signing** (Authenticode / SmartScreen)
- **signs `latest.json` with a legacy Tauri minisign key** via `npx --yes @tauri-apps/cli@^2 signer sign` — the one intentional Node.js dependency left in the pipeline (see the "Remove Node.js / npm infrastructure" entry in [ROADMAP.md](../ROADMAP.md)); it lets clients still on a pre-1.26 Tauri build verify and install an update instead of crashing
- **generates `latest.json`** — version, installer URL, SHA256 checksum, the minisign `signature`, and the current version's changelog section embedded in the `notes` field
- uploads the `.exe` and `latest.json` to the GitHub Release

A separate `.github/workflows/winget-submit.yml` runs after a release and
opens/updates the `Codeby.RIGStats` manifest PR in `microsoft/winget-pkgs`,
skipping submission if an update PR for that release is already open.

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
- `chore: update dependencies`

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
