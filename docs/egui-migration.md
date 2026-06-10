# RigStats — egui migration plan

Replaces the Tauri/WebView2 frontend with a native egui UI.
**Goal:** eliminate the 2–4 % baseline CPU cost from the Chromium rendering loop.
**Constraint:** Windows-first. Linux sensor backend is a separate future project.

## Architecture target

```
┌────────────────────────────────────┐
│        egui UI (src-egui/)         │  same code on all OS
├────────────────────────────────────┤
│           StatsPayload             │  unchanged data contract
├─────────────────────┬──────────────┤
│ Windows             │ Linux (future)│
│ LHM sidecar (pipe)  │ /sys/hwmon   │
│ WMI / winreg        │ sysfs/D-Bus  │
│ sysinfo             │ sysinfo      │
└─────────────────────┴──────────────┘
```

Key CPU trick: `ctx.request_repaint_after(Duration::from_secs(1))` — egui sleeps
completely between repaints → 0 % CPU at idle vs Chromium's constant render loop.

## Backend modules: keep vs rewrite

| File | Lines | Decision |
|---|---|---|
| `lhm.rs` | 1 864 | ✅ Keep as-is |
| `hardware.rs` | 1 390 | ✅ Keep as-is |
| `stats.rs` | 188 | ✅ Keep as-is |
| `settings.rs` | 327 | ✅ Keep as-is |
| `logging.rs` | 225 | ✅ Keep as-is |
| `debug.rs` | 71 | ✅ Keep as-is |
| `lhm_process.rs` | 32 | ✅ Keep as-is |
| `autostart.rs` | 125 | ✅ Keep (pure registry) |
| `diagnostics.rs` | 539 | ✅ Keep, minor adaption |
| `monitor.rs` | 369 | ⚠️ Profile logic kept, placement ported to winit |
| `commands.rs` | 1 632 | ⚠️ Logic kept, Tauri glue removed |
| `main.rs` | 597 | ❌ Replaced by eframe::run_native() |
| `windows.rs` | 752 | ❌ Replaced by egui viewports |
| `updater.rs` | 166 | ❌ Replaced (self_update crate or custom) |

## Phases

### Phase 1 — Scaffold + data pipeline ✅ testable
*Goal: prove the architecture works end-to-end before any UI work.*

- [x] Create `src-egui/` as a new Cargo binary in a workspace alongside `src-tauri/`
- [x] Add `eframe`, `egui` dependencies
- [x] Reuse `lhm.rs`, `hardware.rs`, `stats.rs`, `settings.rs` via workspace path dep
- [x] Spawn poll thread (1 s tick), send `StatsPayload` over `mpsc::channel` to UI thread
- [x] egui window shows raw text: CPU %, GPU %, RAM used, pipe connected yes/no
- [x] `ctx.request_repaint_after(1s)` wired up

**Done when:** app launches, shows a live-updating window with real sensor data,
Task Manager shows < 0.5 % CPU for the process at idle.

---

### Phase 2 — Core panels (CPU, GPU, RAM) ✅ testable
*Goal: establish the panel component pattern and sparklines.*

- [x] Implement `spark.rs` — ring buffer + egui `Painter` polyline drawing
- [x] `tempcolor.rs` — port JS threshold → color logic to Rust
- [x] CPU panel: load bar, sparkline, temp, freq, power
- [x] GPU panel: load bar, sparkline, temp, hotspot, clocks, VRAM, power, fan
- [x] RAM panel: used/total bar, sparkline, spec metadata
- [ ] Panel sizing follows profile height (port `compute_panels_logical_height`)

**Done when:** three panels render correctly with live data and sparklines update
every second without visual glitches.

---

### Phase 3 — Remaining panels ✅ testable
*Goal: reach full panel coverage.*

- [x] Header panel (hostname, CPU label, LHM status indicator)
- [x] Clock panel (time, date, uptime)
- [x] Network panel (up/down Mbps with sparklines, ping, interface name)
- [x] Disk panel (read/write speeds, per-drive bars with temps, page cycling)
- [x] Motherboard panel (fans, temps, voltages in Grid layout)
- [x] Process panel (top 8 processes by CPU, name + CPU % + RAM)
- [x] Battery panel (charge %, charging state, time remaining, power draw)
- [x] Panel visibility + ordering from settings applied at startup

**Done when:** full dashboard matches current feature coverage on a portrait monitor.

---

### Phase 4 — Window chrome ✅ testable
*Goal: correct placement and tray integration.*

- [x] `tray-icon` crate: system tray icon with Show/Hide + Quit menu
- [x] Always-on-top applied at startup from `window_layer` setting
- [x] Opacity via `clear_color()` alpha + `with_transparent(true)` (background transparency)
- [x] Borderless window: `with_decorations(false)`
- [x] Monitor selection: `EnumDisplayMonitors` picks portrait monitor (height > width)
- [x] Window sized from `dashboard_profile` setting (`profile_to_size()`)
- [ ] DWM inset compensation (flush-to-edge positioning, ~3 px detail, deferred)
- [ ] Recording indicator: swap tray icon when logging is active (deferred to Phase 7)

**Done when:** app lands on the correct portrait monitor with the right size, tray
menu works, always-on-top and opacity behave as before.

---

### Phase 5 — Secondary windows ✅ testable
*Goal: Settings, About, Status fully functional.*

- [x] Settings window via `ctx.show_viewport_deferred()` — four-tab layout
  - Dashboard (profile, floating mode toggle)
  - Panels (↑/↓ reorder, toggle visibility)
  - Alerts (thresholds, cooldown, notify-on-crit)
  - Appearance (name, opacity, always-on-top, autostart, logging)
- [x] About window (version, description, open logs folder)
- [x] Status window (debug log viewer, sensor service status, refresh)
- [x] Updater window (version display, check button stub — actual HTTP check in Phase 7)
- [x] All windows opened from tray menu
- [x] Settings changes (visible_panels, opacity) applied to main window immediately on save

**Done when:** all four secondary windows open, save settings correctly, and
the dashboard reacts to changes immediately.

---

### Phase 6 — Visual fidelity ✅ testable
*Goal: dashboard looks and behaves like the Tauri version.*

- [x] Window sizing: width from profile, height = sum of visible panel heights (no overflow)
  - `compute_window_height(visible_panels)` mirrors Tauri's `compute_panels_logical_height`
  - Height updates automatically when panels are toggled or profile changes
  - Panel height constants calibrated (estimates; exact values need live calibration)
- [x] Drag handle: thin strip at top of window — `ViewportCommand::StartDrag` on drag
- [x] Panel styling parity:
  - Dark background `#0B0D12` panel cards with `theme::panel_frame()` helper
  - Gradient accent line at top of each card (transparent → accent → transparent)
  - TL/BR corner bracket decorations per panel
  - Per-panel accent colours: CPU `#00c8ff`, GPU `#ff3a1f`, RAM `#ffb300`,
    Net/Clock/Battery `#39ff88`, Disk `#bf7fff`, Process `#ff9f2f`, MB `#3a99b8`
  - Progress bars: per-panel fill colour, thin (egui default)
  - Sparklines: 1.5px stroke, per-panel accent colour
  - Text: `C_TEXT` (`#b8cce8`) default, `C_TEXT_MUTED` for secondary labels,
    `C_STAT_LABEL` for grid column headers
- [x] Per-panel visual review against live screenshot — all 10 panels verified
- [x] Opacity: `SetLayeredWindowAttributes(LWA_ALPHA)` applies whole-window opacity.
  Window created without `with_transparent(true)` (avoids `WS_EX_NOREDIRECTIONBITMAP`
  conflict). `clear_color()` returns opaque; LWA_ALPHA set on first `ui()` frame and
  re-applied on settings change. Matches Tauri CSS `opacity:` behaviour (entire window
  at the chosen opacity level, not just the background).
- [x] Always-on-top live update: `ViewportCommand::WindowLevel` on settings change
- [x] Panel height constants: live-resize via `ui.min_rect().height()` every frame
  eliminates any residual gap; static constants only affect the cold start size.

**Done when:** side-by-side screenshot of egui and Tauri dashboards shows no obvious
visual differences in layout, colours, or typography.

---

### Phase 7 — Floating mode ✅ complete
*Goal: independent per-panel windows.*

- [x] Each visible panel → separate egui viewport via `show_viewport_immediate()`
- [x] Lock-positions toggle (`floating_panels_locked` setting)
- [x] Positions persisted in settings (`panel_layouts`), updated on drag, loaded at startup
- [x] Scale factor (`floating_panel_scale` 0.4–1.0) applied to window width/height
- [x] Main window moved off-screen when floating mode is active; heartbeat thread drives repaints
- [x] GPU preference changes propagated from floating GPU panel back to main app
- [x] Window-level opacity applied to all floating panel HWNDs
- [x] All panels update every ~1s regardless of focus or OS events

**Architecture note:** `show_viewport_immediate` is used (not `show_viewport_deferred`) so all
panels are rendered synchronously in the parent's frame. A heartbeat thread wakes the parent
every 950 ms → parent renders all panels → live data on every tick. CPU in floating mode:
~2–3 % (release build) vs ~1 % in fixed mode — acceptable given N independent OS windows
each with their own GPU swap chain.

---

### Phase 8 — Autostart, logging, auto-update ✅ testable
*Goal: operational features at parity.*

- [x] Autostart: reuse `autostart.rs` (registry, unchanged)
- [x] Stats CSV logging: reuse `logging.rs` (unchanged) — wired into `poll_loop` via
  `poll_stats_to_log_payload()`; reads `logging_enabled` from shared settings arc each tick
- [x] Log pruning: reuse existing prune logic — runs once per calendar day inside `poll_loop`
- [ ] Auto-update: implement with `self_update` crate or direct GitHub API check
  (tauri-plugin-updater is gone; fetch `latest.json`, compare versions, download NSIS installer)

#### Tray parity (complete alongside logging)

The egui tray is missing several items compared to the Tauri version:

- [x] Add **"Toggle Floating Mode"** menu item — toggles `floating_mode` in settings and
  triggers the same mode-switch logic as the Settings checkbox
- [x] Add **"Start Recording" / "Stop Recording"** menu item — toggles `logging_enabled`
  in settings; label flips dynamically based on current state
- [x] **Recording indicator** — swap tray icon to `tray-recording.png` (red-dot variant)
  when logging is active; restore normal icon when stopped; update tooltip to
  `"RigStats — Recording"` / `"RigStats"` accordingly
- [x] **Left single-click** on tray icon → show/hide main window (previously double-click)

#### Dialog & secondary window review pass

*Done after logging/tray are wired (so the updater window is reviewed in its final state,
not the Phase 5 stub.)*

This is a combined **functionality audit + visual polish pass** — missing features are
implemented here, not deferred. Go through every secondary window and compare against the
Tauri version for:
- [ ] **Settings** — all four tabs: Dashboard, Panels, Alerts, Appearance
  - Floating mode toggle and lock/scale controls visible/functional
  - Drag-to-reorder panels works correctly
  - All threshold fields present; battery direction (drops-below) correct
  - Autostart toggle wired (Phase 8 prerequisite)
  - Logging toggle and retention field wired (Phase 8 prerequisite)
  - Opacity slider live-previews on the main window
  - Profile selector covers all valid profiles
- [ ] **About** — version string, description, "Open log folder" button functional
- [ ] **Status** — debug log viewer shows live content, sensor service status correct,
  Refresh button works
- [ ] **Updater** — version display, "Check for Updates" button triggers real HTTP check
  (Phase 8 prerequisite), changelog rendered, install flow functional
- [ ] Visual consistency across all windows: fonts, colours, spacing match dashboard style
- [ ] All windows position sensibly relative to the tray icon or screen centre
- [ ] **Tray context menu dark mode** — call `SetPreferredAppMode(AllowDark)` via
  `uxtheme.dll` ordinal 135 at startup so Windows renders the OS context menu in dark
  mode when the system theme is dark (undocumented but standard Win32 approach)

**Done when:** autostart toggles correctly, CSV logging writes rows and can be toggled
from the tray with icon feedback, all four secondary windows are functionally and visually
complete, and update check finds new versions and can install them.

---

### Phase 9 — Remove Tauri, ship ✅ testable
*Goal: clean build with no WebView2 dependency.*

- [ ] Remove `src-tauri/` and all Tauri deps from workspace
- [ ] Update NSIS installer: no WebView2 bootstrapper needed
- [ ] Update `npm` scripts (or replace with plain `cargo build`)
- [ ] Run full verify: clippy, fmt, tests
- [ ] Measure CPU in Task Manager: target < 0.5 % at idle
- [ ] Smoke-test on a clean Windows machine (no prior Tauri install)

**Done when:** installer produces a working app with no WebView2, CPU idle < 0.5 %.

---

## Key new dependencies

| Crate | Purpose |
|---|---|
| `eframe` | App framework (window, event loop, wgpu backend) |
| `egui` | Immediate-mode UI |
| `tray-icon` | System tray + context menu |
| `self_update` | Auto-update (replaces tauri-plugin-updater) |

## What does NOT change

- `sensor-sidecar/` — untouched
- `StatsPayload` and all sub-structs — unchanged data contract
- All backend domain modules listed as "Keep" above
- NSIS installer structure (adjusted for no WebView2 bootstrapper)
- Settings file format (`rigstats-settings.json`)
- CSV log format
- GitHub Actions trigger logic (push/release)
- Azure code signing step (signs the NSIS exe regardless)
- `latest.json` generation PowerShell script
- `release-please` workflow
- Sidecar build in CI (`dotnet publish`)

## CI/CD changes in Phase 8

| Step | Now | After |
|---|---|---|
| Build command | `npm run build` (tauri build → NSIS) | `cargo build --release` + NSIS directly |
| Update signature | `npx @tauri-apps/cli signer sign` (minisign) | SHA256 checksum in `latest.json`, or minisign via Rust crate |
| Verify pipeline | `npm ci` + clippy + fmt + Rust tests + JS lint + vitest | Rust only — no npm |
| Installer | Bundles WebView2 bootstrapper | No WebView2 — smaller installer |

## Current status

> **Phase 7 complete.** Floating mode: each visible panel rendered as its own borderless
> egui viewport via `show_viewport_immediate()`. Main window moved off-screen (not hidden —
> hidden windows are not ticked by eframe). A heartbeat thread fires `request_repaint()`
> every 950 ms so the parent runs and all panels update synchronously. Per-panel drag
> handles (disabled when locked). Positions tracked via `Arc<Mutex<HashMap>>` + dirty flag,
> persisted to `panel_layouts` in settings. Scale factor from `floating_panel_scale` (0.4–1.0).
> Opacity applied to each panel's HWND via `FindWindowW` + `SetLayeredWindowAttributes`.
> GPU preference clicks in floating GPU panel propagated back to main app.
> Settings Dashboard tab: lock checkbox + scale slider shown when floating mode is enabled.
> CPU usage: ~2–3 % in floating mode (release build) vs ~1 % fixed — acceptable given
> N independent OS windows each with their own GPU swap chain.
