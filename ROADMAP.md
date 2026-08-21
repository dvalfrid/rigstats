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
| GPU driver version + stale-driver warning (Status dialog) | ✅ Done (v1.31) |
| Fullscreen (fill-screen) mode for dedicated monitors | ✅ Done |
| Floating panel groups | 🔲 Planned (3.0) |
| Desktop background — Level 1 (HWND_BOTTOM) | ✅ Done (v1.24) |
| Desktop background — Level 2 (WorkerW) | ✅ Done |
| Desktop wallpaper mode — per-pixel opacity (DirectComposition) | ✅ Done |
| Desktop background — WE Application wallpaper | 🔲 Planned (3.0) |
| Total system power consumption | ✅ Done (v1.34) |
| Stream Deck integration | 🔲 Planned (3.0) |
| Cross-platform OS abstraction — Linux port | 🔲 Planned (3.0) |
| Landscape monitor support | ✅ Done (v1.32) |
| egui migration — replace Tauri/WebView2 with native egui | ✅ Done (v1.27) |
| UI performance — lighter rendering strategy | ✅ Done (v1.27, via egui migration) |
| Background-only transparency (per-pixel alpha) — main window | ✅ Done (Normal/Always-on-Top/Always-Behind) |
| Background-only transparency (per-pixel alpha) — floating mode | 🔲 Planned (3.0), see #169 |
| Floating mode — reduce multi-window rendering cost | ⏭ Investigated, dropped — not worth the cost for a sub-1% saving |
| Test coverage — sidecar + sensor extraction | ✅ Done (v2.0) |
| Remove Node.js / npm infrastructure | ✅ Done |

---

## GitHub issue tracking

The whole roadmap is mirrored to
[GitHub Issues](https://github.com/dvalfrid/rigstats/issues) across two
milestones — **[v2.0](https://github.com/dvalfrid/rigstats/milestone/1)** for the
current scope and **[v3.0](https://github.com/dvalfrid/rigstats/milestone/2)** for
post-2.0 features (currently Floating panel groups, Stream Deck integration, and
the cross-platform OS abstraction / Linux port) —
with shipped features as closed issues, planned work as open issues, and
investigated-and-dropped items closed as *not planned*.

Each issue is tied to its roadmap entry by a stable **hidden marker** in the
issue body — `<!-- roadmap-id: <id> -->` (invisible in GitHub's rendered view).
`tools/sync-roadmap-issues.ps1` uses that marker to **upsert** issues
idempotently: re-running updates the matching issue in place (title, body,
labels, milestone, open/closed) instead of creating duplicates. A feature's
milestone defaults to `v2.0`; add a `milestone` key to its `$features` entry to
target a later one. The table below is **auto-generated** by that script from its
`$features` data and the live issue numbers — do not edit it by hand; re-run the
script to refresh it.

<!-- roadmap-table:start -->
| Issue | roadmap-id | Feature | Milestone | Status |
| --- | --- | --- | --- | --- |
| [#81](https://github.com/dvalfrid/rigstats/issues/81) | `gpu-driver-warning` | GPU driver version + stale-driver warning | v2.0 | ✅ Done |
| [#83](https://github.com/dvalfrid/rigstats/issues/83) | `fullscreen-mode` | Fullscreen (fill-screen) mode | v2.0 | ✅ Done |
| [#84](https://github.com/dvalfrid/rigstats/issues/84) | `auto-update` | Auto-update | v2.0 | ✅ Done |
| [#85](https://github.com/dvalfrid/rigstats/issues/85) | `nvme-ssd-temperatures` | NVMe / SSD temperatures | v2.0 | ✅ Done |
| [#86](https://github.com/dvalfrid/rigstats/issues/86) | `temperature-threshold-alerts` | Temperature threshold alerts | v2.0 | ✅ Done |
| [#87](https://github.com/dvalfrid/rigstats/issues/87) | `motherboard-panel` | Motherboard panel | v2.0 | ✅ Done |
| [#88](https://github.com/dvalfrid/rigstats/issues/88) | `extended-gpu-panel` | Extended GPU panel | v2.0 | ✅ Done |
| [#89](https://github.com/dvalfrid/rigstats/issues/89) | `customisable-themes` | Customisable themes / accent colours | v2.0 | ✅ Done |
| [#90](https://github.com/dvalfrid/rigstats/issues/90) | `process-monitor-panel` | Process monitor panel | v2.0 | ✅ Done |
| [#91](https://github.com/dvalfrid/rigstats/issues/91) | `floating-panel-layout` | Floating panel layout | v2.0 | ✅ Done |
| [#92](https://github.com/dvalfrid/rigstats/issues/92) | `multi-gpu-selector` | Multi-GPU selector and pinning | v2.0 | ✅ Done |
| [#93](https://github.com/dvalfrid/rigstats/issues/93) | `battery-panel` | Battery panel (laptop support) | v2.0 | ✅ Done |
| [#94](https://github.com/dvalfrid/rigstats/issues/94) | `settings-redesign` | Settings redesign | v2.0 | ✅ Done |
| [#95](https://github.com/dvalfrid/rigstats/issues/95) | `lhm-sensor-sidecar` | LHM stability - sensor sidecar | v2.0 | ✅ Done |
| [#96](https://github.com/dvalfrid/rigstats/issues/96) | `desktop-background-l1` | Desktop background - Level 1 (HWND_BOTTOM) | v2.0 | ✅ Done |
| [#97](https://github.com/dvalfrid/rigstats/issues/97) | `egui-migration` | egui migration - replace Tauri/WebView2 with native egui | v2.0 | ✅ Done |
| [#98](https://github.com/dvalfrid/rigstats/issues/98) | `stats-logging` | Stats logging / data export | v2.0 | ✅ Done |
| [#99](https://github.com/dvalfrid/rigstats/issues/99) | `remove-nodejs-npm` | Remove Node.js / npm infrastructure | v2.0 | ✅ Done |
| [#101](https://github.com/dvalfrid/rigstats/issues/101) | `background-transparency` | Background-only transparency (per-pixel alpha) | v2.0 | ✅ Done |
| [#105](https://github.com/dvalfrid/rigstats/issues/105) | `desktop-background-l2` | Desktop background - Level 2 (WorkerW) | v2.0 | ✅ Done |
| [#107](https://github.com/dvalfrid/rigstats/issues/107) | `total-system-power` | Total system power consumption | v2.0 | ✅ Done |
| [#108](https://github.com/dvalfrid/rigstats/issues/108) | `landscape-support` | Landscape monitor support | v2.0 | ✅ Done |
| [#109](https://github.com/dvalfrid/rigstats/issues/109) | `post-update-notification` | Post-update success notification | v2.0 | ✅ Done |
| [#110](https://github.com/dvalfrid/rigstats/issues/110) | `test-coverage-sidecar` | Test coverage - sidecar + sensor extraction | v2.0 | ✅ Done |
| [#100](https://github.com/dvalfrid/rigstats/issues/100) | `cpu-fan-speed` | CPU fan speed | v2.0 | ⏭ Not planned |
| [#102](https://github.com/dvalfrid/rigstats/issues/102) | `ui-performance-strategy` | UI performance - lighter rendering strategy | v2.0 | ⏭ Not planned |
| [#104](https://github.com/dvalfrid/rigstats/issues/104) | `floating-mode-perf` | Floating mode - reduce multi-window rendering cost | v2.0 | ⏭ Not planned |
| [#103](https://github.com/dvalfrid/rigstats/issues/103) | `floating-panel-groups` | Floating panel groups | v3.0 | 🔲 Planned |
| [#106](https://github.com/dvalfrid/rigstats/issues/106) | `streamdeck` | Stream Deck integration | v3.0 | 🔲 Planned |
| [#117](https://github.com/dvalfrid/rigstats/issues/117) | `cross-platform-port` | Cross-platform OS abstraction - Linux port | v3.0 | 🔲 Planned |
| [#123](https://github.com/dvalfrid/rigstats/issues/123) | `desktop-background-we-hosted` | Desktop background - WE Application wallpaper | v3.0 | 🔲 Planned |
| [#169](https://github.com/dvalfrid/rigstats/issues/169) | `background-transparency-floating` | Extend selective per-pixel transparency (DComp) to floating mode | v3.0 | 🔲 Planned |
<!-- roadmap-table:end -->

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

> **Update (egui rewrite):** theming was later reimplemented natively in
> `theme.rs` — `AppTheme::from_key(key)` maps a theme key straight to
> `egui::Color32` accents and derived tones (no CSS custom properties). The
> preset list has grown to **seven**: Dark Cyan (default), Amber, Green,
> Purple, Slate, Red, Blue (`theme::THEME_KEYS`). Selected via Settings →
> Appearance; still previews live and persists across restarts.

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

> **Update (egui rewrite):** the panel is now drawn natively by
> `panels/process.rs`. The top-8-by-CPU sort/truncate happens in
> `poll.rs` each tick (no serialisation/escaping concerns since there's no
> HTML rendering); the panel itself just reads the already-sorted list.

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

> **Update (egui rewrite):** the panel is now drawn natively by
> `panels/battery.rs`, sourced the same way (WMI `Win32_Battery` +
> `root\wmi BatteryStatus` in `hardware.rs`). The charge/power colour
> thresholds are no longer hardcoded — they're user-configurable warn/crit
> values under Settings → Alerts (defaults: charge warn 10 % / crit 5 %,
> power warn 15 W / crit 25 W).

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

> **Update (egui rewrite):** the Settings dialog was later reimplemented natively
> in egui and restructured into **five** tabs — **Display** (display profile,
> window layer + opacity, floating mode + panel scale, fill screen + alignment),
> **Panels**, **Alerts**, **Appearance** (model name + theme), and **General**
> (launch at startup, stats logging). In Desktop Wallpaper mode no-op controls grey
> out, the Display Profile is locked, and changes apply on Save (see the Desktop
> background mode section below).

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

> **Update (egui rewrite):** floating mode is now `render_floating_panels`
> in `src-egui/src/main.rs` — each visible panel is an egui
> `show_viewport_immediate` viewport instead of a Tauri `WebviewWindow`, sized
> from `panel_initial_h(key) * floating_panel_scale`. A panel's drag zone
> (top 24 px) and padlock hit-rect are drawn and hit-tested inline per panel
> rather than via `data-tauri-drag-region`; dragging triggers
> `egui::ViewportCommand::StartDrag` directly. Positions are read back from
> each viewport's `outer_rect` every frame and written straight to
> `settings.panel_layouts` — no `broadcast_stats`/`stats-broadcast` event, no
> `panel-host.js`; the same in-process `StatsPayload` tick just renders once
> per open viewport. The lock toggle is a single shared
> `floating_lock_arc` rather than a per-window Tauri command, and there is no
> right-click "Open Settings" / "Close panel" context menu — panels are
> shown/hidden entirely from Settings → Panels.

---

## Fullscreen (fill-screen) mode ✅

**Area:** Fixed-mode window sizing (`src-egui/src/main.rs`) + Settings Dashboard tab

For a dedicated small portrait monitor whose resolution matches the chosen
profile, the dashboard can now cover the entire screen instead of shrinking to
fit the visible panels. A **Fill Screen** toggle (Settings → Dashboard → Layout)
makes the non-floating window fill the monitor's height while keeping the profile
width, so panel proportions never change — the dashboard background simply fills
the surrounding space. An **Alignment** option (Top / Center, default Center)
controls where the panel stack sits, so the screen looks like a finished
dashboard regardless of how many panels are enabled.

**Implementation:**
- `Settings.fullscreen_mode: bool` + `Settings.fullscreen_align: String`
  (`#[serde(default ...)]`, no migration).
- `fixed_window_geometry` returns the monitor-height window when fullscreen;
  `pick_window_rect` supplies the monitor rect. The per-frame content-fit resize
  is skipped in fullscreen; centered alignment adds top padding before the panel
  loop. The dashboard background fills the rest via the existing `clear_color`.
- Only applies in non-floating mode (the toggle is disabled while floating).

---

## Floating panel groups 🔲 (Milestone 3.0)

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

## Floating mode — reduce multi-window rendering cost ⏭

**Dropped ([#104](https://github.com/dvalfrid/rigstats/issues/104), closed
2026-06-24 after architectural review):** the low-hanging fruit (Behind-mode
throttle, below) already shipped. The remainder — `Arc`-wrapping shared state
for deferred viewports — is high implementation cost for a sub-1% CPU saving
in release builds on the target hardware (gaming rigs); `show_viewport_immediate`
+ `&`-reference is the correct design for panels sharing one 1 Hz data
snapshot. Deferred indefinitely unless floating-mode CPU complaints arise on
battery-powered laptops. The investigation below is kept for reference if that
changes.

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

## Desktop background mode — Level 2 (WorkerW) ✅

**Panel:** Main window
**Data source:** No new data required

Makes the dashboard a true part of the wallpaper layer — living between the
desktop wallpaper and the desktop icons. Survives `Win+D`, is never covered by
any normal window, and appears below even desktop icons. This is the technique
used by Wallpaper Engine and Lively. Selected via **Settings → Window Layer →
Desktop Wallpaper**.

**Implementation (separate host process):**

The single-process `SetParent`-into-WorkerW approach is unsafe: a child window is
destroyed when its parent is, cross-process included, so an Explorer restart
(Windows update, Explorer crash, sleep/display change) would destroy the
reparented window and kill the app. Instead a dedicated **`rigstats-wallpaper`**
host process owns the wallpaper window:

1. The main `rigstats` app, on entering wallpaper mode, parks its own window
   off-screen, **pauses its sensor polling** (releasing the single-client sensor
   pipe), and spawns + supervises the host. It relaunches the host if it exits
   and kills it on leaving the mode or quitting.
2. The host finds the desktop `WorkerW` (on Windows 11 a child of `Progman`;
   on older Windows 10 the top-level sibling created by
   `SendMessageTimeout(Progman, 0x052C)`), reparents its borderless window into
   it via `SetParent`, and re-attaches each tick if Explorer rebuilds the
   hierarchy. The host exits if its parent PID disappears, so it never orphans.
3. Both binaries share the panel renderer (`DashboardView`) from the
   `rigstats-egui` library, so the wallpaper looks identical to the dashboard.

This split also gives crash isolation (a WorkerW compositing fault takes down only
the renderer) and produces the single-window host artifact that the planned
Wallpaper Engine integration reuses.

**v1 scope — display-only:** no `WH_MOUSE_LL` mouse hook. Drag/padlock are N/A in
wallpaper mode; GPU selection uses **Settings → preferred GPU**. Mutually
exclusive with floating mode (floating wins if both are set). An interactive
Phase B (in-panel clicks via a low-level mouse hook) can follow as a separate
feature.

**Files:** `src-egui/src/win32_wallpaper.rs` (Progman/WorkerW discovery,
`SetParent`, attach/detach/parent-liveness), `src-egui/src/bin/wallpaper.rs` (the
host), `src-egui/src/dashboard.rs` (shared `DashboardView`), supervisor in
`src-egui/src/main.rs`.

## Desktop wallpaper mode — per-pixel opacity (DirectComposition) ✅

**Panel:** `rigstats-wallpaper` host
**Data source:** No new data required

[#131](https://github.com/dvalfrid/rigstats/issues/131) — not tracked in the
`$features`/roadmap-issue-sync table (opened as a standalone spike, no
`roadmap-id` marker); tracked here in prose only.

Window-level opacity (`WS_EX_LAYERED` + `SetLayeredWindowAttributes`, used in
the Normal/Always-on-Top/Always-Behind layers) cannot be applied to a WorkerW
*child* window — `SetParent` strips the layered ex-style and DWM rejects
setting it on a child. Until this issue the opacity slider was hard-disabled
in Settings whenever Desktop Wallpaper mode was selected.

**Implemented as a spike, landed directly (not just a PoC):** the host now
forces the DX12 backend with `wgpu::Dx12SwapchainKind::DxgiFromVisual`
(`NativeOptions.wgpu_options` in `bin/wallpaper.rs`), which makes `wgpu`
create a DirectComposition-backed, per-pixel-alpha swap chain from the
window's HWND automatically — no hand-rolled `IDCompositionDevice`/visual-tree
code needed, and no new `unsafe` in this crate's own code (the
`unsafe_code = "deny"` lint stays satisfied). The one real blocker beyond
prior investigation (see "Background-only transparency" above): eframe/
egui-winit 0.34.3 has no hook to request `WS_EX_NOREDIRECTIONBITMAP` — required
per Microsoft's docs for a DComp swap chain to actually composite instead of
being masked by DWM's own opaque redirection bitmap — at window-creation time.
Verified empirically that applying it *after* the wgpu surface already exists
(`win_opacity::set_no_redirection_bitmap`, via `SetWindowLongPtrW` +
`SetWindowPos(SWP_FRAMECHANGED)`) still works, so eframe did not need to be
replaced. `clear_color()` and `theme::panel_frame()` premultiply the
dashboard's fill/border colors by the opacity setting to match the swap
chain's `PreMultiplied` composite alpha mode. The Settings opacity slider is
now enabled in every window layer.

Sparkline (mini-graph) backgrounds initially still rendered with a hardcoded
opaque fill, unaffected by this opacity plumbing — fixed in
[#168](https://github.com/dvalfrid/rigstats/issues/168) (`Sparkline::draw` in
`spark.rs`, and `net.rs`'s own dual-sparkline renderer, both now premultiply
their background/line/gradient colors by `opacity`).

**Files:** `src-egui/src/bin/wallpaper.rs`, `src-egui/src/win_opacity.rs`,
`src-egui/src/theme.rs` (`premul`), `src-egui/src/dashboard.rs` (`opacity`
threaded through `draw_one_panel`/`render_landscape_grid`),
`src-egui/src/windows/settings.rs`. Standalone research probe kept at
`src-egui/examples/dcomp_probe.rs`.

## Desktop background mode — WE Application wallpaper 🔲

**Panel:** Main window
**Data source:** No new data required

Optional alternative to the built-in WorkerW mode for users who own
[Wallpaper Engine](https://www.wallpaperengine.io/): ship `rigstats-wallpaper` as
a WE **Application wallpaper** so WE handles the WorkerW reparenting, multi-monitor
placement and Explorer-restart recovery. The host binary built for Level 2 is
already the right artifact; this milestone adds a **32-bit build** of it (WE
requires a 32-bit single-window app) and a hosted launch mode that fills the rect
WE assigns instead of auto-targeting a monitor. WE-owners only; the two background
modes are mutually exclusive (both target the same desktop layer). Target: **v3.0**.

---

## Stream Deck integration 🔲 (Milestone 3.0)

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

## Cross-platform OS abstraction — Linux port 🔲 (Milestone 3.0)

**Crates:** [`directories`](https://crates.io/crates/directories) (XDG ↔ Known Folders),
[`interprocess`](https://crates.io/crates/interprocess) (named pipe ↔ Unix socket)
**Data source:** No new data — re-routes existing sensor/hardware access per OS

End goal: make the RIGStats core OS-agnostic so it can be ported to Linux, with
each platform supplying only the adapters its OS needs. Today the app is hard-wired
to Windows — WMI, named pipes, the registry, win32 window calls, and the
LibreHardwareMonitor sidecar are called inline from otherwise-generic modules. The
work is a **ports-and-adapters (hexagonal) refactor**, not a rewrite of LHM:
`SensorPayload`/`LhmData` is already an OS-neutral DTO, and the manifests already
gate `wmi`/`winreg`/`winapi` behind `cfg(windows)`. The bulk of the effort is
extracting an OS-free core and moving the Windows code behind traits — fully
testable on Windows before any Linux code exists.

**Where the Windows coupling lives today:**

| Surface | Windows (today) | Linux equivalent |
| --- | --- | --- |
| Sensor stream | C# sidecar (LHM) + named pipe (`lhm.rs`) | hwmon (`/sys/class/hwmon`), NVML, `amdgpu` sysfs, lm-sensors + Unix socket |
| HW detection | `hardware.rs` — WMI + PowerShell CIM | DMI (`/sys/class/dmi/id`), `/proc`, `lspci`, sysfs |
| Paths | hard-coded `%APPDATA%`/`%PROGRAMDATA%` | XDG (`~/.config`, `/var/lib`) |
| Autostart | `autostart.rs` — HKCU registry | systemd user unit or `~/.config/autostart/*.desktop` |
| Window glue | `win_opacity.rs`, `win32_behind.rs`, `win32_dark_mode.rs`, `geometry.rs` | X11/Wayland (partly hard — see risks) |
| Packaging | NSIS + `sc create` service | .deb/.rpm/AppImage + systemd unit |

**Architecture — ports & adapters:**

A new OS-free crate `rigstats-core` holds the DTOs (`stats.rs`), settings logic,
logging, update-check, and the **port traits**. Each OS ships an adapter crate
selected via `[target.'cfg(...)'.dependencies]` so a Linux build never pulls `wmi`.

```text
rigstats-core/              # OS-agnostic. No win/wmi deps. Defines the ports.
rigstats-platform-windows/  # [cfg(windows)]            wmi, winreg, named pipe, win32
rigstats-platform-linux/    # [cfg(target_os="linux")]  sysfs, NVML, dbus, unix socket
rigstats-egui/              # UI — depends on core + the platform crate via cfg
sensor-sidecar/             # Windows (C#/LHM)
sensor-sidecar-linux/       # optional privileged systemd helper (if root is needed)
```

Port traits (in `rigstats-core`): `SensorProvider` (`read() -> SensorPayload`),
`HardwareProbe` (startup constants), `SystemPaths` (config/data/log dirs),
`Autostart` (enable/disable/query), `WindowPlatform` (opacity, send-to-bottom,
dark titlebar — no-op default on OSes without support). A `Platform` facade bundles
the chosen adapters, built once at startup via a `cfg`-gated `current()`. The rest
of the app talks only to `Platform` — no scattered `cfg(windows)` in UI or logic.

**Known Linux risk points (not refactor problems):**

- **Sensor breadth:** no LHM on Linux. hwmon coverage varies by board; NVIDIA needs
  NVML, AMD `amdgpu` sysfs, Intel `intel_gpu_top`.
- **GPU per-engine load** (`d3d_3d`/`d3d_vdec`) has no direct Linux analog — nearest
  is `/sys/.../gpu_busy_percent`, `nvidia-smi`, or `intel_gpu_top`; expect `None`
  initially.
- **"Always-on-bottom" desktop widget** is trivial on Windows but awkward on Wayland
  (needs `wlr-layer-shell`, only on wlroots compositors) — the biggest UX risk.
- **Privileges:** some sensors require root → a privileged systemd helper mirrors the
  Windows Service model.

**Incremental order (each step compiles and is tested on Windows):**

1. **Refactor (zero behaviour change):** extract `rigstats-core`, define the port
   traits + `Platform` facade, move all Windows code into `platform-windows`, replace
   `%APPDATA%`/`%PROGRAMDATA%` with `directories`, swap the named-pipe transport for
   `interprocess`, rename `lhm.rs`/`LhmData` → `sensors.rs`/`SensorSample` (LHM
   becomes a Windows adapter, not a core concept).
2. **Linux stub** that compiles (`cargo build --target x86_64-unknown-linux-gnu`) but
   returns empty values.
3. **Fill Linux adapters** in order: paths → hardware → sensors → autostart → window glue.
4. **Packaging** last (.deb/AppImage + systemd unit).

**Scope:**

- New `rigstats-core` crate: DTOs, settings/logging/update logic, `ports.rs` traits
- New `rigstats-platform-windows` crate: WMI, registry, named pipe, win32 adapters
- New `rigstats-platform-linux` crate: sysfs/NVML/dbus/unix-socket adapters (stub first)
- `Platform` facade + `cfg`-gated `current()`; remove inline `cfg(windows)` from UI/logic
- Replace hard-coded paths with `directories`; replace pipe transport with `interprocess`
- Rename LHM-specific core names; LHM reduced to the Windows `SensorProvider` adapter
- Per-OS packaging: keep NSIS/`sc` for Windows; add .deb/AppImage + systemd unit for Linux

---

## Total system power consumption ✅

**Implemented in v1.34.** New opt-in `power` panel: estimated total system power (~W) with CPU/GPU breakdown bars and a bottom gauge. Uses CPU Package + GPU Package + platform overhead (~25 W desktop, ~10 W laptop). Both sensors unavailable → panel dims gracefully.

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

## Landscape monitor support ✅

**Panel:** All panels + profile system
**Data source:** No new data required
**Status:** ✅ Done — egui adaptive grid (v1.32)

Portrait-only support meant users with a landscape secondary display (a spare
laptop screen, a tabletop/wall-mounted panel, or a dedicated wide strip monitor)
could not use the app. Landscape profiles add full parity: every panel, theme,
threshold, and floating mode, laid out for a wide, short screen.

**Implementation (egui — no CSS, profile logic lives in `src-egui/src/main.rs`):**

- **Orientation by name.** Landscape profiles use a `landscape-` key prefix;
  `profile_is_landscape(profile)` drives the orientation branches. No new
  `Settings` field and no `settings_version` migration are required.
- **Transposed resolutions.** Each landscape profile is the matching portrait
  profile with its axes swapped (`portrait-xl` 450×1920 → `landscape-xl`
  1920×450); portrait `*-side` profiles map to landscape `*-top` profiles.
- **Adaptive grid.** `render_landscape_grid` packs the visible panels into an
  even grid. The column count is chosen to maximise the per-cell content scale
  (ties broken toward fewer rows, which suits short landscape screens); every
  cell is the same size and the per-cell scale `sc` is derived from the cell
  dimensions, so panels shrink/grow to fit any landscape resolution. Header and
  clock are ordinary equal-sized cells (no rotation needed — the cell is already
  landscape-shaped). The portrait vertical stack is unchanged.
- **Fixed geometry.** Landscape fixes the window to the full profile size and
  pins it to the matching monitor; there is no per-frame content-fit.
- **Profile-aware monitor pick.** `pick_window_rect_for_profile` targets a
  monitor only when its resolution **matches** the profile (both dimensions
  within ~10 %) — a dedicated 1920×450 strip wins for `landscape-xl`. When no
  monitor matches it falls back to the **primary** monitor at the virtual origin
  (so e.g. `landscape-fhd-top` 1080×253 lands on the main screen, not a strip).
  The pure selection step is `select_profile_monitor`, covered by unit tests.
- **Pinnable window.** A padlock in the fixed-mode drag strip pins the whole
  dashboard (the group of panels) to its current screen position; the position
  is saved per profile in `Settings::pinned_positions` and restored across
  restarts instead of auto-targeting. Applies to both portrait and landscape.
- **Shared rendering.** Both orientations call `draw_one_panel`, so panels,
  themes, and thresholds are identical. Floating mode is orientation-independent
  (each panel is its own positioned window) and works unchanged.

**Landscape profiles:**

| Key | Dimensions | Key | Dimensions |
| --- | --- | --- | --- |
| `landscape-xl` | 1920×450 | `landscape-hdplus` | 1366×768 |
| `landscape-slim` | 1920×480 | `landscape-1600x900` | 1600×900 |
| `landscape-hd` | 1280×720 | `landscape-1680x1050` | 1680×1050 |
| `landscape-wxga` | 1280×800 | `landscape-2560x1600` | 2560×1600 |
| `landscape-fhd` | 1920×1080 | `landscape-4k` | 3840×2160 |
| `landscape-wuxga` | 1920×1200 | `landscape-fhd-top` | 1080×253 |
| `landscape-qhd` | 2560×1440 | `landscape-qhd-top` | 1440×338 |
|  |  | `landscape-4k-top` | 2160×506 |

---

## Post-update success notification ✅

**Implemented.** When the NSIS installer runs an in-app update it now relaunches
the app with a confirmation dialog, restoring the feedback the old Tauri build
showed after an update.

### Implementation (command-line argument)

The flag-file pattern in the original plan was replaced by a simpler
command-line hand-off — the installer already relaunches the app, so it just
passes the new version directly:

1. **NSIS installer** (`build/installer.nsi`) — after an `/autoupdate` install,
   the finish step runs `Exec '"$INSTDIR\rigstats.exe" "--just-updated=${VERSION}"'`
   instead of a plain launch.

2. **Startup check** (`src-egui/src/main.rs`) — on launch the app scans
   `std::env::args()` for `--just-updated=VERSION`. When present it sets
   `updater_win.status = UpdateStatus::JustUpdated { version }` and opens (and
   focuses) the updater window.

3. **Updater dialog** (`src-egui/src/windows/updater.rs`) — the
   `UpdateStatus::JustUpdated { version }` variant renders "Updated to v{version}"
   in the hero, the bundled changelog in the central panel, and a single "Close"
   button — same layout as `UpToDate`.

Passing the version as an argument avoids the elevated-installer APPDATA pitfall
entirely: no shared file is read, so there is no ambiguity about which user
profile owns it.

---

## Remove Node.js / npm infrastructure ✅

**Implemented.** All Node.js/npm infrastructure has been removed from the app
and its build following the egui migration (v1.27.0). `package.json`,
`node_modules`, `vitest.config.js`, `frontend/renderer/`, ESLint config, and
lefthook are gone. The build pipeline uses `cargo` and `dotnet` directly.
Brand-key logic from `vendorBranding.js` is covered by `brand.rs` and its
Rust tests.

**One intentional exception:** the release workflow (`.github/workflows/release.yml`)
still runs `npx --yes @tauri-apps/cli@^2 signer sign` to produce a legacy
Tauri minisign signature alongside `latest.json`. This is required so
clients still on a pre-1.26 (Tauri) build can verify and install an update
without crashing — dropping it would strand anyone who hasn't updated in a
while. Remove this step (and the `npx` dependency) once pre-1.26 installs are
judged negligible.

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

## Background-only transparency (per-pixel alpha) ✅

**Reopened, implemented, and shipped 2026-08-21 ([#101](https://github.com/dvalfrid/rigstats/issues/101), closed) — unblocked by [#131](https://github.com/dvalfrid/rigstats/issues/131)/[#168](https://github.com/dvalfrid/rigstats/issues/168).** The investigation below (2026-06-ish) is kept as-is for historical record; see "Implemented for Normal / Always-on-Top / Always-Behind" near the end for what shipped. Floating mode is a separate follow-up: [#169](https://github.com/dvalfrid/rigstats/issues/169).

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

### Path forward (updated 2026-08-21 — unblocked by #131/#168)

Approach 1 above concluded "the wgpu D3D12 backend does not set this up" — true
for the wgpu version tested at the time, but wgpu 27.0.0 (already older than
the version this repo has pinned) added exactly this capability:
`wgpu::Dx12SwapchainKind::DxgiFromVisual`, which makes the DX12 backend create
a DirectComposition-backed, per-pixel-alpha swap chain automatically from a
plain HWND — no custom `IDCompositionDevice`/visual-tree code needed (steps
1-3 above are handled internally by wgpu). #131 proved this works end-to-end
for the wallpaper host, including the one real remaining blocker: eframe/
egui-winit has no hook to request `WS_EX_NOREDIRECTIONBITMAP` at
window-creation time, but applying it *after* the wgpu surface already exists
(`win_opacity::set_no_redirection_bitmap`) still works.

**Selective transparency (this issue's actual goal) turns out to already be
solved as a side effect of how #131/#168 threaded `opacity` through:**
`dashboard.rs`'s `draw_one_panel`/`render_landscape_grid` only pass `opacity`
into `theme::panel_frame` (background fill/border) and the sparkline
background/gradient/line — never into the content drawn inside each panel's
closure (labels, numbers, `ring::show` gauges, bars, logos), which all use
theme colors at full alpha. In Desktop Wallpaper mode this already produces
exactly the "frosted glass" effect this issue wants — confirmed visually.

**Implemented for Normal / Always-on-Top / Always-Behind (2026-08-21):**

1. `main.rs` forces the same swap-chain setup `bin/wallpaper.rs` uses:
   `NativeOptions.wgpu_options` with `Backends::DX12` +
   `Dx12SwapchainKind::DxgiFromVisual`, `ViewportBuilder::with_transparent(true)`
   on the **main viewport only**, and `win_opacity::set_no_redirection_bitmap`
   called once the main window's HWND is known (replaces the old
   `win_opacity::set_opacity` call at that same spot).
2. `main.rs`'s `clear_color()` now premultiplies by opacity
   (`theme::premul(theme::PANEL_FILL, self.opacity)`), matching `bin/wallpaper.rs`.
3. `main.rs`'s `draw_one_panel`/`render_landscape_grid` wrapper methods now pass
   `self.opacity` instead of hardcoded `1.0` into `DashboardView`.
4. The four other `win_opacity::set_opacity(self.hwnd, ...)` re-application call
   sites (window-layer transitions, wallpaper-mode restore, floating toggles)
   were removed — no longer needed since `clear_color`/`draw_one_panel` read
   `self.opacity` live every frame instead of needing a one-shot Win32 push.
   Floating panels keep their own independent `win_opacity::set_opacity` call
   (unaffected — see the floating-mode follow-up below).
5. **Dialogs stay fully opaque** (Settings/About/Status/Updater — see
   `src-egui/src/windows/CLAUDE.md`): none of their `ViewportBuilder`s call
   `.with_transparent(true)`, so `support_transparent_backbuffer` (egui-wgpu's
   alpha-mode gate, `egui-wgpu-0.34.3/src/winit.rs`) stays false for them
   regardless of the main window's DComp setup. Confirmed live.

**Verified visually:** Normal, Always-on-Top, and Always-Behind all render with
the background genuinely transparent (desktop bleeding through) while text,
numbers, gauges, and bars stay fully legible — no black flashes, no
regressions, window levels/Z-order (including `win32_behind`'s `HWND_BOTTOM`
enforcement) unaffected by the DComp swap chain.

**Sparkline correction:** the first pass of this also scaled the sparkline
line/gradient by opacity (copying #168's approach too literally) — caught via
screenshot comparison at 10/50/100% opacity. Fixed to match how text/gauges
behave: only the background rect fades, the graph line/fill stay fully
readable at any opacity.

**Not done — kept as `WS_EX_LAYERED`, unaffected by this issue:** floating
mode. It renders one OS viewport per visible panel
(`show_viewport_immediate`), not a single window, so the same technique needs
repeating per-viewport and re-verifying under panels being created/destroyed
as visibility toggles — tracked separately as
[#169](https://github.com/dvalfrid/rigstats/issues/169).

---

## UI performance — lighter rendering strategy ⏭

Superseded by the egui migration (v1.27.0). The DOM rendering cost — WebView2 process overhead, layout/paint on every tick — is gone entirely. The egui binary sleeps between repaints and idles at ~0 % CPU.

---

## Test coverage — sidecar + sensor extraction ✅

**Background:** A production-readiness audit (2026-06) found the Rust pure-logic
layer well covered (semver, colour math, sparkline, `parse_lhm`, GPU selection,
brand classification, settings migration). Two gaps remain where bugs would be
silent — wrong sensor readings rather than crashes — and are currently caught
only by manual inspection.

**What to add:**

- **Sensor sidecar (`sensor-sidecar/`, .NET) has zero tests.** It is a black box
  for all CPU/GPU/motherboard metrics: named-pipe framing, JSON serialization,
  and the LibreHardwareMonitor sensor mapping are untested. Stand up a test
  project (xUnit) covering payload serialization and the sensor-selection logic,
  and wire it into `cargo xtask verify`.
- **`extract_*` functions in `rigstats-backend/src/lhm.rs` are only tested
  indirectly via `parse_lhm`.** Add direct unit tests for the filtering edge
  cases that silently drop or mis-pair data:
  - `extract_motherboard` — 0-RPM fan exclusion, the `< 5 °C` sentinel filter,
    and the "… VID" voltage exclusion.
  - `extract_network` — mismatched upload/download index counts.
  - `extract_disk_temps` — malformed `SensorId`, "Warning/Critical Composite"
    exclusion, and implausible-reading handling.
  - `extract_ram_temp` — the DIMM `/temperature/0` index assumption.

**Goal:** Move the sensor data path from "covered by integration tests and manual
inspection" to "each filtering rule has a dedicated regression test," and remove
the sidecar's status as the one fully untested component.

**Implementation summary:** A new `sensor-sidecar.Tests/` xUnit project (NSubstitute
for LHM `IComputer`/`IHardware`/`ISensor` mocks) covers the previously untested
.NET sensor path:

- `SerializationTests.cs` — locks the snake_case JSON contract the Rust
  `SidecarPayload` deserializer depends on (every top-level field, all `GpuDevice`
  fields including the explicit `d3d_3d`/`d3d_vdec`, and the `MbFan`/`MbTemp`/`MbVoltage`
  label/value names). A C#-side rename now fails a test instead of silently breaking
  deserialization.
- `SensorReaderTests.cs` — one test per `SensorReader.Extract` filtering rule across
  CPU, GPU (load/D3D/clocks/hotspot, VR SoC temp fallback, AMD iGPU power sum, VRAM
  MB vs GB), disk (storage-prefix + Warning/Critical exclusion, highest-temp), RAM
  (`/temperature/0`-only, max across DIMMs), and motherboard (0-RPM fan, sub-5 °C
  temp, generic-slot voltage exclusion, descending fan sort, no GPU bleed-in).

The project compiles `SensorReader.cs` directly (the shipped sidecar is a
self-contained single-file exe a test lib cannot `ProjectReference`). Wired into
`cargo xtask verify` via `dotnet test sensor-sidecar.Tests/…`.

The Rust `extract_*` functions gained the named direct edge-case tests in
`rigstats-backend/src/lhm.rs` (a `node`/`node_gp` `FlatNode` builder feeding the
extractors directly): 0-RPM fan, sub-5 °C sentinel and `… VID` exclusion in
`extract_motherboard`; mismatched upload/download counts in `extract_network`;
empty `SensorId`, Warning/Critical and zero-reading exclusion in `extract_disk_temps`;
and the `/temperature/0`-only index assumption in `extract_ram_temp`.

---

## GPU driver version + stale-driver warning ✅

**Shipped v1.31.** Motivated by a field bug: a tester's RX 9070 XT showed no GPU
sensors except D3D 3D load, and `rigstats-sensor.exe` crashed with a native ADL
`AccessViolationException` in `AmdGpu.Update()` (LHM issue #736). Root cause was an
outdated AMD driver; updating the GPU + chipset drivers restored all sensors.

**Implementation:**

- `hardware::detect_gpu_drivers()` queries `Win32_VideoController`
  (Name/DriverVersion/DriverDate) WMI-first with a PowerShell CIM fallback,
  filtering virtual adapters via `is_ignored_adapter_name`. `driver_age_days`
  derives whole-day age from `DriverDate` (returns `None` for future/unparseable
  dates).
- The Status dialog (`windows/status.rs`) renders a two-column Components row:
  Dependencies on the left, a GPU Drivers card on the right showing each adapter's
  version + date, an age-based "stale driver" warning (`DRIVER_STALE_DAYS = 270`),
  and a per-adapter right-aligned "↗ Latest driver" link to the vendor download
  page (AMD/NVIDIA/Intel). The Drivers card is forced to the Dependencies card's
  height and its list is wrapped in a scroll area so multi-GPU systems scroll
  rather than overflow.
- `docs/troubleshooting.md` gained a "GPU Sensors Missing … Or Sidecar Crashes"
  section pointing users at the driver-update fix.

No reliable cross-vendor API exists for "latest available driver version," so the
feature surfaces the installed version + an age heuristic + a one-click link to the
vendor's driver download page rather than claiming to know the newest release.
