# tools/

Standalone PowerShell maintenance scripts for RIGStats. These are developer/admin
helpers — not part of the app build (build/test tasks live in `cargo xtask`).
Run them with PowerShell 7+ (`pwsh`).

## `sync-roadmap-issues.ps1`

Mirrors the roadmap to **GitHub Issues** under the `v2.0` milestone and keeps them
in sync. It is an idempotent **upsert**, not a one-shot creator.

- Source of truth is the `$features` array inside the script (id, title, summary,
  status, label, kind). `ROADMAP.md` stays the human-readable doc; the script
  also **regenerates** its "GitHub issue tracking" table in place, between the
  `<!-- roadmap-table:start -->` / `<!-- roadmap-table:end -->` markers, so the
  doc never drifts from the issues.
- Each issue carries a hidden marker in its body — `<!-- roadmap-id: <id> -->`
  (invisible in GitHub's rendered view). Issues are matched by that marker (with
  first-run adoption by exact title), so **re-running never creates duplicates**.
- On each run it reconciles only what differs: title, body, label, milestone, and
  open/closed state (`done`/`dropped` → closed with reason `completed` /
  `not planned`; `planned` → open).
- Pre-existing issues can be tracked without overwriting their content via a
  `pin=<number>` entry (currently #81 and #83). Markers whose id is no longer in
  the data are reported as `ORPHAN` for manual review — nothing is auto-deleted.

```powershell
pwsh -NoProfile -File tools/sync-roadmap-issues.ps1
```

To change an issue later: edit the matching entry in `$features` (and the mirror
row in `ROADMAP.md`), then re-run. Requires the GitHub CLI authenticated
(`gh auth status`); the script calls `gh` at its full install path.

## `dev-sidecar.ps1`

Runs the **debug sensor sidecar** in place of the installed `rigstats-sensor`
service, so Control Center changes can be tested on real hardware. Needs an
elevated PowerShell. Dry-run by default (UI works, nothing is written to the
hardware); `-Live` for real writes. Ctrl+C hands everything back to the
firmware and starts the installed service again. The full workflow is in
"Live testing on real hardware" in
[`docs/control-architecture.md`](../docs/control-architecture.md).

```powershell
pwsh -File tools\dev-sidecar.ps1 -Live
```

## `download-stats.ps1`

Download counts per release from GitHub's release assets: **installer**
(`RIGStats_*_x64-setup.exe` — new installs, winget installs and upgrades, and
the app's own updates, which all download from GitHub) and **checks**
(`latest.json` — the app's update checks, about 10 s after start and then every
6 h; the best measure of active installations). The table also shows each
release per day while it was the newest, so short- and long-lived releases
compare. Needs `gh`, logged in.

```powershell
pwsh -File tools\download-stats.ps1              # recent releases
pwsh -File tools\download-stats.ps1 -Top 0       # every release
pwsh -File tools\download-stats.ps1 -Append x.csv  # add a snapshot to a CSV
```

GitHub keeps only running totals, so `.github/workflows/download-stats.yml`
appends a snapshot every Monday to `downloads.csv` on the `stats` branch (see
"Download Statistics" in [docs/release.md](../docs/release.md)).

## `release-highlights.ps1`

Puts the release's highlights (`release-highlights.md` in the repo root) into a
release; used by `.github/workflows/release.yml`. `-Version X -Notes <text>`
returns the notes with a `### Highlights` section first under that version's
heading (for `latest.json`, shown by the app's Updates window); `-Version X
-Body` returns a `## Highlights` block for the top of the GitHub Release text.
Only a list headed `# X` counts, and HTML comments are skipped. The app does
the same for its bundled copy (`update_check::with_highlights`). See "Release
routine" in [docs/release.md](../docs/release.md).

```powershell
pwsh -File tools\release-highlights.ps1 -Version 1.48.0 -Body   # preview the release text's block
```

## `release-contributors.ps1`

The `## Thanks` block of a GitHub Release: everyone outside the core team
whose work is in it, with their pull requests — the authors and
`Co-authored-by` people (GitHub noreply addresses) of the commits since the
previous real release, not `-Owner`, not bots. Prints nothing when there are
none. Used by `.github/workflows/release.yml`; needs `gh` signed in.

```powershell
pwsh -File tools\release-contributors.ps1 -Tag v1.50.0 -Repo dvalfrid/rigstats -Owner dvalfrid   # preview
```

## `smoke-app.ps1`

**Start-up smoke test** for the egui app — run it before and after a change
to start-up, window modes or `main.rs` structure. It launches
`target\debug\rigstats.exe` once per start mode and checks each:

| Mode | Checks (besides: still running, no `[ERROR]` or panic in the log) |
| --- | --- |
| `normal` | main window on-screen, no wallpaper host |
| `wallpaper` | main window parked, `rigstats-wallpaper` running and attached to WorkerW |
| `floating` | main window parked, a floating panel shown |
| `overlay` | main window on-screen, overlay shown |
| `just-updated` | `--just-updated=…` opens the update window |

It stops every running RIGStats first (the installed app too — single-instance
guard), switches modes by editing `rigstats-settings.json`, and restores the
file byte for byte afterwards. Exit code = number of failed modes; a failure
prints the log's errors and the app's windows.

```powershell
cargo build --manifest-path src-egui/Cargo.toml
pwsh -File tools\smoke-app.ps1                  # all modes
pwsh -File tools\smoke-app.ps1 -Only wallpaper  # one mode
pwsh -File tools\smoke-app.ps1 -Restart         # start the installed app afterwards
```

## `measure-resources.ps1`

**What RIGStats costs the PC** (#463, #462): samples every `rigstats*` process
and `WmiPrvSE` every `-Interval` seconds for `-Minutes` and writes a CSV — CPU
(% of one core, averaged over each interval by Windows), working set, private
bytes, threads, handles, and for the app its GPU dedicated memory and GPU
engine utilization — then prints a summary per process (average and peak
CPU, memory range, handle drift). Read-only: only performance counters, no
WMI while measuring (WmiPrvSE is measured). The service's three processes
show by role when the shell may read their command lines (elevated), else
by pid. The numbers go into "Resource budget" in
[docs/architecture.md](../docs/architecture.md).

```powershell
pwsh -File tools\measure-resources.ps1 -Minutes 30 -Label release-normal
```

## `openrgb-asus-watch.ps1`

Reports ASUS lighting devices **added to OpenRGB** since the last review, so
RIGStats can follow. Reads OpenRGB's ASUS detector sources (product ids and
names only — protocol documentation, no code copied) and compares them with
`openrgb-asus-baseline.json`, the list already reviewed. Each new entry says
which RIGStats driver handles its protocol family (keyboard table, monitor
table, Aura USB) — then support is mostly a table entry plus a hardware check —
or that none does yet (mice, AIO coolers, ...).

```powershell
pwsh -NoProfile -File tools/openrgb-asus-watch.ps1                  # report new entries
pwsh -NoProfile -File tools/openrgb-asus-watch.ps1 -Json            # the same, as JSON
pwsh -NoProfile -File tools/openrgb-asus-watch.ps1 -UpdateBaseline  # after review: mark all as seen
```

A new entry in a supported family becomes an issue and a pull request that
adds it as **unverified** (`Verified` stays false in its model table) — it
is marked verified only once someone has seen it work on the hardware. The
supported-devices list (`docs/supported-devices.md`, the website) is
generated from those tables, so it follows; see `LightingCatalog`.

## `clean-tray-ghosts.ps1`

Removes ghost/orphaned RIGStats entries from the Windows system-tray icon
settings (HKCU) and from Installed Apps / Programs & Features (HKLM) when an
uninstaller is missing. Shows what it will keep vs delete, asks for confirmation,
then restarts Explorer. Self-elevates to admin. See
[`docs/developer-troubleshooting.md`](../docs/developer-troubleshooting.md#ghost-entries-in-other-system-tray-icons)
for when to use it.

```powershell
pwsh -ExecutionPolicy Bypass -File tools\clean-tray-ghosts.ps1
```

## `keyboard-listen-guided.ps1`

A guided, read-only live check of an ASUS keyboard (#457): opens a console
with numbered steps in Swedish ("Håll ned Fn, tryck 2 …"), listens to every
collection of the keyboard except `mi_00`, sends only the documented `12 00`
and `12 01` at the start and the end, and writes prompts and input reports
to a log. Found the Azoth X's profile event (`docs/keyboard-research.md`).

```powershell
pwsh -File toolskeyboard-listen-guided.ps1                         # on the ROG Omni receiver (report 0x02)
pwsh -File toolskeyboard-listen-guided.ps1 -ProductId 1c24 -Report 0   # by cable
```

The log holds the key-state reports (`71`) of what was typed meanwhile —
don't share it beyond the issue.

## `hid-caps.ps1`

Prints every HID collection of a device and what its report descriptor
declares — usage page/usage, report lengths, each input/output/feature
usage with its report id. Read-only (the collections are opened without
access rights; nothing is sent), so it is the first step before any code
for a new device: it shows what the device can be asked or told through
*documented* HID usages — a telephony headset's LED outputs, a consumer
control's buttons — and which collections are vendor-defined (page `0xFFxx`,
whose commands must come from a schema, never from probing).

```powershell
pwsh -NoProfile -File tools/hid-caps.ps1 -Match "vid_0b05&pid_1afa"   # the ROG Delta II
```

Used for the Delta II's mute lamp (`docs/control-architecture.md`,
"Microphone mute and the mute lamp"): the collection declares the standard
LED usages, but the lamp doesn't follow them.
