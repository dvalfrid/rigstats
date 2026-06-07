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

- [ ] `tray-icon` crate: system tray icon + context menu (Show, Settings, Quit, etc.)
- [ ] Always-on-top toggle via `eframe::WindowBuilder`
- [ ] Opacity/transparency (eframe window decorations off + clear color alpha)
- [ ] Monitor selection: port `pick_target_monitor` to winit monitor enumeration
- [ ] Window placed flush to monitor edge (DWM inset compensation, same logic as now)
- [ ] Recording indicator: swap tray icon when logging is active

**Done when:** app lands on the correct portrait monitor with the right size, tray
menu works, always-on-top and opacity behave as before.

---

### Phase 5 — Secondary windows ✅ testable
*Goal: Settings, About, Status fully functional.*

- [ ] Settings window via `ctx.show_viewport_deferred()` — four-tab layout
  - Dashboard (profile, floating mode toggle)
  - Panels (drag-to-reorder, toggle visibility)
  - Alerts (thresholds, cooldown, notify-on-crit)
  - Appearance (name, opacity, always-on-top, autostart, theme)
- [ ] About window
- [ ] Status/diagnostics window (debug log, dependency health, collect-diagnostics button)
- [ ] Updater window (check, changelog, install flow)

**Done when:** all four secondary windows open, save settings correctly, and
the dashboard reacts to changes immediately.

---

### Phase 6 — Floating mode ✅ testable
*Goal: independent per-panel windows.*

- [ ] Each visible panel → separate egui viewport (or `eframe` child window)
- [ ] Lock-positions toggle
- [ ] Positions persisted in settings (`panel_layouts`)
- [ ] Scale factor (`floating_panel_scale`) applied
- [ ] Main window hidden when floating mode is active
- [ ] Prewarm: windows created hidden at startup so first toggle is instant

**Done when:** floating mode works with independent draggable panels, positions
survive app restart.

---

### Phase 7 — Autostart, logging, auto-update ✅ testable
*Goal: operational features at parity.*

- [ ] Autostart: reuse `autostart.rs` (registry, unchanged)
- [ ] Stats CSV logging: reuse `logging.rs` (unchanged)
- [ ] Log pruning: reuse existing prune logic
- [ ] Auto-update: implement with `self_update` crate or direct GitHub API check
  (tauri-plugin-updater is gone; fetch `latest.json`, compare versions, download NSIS installer)

**Done when:** autostart toggles correctly, CSV logging writes rows, update check
finds new versions and can install them.

---

### Phase 8 — Remove Tauri, ship ✅ testable
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

> Phase 3 complete. All 10 panels implemented (header, clock, cpu, gpu, ram, net,
> disk, motherboard, process, battery). Panel visibility read from settings at startup.
> Disk page cycling and battery/ping caching ported from Tauri backend logic.
