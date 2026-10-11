# Release And CI

## Verify Workflow

The repository includes `.github/workflows/verify.yml`.

It runs on Windows for every push and pull request and executes `cargo xtask verify`:

- Validates `website/pad.xml` (well-formed XML; `Program_Version` and `Primary_Download_URL` match `src-egui/Cargo.toml`'s version)
- Publishes the .NET sensor sidecar
- `cargo test` on `rigstats-backend` and `src-egui`
- `cargo clippy -- -D warnings`
- `cargo fmt --check`
- `cargo deny check advisories` (RustSec; see [Supply chain](#supply-chain))

**`main` is protected:** a pull request — Dependabot's, release-please's or
anyone's — can only be merged once `Verify (Windows)` has passed (required
status check, bound to GitHub Actions). Force pushes and deleting `main` are
blocked. Admins aren't held to it (`enforce_admins: false`): the maintainer
can still push directly to `main`, and GitHub records it as a bypass.
Settings → Branches → `main` to change it.

**CodeQL is not a merge gate:** once `Verify (Windows)` is green a pull
request can be merged — don't wait for the CodeQL jobs. They keep running
and report to Security → Code scanning after the merge too. `codeql.yml`
covers GitHub Actions and C# (about a minute); `codeql-rust.yml` covers
Rust (about 20 minutes, on Windows so `#[cfg(windows)]` code is analysed)
and runs on pull requests only when `.rs` or Cargo files changed — every
push to `main` and the weekly schedule always run it.

On a pull request without Rust changes, the "Code scanning results / CodeQL"
check reports **"1 configuration not found"** and shows as skipped. That is
expected: `main` has a Rust analysis, the pull request doesn't. The
`Analyze (actions)` and `Analyze (csharp)` jobs still run and cover the
change. Nothing to fix.

### Pinned Rust toolchain

`build.yml`, `verify.yml`, and `release.yml` all install Rust via
`dtolnay/rust-toolchain` pinned by commit SHA with `toolchain: 1.98.0` — a
specific version, not `@stable`. `@stable`
floated to whatever Rust shipped that day, which twice broke CI on an
unrelated push (a new clippy lint, then a new rustc future-incompat lint)
with no code change to blame. `rust-toolchain.toml` at the repo root pins the
same version for local development, so `rustup` auto-installs it the first
time `cargo` runs in the repo — no drift between a contributor's machine and
CI. Bumping the pin (the workflows' `toolchain:` inputs — and the action SHA,
whose `# 1.98.0` comment names the branch it came from — plus
`rust-toolchain.toml`) is a deliberate, standalone commit.

### Supply chain

`release.yml` holds the signing credentials, so what CI runs is pinned (#224):

- **Actions** are pinned to full commit SHAs with the version as a comment
  (`uses: owner/repo@<sha> # v4.4.0`). A tag can be moved by its owner; a SHA
  can't. **Dependabot** (`.github/dependabot.yml`) opens a grouped PR weekly
  per ecosystem — GitHub Actions, Cargo, NuGet — so the pins stay current.
  `LibreHardwareMonitorLib` is excluded: it is pinned to an exact build on
  purpose and moved by hand after a hardware test.
- **NSIS** (`build.yml`, `release.yml`): `choco install nsis --version=3.13.0`,
  then `makensis /VERSION` is checked. Bump the version and the check together.
- **wingetcreate** (`winget-submit.yml`): a fixed GitHub release, checked
  against its SHA-256 (`WINGETCREATE_TAG` / `WINGETCREATE_SHA256`; the hash is
  on the release page next to `wingetcreate.exe`).
- **Dependency audit:** `cargo xtask verify` runs `cargo deny check advisories`
  (config: `deny.toml`; an exception goes in its `ignore` list with the
  advisory id and a reason). NuGet is audited by the restore itself:
  `Directory.Build.props` turns on `NuGetAudit` for both .NET projects,
  transitive packages included, and a moderate-or-worse vulnerability fails
  the build. `.github/workflows/audit.yml` runs both checks every Monday, so
  a new advisory shows up without a push.
- Locally, verify skips the Rust audit with a hint when cargo-deny is missing:
  `cargo install cargo-deny --locked`. In CI it is required.

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
- Bumps versions in `src-egui/Cargo.toml` and `sensor-sidecar/sensor-sidecar.csproj` (marked with `x-release-please-version`) — the sensor service reports the same version as the app (`service_version` in the control pipe's `hello`)
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
`website/pad.xml` are set by the `Sync PAD version, release date and
changelog` step in `release-please.yml`, from the version release-please wrote
into `src-egui/Cargo.toml`, every time it opens or updates the release PR. The
same step sets `softwareVersion` in the website's structured data
(`website/index.html`, the JSON-LD block search engines read).

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
- **signs `rigstats.exe`, `rigstats-wallpaper.exe` and `rigstats-sensor.exe`** with Azure Trusted Signing in one step, **before** packaging, and checks that the app and the service share one signer. This is not just for SmartScreen: the service accepts Control Center connections only from the installed `rigstats.exe` signed by its own signer (`PipeClientVerifier`), so unsigned binaries in the installer would leave the Control Center dead for every user
- builds the NSIS installer
- **signs the installer with Azure Trusted Signing** (Authenticode / SmartScreen)
- **generates `latest.json`** — version, installer URL, SHA256 checksum, and the current version's changelog section embedded in the `notes` field. The app refuses an update unless the download matches the checksum and carries a valid signature from the app's own publisher (`update_check.rs`). Clients older than 1.26 (Tauri updater, minisign) can no longer update in place
- uploads the `.exe` and `latest.json` to the GitHub Release

### Winget

After publishing, `release.yml` starts `.github/workflows/winget-submit.yml`
with the tag, which opens/updates the `Codeby.RIGStats` manifest PR in
`microsoft/winget-pkgs` (skipped if an update PR is already open). It is a
dispatch rather than a `workflow_run` chain: the Release run is started with
`GITHUB_TOKEN`, and GitHub starts no workflow from a `GITHUB_TOKEN`-caused
event except a dispatch (#246). Pre-releases are never submitted.

**Holding a release back** (e.g. to let testers try it first): add the label
**`winget: hold`** to the release-please PR before merging it. The release is
published as usual — installer, `latest.json`, in-app updates — but not
submitted to winget; the Release Please run logs the decision. When it's
ready: Actions → Submit to Winget → Run workflow → the tag (e.g. `v1.45.0`).

### Signing

| Key | Purpose | Where stored |
| --- | --- | --- |
| Azure Trusted Signing | Authenticode (SmartScreen trust, Windows installer) | GitHub Actions secrets: `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` |

### Manual re-run

If the release build fails or needs to be re-run for an existing tag:

1. Open GitHub → Actions → Release
2. Click `Run workflow`
3. Enter the existing tag (e.g. `v1.27.0`)
4. Untick "Submit this release to winget" if winget already has it or should wait
5. Run

Leave the "Use workflow from" dropdown on `main`.

## Day-To-Day Process

1. Develop normally and merge PRs to `main`.
2. Release Please keeps one release PR updated automatically.
3. When ready to release, go through the [release routine](#release-routine)
   and merge the release PR.
4. GitHub creates the tag/release and `release.yml` attaches the signed installer.

## Release Routine

**Before merging the release PR:**

1. **Checks on `main`:** `cargo xtask verify`. While the dev sidecar
   (`tools\dev-sidecar.ps1`) runs, it locks the sidecar's build, so the .NET
   part can't run. If the sidecar hasn't changed since the last release
   (`git diff --stat v<last>..main -- sensor-sidecar sensor-sidecar.Tests` is
   empty), its tests passed for that release and the Rust part
   (`cargo xtask fmt-check`, `clippy`, `test`) is enough. Otherwise stop the dev
   sidecar first (Ctrl+C) and run the whole of `verify`.
2. **Start-up smoke test:** `pwsh -File tools\smoke-app.ps1`, on a fresh debug
   build of `main`. Close the debug app before building. A running app locks
   `rigstats.exe`, so `cargo build` fails while the smoke test still runs the
   older exe. Check that the build said `Finished`.
3. **The [manual pre-release checklist](#manual-pre-release-checklist)** for
   whatever this cycle touched.
4. **Read the release PR's changelog** as a user would: each line is what they
   get. A line that only makes sense to us can be reworded without touching
   history. Put a `BEGIN_COMMIT_OVERRIDE` … `END_COMMIT_OVERRIDE` block, holding
   the line as it should read (conventional-commit form), in the description of
   the merged PR it came from. Release-please uses it on its next run (any push
   to `main`).
5. **Docs say what ships:** the README (the Control Center table, feature
   sections, the dependency versions it names), the website's text and
   screenshots (`assets/*.png`, copied to the site when it is deployed), and
   `docs/troubleshooting.md` for every new feature that can go wrong.
6. **Highlights:** write `release-highlights.md` for the version the release
   PR will publish: a few lines for users, `- **Title** — what it does for
   you`, under `# <version>` (the file's comment has an example). The big
   features, anything optional, and where to report a problem if something new
   can go wrong. The Release workflow (`tools/release-highlights.ps1`) puts
   them first in `latest.json`'s notes, so the app's Updates window shows them
   for the update, and above the generated text on the GitHub Release. The
   release build includes them, so the new version's "What's new" shows them
   too (`update_check::with_highlights`). Only a list for the version being
   released counts: last release's list is ignored, and without one nothing is
   added.
7. **`winget: hold`?** Only when testers should try it first (see
   [Winget](#winget)).

**After merging it:**

8. **Wait for the Release workflow** until the installer and `latest.json` are
   on the release, and check that the highlights are on top of the release
   text.
9. **The signed installer** — the Control Center check in the manual checklist,
   which only the released installer can show.
10. **Screenshots that show a version** (the website's Status screenshot names
    the built-in dependency versions): retake them from the installed release.

**Fixing the release text later:** `CHANGELOG.md` (written by release-please,
built into the app) and the GitHub Release's text are two different things. The
release text can be edited at any time, either with **Edit release** on the
release page or with `gh release edit vX.Y.Z --notes-file notes.md`, without
touching the tag, the code or the installer. The app's Updates window doesn't
read it, though: what users see there is fixed when the release is built, so
the highlights belong in `release-highlights.md` before the release.

## Commit Style

Use [Conventional Commits](https://www.conventionalcommits.org/) for best changelog quality:

- `feat: add manual GPU sensor override`
- `fix: handle missing LHM network throughput`
- `docs: update release instructions`
- `chore: update dependencies`

---

## Manual Pre-Release Checklist

`cargo xtask verify` covers everything that can be asserted in code. The
items below can't — they need a real DWM compositor, real window modes, or a
real Explorer session — so they're a manual pass before tagging a release.
Add to this list whenever a bug turns out to only be catchable by hand.

Run through this on the debug build (`cargo build --manifest-path
src-egui/Cargo.toml`) before merging the release PR:

- **Window layers** — cycle Normal / Always-on-Top / Always-Behind (tray menu
  or Settings) and confirm the dashboard renders correctly, including
  translucency at a partial opacity setting, in each.
- **Profiles** — spot-check at least one portrait and one landscape profile
  beyond whatever was touched by this cycle's changes; pay attention to
  scrollable content (CPU cores, disk drives) at different `sc` scales.
- **Desktop Wallpaper mode** — attach, confirm it survives `Win+D`, sits under
  desktop icons, and opacity renders per-pixel (not a uniform dim). Restart
  `explorer.exe` and confirm it re-attaches on its own.
- **Floating mode** — panels reposition/persist correctly across a restart.
- **Opacity extremes** — check both ~10% and 100% opacity; background should
  fade but text/gauges/graph lines/bars must stay fully legible at every
  level (see [ROADMAP.md](../ROADMAP.md) background-transparency entries for
  why this specific check exists).
- **Settings dialog** — open every tab, change a setting, close, reopen the
  app, confirm it persisted. Confirm dialogs themselves never become
  translucent regardless of the dashboard opacity setting.
- **Fresh install vs. upgrade** — if `settings.rs` migration logic changed
  this cycle, test loading an old-format `rigstats-settings.json` in addition
  to a clean install.
- **Website "Coming in vX" badges** — remove the `badge-soon` spans for
  features this release ships (`website/index.html`), in the last commit
  before merging the release PR, so the site and the release go out together.
- **Control Center on the signed installer** — debug builds and the Build
  workflow's installer artifact are unsigned, so the service refuses their
  Control Center connection by design (`PipeClientVerifier`); only the signed
  release installer can be checked. Right after the release: install it, open
  the Control Center, confirm it connects and a profile applies. If it says it
  can't connect, check `%ProgramData%\se.codeby.rigstats\rigstats-sensor.log`
  for `Client rejected: …` and fix forward before announcing.
- **PawnIO on a machine without it** (#205 — required once before the first
  release that bundles `PawnIO_setup.exe`, then whenever the pinned PawnIO
  version changes). Run on real hardware; a VM can't show CPU temp.
  Elevated: `sc.exe stop rigstats-sensor`, then
  `& "C:\Program Files\PawnIO\uninstall.exe" -uninstall`. Confirm the bug
  reproduces (`sc.exe query PawnIO` → 1060, no `ROOT\PAWNIO` device in
  `Get-PnpDevice`). Optionally stage the old driver like ≤ 1.41 did
  (`git archive 73fbb65e5 build/pawnio | tar -x -C $env:TEMP`, then
  `pnputil /add-driver $env:TEMP\build\pawnio\pawnio.inf /install`). Run the
  installer, then check: `rigstats-install.log` has `pawnio_exit=0` (183 means
  PawnIO was already there — not a real test), `sc.exe query PawnIO` is
  RUNNING, and CPU temp/package power show in the dashboard without a reboot.
  Recovery if it fails: `winget install namazso.PawnIO`.

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
