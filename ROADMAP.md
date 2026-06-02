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
| Stats logging / data export | 🔲 Planned |
| Floating panel groups | 🔲 Planned |
| Desktop background — Level 1 (HWND_BOTTOM) | 🔲 Planned |
| Desktop background — Level 2 (WorkerW) | 🔲 Planned |
| Total system power consumption | 🔲 Planned |
| Stream Deck integration | 🔲 Planned |
| Landscape monitor support | 🔲 Planned |
| UI performance — lighter rendering strategy | 🔲 Planned |

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

**What to add:**

- **Hotspot temperature** — junction/hotspot reading (AMD `GPU Hot Spot`, NVIDIA
  `GPU Hot Spot Temperature`) alongside the existing package temp
- **Power draw vs. power limit** — actual GPU power (W) and the board power limit
  so users can see how close to the limit the card is running
- **Memory controller load %** — separate from shader load; indicates VRAM
  bandwidth pressure
- **Memory clock** — VRAM frequency, useful when debugging memory throttling

**Scope:**

- Extend `LhmData` / `GpuStats` structs with the new fields (`Option<f32>` to
  handle cards that do not expose every sensor)
- Update `lhm.rs` GPU extraction to collect the additional sensor types
- Expand `panels/gpu.js` to render the new rows; hide rows whose value is `null`

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

## Stats logging / data export 🔲

**Panel:** Settings (new Logging card) + tray menu shortcut
**Data source:** Existing `StatsPayload` — no new sensors required

Lets overclockers and benchmark enthusiasts record hardware metrics over time and
analyse them after a gaming session or stress test. A common request on monitoring
tools: "I want to see what my GPU temperature peaked at during that boss fight."

**Architecture:**

Logging runs as an opt-in background task inside the Rust backend. When enabled,
each `get_stats()` tick appends a CSV row to a rolling log file in the Tauri app
data directory (`rigstats-log-YYYY-MM-DD.csv`). Log files roll daily and are
automatically pruned after a configurable retention period (default 7 days).

**What is logged (one row per tick):**

`timestamp_unix, cpu_load, cpu_temp, cpu_freq_mhz, gpu_load, gpu_temp, gpu_vram_used_mb, ram_used_gb, disk_read_kbs, disk_write_kbs, net_up_kbs, net_down_kbs, ping_ms`

**Scope:**

- New `logging.rs` module: `append_stats_row(&StatsPayload, path)`, `prune_old_logs(dir, days)`
- `AppState` gains `logging_enabled: bool` and current log file handle
- Settings window: "Stats Logging" card with on/off toggle, retention selector
  (1 / 7 / 30 days), and an "Open log folder" button
- Tray menu: "Start/Stop logging" shortcut for quick toggle without opening Settings
- Persist `logging_enabled` and `log_retention_days` in `Settings` struct

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

## Desktop background mode — Level 1 (HWND_BOTTOM) 🔲

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

**Architecture:**

The existing always-on-top implementation in `commands.rs` / `windows.rs` is extended
with a third state. When "Always behind" is selected, `set_always_on_top(false)` is
called first, then `SetWindowPos` with `HWND_BOTTOM` and `SWP_NOMOVE | SWP_NOSIZE |
SWP_NOACTIVATE`. A `WM_WINDOWPOSCHANGING` subclass hook re-applies `HWND_BOTTOM` if
another operation reorders the z-stack. The setting is persisted in `Settings` as a
new `window_layer` enum field (`"normal"` | `"on_top"` | `"behind"`).

**Scope:**

- New `window_layer: String` field in `Settings` struct (`#[serde(default)]`)
- `set_window_layer(layer)` Tauri command replacing the current boolean `always_on_top`
  toggle (backwards-compatible via migration)
- Win32 `SetWindowPos` call via the `windows` crate in `windows.rs`
- `WM_WINDOWPOSCHANGING` subclass to keep the window pinned at HWND_BOTTOM
- Settings window: replace the "Always on top" checkbox with a three-way selector
  (Normal / Always on top / Always behind)
- Applies to the main portrait window; optionally also to each floating panel

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
