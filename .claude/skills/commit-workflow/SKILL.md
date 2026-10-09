---
name: commit-workflow
description: Full ceremony for shipping any bug fix or feature in RigStats — from opening a GitHub issue through roadmap sync. Run when starting work on a feature/bug or when ready to commit.
---

This is the mandatory sequence for every bug fix and feature. Do not skip steps or reorder them.

## 1. Open a GitHub issue before starting work

```powershell
& "C:\Program Files\GitHub CLI\gh.exe" issue create --title "..." --body "..." --label bug
# or: --label enhancement
```

`gh` is not on PATH — always use the full path above.

- **Bug:** describe the incorrect behaviour, steps to reproduce, and expected behaviour.
- **Feature:** describe the user-visible change and why it is needed.

## 2. Branch from `main`, then implement

```powershell
git switch main; git pull; git switch -c <type>/<short-name>   # e.g. fix/tray-ghost, refactor/ui-frame
```

Never commit to `main` directly. Do not branch off another unmerged branch — PRs are squash-merged, so a stacked PR conflicts after the first one lands.

## 3. Test in the running app — required before any commit

```powershell
# Kill → build → verify exe timestamp → launch
$proc = Get-Process rigstats -ErrorAction SilentlyContinue
if ($proc) { Stop-Process -Id $proc.Id -Force }
cargo build --manifest-path src-egui/Cargo.toml
(Get-Item .\target\debug\rigstats.exe).LastWriteTime   # must have advanced
Start-Process .\target\debug\rigstats.exe
```

Verify the golden path **and** edge cases. Do not commit until the fix is confirmed working in the running app — passing tests and clean clippy verify code correctness, not behaviour.

## 4. Run checks, then commit with `Closes #N`

```bash
cargo xtask fmt
cargo xtask clippy        # zero warnings required
```

Commit message format ([Conventional Commits](https://www.conventionalcommits.org/)):

```
<type>(<scope>): <subject>

<optional body>

Closes #N
```

- **type:** `feat`, `fix`, `perf`, `docs`, `refactor`, `test`, `build`, `chore`, `style`
- **scope:** lower-case area, e.g. `cpu`, `gpu`, `settings`, `wallpaper`, `status`
- **subject:** imperative, lower-case start, no trailing period
- Breaking change: `feat!:` or `BREAKING CHANGE:` footer

## 4b. Push and open a pull request

```powershell
git push -u origin HEAD
gh pr create --fill   # title = the commit subject; body ends with "Closes #N"
gh pr checks <n> --required --watch
```

The PR is ready as soon as the required check **Verify (Windows)** is green — CodeQL is not a merge gate (`docs/release.md`), don't wait for it. The owner merges (`gh pr merge <n> --squash --admin --delete-branch`); tell them it's ready.

## 5. If you forgot `Closes #N`, close the issue manually

```powershell
& "C:\Program Files\GitHub CLI\gh.exe" issue close 77 --comment "Fixed in commit abc1234."
```

## 6. If the issue is a roadmap feature — mandatory sync

When the closed issue has a `$features` entry in `tools/sync-roadmap-issues.ps1` (i.e. it has a `roadmap-id`), `Closes #N` alone is not enough. The script is the source of truth: if the entry still says `kind="planned"`, the next sync run will **re-open the finished issue**. In the same commit:

1. Flip `kind="planned"` → `kind="done"` and update `status=` text in `tools/sync-roadmap-issues.ps1`.
2. Update the matching ✅/status in `ROADMAP.md` (status overview + section heading).
3. Re-run `pwsh -NoProfile -File tools/sync-roadmap-issues.ps1` to reconcile and regenerate the table.

A red `DRIFT`/`ERROR` line means step 6 was skipped — fix the `kind` and re-run.

## 7. Update documentation for feature changes

Every feature change must also update all of these — do not wait to be asked:

| What changed | Where to update |
| --- | --- |
| New panel, data field, or backend module | `docs/architecture.md` — backend modules + renderer modules sections |
| New panel or user-visible feature | `website/index.html` — panel count in `<h2>`, panel card in `.panels-grid`, hero description if relevant |
| Feature complete or scope change | `ROADMAP.md` — mark ✓ and add implementation summary |
| New module or architectural rule | `docs/architecture.md` (or `docs/control-architecture.md` for the sidecar's Control side). Touch `CLAUDE.md` only for a rule Claude must follow in *every* session — it is loaded each time, so module descriptions don't belong there |
