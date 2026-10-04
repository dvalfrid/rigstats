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
[`docs/troubleshooting.md`](../docs/troubleshooting.md) for when to use it.

```powershell
pwsh -ExecutionPolicy Bypass -File tools\clean-tray-ghosts.ps1
```
