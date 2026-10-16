# Upstream: tray-icon can't hide the Windows tooltip

Working notes for the upstream change behind our local patch
(`third_party/tray-icon`, see its `RIGSTATS-PATCH.md`). Update **Status**
whenever something happens.

## Status

| Date | Event |
|---|---|
| 2026-10-14 | RIGStats works around it locally: #359. No upstream issue yet. |

**Next step:** once the patch is confirmed on hardware, open an issue (or a small PR) at <https://github.com/tauri-apps/tray-icon>. Daniel reviews the text before it's posted.

## The problem (verified 2026-10-14)

- `tray-icon` (0.23.1 and the latest 0.26.1) registers the notify icon with `NIM_ADD` and never calls `NIM_SETVERSION`, so the icon uses the legacy version. Without a tooltip it leaves `NIF_TIP` out.
- An app that draws its own hover popup (RIGStats' tray card, #290) can't hide the standard tooltip. With no tip, Windows 11 shows an empty tooltip box over the popup about a second after hover. With a tip, the text shows over the popup.
- The Windows way to hide it is `NOTIFYICON_VERSION_4` with `NIF_TIP` but without `NIF_SHOWTIP`. Under version 4, the callback's `lParam` carries the event in `LOWORD` and the icon id in `HIWORD`. `tray-icon` matches `lparam as u32` against mouse messages, so switching the version from outside the crate breaks its events.

## Proposed upstream change

Register in version 4 and read `LOWORD(lParam)` (as in our patch). Add `NIF_SHOWTIP` whenever a tooltip is set, so apps that use a tooltip keep it. That flag is the one difference from our patch. Optionally add a builder option such as `with_tooltip_hidden(bool)` for apps that want a tip without a tooltip.

## When upstream ships the fix

1. Bump `tray-icon` to that release.
2. Delete `third_party/tray-icon` and its `[patch.crates-io]` line in the workspace `Cargo.toml`.
3. On Windows 11, hover the tray icon and check that no empty box appears over the card. Check that left click, the right-click menu and an Explorer restart still work.
