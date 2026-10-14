# tray-icon 0.23.1 — RIGStats patch

Unmodified copy of `tray-icon` 0.23.1 from crates.io with one change, wired in
through `[patch.crates-io]` in the workspace `Cargo.toml`.

**Change** (`src/platform_impl/windows/mod.rs`): `register_tray_icon` sets
`NOTIFYICON_VERSION_4` with `NIM_SETVERSION` after `NIM_ADD` (also on the
re-register after an Explorer restart), and the `WM_USER_TRAYICON` handler reads
the mouse message from `LOWORD(lParam)` (`tray_event`) — version 4 puts the
icon id in the high word.

**Why** (#359): RIGStats shows its own hover card over the tray icon and has
no native tooltip. In the legacy notify-icon version Windows 11 still shows an
empty tooltip box over the card when the icon has no tip. In version 4 the
standard tooltip is only shown with `NIF_SHOWTIP`, which is never set — so the
app can give the icon a tip ("RIGStats", for screen readers) without a tooltip
appearing.

**Remove when** upstream `tray-icon` can register in version 4 / suppress the
tooltip. Status and notes in `docs/upstream/tray-icon-tooltip.md`. To drop the
patch: delete `third_party/tray-icon` and its `[patch.crates-io]` entry, then
hover the tray icon on Windows 11 and check no empty box appears over the card.

Licence: MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`), as upstream.
