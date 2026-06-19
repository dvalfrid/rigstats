# tools/

Standalone PowerShell maintenance scripts for RIGStats. These are developer/admin
helpers — not part of the app build (build/test tasks live in `cargo xtask`).
Run them with PowerShell 7+ (`pwsh`).

## `sync-roadmap-issues.ps1`

Mirrors the roadmap to **GitHub Issues** under the `v2.0` milestone and keeps them
in sync. It is an idempotent **upsert**, not a one-shot creator.

- Source of truth is the `$features` array inside the script (id, title, summary,
  status, label, kind). `ROADMAP.md` stays the human-readable doc; its
  "GitHub issue tracking" table is the visible counterpart of the markers below.
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

## `clean-tray-ghosts.ps1`

Removes ghost/orphaned RIGStats entries from the Windows system-tray icon
settings (HKCU) and from Installed Apps / Programs & Features (HKLM) when an
uninstaller is missing. Shows what it will keep vs delete, asks for confirmation,
then restarts Explorer. Self-elevates to admin. See
[`docs/troubleshooting.md`](../docs/troubleshooting.md) for when to use it.

```powershell
pwsh -ExecutionPolicy Bypass -File tools\clean-tray-ghosts.ps1
```
