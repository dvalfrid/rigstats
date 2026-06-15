# Roadmap

Planned features in rough priority order. Each item is scoped as a self-contained release.

---

## Status overview

| Feature | Status |
| --- | --- |
| Auto-update | ✅ Done (v1.6) |
| NVMe / SSD temperatures | ✅ Done (v1.8) |
| Temperature threshold alerts | ✅ Done (v1.9) |
| Motherboard panel | ✅ Done (v1.11) |
| Extended GPU panel | ✅ Done (v1.13) |
| Customisable themes / accent colours | ✅ Done (v1.14) |
| Process monitor panel | ✅ Done (v1.15) |
| Floating panel layout | ✅ Done (v1.16) |
| Multi-GPU selector and pinning | ✅ Done (v1.19) |
| Battery panel (laptop support) | ✅ Done (v1.20) |
| Settings redesign | ✅ Done (v1.20) |
| LHM stability — sensor sidecar | ✅ Done (v1.21) |
| CPU fan speed | ⏭ Investigated, skipped |
| Stats logging / data export | ✅ Done |
| Floating panel groups | 🔲 Planned |
| Desktop background — Level 1 (HWND_BOTTOM) | ✅ Done (v1.24) |
| Desktop background — Level 2 (WorkerW) | 🔲 Planned |
| Total system power consumption | 🔲 Planned |
| Stream Deck integration | 🔲 Planned |
| Landscape monitor support | 🔲 Planned |
| egui migration — replace Tauri/WebView2 with native egui | ✅ Done (v1.27) |
| UI performance — lighter rendering strategy | ✅ Done (v1.27, via egui migration) |
| Background-only transparency (per-pixel alpha) | ⏭ Investigated, blocked — needs DirectComposition |
| Floating mode — reduce multi-window rendering cost | 🔲 Planned |

---

## Auto-update ✅

**Plugin:** `tauri-plugin-updater`
**Distribution:** GitHub Releases (existing pipeline)

**Implemented.** On startup the app silently checks for updates after a 10-second
delay, then every 6 hours (handles sleep/wake). If a newer version is available
a badge appears in the dashboard header. Clicking the badge (or "Updates & Changelog"
in the tray menu) opens the updater dialog showing the new version's release notes
from GitHub, the full local version history, and a download progress bar.
After installation the app restarts automatically; the About window opens on the
first launch following an upgrade.

---

## NVMe / SSD temperatures ✅

**Panel:** Disk
**Data source:** LHM `Temperatures` section per storage device

**Implemented.** Each drive in the disk panel now shows a live temperature reading
in °C, color-coded by `resolveTempColor` (warm at 55 °C, hot at 70 °C).

LHM sensor identification uses the `SensorId` field (`/nvme/`, `/hdd/`, `/ata/`, `/scsi/`, `/ssd/`
prefixes) rather than sensor names, so motherboard and RAM thermal sensors are never
mixed in with disk readings. Warning Composite and Critical Composite threshold
sensors are excluded; the highest real temperature per device is shown.

Drive-letter-to-model mapping is resolved at startup via a WMI three-table join
(`Win32_DiskDrive → Win32_DiskDriveToDiskPartition → Win32_LogicalDiskToPartition`),
with a PowerShell CIM fallback. Temperatures are matched by model name (case-insensitive
substring match), so inserting a USB drive never shifts temperatures to the wrong
drive.

---

## Temperature threshold alerts ✅

**Panel:** Settings (new threshold fields) + tray notifications
**Data source:** Existing CPU / GPU / RAM / disk temp fields

**Implemented.** A configurable alert system fires a Windows tray notification when
a component exceeds its threshold, making the app useful during gaming or overclocking.

Thresholds are stored as `Settings.thresholds: HashMap<String, ComponentThresholds>`
where `ComponentThresholds { warn: Option<u8>, crit: Option<u8> }` and keys are
`"cpu"`, `"gpu"`, `"ram"`, `"disk"`. `None` = disabled. A `settings_version: u8`
sentinel handles one-time migration from the original eight flat `Option<u8>` fields
to the map format, preserving existing user values.

Per-tick comparison runs in `commands.rs` inside `get_stats()` after the
`StatsPayload` is assembled. Warning and Critical are checked independently —
each has its own cooldown key (e.g. `"cpu_warning"` vs `"cpu_critical"`)
stored in `AppState.last_alert`. Disk alerts fire on the hottest drive's temperature.
Notifications are sent via `tauri-plugin-notification`; errors are silently discarded
so a failed toast never disrupts the stats tick.

The Settings window has a compact "Temp Alerts" card with number inputs for all
eight thresholds. Blank = disabled (maps to `None`). Yellow column headers for
Warning, red for Critical.

---

## CPU fan speed ⏭

**Panel:** CPU

After investigation across real user LHM data: CPU cooler fans are wired to the
motherboard Super I/O chip and appear as generic `Fan #N` channels alongside all
other chassis fans. LHM provides no signal that identifies which channel is the CPU
cooler. A highest-RPM heuristic was considered but rejected as unreliable (pump
heads, high-RPM case fans, and AIO radiator fans all exceed chassis fan RPM on some
builds). CPU cooler fan speed is instead available in the **Motherboard panel**
alongside all other fan channels.

---

## Motherboard panel ✅

**Panel:** New `motherboard` panel
**Data source:** LHM Super I/O chip node (`/lpc/` SensorId prefix) + WMI `Win32_BaseBoard`

**Implemented.** An optional panel showing the sensors exposed by the motherboard's
Super I/O chip (Nuvoton NCT6799D, ITE IT87xx, Winbond W836xx, etc.) alongside the
detected board name. Useful for monitoring system cooling and voltage rails without
opening the BIOS.

The panel is opt-in (off by default) and enabled via Settings → panel toggles.

**What is shown:**

- **Board name** (e.g. "ASUS PRIME B650M-A AX6 II") — detected at startup via WMI
  `Win32_BaseBoard`; manufacturer normalized (ASUSTeK → ASUS, Micro-Star → MSI, etc.)
- **Super I/O chip name** (e.g. "Nuvoton NCT6799D") — the `grandparent` of the first
  `/lpc/` sensor node
- **Fans:** all active channels in RPM, sorted descending; 0-RPM channels hidden
- **Temperatures:** readings ≥ 5 °C (LHM sentinel value filtered); unnamed channels
  displayed as T1–T6, named channels (e.g. "CPU Core") shown as-is
- **Voltages:** named rails only (`Vcore`, `AVCC`, `+3.3V`, `CPU Termination`, etc.);
  generic `Voltage #N` unmapped slots excluded

**Extraction strategy:** `/lpc/` SensorId prefix is chip-agnostic and works across
all Super I/O models without hardcoding chip names or sensor indices. The same
approach is used for disk temperature matching.

---

## Extended GPU panel ✅

**Panel:** GPU
**Data source:** LHM sensors already fetched each tick

The GPU panel currently shows load, temperature, VRAM used/total, and core clock.
LHM exposes several additional metrics that are already present in the flat sensor
tree but not yet surfaced in the UI.

**Implemented.** Stats grid extended to four columns: TEMP · HOT · FREQ · POWER. Hotspot
temperature is coloured with its own warn/crit thresholds (90 °C / 105 °C defaults). Bar
section now shows two rows — VRAM + 3D (left/right) and FAN + VDEC (left/right) — when D3D
data is present; VDEC is rendered dimmed at 0 % when the driver does not expose a VDEC
sensor. Without D3D data, VRAM and FAN display full-width. Panel proportions are preserved at
all scale factors via `allocate_exact_size` + `new_child` clipping.

---

## Multi-GPU selector and pinning ✅

**Panel:** GPU  
**Data source:** LHM sensor tree (`/gpu-*` SensorId family + GPU data/load fallbacks)

**Implemented.** Systems with both iGPU and dGPU can now pin which GPU is shown
in the GPU panel. Small selector dots appear next to `GPU LOAD` in both fixed
and floating modes, with tooltips that show the full GPU name.

**Behavior:**

- Selector is shown when multiple GPU candidates are available
- Clicking a selector dot calls `set_gpu_preference` and persists
  `Settings.preferred_gpu`
- Backend uses preferred GPU when available (case-insensitive fuzzy match)
- Without preference, backend uses a stable default (highest VRAM, tie-break by load)
  to avoid per-tick auto-switching

**Implementation notes:**

- Added `GpuStats.available_gpus` for renderer selector metadata
- Added `LhmData.gpu_devices` extracted from multi-sensor GPU candidates
- Added ACL permission + window capability entries for `set_gpu_preference`
- Added parser tests for exact/fuzzy preferred GPU matching and default stability
- Added frontend tests for selector model/markup helpers

---

## Customisable themes / accent colours ✅

**Panel:** Settings (new Appearance card) + CSS custom properties across all panels

**Implemented.** All accent colours are expressed as CSS custom properties driven
by a single theme key. The Settings window exposes an "Appearance" card with five
built-in presets; the selection previews live and is persisted across restarts.

Five presets: Dark Cyan (default), Amber, Green, Purple, Slate. Each preset
derives the full accent palette — borders, backgrounds, scrollbar tints, grid
overlay — plus tonal variants for section headers (`--stat-label`), meta-key
labels (`--text-muted`), and motherboard column headers (`--mb-accent`) using
HSL hue extraction, so all text stays tonally consistent with the active theme
without sharing the exact accent colour.

**What was done:**

- Audited and replaced all hardcoded colour values in `frontend/` with CSS custom
  properties (`--accent`, `--accent-border`, `--accent-bg`, `--accent-bg-thin`,
  `--accent-scrollbar`, `--accent-grid`, `--stat-label`, `--text-muted`, `--mb-accent`)
- `renderer/themes.js` — pure colour-conversion helpers (`hexToRgba`, `hexToHsl`,
  `hslToHex`) and `applyTheme(key)` that sets all CSS variables in one call
- Appearance card added to the Settings window; live preview via `preview-theme`
  Tauri command; restores original on cancel
- Theme key persisted in `Settings` struct (`String`, default `"dark-cyan"`,
  `#[serde(default)]` for backwards-compatible JSON evolution)
- `apply-theme` event emitted to the main window after `save_settings`
- `renderer/themes.test.js` — 16 tests covering preset enumeration, hex↔HSL
  round-trip accuracy, and derived-colour saturation invariants

---

## Process monitor panel ✅

**Panel:** New `process` panel (opt-in)
**Data source:** `sysinfo::Process` — CPU %, memory used, name

**Implemented.** An optional panel showing the top 8 processes sorted by CPU
usage — a miniature Task Manager always visible on the portrait monitor.
Enabled via Settings → panel toggles.

**What is shown:**

- Top 8 processes sorted by CPU % (descending)
- Columns: process name (truncated to 16 chars, `.exe` suffix stripped), CPU %
  of total system capacity, RAM in MB or GB
- Auto-refreshes on every stats tick (1 s interval)

**Implementation:**

- `ProcessEntry` struct in `stats.rs`: `name`, `cpu` (% of total system), `mem_mb`
- `StatsPayload.top_processes: Vec<ProcessEntry>` — sorted and truncated to 8
  before serialisation in `get_stats()` in `commands.rs`
- `sysinfo::System::refresh_processes()` called each tick alongside CPU/RAM refresh
- CPU % = `process.cpu_usage() / num_cpus` so 100 % means fully loaded system
- `panels/process.js` — pure helper functions `truncateName` and `formatRam` are
  exported and covered by unit tests; process names are HTML-escaped before
  insertion into `innerHTML` to prevent XSS from adversarial process names
- `"process"` added to the valid panel key list in `monitor.rs` and `settings.js`

---

## Battery panel (laptop support) ✅

**Panel:** New `battery` panel
**Data source:** WMI `Win32_Battery` (sysinfo 0.30 has no battery API)

Relevant for gaming laptops (ASUS ROG, Razer, Alienware). Shows charge %, status
(CHARGING / DISCHARGING), and estimated time remaining. The panel renders a
"NO BATTERY" state on desktops — always safe to enable.

**Implemented:**

- WMI `Win32_Battery` query in `hardware.rs`: `sample_battery_wmi()` returns
  `(charge_pct, is_charging, time_remaining_mins, power_w)` or `None` when no battery
  present. `EstimatedRunTime == 71582788` (Windows sentinel) is filtered to `None`.
  A second query against `root\wmi BatteryStatus` provides `ChargeRate` /
  `DischargeRate` in mW, converted to watts for `power_w`.
- Battery sampled every 10 s via a cache in `AppState.last_battery_sample` — frequent
  enough to catch charger connect/disconnect, avoids WMI overhead every tick.
- `BatteryStats { present, charge_pct, charging, time_remaining_mins, power_w }` struct
  in `stats.rs`, included in `StatsPayload`.
- `panels/battery.js` — charge bar colour adapts: accent when charging, green > 50 %,
  amber 20–50 %, red < 20 %. POWER field colour-coded by discharge rate: green < 12 W,
  amber 12–20 W, red > 20 W (no colour when charging). Shows "NO BATTERY" when
  `present == false`.
- Floating panel: `panel-battery.html` (256 px, standard drag handle).
- `"battery"` added to valid panel keys in `monitor.rs`, `windows.rs`, `settings.js`,
  `app.js`, and `panel-host.js`.

---

## Settings redesign ✅

**UI:** Four-tab layout replacing the previous two-column scroll.

**Implemented.** The Settings window was reorganised from a tall two-column layout
into a compact 560×560 tabbed interface that scales cleanly as new settings are added.

**Tabs:**

| Tab | Content |
| --- | --- |
| Dashboard | Display profile, floating mode + panel scale |
| Panels | Drag-to-reorder panel list with visibility toggles |
| Alerts | Notification cooldown, notify-on-critical toggle, temperature thresholds (CPU/GPU/RAM/Disk), battery charge thresholds |
| Appearance | Rig name, opacity, always-on-top, autostart, theme |

The active tab is remembered across sessions in `localStorage`. The window is
centered on the monitor that contains the tray icon using DPI-aware coordinate
conversion (`center_on_tray_monitor`) — the previous `tray_anchor_position`
approach mixed physical and logical pixel coordinates and placed the window
off-screen on scaled displays.

Battery charge alert thresholds were added alongside the redesign. The backend
fires a Windows notification when the battery charge drops below a configurable
percentage while discharging. Semantics are reversed from temperature alerts —
the warn threshold must be *above* the crit threshold (e.g. warn at 20 %, crit at 10 %).
Validation enforces this constraint in `save_settings`. Default thresholds: warn 20 %, crit 10 %.
Warning-level notifications are permanently disabled (`notify_on_warn` is always
sent as `false`); only Critical alerts have a user-visible toggle.

---

## Stats logging / data export ✅

**Panel:** Settings (new Logging card, Dashboard tab) + tray menu shortcut
**Data source:** Existing `StatsPayload` — no new sensors required

Lets overclockers and benchmark enthusiasts record hardware metrics over time and
analyse them after a gaming session or stress test.

**Architecture:**

Logging runs as an opt-in background task inside the Rust backend. When enabled,
each `get_stats()` tick appends a CSV row to a rolling log file in the Tauri app
data directory (`rigstats-log-YYYY-MM-DD.csv`). Log files roll daily and are
automatically pruned once per calendar day based on file modification time.

**What is logged (one row per tick):**

`timestamp_unix, cpu_load, cpu_temp, cpu_freq_mhz, gpu_load, gpu_temp, gpu_vram_used_mb, ram_used_gb, disk_read_mbs, disk_write_mbs, net_up_mbps, net_down_mbps, ping_ms`

Note: disk throughput is in MB/s (from LHM); network throughput is in Mbps (from sysinfo).
Optional fields (cpu_temp, gpu_*, ping_ms) are blank when the sensor is unavailable.

**Implemented:**

- `logging.rs` — `append_stats_row(&StatsPayload, dir)`, `prune_old_logs(dir, days)`,
  `current_log_path(dir)`, and `ymd_from_unix` (Howard Hinnant algorithm, no chrono dep)
- `AppState.last_log_prune_day` — throttles directory scan to at most once per calendar day
- `Settings.logging_enabled` (default `false`) and `Settings.log_retention_days` (default 7)
- Settings → Dashboard tab: "Stats Logging" card with enable toggle, retention selector
  (1 / 7 / 30 days), and "Open Log Folder" button
- Tray menu: "Start Recording" / "Stop Recording" item for quick toggle without opening Settings
- Tray recording indicator: icon swaps to a red-dot variant while recording; tooltip shows "RIGStats — Recording"
- `apply_tray_logging_indicator` rebuilds the tray menu and swaps the icon atomically on each toggle
- `open_log_folder` Tauri command opens `%APPDATA%\se.codeby.rigstats\` in Explorer
- `assets/tray-recording.png` — 32×32 recording-state tray icon (original icon + red dot, bottom-right corner)

---

## Floating panel layout ✅

**Panel:** All panels + new window management
**Data source:** Existing stats tick — no new sensors required

Portrait mode stays as-is. A new "Floating" mode (toggled in Settings) hides the
main portrait window and opens each visible panel as its own frameless,
always-on-top Tauri window. Panels can be placed anywhere across any number of
monitors and remember their positions across restarts. CPU on one screen, GPU on
another, disk hidden entirely — fully under user control.

Note: this supersedes the planned "Overlay mode" entry. Floating panels with
corner positioning and `set_ignore_cursor_events` cover that use case as a subset.

**Architecture:**

Settings gains three new fields: `floating_mode: bool`,
`floating_panel_scale: f64`, and `panel_layouts: HashMap<String, PanelLayout>` where
`PanelLayout { x: i32, y: i32 }` stores the last known position for each panel
key. All three fields use `#[serde(default)]` so existing settings files load
cleanly without migration logic.

Each panel is a separate Tauri window created via `WebviewWindowBuilder` in
`windows.rs`, with `.decorations(false)`, `.always_on_top(true)`, and
`.skip_taskbar(true)`. Window labels follow the scheme `"panel-cpu"`,
`"panel-gpu"`, etc. The DWM invisible resize border compensation already
implemented in `pick_target_monitor()` in `monitor.rs` applies here too — saved
positions are adjusted by the inset before calling `set_position`.

To harden mode transitions, panel sync is scheduled through
`spawn_sync_floating_panels` (main-thread dispatch + queue coalescing) and
`close_floating_panels` now hides windows instead of destroying them. This avoids
WebView2 create/destroy churn during rapid toggles and keeps transitions stable.

Each panel loads its own HTML file (`panel-cpu.html`, `panel-gpu.html`, etc.)
containing only that panel's DOM structure — a copy of the relevant section from
`index.html`. No changes to the existing panel JS modules are required.

A new `renderer/panel-host.js` serves as the shared entry point for all floating
panel windows. It detects which panel it hosts (from the window label), subscribes
to the `stats-broadcast` Tauri event, and calls the corresponding panel update
function on each tick. It also applies `apply-theme` and `apply-opacity` events so
panels stay in sync with settings changes.

Stats delivery: the main window (hidden in floating mode) continues its `tick()`
loop as before, calling `get_stats()` once per second. After receiving the payload
it calls `broadcast_stats(stats)` — a thin Tauri command that calls
`app.emit("stats-broadcast", stats)`, broadcasting to all open panel windows.
Only one stats collection runs per second regardless of how many panels are open.

Each panel has a drag handle bar at the top (`data-tauri-drag-region`). When the
user stops dragging, `save_panel_positions` is called — a Tauri command that reads
the current `outer_position()` of every open panel window and writes them to
settings. Right-clicking the drag handle shows a small context menu: "Open
Settings" and "Close panel".

**Scope:**

- `PanelLayout { x: i32, y: i32 }` struct + `Settings.floating_mode` +
  `Settings.panel_layouts` in `settings.rs`
- `launch_floating_panels(app, state)`, `spawn_sync_floating_panels(app)`, and
  `close_floating_panels(app)` in `windows.rs` — opens/reconciles/hides windows
  per entry in `visible_panels`
- New Tauri commands: `toggle_floating_mode`, `broadcast_stats`,
  `save_panel_positions`, `preview_floating_scale`
- One HTML file per panel (7–9 new files) + `renderer/panel-host.js`
- Settings window: "Floating mode" toggle and "Panel Scale" slider in a new
  Layout card
- Tray menu: "Floating mode" shortcut to toggle without opening Settings

---

## Floating panel groups 🔲

**Panel:** Floating panel layout (requires the above feature)
**Data source:** No new data required

Builds on floating panel layout. Panels can be snapped together magnetically while
dragging — when a panel edge comes within 20 px of another panel's edge, a snap
preview highlights the target; releasing the mouse joins them into a group. All
panels in a group move together as a unit. Groups can be oriented vertically or
horizontally. A "Collect panels" tray command gathers all floating panels into a
vertical stack on a chosen monitor.

**Architecture:**

Settings gains `panel_groups: Vec<GroupLayout>` where
`GroupLayout { members: Vec<String>, orientation: String }` — `orientation` is
`"vertical"` or `"horizontal"`. An empty vec means no groups. Panels not listed in
any group are free-standing.

Snap detection runs in `panel-host.js` during drag: `outerPosition()` is polled at
pointer-move rate and compared against sibling panel positions fetched once at drag
start. When a snap candidate is found within the threshold, a CSS outline preview
appears on the target panel. On `mouseup`, if a snap candidate is active, a
`set_panel_group` command writes the updated group membership to settings; both
panels are then re-positioned so their edges align flush.

Moving a group: when the user starts dragging any group member, `panel-host.js`
reads the group membership from settings and calls `move_panel_group(label, dx, dy)`
on `mousemove` — a Tauri command that applies the same delta to all sibling windows
via `set_position`, keeping the group locked together.

Group orientation is toggled via right-click → "Flip group orientation"; the
command re-stacks the group members in the new direction and saves the updated
`GroupLayout`. Right-click → "Detach from group" removes the panel from its group
and saves; remaining members keep their current positions.

"Collect panels to screen": a tray submenu lists available monitors by name.
Selecting one calls `collect_panels_to_monitor(monitor_index)` — a Tauri command
that re-stacks all open panel windows vertically on the chosen monitor in
`visible_panels` order, then saves the new positions.

**Scope:**

- `GroupLayout` struct + `Settings.panel_groups` in `settings.rs`
- Snap detection and drag-group logic in `renderer/panel-host.js`
- New Tauri commands: `move_panel_group`, `set_panel_group`,
  `collect_panels_to_monitor`
- Group orientation toggle + "Detach from group" in the drag-handle context menu
- "Collect panels" tray submenu with per-monitor options

---

## Floating mode — reduce multi-window rendering cost 🔲

**Area:** Floating panel rendering (`src-egui/src/main.rs::render_floating_panels`)
**Data source:** No new data required — pure rendering/architecture work

### Problem

In floating mode each visible panel is rendered as its **own borderless OS
window** via `ctx.show_viewport_immediate()` (one viewport per panel key). With
`show_viewport_immediate`, every child viewport is re-rendered **synchronously as
part of the parent frame**: on each parent repaint we re-tessellate and present N
separate swapchains (N = number of visible panels), plus the off-screen parent
window itself still paints.

Measured cost (debug build, 6 panels, `behind` layer, idle):

| Mode | Process CPU |
| --- | --- |
| Fixed (single portrait window) | ~3 % total / ~0.5 % at true idle |
| Floating (6 separate viewports) | ~24 % of one core (~1.5 % total) after the fixes below |

Release builds are substantially cheaper (tessellation is the dominant cost and
is ~5–20× faster optimised), but floating mode is still inherently several times
more expensive than fixed mode because the work scales linearly with panel count.

### Already done (don't redo these)

These low-hanging wins are already merged — the remaining cost is structural:

- **Behind-mode Z-order throttle** (`BehindEnforce` in `main.rs`): `behind`
  panels used to re-assert `WindowLevel(AlwaysOnBottom)` + `SetWindowPos` every
  frame, and because each `send_viewport_cmd`/`SetWindowPos` schedules a repaint
  this span the panels at the monitor's full refresh rate. Now throttled to
  creation + a 400 ms post-drag burst + ~1/s idle. This alone cut floating CPU
  from ~5.6 % to ~1.5 % total. See `win32_behind.rs`.
- **Per-frame `InnerSize` guard** (fixed mode): no longer dispatches a resize
  command every frame.
- **Idle repaint rate** is already 1 fps (heartbeat thread + `request_repaint_after(1 s)`).

### Approaches to investigate (in rough preference order)

1. **Switch to `show_viewport_deferred` for idle panels.**
   Deferred viewports render on their **own** schedule rather than inside every
   parent frame, so panels that haven't received new data don't get re-tessellated
   when an unrelated panel repaints. The current code deliberately uses *immediate*
   so the 1 fps parent heartbeat drives all panels in lockstep; a deferred design
   would instead give each panel viewport its own `request_repaint_after(1 s)` and
   push new stats via `ctx.request_repaint()` only when a fresh `PollStats` arrives.
   Risk: deferred viewports run their own UI closure with no direct `&mut self`
   borrow — shared state (`latest`, sparklines, thresholds, textures) must move
   behind `Arc`/channels. This is the biggest change but the most correct fix.

2. **Skip re-rendering panels whose data is unchanged.**
   The poll loop emits one `PollStats` per second; between ticks nothing changes.
   Track a per-panel content hash (or simply a "new data this tick" flag) and call
   `request_repaint` only for the affected viewports. In immediate mode this is
   awkward (all children render with the parent); pairs naturally with approach 1.

3. **Stop painting the off-screen parent window.**
   In floating mode the main window is parked at `(-32000, -32000)` but still
   clears + presents a swapchain every frame (it can't use `Visible(false)` — a
   hidden window isn't ticked, which would freeze the immediate children). Explore
   whether the parent can present a 1×1 / minimal surface, or whether a deferred
   design (approach 1) lets the parent stop ticking entirely once children own
   their own repaint schedules.

4. **Cache tessellated panel meshes.**
   egui re-tessellates from scratch each frame. For static-ish panels a cached
   `egui::Shape`/mesh keyed on content could skip tessellation. High effort, egui
   doesn't expose this cleanly today — likely not worth it versus approach 1.

### Scope / files

- `src-egui/src/main.rs` — `render_floating_panels`, the heartbeat thread, the
  `RigStatsApp` shared-state fields (would need `Arc`-wrapping for deferred mode).
- `src-egui/src/win32_behind.rs` — behind-mode enforcement already throttled;
  re-check interaction with a deferred design.
- Panels themselves (`src-egui/src/panels/*.rs`) should need **no** changes —
  they already take all inputs as `&` parameters and return an `egui::Rect`.

### Acceptance criteria

- Floating mode idle CPU materially lower than today (target: within ~2× of fixed
  mode rather than ~8×), measured the same way (`TotalProcessorTime` delta over
  10 s, debug build, 6 panels, `behind` layer).
- No regression to: drag-to-move, padlock lock/unlock, per-panel position
  persistence, `behind`/`on_top`/`normal` window layers, live settings preview,
  or the GPU selector on the floating GPU panel.
- 1 fps visual update rate preserved (no frozen panels after settings changes or
  mode toggles).

### When to do this

After the current correctness/stability pass. This is a performance refinement,
not a bug — floating mode is fully functional today, just heavier than fixed mode.
Prioritise if users commonly run many floating panels on battery-powered laptops.

---

## Desktop background mode — Level 1 (HWND_BOTTOM) ✅

**Panel:** Main window + floating panels
**Data source:** No new data required

Adds a third window-layer option alongside the existing "Normal" and "Always on top"
modes. In "Always behind" mode the dashboard sits below all other windows — visible
when the desktop is clear but automatically covered whenever another app is in focus.
Behaves like a classic Windows desktop gadget from Windows Vista/7.

**Pros:**

- Non-intrusive — never overlaps work windows; visible only when the desktop is exposed
- Simple Win32 implementation: a single `SetWindowPos(hwnd, HWND_BOTTOM, ...)` call
- No undocumented APIs or Explorer dependency
- Works for both the portrait main window and individual floating panels
- Compatible with existing always-on-top and floating-mode settings

**Cons:**

- Does **not** survive `Win+D` (Show Desktop) — Windows minimises all non-desktop
  windows, including HWND_BOTTOM windows, when the user presses Win+D
- The window can temporarily flicker to a higher z-order during monitor changes or
  DWM redraws; a `WM_WINDOWPOSCHANGING` hook is needed to suppress this
- Clicking on the dashboard area while it is "behind" requires all other windows to
  be moved first — the window itself cannot be brought forward without toggling the mode

**Implemented.** The "Always on top" checkbox in Settings → Appearance is replaced with
a three-way **Window Layer** selector: Normal / Always On Top / Always Behind.

**Architecture:**

`Settings.window_layer: String` (`"normal"` | `"on_top"` | `"behind"`) replaces the
old `always_on_top: bool`. Old settings files are migrated on load: `always_on_top: true`
maps to `"on_top"`.

`apply_window_layer(window, layer)` in `windows.rs` handles all three states:

- `"on_top"` → Tauri's `set_always_on_top(true)`
- `"behind"` → `set_always_on_top(false)` then `SetWindowPos(hwnd, HWND_BOTTOM, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)` via the `windows 0.61` crate
- `"normal"` → `set_always_on_top(false)`

Z-order is re-applied in `on_window_event` when the main window receives `Focused(true)`
while in `"behind"` mode, self-healing any drift from DWM restarts. The `WM_WINDOWPOSCHANGING`
subclass hook is not implemented — focus-based re-pinning is sufficient for practical use.

Applies to both the main portrait window and all floating panel windows. In floating mode,
`launch_floating_panels` and `sync_floating_panels` read `window_layer` from settings and
call `apply_window_layer` on each panel window at creation and on every sync.

---

## Desktop background mode — Level 2 (WorkerW) 🔲

**Panel:** Main window + floating panels
**Data source:** No new data required

Makes the dashboard a true part of the wallpaper layer — living between the
desktop wallpaper and the desktop icons. Survives `Win+D`, is never covered by
any normal window, and appears below even desktop icons. This is the technique
used by Wallpaper Engine and similar tools.

**Pros:**

- Genuinely part of the background — survives `Win+D`, screen switches, and
  window maximize/fullscreen operations
- Never accidentally visible above game fullscreen windows (unlike HWND_BOTTOM
  which can flicker)
- Dashboard is accessible without any window management — it is always "there"

**Cons:**

- Relies on undocumented Windows internals: finding the `WorkerW` child of
  `Progman` via `SendMessageTimeout(0x052C)` and reparenting into it. Microsoft
  could remove or change this behaviour in any Windows update
- When Explorer crashes and restarts, the WorkerW hierarchy is rebuilt —
  the dashboard process must detect this (via `WM_SHELLHOOKMESSAGE` or polling)
  and re-parent itself
- WebView2 as a child of WorkerW has known rendering edge cases: some GPU
  compositing modes draw incorrectly, and hardware-accelerated WebView2 may
  not composite cleanly in the wallpaper layer
- Mouse input is not forwarded to WorkerW children by default — click-through
  to the desktop is expected, but interactive elements (GPU selector dots,
  drag handles) would stop working unless the window is temporarily un-parented
- Incompatible with floating panel mode (each floating window would need its own
  WorkerW reparenting and input-forwarding solution)
- Significantly harder to test across Windows 10 / 11 versions

**Architecture:**

On mode activation, the app:

1. Sends `SendMessageTimeout(progman_hwnd, 0x052C, 0, 0, ...)` to Progman to
   force creation of the split `WorkerW` sibling
2. Enumerates top-level windows to find the `WorkerW` that sits *behind* the
   desktop icon layer (identified by checking for a `SHELLDLL_DefView` child)
3. Calls `SetParent(dashboard_hwnd, workerview_hwnd)` to reparent the window
4. Subscribes to shell hook messages to detect Explorer restarts and re-parent

Input handling: because WorkerW does not relay mouse events, a separate
`WH_MOUSE_LL` low-level hook captures clicks in the dashboard's bounding rect
and injects them directly via `PostMessage`.

**Scope:**

- Win32 interop in `windows.rs`: Progman discovery, WorkerW enumeration,
  `SetParent`, shell hook registration
- Explorer restart watchdog (polling or `WM_SHELLHOOKMESSAGE`)
- Low-level mouse hook for interactive elements in wallpaper mode
- Settings toggle (mutually exclusive with floating mode)
- Comprehensive testing on Windows 10 21H2, Windows 11 22H2 and 24H2

---

## Stream Deck integration 🔲

**Crate:** [`elgato-streamdeck`](https://crates.io/crates/elgato-streamdeck) — talks directly to the Stream Deck hardware over USB HID

Lets streamers and content creators display live hardware stats — CPU load, GPU
temp, VRAM, fan RPM — directly on Stream Deck keys. No Elgato software, no
separate plugin, no HTTP server: RIGStats owns the device entirely.

**Architecture:**

The `elgato-streamdeck` crate wraps `hidapi` and communicates directly with the
USB HID interface. RIGStats detects connected Stream Deck devices on startup,
renders metric values as button images, and pushes them to the device on every
stats tick alongside the normal dashboard update.

**Trade-off:** because HID devices can only be held by one process at a time,
the official Elgato Stream Deck software must not be running simultaneously.
Users who rely on Elgato's software for other profiles/macros cannot use both
at once. This should be clearly communicated at setup time.

**Scope:**

- Add `elgato-streamdeck` (+ `hidapi`) to `Cargo.toml`
- Detect connected Stream Deck devices at startup; store handle in `AppState`
- New `streamdeck.rs` module: `render_key(metric, value, unit) → image`,
  `push_stats(device, &StatsPayload, layout)` called from the stats tick
- Per-key layout configured in Settings: pick metric (CPU load/temp/power,
  GPU load/temp/VRAM, RAM used, disk read/write, ping) and colour thresholds
- Brightness and layout persisted in `Settings`
- Stream Deck integration is opt-in (off by default); auto-disabled when no
  device is detected so the crate has zero overhead on systems without one

---

## Total system power consumption 🔲

**Panel:** Header or dedicated power row in CPU/GPU panel
**Data source:** LHM sensor tree (built-in sensors only — no external hardware required)

Shows how much power the computer draws in total, in real time.

**Status by platform:**

- **Laptops:** Already solved. Battery discharge rate (`power_w` in the battery panel)
  is the total system power draw when running on battery. PSU losses don't apply —
  the battery measures what the whole system actually consumes.
- **Desktops:** Requires investigation per machine. Two approaches, in priority order:

### Approach 1 — Motherboard power sensor (preferred)

Some motherboards expose a total VRM input power or "System Power" sensor via their
Super I/O chip or dedicated power management IC (e.g. ASUS DIGI+ VRM, MSI MEG sensors).
If present, this appears in the LHM sensor tree under the motherboard hardware node as a
`Power` sensor type. This is the most accurate built-in reading — it covers CPU, RAM,
and other VRM-fed components.

Implementation: scan `SensorReader.cs` motherboard extraction for `SensorType.Power`
sensors under the `/lpc/` node and surface the highest one as `system_power_w` in
`SensorPayload`. No new data collection needed — if the sensor is there, it's free.

### Approach 2 — Component sum estimate (fallback)

When no motherboard power sensor is available, sum the known component readings:

```text
estimated_total = cpu_package_w + gpu_power_w + (dram_power_w if available) + fixed_overhead_w
```

`fixed_overhead_w` covers fans, storage, USB devices, and MB standby (typically 20–40 W
on a desktop). The result is labelled clearly as an estimate (`~XXX W`) rather than a
measured value. Accuracy: roughly ±20 % of actual wall power (excludes PSU efficiency
losses which are 10–15 % on a typical 80 Plus Gold unit).

Intel CPUs expose DRAM power via RAPL (`/intelcpu/N/power/2`). AMD SMU may expose it
on some platforms. LHM already reads these — `SensorReader.cs` just needs to extract
and include them.

**Investigation step first:**

Before implementing, collect `sensor-tree.txt` from a representative desktop and check
for `Power` sensors under the `Motherboard` hardware node. If present, approach 1 is
sufficient. If absent, approach 2 is the fallback.

**Scope:**

- `SensorReader.cs`: extract MB power sensor if present; extract DRAM power if available
- `SensorPayload`: add `system_power_w: float?` (measured) and `dram_power_w: float?`
- `StatsPayload` / `stats.rs`: surface as `system_power_w` with fallback sum logic
- Frontend: display in header panel or as a new row in the CPU panel

---

## Landscape monitor support 🔲

**Panel:** All panels + profile system
**Data source:** No new data required

The app currently assumes a portrait secondary monitor. Users with a landscape
secondary display (or a wide ultrawide primary they want to dedicate a strip of)
have no way to use the app today. Landscape profiles would also unlock tabletop
or wall-mounted dashboard builds where the monitor is rotated horizontally.

**Architecture:**

Profiles are extended with an orientation field. Landscape profiles use a
horizontal flow layout: panels are arranged left-to-right in columns rather than
stacking top-to-bottom. CSS custom properties (`--layout-direction`,
`--panel-width`, `--panel-height`) drive the layout so the same panel JS modules
work unmodified. A new set of landscape profile names is added alongside the
existing portrait ones.

**New landscape profiles (examples):**

| Key | Dimensions |
| --- | --- |
| `landscape-fhd` | 1920×1080 |
| `landscape-hd` | 1280×720 |
| `landscape-4k` | 3840×2160 |
| `landscape-wxga` | 1280×800 |
| `landscape-strip` | 1920×360 (ultra-wide status bar) |

**Scope:**

- Extend `profile_dimensions` and `normalize_profile` in `monitor.rs` to accept
  `landscape-*` keys and return appropriate dimensions
- Add an orientation field to the profile lookup so `pick_target_monitor` can
  choose the best landscape display when multiple monitors are connected
- New `landscape.css` (or `orientation-landscape` CSS class on `<body>`) that
  switches `--layout-direction` from `column` to `row` and adjusts panel sizing
- `applyProfile()` in `app.js` sets the orientation class based on profile key
  prefix; panel modules require no changes
- Settings profile picker groups profiles under "Portrait" / "Landscape" headings

---

## Post-update success notification 🔲

**Background:** When the NSIS installer runs silently (in-app update), the old
app is killed and restarted without any feedback. The old Tauri-based app showed
a "RigStats Update" dialog confirming the update succeeded.

### Approach (flag-file pattern)

1. **NSIS installer** — when running silently (`IfSilent`), write
   `%PROGRAMDATA%\se.codeby.rigstats\post-update.txt` containing the new version
   number, before launching `rigstats.exe`.

2. **Startup check** (`src-egui/src/main.rs`) — after loading settings and before
   `eframe::run_native`, check for the flag file. If found: read version, delete
   file, set `updater_win.status = UpdateStatus::JustUpdated { version }`, set
   `updater_open = true`.

3. **Updater dialog** (`src-egui/src/windows/updater.rs`) — add
   `UpdateStatus::JustUpdated { version: String }` variant. Renders "Updated to
   v{version}" in green in the hero, changelog in the central panel, and a single
   "Close" button in the footer — same layout as `UpToDate`.

PROGRAMDATA is used (not APPDATA) because the installer runs elevated and APPDATA
would point to the admin profile, not the current user.

### When to do this

Next polish pass after v1.27.0 is confirmed stable.

---

## Remove Node.js / npm infrastructure 🔲

**Background:** Node.js was introduced for the Tauri build pipeline and JS
frontend. Since the egui migration (v1.27.0), the app is pure Rust at runtime.
The `frontend/renderer/` JS files are legacy code that is never loaded by the
egui binary.

Node.js is currently kept for three reasons:

1. **vitest** — unit tests for logic helpers in `frontend/renderer/` (tempColors,
   vendorBranding, panel formatters). Most of this logic now has Rust equivalents
   with their own tests; `vendorBranding.js` is the only file with non-duplicated
   test coverage.
2. **ESLint** — lints JS files that are not used at runtime.
3. **markdownlint-cli2** and **lefthook** — tooling that could be replaced with
   Rust-native alternatives or removed.

### What needs to happen

- Port `vendorBranding.js` brand-key mapping to Rust (or remove it if the
  `brand.rs` logo loader already covers the same logic) and add Rust tests.
- Delete `frontend/renderer/` entirely, or keep only non-JS assets.
- Remove `package.json`, `node_modules`, `vitest.config.js`, `.eslintrc.*`,
  `lefthook.yml`.
- Rewrite `.github/workflows/verify.yml` and `build.yml` to use `cargo` and
  `dotnet` directly instead of `npm run verify` / `npm run build`.
- Update `CLAUDE.md`, `STANDARDS.md`, and this file to remove all npm/Node
  references.

### When to do this

After v1.27.0 is stable in production. Not a blocker for any feature work.

---

## LHM stability — sensor sidecar replaces HTTP LHM ✅

**Implemented in v1.21.0.**

Replaced the standalone LibreHardwareMonitor HTTP server with a managed .NET 10
sidecar (`sensor-sidecar/rigstats-sensor.exe`) that embeds the
`LibreHardwareMonitorLib` NuGet package and streams sensor data over a Windows
named pipe (`\\.\pipe\rigstats-sensors`). The Rust backend connects as a
read-only pipe client, deserialising one JSON payload per second — no scheduled
task, no HTTP, no external process lifecycle to manage.

**What was built:**

- `sensor-sidecar/` — self-contained .NET 10 single-file exe (no runtime required
  on user machines). `Program.cs` runs the pipe server loop and LHM update
  visitor. `SensorReader.cs` maps `IComputer` → `SensorPayload` using the same
  sensor-name and SensorId-prefix rules previously in `lhm.rs`.
- `lhm.rs` — converted from HTTP polling to named pipe client.
  `fetch_lhm_pipe` connects with `.write(false)` (pipe is `PipeDirection.Out`;
  requesting write access returns `ERROR_ACCESS_DENIED`). Connection failures are
  throttled to one log line per 30 s.
- `select_gpu_idx` — extracted GPU selection into a pure function with 10 unit
  tests covering preference matching, VRAM tiebreak, load tiebreak, and fallback.
- Old HTTP parsing code (`flatten_lhm`, `parse_lhm`, `FlatNode`, etc.) is
  retained under `#[cfg(test)]` to keep the existing 104-test suite green.

**Also completed (same release):**

- Windows Service installer (NSIS) — `build/installer.nsh` registers
  `rigstats-sensor` as a Windows Service (`sc create`, restart-on-failure
  policy, `sc start`). Uninstaller stops and deletes the service.
- Status screen — "Service" and "Pipe" fields replace the old LHM task fields;
  dependency table now shows `rigstats-sensor` with live pipe-connected state.

**Remaining:**

- Disk I/O throughput — `SensorReader.cs` does not yet expose disk read/write
  throughput (coming from sysinfo in the interim).

---

## egui migration — replace Tauri/WebView2 with native egui ✅

**Background:** WebView2 (Chromium) costs 2–4 % CPU at idle due to its internal render loop, even though the dashboard only updates once per second. Replacing the frontend with egui + `ctx.request_repaint_after(Duration::from_secs(1))` allows the process to sleep completely between repaints.

**Full plan:** `docs/egui-migration.md` — 8 phases, each producing a runnable binary.

### Phase 1 — Scaffold + data pipeline ✓

- Created `rigstats-backend` shared lib (Tauri-free copies of `lhm.rs`, `hardware.rs`, `stats.rs`, `settings.rs`, `debug.rs`, `lhm_process.rs`, `logging.rs`, `autostart.rs`). Key API change: `AppHandle` replaced by `&Path` for settings/debug path resolution.
- Created `src-egui` binary (`eframe` 0.34 + `egui` 0.34). Poll thread via `tokio::spawn` sends stats over `std::sync::mpsc` to the egui main thread.
- `ctx.request_repaint_after(1 s)` wired — process sleeps between ticks.
- Verified: window shows live CPU %, GPU %, RAM, LHM pipe state. CPU idle ~0.5 % in debug build.
- Root `Cargo.toml` workspace covers all three crates; existing Tauri npm scripts unaffected.

**All 8 phases complete.** Shipped as v1.27.0. The egui binary (`rigstats.exe`) replaces Tauri entirely — `src-tauri/` removed. CPU idle reduced from ~2–4 % (WebView2) to ~0 % between repaints. All panels, floating mode, settings, auto-updater, tray, and brand logos are fully implemented in egui/eframe.

---

## Background-only transparency (per-pixel alpha) ⏭

**Goal:** Make panel backgrounds transparent to the desktop wallpaper while keeping text, numbers, labels, graphs, and borders fully opaque — a "frosted glass" style where only the dark fill fades through.

**Current behaviour:** The opacity slider uses `SetLayeredWindowAttributes` with `LWA_ALPHA`, which scales every pixel equally. Lowering opacity makes text, sparklines, and borders semi-transparent alongside the background — not the desired effect.

### What was investigated

All four approaches below were implemented and tested; all failed to achieve selective background transparency.

#### 1. `with_transparent(true)` + wgpu D3D12 (eframe default renderer)

eframe exposes a `with_transparent(true)` `NativeOptions` flag that requests per-pixel alpha compositing. On Windows this requires `DXGI_ALPHA_MODE_PREMULTIPLIED` on the swap chain. The wgpu D3D12 backend does not set this up — `CompositeAlphaMode::PreMultiplied` is not supported without an underlying DirectComposition (DComp) surface. The flag silently falls back to `CompositeAlphaMode::Auto` (effectively Opaque). Setting `clear_color` to `[0, 0, 0, 0]` produced solid black rather than transparent, because DWM never received premultiplied alpha data.

#### 2. glow (OpenGL) backend

Switched eframe to `glow` renderer (software OpenGL). Same outcome — per-pixel alpha compositing still failed. The glow backend on Windows does not use DirectComposition either, so DWM receives no per-pixel alpha information from the swap chain.

#### 3. `LWA_COLORKEY` with `PANEL_FILL` colour `(11, 13, 18)`

`SetLayeredWindowAttributes` with `LWA_COLORKEY` keys out a specific colour (makes pixels with that exact COLORREF value fully transparent). The panel background fill is `Color32 { r: 11, g: 13, b: 18 }`. The key did not match: the D3D12 swap chain operates in sRGB colour space, so the framebuffer may store gamma-corrected byte values that differ from the intended COLORREF. The colour key produced no transparency effect.

#### 4. `LWA_COLORKEY` with pure black `RGB(0, 0, 0)`

Black is gamma-invariant (`0^n = 0`) so the stored framebuffer bytes match the COLORREF exactly. This did key out black pixels — but it made *everything* black transparent: panel content areas, shadow regions, sparkline valleys, and dark label text all disappeared. The combined effect was unusable. Additionally, all non-keyed pixels were uniformly dimmed by `LWA_ALPHA` when both flags are combined.

### Why Tauri worked

The previous Tauri frontend achieved background-only transparency successfully. WebView2 (the Chromium-based renderer embedded in Tauri) uses **DirectComposition** internally. DComp creates swap chains via `IDXGIFactory2::CreateSwapChainForComposition` with `DXGI_ALPHA_MODE_PREMULTIPLIED`, and presents them to DWM through a composition tree. DWM then composites the window using per-pixel alpha from the swap chain, so pixels with `alpha = 0` are fully transparent while fully-opaque pixels (text, borders, graphs) are unaffected.

### Path forward

Background-only transparency requires **custom Win32 DirectComposition integration**. The steps are:

1. Create a `IDCompositionDevice` (via `DCompositionCreateDevice`)
2. Create the DXGI swap chain for composition: `IDXGIFactory2::CreateSwapChainForComposition` with `DXGI_ALPHA_MODE_PREMULTIPLIED` in `DXGI_SWAP_CHAIN_DESC1`
3. Bind the swap chain to a DComp visual and set it as the root visual on the window
4. Render clear color as `[r, g, b, 0.0]` (premultiplied alpha — black transparent background) and UI content at `alpha = 1.0`
5. Commit the DComp transaction each frame

This is non-trivial and requires either:

- **A custom eframe/wgpu patch** that hooks in DComp before swap chain creation, or
- **A completely custom Win32 rendering path** that bypasses eframe's window setup

The effort is significant (estimated 1–2 weeks of Win32 graphics work) and carries risk: the DComp surface owner must be the same process and thread that creates the egui window, making it hard to layer on top of eframe's existing setup. This feature is deprioritised until the rest of the egui migration is stable.

---

## UI performance — lighter rendering strategy 🔲

**Background:** The dashboard updates the DOM every second via vanilla JS. As panel count grows (floating mode, battery, motherboard, process) layout cost increases. It is worth investigating whether a simpler or faster rendering model can reduce CPU and GPU overhead on the UI thread.

**What to investigate:**

- **Dirty-check before DOM writes** — profile with Chrome DevTools to identify panels causing unnecessary reflows; only write to the DOM when a value has actually changed (`textContent` set guarded by a previous-value comparison).
- **Canvas-based rendering** — replace DOM panels with canvas drawing (already done for sparklines). Gives sub-millisecond updates without layout/paint overhead at the cost of CSS flexibility.
- **OffscreenCanvas + Worker** — move canvas rendering to a Web Worker to fully offload the main thread.
- **WebGL / GPU-accelerated rendering** — relevant if animations or richer visuals are added without incurring CPU cost.
- **Tauri `wry` / WebView2 process overhead** — measure whether the WebView2 process itself is the bottleneck compared to a native Win32 surface with Direct2D.
- **Baseline measurement first** — record CPU % for the WebView2 process at idle vs. during a stats tick; target < 1 % on a modern CPU as the acceptance criterion before and after any change.

**Goal:** Identify where UI overhead actually lives and find the highest-impact, lowest-effort fix — likely dirty-checking before DOM writes rather than a full architectural rewrite.

---
