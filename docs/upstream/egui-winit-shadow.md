# Upstream: egui-winit forces the undecorated-window shadow on Windows

Working notes for the upstream fix behind our local patch
(`third_party/egui-winit`, see its `RIGSTATS-PATCH.md`). Update **Status**
whenever something happens.

## Status

| Date | Event |
|---|---|
| 2026-10-20 | Found with the egui 0.37 upgrade (#200): the dashboard turned light grey. Patched locally (#544). |
| 2026-10-20 | Issue filed with the repro and the one-line patch: <https://github.com/emilk/egui/issues/8750>. |

**Next step:** wait for a maintainer reply on #8750; open the PR (draft until Daniel has reviewed it) if asked. Check with:
`gh issue view 8750 --repo emilk/egui --comments`

## The problem (verified 2026-10-20)

- egui-winit 0.37.0, `create_winit_window_attributes` (`src/lib.rs`, Windows block):
  `let want_undecorated_shadow = !decorations.unwrap_or(true) && !fullscreen.unwrap_or(false);`
  then `window_attributes.with_undecorated_shadow(want_undecorated_shadow)`. 0.34.3 never asked for the shadow.
- `ViewportBuilder::has_shadow` is destructured as `_has_shadow` and only used on macOS, so a Windows app can't opt out.
- winit's undecorated shadow extends the DWM frame into the whole client area. A per-pixel-transparent window (DComp swap chain, `with_transparent(true)`) then shows the light DWM frame through every transparent pixel, with rounded corners and a 1 px border.
- **Evidence:** a bare eframe window (`D:\Development\scratch-transparency-1`, outside the repo): DX12 `DxgiFromVisual`, `with_transparent(true)`, `with_decorations(false)`, a 10 % dark premultiplied clear colour, over a black desktop:
  - eframe 0.34.3: black.
  - eframe 0.37.0: light grey, rounded corners.
  - eframe 0.37.0 with `want_undecorated_shadow = false`: black again.
- Ruled out on the way (each A/B-tested): egui's light theme, `WS_EX_NOREDIRECTIONBITMAP` (absent in 0.34 too), eframe's hide-until-first-frame, wgpu 30's new `SetColorSpace1` call.

## Proposed upstream change

Honour `has_shadow` on Windows:

```rust
let want_undecorated_shadow = _has_shadow.unwrap_or(true)
    && !decorations.unwrap_or(true)
    && !fullscreen.unwrap_or(false);
```

(and rename `_has_shadow`, now used on two platforms). The default stays the same, so only apps that set `with_has_shadow(false)` see a change. `ViewportCommand::Decorations` sets the shadow again; whether it should also look at `has_shadow` is for the maintainer.

## When upstream ships the fix

1. Bump egui/eframe to the release that contains it.
2. Delete `third_party/egui-winit` and its `[patch.crates-io]` line in the workspace `Cargo.toml`.
3. Keep `.with_has_shadow(false)` on the undecorated viewports (startup, overlay, floating, tray card, wallpaper host).
4. Check the dashboard over a black desktop: dark, not light grey, with square corners.
