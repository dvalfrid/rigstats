# Architecture

## Contents

- [Overview](#overview)
- [Data Flow](#data-flow)
- [File Structure](#file-structure)
- [Backend Modules](#backend-modules)
- [Frontend Modules](#frontend-modules)
- [Dashboard Panels](#dashboard-panels)
- [Diagnostics Export](#diagnostics-export)
- [Design Decisions](#design-decisions)

---

## Overview

RIGStats is a Windows-only Tauri v2 desktop app that displays live hardware
telemetry on a secondary portrait monitor. The frontend is vanilla ES modules
served directly by Tauri — no bundler or framework. The backend is Rust and
uses three data sources: LibreHardwareMonitor (GPU/sensor data via HTTP),
sysinfo (CPU/RAM/disk/network), and WMI (hardware metadata at startup).

---

## Data Flow

```text
LibreHardwareMonitor (localhost:8085/data.json)
    └─► lhm.rs          fetch + flatten JSON tree → LhmData

sysinfo crate           CPU load/freq, RAM, disk, network, processes
wmi crate               GPU name, VRAM, RAM spec, system brand (startup only)

    └─► commands.rs     get_stats() assembles StatsPayload every tick
            └─► Tauri IPC  invoke("get-stats")
                    └─► app.js  tick() every 1 s
                            └─► panel modules update DOM
```

**Tick rate:** 1 second. LHM is polled with an 800 ms timeout; on failure
the last successful sample is reused so the UI never resets to `--`.

---

## File Structure

```text
rig-dashboard/
├── frontend/
│   ├── index.html          Main dashboard
│   ├── settings.html
│   ├── status.html
│   ├── about.html
│   ├── updater.html
│   ├── assets/
│   └── renderer/
│       ├── panels/         One JS module per panel
│       └── *.js            Shared utilities and entry scripts
├── src-tauri/
│   ├── src/                Rust source (one module per concern)
│   ├── Cargo.toml
│   └── tauri.conf.json
├── docs/
├── website/
├── assets/
├── vendor/lhm/
└── build/
    ├── installer.nsh
    └── lhm-default/
```

---

## Backend Modules

### Quick reference

| Module | Responsibility |
| --- | --- |
| `main.rs` | Tauri builder, tray, lifecycle, startup orchestration |
| `stats.rs` | Shared state (`HardwareInfo` + `AppState`) and all payload structs |
| `commands.rs` | `#[tauri::command]` handlers — thin wrappers only |
| `hardware.rs` | WMI/PowerShell hardware detection at startup |
| `lhm.rs` | LHM HTTP polling and sensor tree flattening |
| `lhm_process.rs` | LHM process lifecycle (scheduled task / direct spawn) |
| `monitor.rs` | Display profiles, monitor selection, panel key validation |
| `settings.rs` | Settings struct, JSON persistence |
| `windows.rs` | Secondary window creation and tray-anchored positioning |
| `updater.rs` | Background update checks and install flow |
| `autostart.rs` | Windows startup registry management |
| `diagnostics.rs` | Diagnostics ZIP export |
| `debug.rs` | Debug log helpers (no deps on other modules) |

### Module details

#### `main.rs`

Tauri builder, tray icon, and lifecycle. Registers two managed state types at
startup: `HardwareInfo` (one-time WMI/sysinfo hardware detection) and `AppState`
(per-tick runtime state). Picks the best monitor for the profile and starts LHM.
Spawns two background tasks:

- **`spawn_wmi_retry`** — re-runs WMI detection for any fields that returned
  fallback values at startup (e.g. WMI not yet ready). Retries up to 3 times
  at 30 s / 60 s / 120 s; emits `hardware-refreshed` to the renderer when a
  field is resolved so static labels update without a page reload.
- **`updater::spawn_background_check`** — checks for updates every 6 hours
  (first check after 10 s).

#### `stats.rs`

Defines two shared state structs and all serializable payload structs sent to
the frontend.

**`HardwareInfo`** — startup-detected constants registered once and never
mutated: `disk_model_map`, `ram_spec`, `ram_details`, `gpu_vram_total_mb`,
`system_brand`, `mb_name`, `ping_target`, `sysinfo_available`, `wmi_available`.
Registered with `app.manage(HardwareInfo { ... })`.

**`AppState`** — per-tick mutable state behind a `Mutex`: `lhm_client`,
`settings`, `system`, `disks`, `networks`, `last_net_sample`, `last_ping_sample`,
`last_lhm`, `last_alert`.

**Payload structs:**

| Struct | Contents |
| --- | --- |
| `StatsPayload` | Top-level payload returned by `get_stats()` |
| `CpuStats` | Load, per-core loads, temp, freq, power |
| `GpuStats` | Load, temps, clocks, VRAM, fan, power, D3D |
| `RamStats` | Used/free/total, spec string, DIMM temp |
| `NetStats` | Up/down throughput, interface name, ping |
| `DiskStats` | Read/write throughput, per-drive entries |
| `DiskDrive` | Filesystem label, size, used, pct, temp |
| `MotherboardStats` | Fans, temps, voltages, chip name, board name |
| `ProcessEntry` | Process name, CPU % of total system, RAM in MB |

`StatsPayload.top_processes` is a `Vec<ProcessEntry>` pre-sorted by CPU usage
and capped at 8 entries before serialisation.

#### `commands.rs`

Thin `#[tauri::command]` handlers only — no business logic. Each handler
delegates to a domain module.

`get_stats()` is the main tick handler. Per call it:

1. Fetches a fresh LHM sample (falls back to last good sample on failure)
2. Calls `system.refresh_cpu()`, `refresh_memory()`, `refresh_processes()`
3. Collects disk throughput and drive metadata
4. Computes network throughput delta over elapsed time
5. Refreshes ping (cached, re-measured every 5 s)
6. Assembles `StatsPayload` including top 8 processes sorted by CPU
7. Checks temperature thresholds and fires tray notifications if due

#### `hardware.rs`

All startup hardware detection. Each function tries WMI first, falls back to
PowerShell CIM on failure.

| Function | What it detects |
| --- | --- |
| `detect_gpu_name` | Primary discrete GPU name |
| `detect_gpu_vram_total_mb` | VRAM total (MB) |
| `detect_system_brand` | Brand key: `rog`, `msi`, `alienware`, etc. |
| `classify_system_brand` | Brand classification logic |
| `detect_model_name` | System model name |
| `detect_motherboard_name` | Board manufacturer + product (normalised) |
| `detect_ram_spec` | Type + speed string, e.g. "DDR5 6000 MT/s" |
| `detect_ram_details` | Stick count, capacity, vendor, part number |
| `detect_disk_model_map` | `HashMap<drive_letter, model_name>` via WMI join |
| `detect_ping_target` | Default gateway or public fallback |
| `probe_wmi_status` | Checks whether WMI is reachable |

`detect_disk_model_map` builds its map via a three-table WMI join:
`Win32_DiskDrive → Win32_DiskDriveToDiskPartition → Win32_LogicalDiskToPartition`.
Results are stored in `HardwareInfo` so LHM temperatures can be matched by model
name rather than by index (stable when USB drives are inserted/removed).

#### `lhm.rs`

HTTP client that fetches `/data.json` from LHM, flattens the nested sensor tree
into `Vec<FlatNode>`, and extracts metrics into `LhmData`.

Each `FlatNode` carries: `text`, `value`, `parent`, `grandparent`, `sensor_id`.

**GPU extraction:** Anchored on the `GPU Memory Total` node with the highest
value (selects dGPU over iGPU on multi-GPU systems). All sensors sharing that
anchor's `grandparent` (the GPU device name) are collected.

Extracted GPU fields: core load, core temp, hot-spot, core clock (`gpu_freq`),
memory clock (`gpu_mem_freq`), power, fan, VRAM used/total, D3D 3D load
(`gpu_d3d_3d`), D3D Video Decode load (`gpu_d3d_vdec`).

**Disk temperatures:** Identified by `SensorId` prefix
(`/nvme/`, `/hdd/`, `/ata/`, `/scsi/`, `/ssd/`) — not by sensor name.
Warning/Critical Composite sensors are excluded. Highest real temp per device
stored as `disk_temps: Vec<(device_name, °C)>`.

**RAM temperature:** `SensorId` prefix `/memory/dimm/` with suffix
`/temperature/0`. Index 0 is the actual reading; indices 1–5 are resolution and
threshold limits and are excluded. Max across all populated DIMM slots stored
as `ram_temp: Option<f64>`.

**CPU temperature:** Matched by name (`"Core (Tctl/Tdie)"` for AMD,
`"CPU Package"` / `"Core Average"` for Intel) restricted to
`parent == "Temperatures"` — prevents the Intel CPU Package *power* sensor
(same name, different parent) from being picked up.

**Motherboard Super I/O:** `/lpc/` `SensorId` prefix (chip-agnostic — works
on NCT, ITE, Winbond, etc.). Fans > 0 RPM sorted descending, temps ≥ 5 °C,
named voltage rails only (generic `Voltage #N` slots excluded > 0.1 V).

#### `monitor.rs`

- `normalize_profile` / `profile_dimensions` — canonical profile name → pixel
  dimensions
- `pick_target_monitor` — selects the best available monitor for a profile using
  an aspect-ratio + area fit score; positions the window borderless using
  `set_size` + `set_decorations(false)` + `set_position`
- `normalize_visible_panels` — validates and deduplicates panel key lists

Valid panel keys: `header`, `clock`, `cpu`, `gpu`, `ram`, `net`, `disk`,
`motherboard`, `process`. Both `motherboard` and `process` are opt-in.

#### `settings.rs`

`Settings` struct persisted as JSON to
`%APPDATA%\se.codeby.rigstats\rigstats-settings.json`.

All fields use `#[serde(default)]` for backwards-compatible schema evolution —
new fields deserialise cleanly from older settings files. `last_seen_version`
is compared against `CARGO_PKG_VERSION` at startup to detect the first launch
after an upgrade.

Temperature alert thresholds are stored as
`thresholds: HashMap<String, ComponentThresholds>` where
`ComponentThresholds { warn: Option<u8>, crit: Option<u8> }` and the keys are
`"cpu"`, `"gpu"`, `"ram"`, `"disk"`. A `settings_version: u8` field acts as a
migration sentinel (0 = legacy format, 1 = current). When `load_settings` reads
a version-0 file it runs `migrate_v0_thresholds` once — copying the eight old
flat fields into the map — then re-persists. The eight legacy flat fields are
kept as private `#[serde(default, skip_serializing)]` shims so old files can
be read but are never written back.

#### `windows.rs`

Creates and positions the four secondary windows:
`ensure_settings_window`, `ensure_about_window`, `ensure_status_window`,
`ensure_updater_window`. Windows anchor to the last tray icon click position
via `set_last_tray_click_position`.

#### `updater.rs`

`spawn_background_check` starts a loop that checks GitHub Releases every 6
hours (first check after 10 s). Emits `update-available` to the frontend when
a newer version is found. Also exposes `check_for_update`, `install_update`,
and `open_updater_window` commands.

#### `autostart.rs`

Per-user Windows autostart via
`HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run`. Uses `winreg` for direct
registry access (no subprocesses). Also manages `StartupApproved\Run` to stay
in sync with Windows Settings → Apps → Startup.

#### `debug.rs`

`append_debug_log`, `reset_debug_log`, `run_hidden_command`, `unix_now_secs`.
No dependencies on other crate modules — safe to import from anywhere.

---

## Frontend Modules

### Quick reference

| Module | Responsibility |
| --- | --- |
| `environment.js` | Tauri detection, `backend` wrapper, `IS_DESKTOP` flag |
| `app.js` | 1 s poll loop, settings/events, panel orchestration |
| `systemInfo.js` | Hostname, CPU/GPU model strings, brand logo |
| `clock.js` | Time, day, date, uptime |
| `spark.js` | Sparkline ring buffer and canvas drawing |
| `tempColors.js` | Temperature → colour threshold mapping |
| `vendorBranding.js` | Brand key → logo asset + label (pure, testable) |
| `simulator.js` | Synthetic stats for browser-mode development |
| `themes.js` | CSS custom property application for colour themes |
| `panels/*.js` | One module per panel (see Dashboard Panels) |
| `settings.js` | Settings window entry script |
| `about.js` | About window entry script |
| `status.js` | Status window entry script |
| `updater.js` | Updates & Changelog window entry script |

### Module details

#### `app.js`

Main dashboard orchestrator:

- Drives the 1 s `tick()` poll loop (skips if previous tick is still in flight)
- Validates `StatsPayload` before rendering to avoid UI resets on malformed data
- Calls `applyThresholds(s)` from the `get-settings` response at startup and
  from every `apply-thresholds` event after `save_settings`
- `applyVisiblePanels` hides/shows panels and reorders them in the DOM via
  `appendChild` to match the saved order
- Resizes the window to the height of the visible panels after each reorder

#### `spark.js`

- `createHistory(n)` — creates a ring buffer of size `n` for all series
- `drawSpark` — single-series sparkline on a canvas element
- `drawDoubleSpark` — two series on a shared scale, used by network
  (upload=green, download=cyan) and disk (read=purple, write=pink)

#### `panels/`

Each panel exports one `update*Panel(stats, ...)` function called from
`app.js` every tick.

| Panel module | Key behaviour |
| --- | --- |
| `cpu.js` | Ring gauge, per-core bar list (scrollable), sparkline |
| `gpu.js` | Ring gauge, 3×2 metadata grid, VRAM bar, two optional D3D bars hidden when `null` |
| `ram.js` | Usage bar, spec metadata, DIMM temperature |
| `network.js` | Upload/download values, dual-series sparkline |
| `disk.js` | Paginates 3 drives per page every 5 ticks when > 3 drives present |
| `motherboard.js` | Three-column layout: fans / temps / voltages; `shortLabel()` maps `"Temperature #N"` → `"TN"` |
| `process.js` | Top 8 processes: name (`.exe` stripped, 16 char max), CPU %, RAM. Names are HTML-escaped before `innerHTML` insertion. `truncateName` and `formatRam` exported for unit tests. |
| `clock.js` | Time, weekday, date |

#### `settings.js`

- `panelOrder` tracks all panels (visible + hidden) in user-defined sequence
- `hiddenPanels` is a `Set` of unchecked keys
- Drag-to-reorder uses the Pointer Events API with `setPointerCapture` instead
  of the HTML5 Drag API (which shows a prohibition cursor inside WebView2)

#### `updater.js`

Invokes `check-for-update` on load, renders release notes from `latest.json`
combined with the bundled `CHANGELOG.md`, and drives the `install-update`
download + progress flow.

---

## Dashboard Panels

| Key | Panel name | Default | Data source |
| --- | --- | --- | --- |
| `header` | System Identity | ✓ | WMI · sysinfo |
| `clock` | Clock | ✓ | system time |
| `cpu` | CPU | ✓ | sysinfo · LHM |
| `gpu` | GPU | ✓ | LHM |
| `ram` | RAM | ✓ | sysinfo · WMI · LHM |
| `net` | Network | ✓ | sysinfo |
| `disk` | Storage | ✓ | LHM · sysinfo |
| `motherboard` | Motherboard | opt-in | LHM · WMI |
| `process` | Processes | opt-in | sysinfo |

Panel visibility and order are saved in `Settings.visible_panels` and
validated by `normalize_visible_panels` on both frontend and backend.

---

## Diagnostics Export

Invoked from Status dialog → **Collect Diagnostics…**. Opens a native Windows
save dialog via `rfd::FileDialog` (Win32 requires STA; runs on a dedicated OS
thread). Produces a self-contained ZIP for bug reports.

### Collection flow

1. Native save dialog opened on a blocking OS thread
2. On cancel → `Ok(None)`, no file written
3. On confirm → assemble and compress the following files, return path to UI

### ZIP contents

| File | Source | Notes |
| --- | --- | --- |
| `manifest.json` | inline | Unix timestamp + `CARGO_PKG_VERSION` |
| `debug.log` | `std::fs::read(debug_log_path)` | Full file, not the tail shown in the UI |
| `settings.json` | serde_json of `AppState.settings` | Read-only snapshot |
| `lhm-data.json` | `GET localhost:8085/data.json` | 3 s timeout; error payload on failure |
| `hardware.json` | PowerShell `Get-CimInstance` | OS, CPU, GPU, board, RAM |
| `sched-task.txt` | `schtasks /Query /V` | Both LHM task names |
| `environment.txt` | env vars + Windows registry | Arch, build, hostname |
| `install.log` | `rigstats-install.log` from app data | Written by NSIS installer |
| `sysinfo.json` | `AppState` mutexes | CPU brand, RAM totals, mount points, interfaces |
| `displays.json` | Tauri monitor list | Resolution, position, scale, fit score, selected flag |

---

## Design Decisions

### Sensor identification

- **Disk temperatures** are matched to drive letters by physical disk model name
  (startup WMI query) rather than by index. Index-based matching silently assigns
  temperatures to the wrong drives when a USB device is inserted.
- **LHM disk sensors** use the `SensorId` field (`/nvme/`, `/hdd/`, `/ata/`,
  `/scsi/`, `/ssd/` prefixes) instead of sensor text. Text-based filtering picks
  up motherboard chip sensors and RAM DIMM sensors that share the same
  parent-category name.
- **RAM DIMM temperature** uses `SensorId` prefix `/memory/dimm/` with suffix
  `/temperature/0`. Each DIMM slot exposes 6 temperature-category sensors;
  index 0 is the actual reading, indices 1–5 are resolution and threshold limits.
- **CPU temperature** is restricted to `parent == "Temperatures"` to prevent the
  Intel CPU Package *power* sensor (same name, different parent) from being
  returned instead of the thermal sensor.

### Data sources

- **Network throughput** always comes from sysinfo, not LHM. Sysinfo reads the
  same OS counters as Task Manager. LHM tracks adapters by GUID and can latch
  onto a VPN or Hyper-V bridge, producing near-zero readings.
- **GPU identification** anchors on the `GPU Memory Total` sensor with the
  highest value, selecting the dGPU over iGPU on multi-GPU systems without
  hardcoding device names.

### Frontend architecture

- **No bundler or framework** — vanilla ES modules served directly by Tauri's
  asset server. `frontend/` is the Tauri web root.
- **Panel reordering** uses CSS flexbox + DOM `appendChild`, not CSS grid, so
  panels can be reordered without any layout recalculation.
- **Drag-to-reorder** in Settings uses the Pointer Events API with
  `setPointerCapture` instead of the HTML5 Drag API, which shows a prohibition
  cursor inside WebView2.
- **Process names** are HTML-escaped before `innerHTML` insertion in
  `process.js` to prevent rendering breakage from adversarial process names.

### Reliability and correctness

- **LHM fallback** — the last successful sample is kept in memory so the UI
  never resets to `--` during transient LHM timeouts.
- **Payload validation** — `isValidStatsPayload` rejects malformed or empty
  payloads before rendering to avoid visual resets.
- **No tick overlap** — the tick loop sets `isTicking` before the async call and
  clears it in `finally`, preventing out-of-order UI updates.
- **Alert cooldowns** use a `Mutex<HashMap<String, Instant>>` keyed on
  `"<component>_<level>"`. Warning and critical are independent clocks.
  `notify_on_warn`/`notify_on_crit` gate whole levels without clearing thresholds
  so colour indicators remain active while notifications are silenced.
- **`TempThresholdPayload`** (the `apply-thresholds` event) carries only
  numeric thresholds, not the notify flags. Whether a notification fires is
  a backend concern; the frontend uses thresholds only for colour mapping.

### Window placement

- `pick_target_monitor` never calls `set_fullscreen` — borderless positioning
  via `set_size` + `set_decorations(false)` + `set_position` is sufficient.
- `set_decorations(false)` is always called *after* `set_size` because
  Windows `SetWindowPos` can restore `WS_CAPTION`/`WS_THICKFRAME`.
- `set_position` compensates for the DWM invisible resize border
  (`inset = inner_position − outer_position`) so content lands flush with the
  monitor edge.
- `pick_target_monitor` is called only when the profile *changes* in
  `save_settings`. Calling it unconditionally causes a ~3 px drift on every
  save due to the DWM inset compensation.
