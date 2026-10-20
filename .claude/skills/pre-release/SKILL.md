---
name: pre-release
description: Publish a release candidate (GitHub pre-release with a signed installer) for beta testers, without reaching normal users or winget — pick the code and the version, create the pre-release, guide the owner through the signing approval, check the result, hand over the link, clean up. Use when the owner says "gör en RC", "pre-release", "release candidate", "beta till testarna", or wants testers to try main or an unmerged PR before a real release.
---

What a pre-release reaches, and what it doesn't — checked against the code:

- **Built and signed like a release.** `release.yml` runs on `release: published`, pre-releases included: build, then the signing job, which waits for the owner's approval in the `release-signing` environment, then publish (installer + `latest.json`).
- **Not offered to users on the default channel.** It reads `releases/latest/download/latest.json` (`update_check.rs`), and `releases/latest` never points at a pre-release.
- **Not sent to winget.** `winget-submit.yml` only runs when publish dispatches it, and that step returns for a pre-release ("is a pre-release, not submitted").
- **Visible to anyone** on the releases page, marked **Pre-release**. **Latest** stays on the last real release, and the website's download button follows Latest.
- **Offered in the app to testers on the RC channel.** Settings → General → Updates → "Release candidates too" (#535, from 1.51.0): the newest published pre-release with a `latest.json` is offered as soon as Publish has uploaded it, so testers on that channel need no link. Everyone else is not offered it (see "Update channels" in `docs/release.md`).
- **Testers move on by themselves.** The app knows it is `X.Y.Z-rc.N` (the release workflow passes the tag's version, `update_check::APP_VERSION`) and semver puts the real `X.Y.Z` after every RC of it, so the app offers the real release when it ships, and on the RC channel the next RC before that.
- **Versions before 1.51.0** have no channel and show `Cargo.toml`'s version (an RC built before #535 said 1.50.0 in About). The installer's file name, its window title and Windows' "Installed apps" always showed the RC version.

## 1. Which code, which version

- **Code:** ask if it isn't said — `main` (what is merged) or a PR/branch (not merged yet). Use the commit **on origin**: `git fetch` then `git rev-parse origin/<branch>`. The workflow that runs is the `release.yml` **at that commit**, so a branch from before #402 signs without an approval.
- **Version:** the next real version + `-rc.N`.
  - Open release-please PR (`gh pr list --search "chore(main): release" --state open`): its version, e.g. `chore(main): release 1.49.0` → `v1.49.0-rc.1`.
  - No release PR: `.release-please-manifest.json`'s version with the patch +1, and tell the owner the real number may end up higher.
  - Earlier RCs of the same version (`gh release list | grep -- -rc.`): the next N.

## 2. Create it

```bash
gh release create v<X.Y.Z>-rc.<N> --target <sha> --prerelease \
  --title "v<X.Y.Z>-rc.<N> (release candidate)" \
  --notes "Release candidate for testers — not the latest release. <what to test>"
```

**Never without `--prerelease`.** A normal release becomes Latest: every installed app is offered it and it goes to winget. Check right after: `gh release view <tag> --json isPrerelease` must say `true`. If it says `false`, delete the release (`gh release delete <tag> --cleanup-tag --yes`) before the build gets as far as publishing.

## 3. Approval

- Find the run: `gh run list --workflow release.yml --limit 1`. Its first job must be named "Build (Windows)" (the split workflow).
- Watch it in the background until its status is `waiting`, then tell the owner: run link → **Review deployments** → tick `release-signing` → **Approve and deploy**.
- If Build fails, nothing is signed or published. Show the failing step's log (`gh run view <id> --log-failed`).

## 4. Check

- The run is `success`, and the log of "Dispatch winget submission" says `is a pre-release, not submitted`.
- `gh release view <tag> --json assets` lists `RIGStats_<X.Y.Z>-rc.<N>_x64-setup.exe` and `latest.json`.
- `gh release list --limit 3`: **Latest** is still the last real release.
- Hand over the link for the testers: `https://github.com/dvalfrid/rigstats/releases/tag/<tag>`.

## 5. Clean up

Only when the owner says so (deleting can't be undone): `gh release delete <tag> --cleanup-tag --yes`. A kept RC does no harm — it stays marked Pre-release. `download-stats.yml` counts its downloads like any release.

An `-rc` tag doesn't move Release Please: its version comes from `.release-please-manifest.json`. Seen 2026-10-16 — `v1.49.0-rc.1` was created, Release Please ran on `main` an hour later and the release PR still said 1.49.0.
