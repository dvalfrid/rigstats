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
uses three data sources: a managed sensor sidecar (GPU/sensor data via named
pipe), sysinfo (CPU/RAM/disk/network), and WMI (hardware metadata at startup).

---

## Data Flow

```text
rigstats-sensor.exe  (sensor-sidecar/, .NET 10, Windows Service / LocalSystem)
    └─► LibreHardwareMonitor NuGet → PawnIO kernel driver
            └─► named pipe \\.\pipe\rigstats-sensors  (newline-delimited JSON)
                    └─► lhm.rs  pipe client → LhmData

sysinfo crate           CPU load/freq, RAM, disk, network, processes
wmi crate               GPU name, VRAM, RAM spec, system brand (startup only)

    └─► commands.rs     get_stats() assembles StatsPayload every tick
            └─► Tauri IPC  invoke("get-stats")
                    └─► app.js  tick() every 1 s
                            ├─► panel modules update DOM          (portrait mode)
                            └─► invoke("broadcast-stats")         (floating mode)
                                    └─► app.emit per panel window
                                            └─► panel-host.js updates DOM
```

**Tick rate:** 1 second. The sidecar pushes one JSON line per second; on
failure the last successful sample is reused so the UI never resets to `--`.

**Floating mode broadcast:** In floating mode the main window (hidden) still
runs `get-stats` and then calls `broadcast-stats`. The backend emits
`stats-broadcast` to each open `panel-{key}` window individually — settings,
about, status, and updater windows are never targeted.

---

## File Structure

```text
rig-dashboard/
├── frontend/
│   ├── index.html          Main dashboard (portrait mode)
│   ├── panel-{key}.html    One HTML file per floating panel (9 total)
│   ├── settings.html
│   ├── status.html
│   ├── about.html
│   ├── updater.html
│   ├── panel-base.css      Shared styles for all floating panel windows
│   ├── assets/
│   └── renderer/
│       ├── panels/         One JS module per panel
│       ├── panel-host.js   Shared entry for floating panel windows
│       └── *.js            Shared utilities and entry scripts
├── sensor-sidecar/         .NET 10 C# sidecar (rigstats-sensor.exe)
│   ├── Program.cs          Entry point, pipe server loop, UpdateVisitor
│   ├── SensorReader.cs     SensorPayload model + Extract() mapping
│   ├── app.manifest        requireAdministrator manifest
│   └── sensor-sidecar.csproj
├── src-tauri/
│   ├── src/                Rust source (one module per concern)
│   ├── Cargo.toml
│   └── tauri.conf.json
├── docs/
├── website/
├── assets/
└── build/
    └── installer.nsh
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
| `lhm.rs` | Named pipe client → `LhmData`; GPU selection and sensor extraction |
| `lhm_process.rs` | LHM scheduled-task query helpers (task details, connection state tracking) |
| `monitor.rs` | Display profiles, monitor selection, panel key validation |
| `settings.rs` | Settings struct, JSON persistence |
| `windows.rs` | Secondary window creation and tray-anchored positioning |
| `updater.rs` | Background update checks and install flow |
| `autostart.rs` | Windows startup registry management |
| `diagnostics.rs` | Diagnostics ZIP export |
| `logging.rs` | Stats CSV logging — `append_stats_row`, `prune_old_logs`, `current_log_path` |
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

**`AppState`** — per-tick mutable state behind a `Mutex`: `lhm_pipe`,
`settings`, `system`, `disks`, `networks`, `last_net_sample`, `last_ping_sample`,
`last_lhm`, `last_alert`, `last_battery_sample`.

**Payload structs:**

| Struct | Contents |
| --- | --- |
| `StatsPayload` | Top-level payload returned by `get_stats()` |
| `CpuStats` | Load, per-core loads, temp, freq, power |
| `GpuStats` | Load, temps, clocks, VRAM, fan, power, D3D, and `available_gpus` selector metadata |
| `RamStats` | Used/free/total, spec string, DIMM temp |
| `NetStats` | Up/down throughput, interface name, ping |
| `DiskStats` | Read/write throughput, per-drive entries |
| `DiskDrive` | Filesystem label, size, used, pct, temp |
| `MotherboardStats` | Fans, temps, voltages, chip name, board name |
| `ProcessEntry` | Process name, CPU % of total system, RAM in MB |
| `BatteryStats` | `present`, charge %, charging state, time remaining, power draw (W) |

`StatsPayload.top_processes` is a `Vec<ProcessEntry>` pre-sorted by CPU usage
and capped at 8 entries before serialisation.

#### `commands.rs`

Thin `#[tauri::command]` handlers only — no business logic. Each handler
delegates to a domain module.

`get_stats()` is the main tick handler. Per call it:

1. Fetches a fresh LHM sample (falls back to last good sample on failure)
2. Reads `settings.preferred_gpu` and passes it into LHM parsing
3. Calls `system.refresh_cpu()`, `refresh_memory()`, `refresh_processes()`
4. Collects disk throughput and drive metadata
5. Computes network throughput delta over elapsed time
6. Refreshes ping (cached, re-measured every 5 s)
7. Refreshes battery via WMI (cached, re-sampled every 10 s)
8. Assembles `StatsPayload` including top 8 processes sorted by CPU
9. Checks temperature thresholds and fires tray notifications if due

Floating mode commands:

| Command | Purpose |
| --- | --- |
| `toggle_floating_mode(enabled)` | Persists the setting, emits `apply-floating-mode`, and routes window transitions through `spawn_sync_floating_panels` / `close_floating_panels` with a mutex guard to serialize rapid toggles |
| `toggle_floating_lock` | Flips `floating_panels_locked`, persists immediately, and emits `floating-lock-changed` to all open panel windows |
| `preview_floating_scale(scale)` | Applies floating panel scale preview (`0.4..=1.0`) and re-syncs floating windows when floating mode is active |
| `set_gpu_preference(gpu_name)` | Persists the user-selected GPU name used by LHM extraction on subsequent ticks |
| `broadcast_stats(stats)` | Emits `stats-broadcast` to each open `panel-{key}` window; takes `serde_json::Value` to avoid needing `Deserialize` on `StatsPayload` |
| `save_panel_positions(positions)` | Merges `HashMap<key, PanelLayout>` into `settings.panel_layouts` and persists |
| `open_settings_window` | Opens the settings window from a floating panel's context menu |

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
| `sample_battery_wmi` | Per-tick (cached 10 s) battery query via `Win32_Battery` + `root\wmi BatteryStatus` (charge/discharge rate in mW) |

`detect_disk_model_map` builds its map via a three-table WMI join:
`Win32_DiskDrive → Win32_DiskDriveToDiskPartition → Win32_LogicalDiskToPartition`.
Results are stored in `HardwareInfo` so LHM temperatures can be matched by model
name rather than by index (stable when USB drives are inserted/removed).

#### `lhm.rs`

Named pipe client that connects to `\\.\pipe\rigstats-sensors` (written by the
`sensor-sidecar` process) and deserialises the newline-delimited JSON stream
into `LhmData`.

`fetch_lhm_pipe` is called once per tick. It reuses an established connection
stored in `AppState.lhm_pipe` (`tokio::sync::Mutex<Option<LhmPipeReader>>`).
On connection failure, errors are logged at most once every 30 s via a
`LAST_PIPE_FAIL_LOG_SECS` atomic to avoid log spam. The pipe client requests
read-only access (`.write(false)` on `ClientOptions`) because the sidecar pipe
is `PipeDirection.Out` and Windows denies write-access requests to outbound-only
pipes.

The incoming JSON deserialises into `SidecarPayload`, which mirrors the
`SensorPayload` record from `SensorReader.cs`. All extraction logic (sensor-name
matching, SensorId-prefix filtering) is implemented by the sidecar; `lhm.rs`
handles GPU selection and assembles `LhmData` from the payload.

**GPU selection:** `SidecarPayload.gpu_devices` carries one `SidecarGpuDevice`
per detected GPU (name, VRAM total, core load). `select_gpu_idx` picks the
index using the same policy as the old HTTP parser:

- Use `preferred_gpu` if it matches a candidate (case-insensitive substring)
- Otherwise pick the highest VRAM GPU (stable default)
- Tie-break by load

Extracted GPU fields: core load, core temp, hot-spot, core clock (`gpu_freq`),
memory clock (`gpu_mem_freq`), power, fan, VRAM used/total, D3D 3D load
(`gpu_d3d_3d`), D3D Video Decode load (`gpu_d3d_vdec`), plus
`gpu_devices: Vec<(device_name, vram_total_mb)>` for the frontend selector.

**Disk temperatures**, **RAM temperature**, **CPU temperature**, and
**Motherboard Super I/O** sensor extraction are all performed inside the sidecar
(`SensorReader.cs`) using the same filtering rules previously in `lhm.rs`
(SensorId prefixes `/nvme/`, `/hdd/`, `/memory/dimm/`, `/lpc/`, etc.). The
`LhmData` fields for these are populated directly from the sidecar payload.

#### `lhm_process.rs`

Retained query helpers for the legacy LHM scheduled task — used only by
`diagnostics.rs` to include task state in the diagnostics ZIP. No longer
manages LHM process lifecycle (that responsibility has moved to the sidecar).

- `get_lhm_task_details` / `get_lhm_task_diagnosis` — query `schtasks` for the
  LHM task status and parse the result into a structured string
- `track_lhm_connection_state` — logs connect/disconnect transitions at most
  once every 30 s (shares the throttle approach used by `fetch_lhm_pipe`)
- `can_reach_lhm_endpoint` — retained for diagnostics; checks whether the old
  LHM HTTP port is still reachable (helps diagnose mixed-install scenarios)

#### `monitor.rs`

- `normalize_profile` / `profile_dimensions` — canonical profile name → pixel
  dimensions
- `pick_target_monitor` — selects the best available monitor for a profile using
  an aspect-ratio + area fit score; positions the window borderless using
  `set_size` + `set_decorations(false)` + `set_position`
- `normalize_visible_panels` — validates and deduplicates panel key lists

Valid panel keys: `header`, `clock`, `cpu`, `gpu`, `ram`, `net`, `disk`,
`motherboard`, `process`, `battery`. `motherboard`, `process`, and `battery` are opt-in.

#### `settings.rs`

`Settings` struct persisted as JSON to
`%APPDATA%\se.codeby.rigstats\rigstats-settings.json`.

All fields use `#[serde(default)]` for backwards-compatible schema evolution —
new fields deserialise cleanly from older settings files. `last_seen_version`
is compared against `CARGO_PKG_VERSION` at startup to detect the first launch
after an upgrade.

Alert thresholds are stored as `thresholds: HashMap<String, ComponentThresholds>`
where `ComponentThresholds { warn: Option<u8>, crit: Option<u8> }` and the keys
are `"cpu"`, `"gpu"`, `"ram"`, `"disk"`, `"battery"`.

Threshold semantics differ by key:

- **Temperature keys** (`cpu`/`gpu`/`ram`/`disk`): alert fires when the reading
  **exceeds** the threshold. `warn < crit` is enforced.
- **Battery key**: alert fires when charge % **drops below** the threshold.
  `warn > crit` is enforced (warn at 20 %, crit at 10 % is the default).
  Only fires when discharging. Alert message: "Battery WARNING — 18% remaining".

A `settings_version: u8` field acts as a migration sentinel (0 = legacy format,
1 = current). When `load_settings` reads a version-0 file it runs
`migrate_v0_thresholds` once — copying the eight old flat fields into the map —
then re-persists. The eight legacy flat fields are kept as private
`#[serde(default, skip_serializing)]` shims so old files can be read but are
never written back.

Floating panel layout adds four fields — all `#[serde(default)]`, no migration
needed:

- **`floating_mode: bool`** — whether the app starts in floating mode.
- **`floating_panel_scale: f64`** — floating panel size multiplier in the
  range `0.4..=1.0` (sanitized in command handlers before persistence).
- **`floating_panels_locked: bool`** — when true, drag handles are disabled on
  all panel windows; toggled by `toggle_floating_lock` and persisted immediately.
- **`panel_layouts: HashMap<String, PanelLayout>`** — last known `outer_position`
  (`x: i32, y: i32`) per panel key. Positions are saved by `panel-host.js`
  after each move (debounced 500 ms) and re-applied with DWM inset compensation
  on next startup.

Multi-GPU pinning adds one field:

- **`preferred_gpu: Option<String>`** — user-selected GPU device name for stable
  display across ticks; `None` means use backend stable default (highest VRAM).

#### `windows.rs`

Creates and positions secondary windows:
`ensure_settings_window`, `ensure_about_window`, `ensure_status_window`,
`ensure_updater_window`.

Settings window uses `center_on_tray_monitor` which converts the physical tray
click position to logical pixels using `monitor.scale_factor()`, then centres
the 560×600 window on that monitor. Other secondary windows use
`tray_anchor_position` (anchored above the tray icon). Both functions read
`LAST_TRAY_CLICK_X/Y` set by `set_last_tray_click_position`; fall back to the
first available monitor when no click has been recorded.

Floating panel management:

- **`all_panel_keys()`** — canonical ordered list of the 10 panel keys; exported
  so `commands.rs` can iterate panel windows without duplicating the list.
- **`panel_base_size(key, dashboard_profile, user_scale)`** — scales each
  panel's logical dimensions to match the active profile, then applies the user
  `floating_panel_scale` multiplier.
- **`launch_floating_panels(app, state)`** — opens one frameless `always_on_top`
  `panel-{key}` window per panel and reconciles already-open windows by
  resize/show/hide instead of skipping them. Applies DWM invisible resize border
  compensation (`inner_position − outer_position`) to saved positions from
  `settings.panel_layouts`. Panels without a saved position are staggered
  diagonally. Build failures and panics are logged and skipped; the remaining
  panels are still created.
- **`sync_floating_panels(app, state)`** — reconciles open windows with the
  current settings without tearing everything down: hides unwanted panels,
  resizes/shows existing ones, then calls `launch_floating_panels` for any
  that are missing. Main window hide/show is fail-safe: it hides main only when
  at least one requested floating panel is visible, otherwise it logs and keeps
  main visible.
- **`spawn_sync_floating_panels(app)`** — schedules floating sync on the main
  thread (required by `WebviewWindowBuilder::build`) with queue coalescing so
  repeated preview/toggle calls do not flood the event loop.
- **`close_floating_panels(app)`** — hides (not closes) all open panel windows
  for fast mode switching.

`on_window_event` handles `CloseRequested` (hide-to-tray) for the main window
and re-applies `set_decorations(false)` on `Moved` for the main window and
floating panel windows only. The re-application is necessary because Windows
can restore `WS_CAPTION`/`WS_THICKFRAME` when a borderless window is dragged
between monitors with different DPI settings.

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
| `panel-host.js` | Shared entry for floating panel windows — detects panel from window label, subscribes to `stats-broadcast`, saves positions on move |
| `settings.js` | Settings window — four-tab segmented UI (Dashboard · Panels · Alerts · Appearance); active tab persisted in `localStorage`; warning alerts permanently off, only Critical has a toggle |
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
| `gpu.js` | Ring gauge, 3×2 metadata grid (TEMP, HOT SPOT, CORE CLK, MEM CLK, POWER, FAN), VRAM + GPU load bars, one optional D3D row (3D + VID side by side) hidden when both fields are `null`, and compact selector dots that persist preferred GPU via `set_gpu_preference` |
| `ram.js` | Usage bar, spec metadata, DIMM temperature |
| `network.js` | Upload/download values, dual-series sparkline |
| `disk.js` | Paginates 3 drives per page every 5 ticks when > 3 drives present |
| `motherboard.js` | Three-column layout: fans / temps / voltages; `shortLabel()` maps `"Temperature #N"` → `"TN"` |
| `process.js` | Top 8 processes: name (`.exe` stripped, 16 char max), CPU %, RAM. Names are HTML-escaped before `innerHTML` insertion. `truncateName` and `formatRam` exported for unit tests. |
| `battery.js` | Charge % (big number), dynamic bar colour (accent=charging, green >50 %, amber 20–50 %, red <20 %), status (CHARGING / DISCHARGING), time remaining, live power draw (W) colour-coded by rate (green <12 W, amber 12–20 W, red >20 W, no colour when charging). Shows "NO BATTERY" when `present == false` (desktops). |
| `clock.js` | Time, weekday, date |

#### `settings.js`

- `panelOrder` tracks all panels (visible + hidden) in user-defined sequence
- `hiddenPanels` is a `Set` of unchecked keys
- Drag-to-reorder uses the Pointer Events API with `setPointerCapture` instead
  of the HTML5 Drag API (which shows a prohibition cursor inside WebView2)
- Floating mode preview toggles are serialized in the renderer to avoid
  overlapping IPC transitions; the latest requested state is queued
- Floating panel scale slider previews are sent via `preview-floating-scale`
  and restored on cancel

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
| `cpu` | CPU | ✓ | sysinfo · sidecar |
| `gpu` | GPU | ✓ | sidecar |
| `ram` | RAM | ✓ | sysinfo · WMI · sidecar |
| `net` | Network | ✓ | sysinfo |
| `disk` | Storage | ✓ | sidecar · sysinfo |
| `motherboard` | Motherboard | opt-in | sidecar · WMI |
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

| File | Source | Key fields |
| --- | --- | --- |
| `manifest.json` | inline | Unix timestamp + `CARGO_PKG_VERSION` |
| `debug.log` | `std::fs::read(debug_log_path)` | Full startup + runtime log |
| `install.log` | `%PROGRAMDATA%\se.codeby.rigstats\` | Written by NSIS installer |
| `settings.json` | `AppState.settings` snapshot | All user settings |
| `sidecar-parsed.json` | `AppState.last_lhm` snapshot | Extracted values: temps, clocks, fans, voltages |
| `sidecar-log.txt` | `%PROGRAMDATA%\se.codeby.rigstats\rigstats-sensor.log` | Sidecar file log: start/stop, connect/disconnect, errors |
| `sidecar-service.txt` | `sc query` + `sc qc` + legacy schtasks | Service status, config, and any lingering LHM scheduled tasks |
| `hardware.json` | PowerShell `Get-CimInstance` | OS, CPU, GPU, board, RAM modules, disks |
| `battery.json` | WMI probes + `AppState.last_battery_sample` | See battery diagnostics below |
| `environment.txt` | env vars + Windows registry | Arch, build number, hostname |
| `sysinfo.json` | `AppState` + WMI shell probes | See sysinfo diagnostics below |
| `displays.json` | Tauri monitor list | Resolution, position, scale, fit score, which was selected |

### `sysinfo.json` — key fields

| Field | What it tells you |
| --- | --- |
| `ramSpec` | What `detect_ram_spec()` produced at startup. `"RAM"` = detection failed — check `ramSpecShellTest`. |
| `ramSpecShellTest` | Runs the exact same PowerShell command as `detect_ram_spec`. Has `stdout`, `stderr`, `exit_code`. Non-zero exit or non-empty `stderr` explains the failure immediately (e.g. the `\| Out-String` bug that caused exit 1 with "An empty pipe element is not allowed"). |
| `diskModelMap` | Drive-letter → model-name map built at startup. Empty map = WMI join failed — check `diskModelMapProbe`. |
| `diskModelMapProbe` | Runs the WMI three-table join used by `detect_disk_model_map`. Empty result means the BIOS doesn't expose the partition associations. |
| `wmiAvailable` | Whether WMI was reachable at startup. `false` means all WMI-sourced fields (RAM type/speed, GPU VRAM, etc.) will be missing. |

### `battery.json` — key fields

| Field | What it tells you |
| --- | --- |
| `cached.present` | `false` = no battery detected at last 10 s sample. On desktops this is expected. |
| `cached.ageSecs` | Seconds since the last battery WMI sample. Should be ≤ 10 on a live system. |
| `cached.powerW` | `null` = `root\wmi BatteryStatus` didn't return a rate — check `wmiStatusProbe`. |
| `win32Battery` | Raw `Win32_Battery` values: `EstimatedChargeRemaining`, `BatteryStatus`, `EstimatedRunTime`. `exit_code` 0 = query succeeded. Non-empty `stderr` = access or class error. |
| `wmiBatteryStatus` | Raw `root\wmi BatteryStatus` values: `ChargeRate`, `DischargeRate` in mW. Many desktop drivers don't expose this class — `"(no data)"` is expected on non-laptop systems. |

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

- **Sidecar fallback** — the last successful sample is kept in memory so the UI
  never resets to `--` when the sidecar pipe is temporarily unavailable.
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
- `on_window_event` re-applies `set_decorations(false)` on every `Moved` event
  for **all** windows (not just `"main"`), because Windows can re-enable the
  title bar when any borderless window crosses a DPI boundary.

### Floating mode

- **Stats delivery** — the hidden main window runs `get-stats` once per second
  as normal, then calls `broadcast-stats`. The backend iterates `all_panel_keys`
  and emits `stats-broadcast` directly to each open `panel-{key}` window. This
  keeps exactly one IPC round-trip per tick regardless of how many panels are open.
- **Drag on transparent windows** — `data-tauri-drag-region` is unreliable on
  transparent borderless WebView2 windows. `panel-host.js` instead calls
  `invoke("start-window-drag")` explicitly on `pointerdown` (capture phase),
  guarding against interactive elements and scrollable regions.
- **Sync vs launch** — `sync_floating_panels` reconciles the current window set
  against settings without teardown, enabling live preview of panel
  visibility changes in Settings. `launch_floating_panels` only creates
  windows that do not yet exist.
- **Position persistence** — `panel-host.js` reads `currentWindow.outerPosition()`
  500 ms after each `tauri://moved` event and persists it via `save-panel-positions`.
  Positions are stored as raw `outer_position` values; DWM inset compensation is
  re-applied by `launch_floating_panels` at next startup.
