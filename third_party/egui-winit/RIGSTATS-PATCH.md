# egui-winit 0.37.0 — RIGStats patch

Unmodified copy of `egui-winit` 0.37.0 from crates.io (upstream tag `0.37.0`,
commit `7c0b4040`) with one change, wired in through `[patch.crates-io]` in the
workspace `Cargo.toml`.

**Change:** `create_winit_window_attributes` (`src/lib.rs`) requests winit's
undecorated-window drop shadow on Windows only when the viewport doesn't opt
out with `ViewportBuilder::with_has_shadow(false)`. Upstream ignores
`has_shadow` on Windows and always asks for the shadow on an undecorated
window.

**Why:** since egui-winit 0.35 (emilk/egui#8169, "Always enable windows
undecorated shadows"), every undecorated window gets
`with_undecorated_shadow(true)`. winit then extends the DWM frame into the
whole client area, so a per-pixel-transparent window (RIGStats' DComp
dashboard, overlay, floating panels, wallpaper host) shows the light DWM
frame through every transparent pixel, with rounded corners and a border —
the dashboard turned light grey over a black desktop. Verified with a bare
eframe window (same DComp setup, 10 % dark clear colour): 0.34.3 black,
0.37.0 light grey, 0.37.0 with the shadow off black again (2026-10-20).

**Remove when** upstream honours `has_shadow` on Windows (or stops forcing the
shadow). Status and the upstream issue: `docs/upstream/egui-winit-shadow.md`.
To drop the patch: delete `third_party/egui-winit` and its
`[patch.crates-io]` line, then check that the dashboard over a black desktop
is dark, not light grey.

Licence: MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`), as upstream.
