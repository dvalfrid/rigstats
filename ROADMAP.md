# Roadmap

Planned features in rough priority order. Each item is scoped as a self-contained release.

---

## Auto-update ✓

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

## NVMe / SSD temperatures ✓

**Panel:** Disk
**Data source:** LHM `Temperatures` section per storage device

**Implemented.** Each drive in the disk panel now shows a live temperature reading
in °C, color-coded by `resolveTempColor` (warm at 55 °C, hot at 70 °C).

LHM sensor identification uses the `SensorId` field (`/nvme/`, `/hdd/`, `/ata/`, `/scsi/`
prefixes) rather than sensor names, so motherboard and RAM thermal sensors are never
mixed in with disk readings. Warning Composite and Critical Composite threshold
sensors are excluded; the highest real temperature per device is shown.

Drive-letter-to-model mapping is resolved at startup via a WMI three-table join
(`Win32_DiskDrive → Win32_DiskDriveToDiskPartition → Win32_LogicalDiskToPartition`),
with a PowerShell CIM fallback. Temperatures are matched by model name (case-insensitive
substring match), so inserting a USB drive never shifts temperatures to the wrong
drive.

---

## Temperature threshold alerts ✓

**Panel:** Settings (new threshold fields) + tray notifications
**Data source:** Existing CPU / GPU / RAM / disk temp fields

**Implemented.** A configurable alert system fires a Windows tray notification when
a component exceeds its threshold, making the app useful during gaming or overclocking.

Eight optional `Option<u8>` fields added to `Settings` (serialised as camelCase JSON):
`warningCpuTemp`, `warningGpuTemp`, `warningRamTemp`, `warningDiskTemp`,
`criticalCpuTemp`, `criticalGpuTemp`, `criticalRamTemp`, `criticalDiskTemp`.

Per-tick comparison runs in `commands.rs` inside `get_stats()` after the
`StatsPayload` is assembled. Warning and Critical are checked independently —
each has its own 60-second cooldown key (e.g. `"cpu_warning"` vs `"cpu_critical"`)
stored in `AppState.last_alert`. Disk alerts fire on the hottest drive's temperature.
Notifications are sent via `tauri-plugin-notification`; errors are silently discarded
so a failed toast never disrupts the stats tick.

The Settings window has a compact "Temp Alerts" card with number inputs for all
eight thresholds. Blank = disabled (maps to `None`). Yellow column headers for
Warning, red for Critical. Window height bumped from 620 → 700 px to accommodate
the new card.

---

## CPU fan speed — investigated, skipped

**Panel:** CPU

After investigation across real user LHM data: CPU cooler fans are wired to the
motherboard Super I/O chip and appear as generic `Fan #N` channels alongside all
other chassis fans. LHM provides no signal that identifies which channel is the CPU
cooler. A highest-RPM heuristic was considered but rejected as unreliable (pump
heads, high-RPM case fans, and AIO radiator fans all exceed chassis fan RPM on some
builds). CPU cooler fan speed is instead available in the **Motherboard panel**
alongside all other fan channels.

---

## Motherboard panel ✓

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

## Extended GPU panel ✓

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

## Customisable themes / accent colours ✓

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

## Process monitor panel

**Panel:** New `process` panel (opt-in)
**Data source:** `sysinfo::Process` — CPU %, memory used, name

Shows the top processes by resource usage so users can instantly see which game,
encoder, or background service is consuming their hardware — a miniature Task
Manager always visible on the portrait monitor.

**What to show:**

- Top 5–8 processes sorted by CPU % (configurable: CPU / RAM / GPU)
- Columns: process name (truncated), CPU %, RAM (MB/GB)
- Optional GPU column via LHM per-process sensor if available
- Auto-refreshes each tick; processes with 0 % CPU for 3+ ticks fade out

**Scope:**

- Collect process list in `commands.rs` / new `processes.rs` using
  `sysinfo::System::processes()`; sort and truncate to top N before serialising
- New `ProcessEntry` struct in `stats.rs`; `StatsPayload.top_processes: Vec<ProcessEntry>`
- New `panels/process.js` frontend panel
- Add `process` to the valid panel keys list in `monitor.rs` and settings

---

## Landscape monitor support

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

## Battery panel (laptop support)

**Panel:** New `battery` panel
**Data source:** `sysinfo` battery API

Relevant for gaming laptops (ASUS ROG, Razer, Alienware). Shows charge %, charge
rate (W), and estimated time remaining. Panel is hidden automatically on systems
with no battery detected.

**Scope:**

- Query `sysinfo::Battery` on startup; store in `AppState` if present
- New `BatteryStats` struct in `stats.rs`, included in `StatsPayload`
- New `panels/battery.js` frontend panel
- Add `battery` to the valid panel keys list in `monitor.rs` and settings

---

## Overlay mode (single-monitor support)

**Panel:** Main window (new window mode)
**Data source:** Existing stats tick — no backend changes required

The app is currently designed exclusively for a secondary portrait monitor. Users
without a second screen regularly ask for a way to show a compact stats overlay
in a corner of their primary display during gaming — a use case served by tools
like MSI Afterburner's OSD or RTSS.

**Architecture:**

A new "Overlay" profile type that renders a compact, always-on-top, semi-transparent
floating widget instead of a full portrait panel. The widget snaps to one of the
four screen corners (configurable in Settings). It reuses the existing panel
modules but with a condensed single-column layout (`overlay-compact` CSS class).
`set_decorations(false)` + `always_on_top` + a click-through flag
(`set_ignore_cursor_events(true)` while not in settings mode) let the overlay
stay visible without interfering with the game.

**Scope:**

- Add `overlay` as a special profile key; `profile_dimensions` returns a small
  fixed size (e.g. 260×420)
- New corner-snap setting (`top-left`, `top-right`, `bottom-left`, `bottom-right`)
  persisted in `Settings`
- `pick_target_monitor` places the window at the selected corner of the primary
  monitor when overlay mode is active
- `set_ignore_cursor_events(true)` called after window creation in overlay mode;
  toggled off temporarily when the user moves the mouse to the widget area so
  they can interact with it (hover-to-unlock pattern)
- Compact CSS layout for overlay panels; shared panel JS modules render a
  subset of metrics to fit the smaller footprint
- Settings window gets an "Overlay mode" toggle with corner selector

---

## Stats logging / data export

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

## Stream Deck integration

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
