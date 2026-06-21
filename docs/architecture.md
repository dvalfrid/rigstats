# Architecture

## Contents

- [Overview](#overview)
- [Data Flow](#data-flow)
- [File Structure](#file-structure)
- [Backend Modules](#backend-modules)
- [Dashboard Panels](#dashboard-panels)
- [Diagnostics Export](#diagnostics-export)
- [Design Decisions](#design-decisions)

---

## Overview

RIGStats is a Windows-only **egui** desktop app (`src-egui/`) that displays
live hardware telemetry on a secondary portrait or landscape monitor. The UI is rendered
natively via eframe/wgpu — no WebView2, no JavaScript at runtime. The backend
is Rust and uses three data sources: a managed sensor sidecar (GPU/sensor data
via named pipe), sysinfo (CPU/RAM/disk/network), and WMI (hardware metadata at
startup).

The repository is a Cargo workspace with two members:

| Crate | Path | Role |
| --- | --- | --- |
| `rigstats-backend` | `rigstats-backend/` | Shared lib — all backend modules, no framework coupling |
| `rigstats-egui` | `src-egui/` | Production binary — eframe app, all panels, tray, settings windows |

---

## Data Flow

```text
rigstats-sensor.exe  (sensor-sidecar/, .NET 10, Windows Service / LocalSystem)
    └─► LibreHardwareMonitor NuGet → PawnIO kernel driver
            └─► named pipe \\.\pipe\rigstats-sensors  (newline-delimited JSON)
                    └─► lhm.rs (rigstats-backend): pipe client → LhmData struct

sysinfo crate           CPU load/freq, RAM, disk, network, processes
wmi crate               GPU name, VRAM, RAM spec, system brand (startup only)

    └─► poll_loop (src-egui/poll.rs): samples hardware → PollStats → mpsc::Sender
            └─► egui UI thread: receives PollStats each 1 s tick
                    ├─► panel draw() calls (portrait mode — single window)
                    └─► panel draw() calls (floating mode — one viewport per panel)
```

**Tick rate:** 1 second. The sidecar pushes one JSON line per second; on
failure the last successful sample is reused so the UI never resets to `--`.

**Floating mode:** Each visible panel is rendered in its own `egui` deferred
viewport (a separate OS window). The main window is moved off-screen (`-32000,
-32000`) rather than hidden so egui continues ticking. Panel positions are
persisted to settings on change via a dirty-flag debounce.

---

## File Structure

```text
rig-dashboard/
├── src-egui/               egui binary (rigstats.exe)
│   ├── src/
│   │   ├── main.rs         Entry point, eframe run_native, RigStatsApp, rendering
│   │   ├── geometry.rs     Profile dimensions, monitor selection, pinned position
│   │   ├── poll.rs         Poll thread, PollStats/DriveInfo/ProcessInfo data types
│   │   ├── tray.rs         System tray icon, menu, TrayCmd, panel-label helpers
│   │   ├── theme.rs        AppTheme, colours, panel_frame(), dialog button API
│   │   ├── brand.rs        Brand logo PNG loading
│   │   ├── tempcolor.rs    temp_color() — value → green/yellow/red
│   │   ├── ring.rs         Ring gauge renderer
│   │   ├── spark.rs        Sparkline ring buffer
│   │   ├── update_check.rs Update detection, download, installer launch
│   │   ├── win_opacity.rs  SetLayeredWindowAttributes wrapper
│   │   ├── win32_dark_mode.rs  Dark-mode tray context menu
│   │   ├── panels/         One file per panel (cpu, gpu, ram, net, disk, …)
│   │   └── windows/        Secondary windows (settings, about, status, updater)
│   ├── assets/             Embedded PNGs (brand logos, tray icon)
│   └── Cargo.toml
├── rigstats-backend/       Shared Rust lib (telemetry, hardware, settings)
│   └── src/
│       ├── stats.rs        StatsPayload and all sub-structs, HardwareInfo, AppState
│       ├── hardware.rs     WMI/PowerShell hardware detection at startup
│       ├── lhm.rs          Named pipe client → LhmData; GPU selection
│       ├── lhm_process.rs  LHM connection state tracking
│       ├── monitor.rs      Display profiles, panel key validation
│       ├── settings.rs     Settings struct, JSON persistence, atomic_write
│       ├── autostart.rs    Windows startup registry management
│       ├── logging.rs      Stats CSV logging
│       └── debug.rs        Debug log helpers
├── sensor-sidecar/         .NET 10 C# sidecar (rigstats-sensor.exe)
│   ├── Program.cs          Entry point, pipe server loop
│   ├── SensorReader.cs     SensorPayload model + Extract() mapping
│   └── sensor-sidecar.csproj
├── sensor-sidecar.Tests/   xUnit tests: SensorReader.Extract rules + JSON contract
├── docs/
├── website/
├── assets/                 Screenshot PNGs for website/README
└── build/
    ├── installer.nsi       NSIS installer script
    └── pawnio/             Signed PawnIO kernel driver files
```

---

## Backend Modules

### Quick reference

**`rigstats-backend/src/`** — shared lib consumed by the egui binary:

| Module | Responsibility |
| --- | --- |
| `stats.rs` | `StatsPayload` and all sub-structs; `HardwareInfo` (startup constants) and `AppState` (per-tick state) |
| `hardware.rs` | WMI/PowerShell hardware detection at startup |
| `lhm.rs` | Named pipe client → `LhmData`; GPU selection and sensor extraction |
| `lhm_process.rs` | LHM connection state tracking, 30 s log throttle |
| `monitor.rs` | Display profiles, monitor selection, panel key validation |
| `settings.rs` | `Settings` struct, JSON persistence, `atomic_write` |
| `autostart.rs` | Windows startup registry management (HKCU run key) |
| `logging.rs` | Stats CSV logging — `append_stats_row`, `prune_old_logs`, `current_log_path` |
| `debug.rs` | Debug log helpers — no deps on other modules |

**`src-egui/src/`** — egui binary:

| Module | Responsibility |
| --- | --- |
| `main.rs` | Entry point, eframe `run_native`, `RigStatsApp` + `eframe::App` impl, `PanelThresholds`, panel rendering |
| `geometry.rs` | Profile dimensions (`profile_to_size`), monitor enumeration/selection, pinned-position resolution (unit-tested) |
| `poll.rs` | Background `poll_loop` (tokio), `PollStats`/`DriveInfo`/`ProcessInfo` data types, CSV log payload mapping |
| `tray.rs` | System tray icon + menu, `TrayCmd` channel, `load_app_icon`, `panel_label`/`panel_initial_h` |
| `theme.rs` | `AppTheme`, colour constants, `panel_frame()`, sparkline/bar helpers, dialog button API |
| `brand.rs` | Brand logo PNG loading (13 logos embedded at compile time) |
| `tempcolor.rs` | `temp_color(value, warn, crit)` → green/yellow/red |
| `update_check.rs` | Update detection (`check()`), download with progress, `launch_installer()` |
| `win_opacity.rs` | `SetLayeredWindowAttributes` wrapper for window-level opacity |
| `win32_dark_mode.rs` | Dark-mode tray context menu via `uxtheme.dll` ordinals |
| `panels/` | One file per panel — each exports `draw(ui, stats, opacity, th, sc, ...)` returning `egui::Rect` |
| `windows/` | Secondary windows: `settings.rs`, `about.rs`, `status.rs`, `updater.rs` |

### Module details

#### `main.rs`

Entry point: settings load, window placement, and `eframe::run_native`, hosting `RigStatsApp` and its per-frame `ui()` loop. Monitor selection lives in `geometry.rs`, tray setup in `tray.rs`, and the poll thread in `poll.rs`. Starts a tokio runtime for the poll loop and the background auto-update check (first check after 10 s, then every 6 h). The poll loop samples hardware once per second and sends a `PollStats` snapshot to the UI thread via `mpsc::SyncSender`. On startup, checks for `--just-updated=VERSION` argument (set by the NSIS in-app updater) and opens the updater dialog in `JustUpdated` state if present.

#### `stats.rs`

Defines two shared state structs and all serializable telemetry payload
structs.

**`HardwareInfo`** — startup-detected constants, held behind a `Mutex` and read once at poll-thread start: `disk_model_map`, `ram_spec`, `ram_details`, `gpu_vram_total_mb`, `system_brand`, `mb_name`, `ping_target`, `sysinfo_available`, `wmi_available`.

**`AppState`** — per-tick mutable state behind a `Mutex`: `lhm_pipe`, `settings`, `system`, `disks`, `networks`, `last_net_sample`, `last_ping_sample`, `last_lhm`, `last_alert`, `last_battery_sample`.

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


#### `hardware.rs`

All startup hardware detection. Each function tries WMI first, falls back to
PowerShell CIM on failure.

| Function | What it detects |
| --- | --- |
| `detect_gpu_name` | Primary discrete GPU name |
| `detect_gpu_vram_total_mb` | VRAM total (MB) |
| `detect_gpu_drivers` | Installed GPU driver name/version/date per adapter (`Vec<GpuDriverInfo>`); `driver_age_days` derives age from `DriverDate` |
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
`gpu_devices: Vec<(device_name, vram_total_mb)>` for the GPU selector in the UI.

**Disk temperatures**, **RAM temperature**, **CPU temperature**, and
**Motherboard Super I/O** sensor extraction are all performed inside the sidecar
(`SensorReader.cs`) using the same filtering rules previously in `lhm.rs`
(SensorId prefixes `/nvme/`, `/hdd/`, `/memory/dimm/`, `/lpc/`, etc.). The
`LhmData` fields for these are populated directly from the sidecar payload.

`SensorReader.Extract` and the `SensorPayload` JSON contract are covered by the
`sensor-sidecar.Tests` xUnit project (run by `cargo xtask verify`).

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

Fullscreen (fill-screen) mode adds two fields — both `#[serde(default ...)]`, no
migration needed:

- **`fullscreen_mode: bool`** — when true (and not floating), the fixed window
  fills the portrait monitor's height instead of fitting panel content; the
  dashboard background fills the rest. Width stays at the profile width so panel
  proportions never stretch.
- **`fullscreen_align: String`** — `"top"` or `"center"` (default `"center"`):
  where the panel stack sits within the filled window.

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
hours (first check after 10 s). Notifies the UI when a newer version is found.
Also exposes `check_for_update`, `install_update`, and `open_updater_window`.

#### `autostart.rs`

Per-user Windows autostart via
`HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run`. Uses `winreg` for direct
registry access (no subprocesses). Also manages `StartupApproved\Run` to stay
in sync with Windows Settings → Apps → Startup.

#### `debug.rs`

`append_debug_log` (INFO) and the `log_debug` / `log_warn` / `log_error` level
variants (all via `append_debug_log_lvl` + the `LogLevel` enum), `reset_debug_log`,
`run_hidden_command`, `unix_now_secs`. Log lines are formatted as
`[YYYY-MM-DD HH:MM:SS] [LEVEL] message` using local time (`chrono`).
No dependencies on other crate modules — safe to import from anywhere.

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
| `battery` | Battery | opt-in | sidecar · WMI |

Panel visibility and order are saved in `Settings.visible_panels` and
validated by `normalize_visible_panels` in the backend.

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
| `debug.log` | `std::fs::read(debug_log_path)` | Current session: first lines always include `settings dir`, `os_dark_mode`, settings summary. Ends with `shutdown: clean` on normal exit. |
| `debug-prev.log` | `rigstats-debug-prev.log` (renamed from `debug.log` on previous startup) | Previous session log — preserved so crash evidence survives restart. Missing `shutdown: clean` at end = crash. |
| `install.log` | `%PROGRAMDATA%\se.codeby.rigstats\` | Written by NSIS installer |
| `settings.json` | `AppState.settings` snapshot | All user settings |
| `sidecar-parsed.json` | `AppState.last_lhm` snapshot | Extracted values: temps, clocks, fans, voltages |
| `sidecar-log.txt` | `%PROGRAMDATA%\se.codeby.rigstats\rigstats-sensor.log` | Sidecar file log: start/stop, connect/disconnect, errors |
| `sidecar-service.txt` | `sc query` + `sc qc` + legacy schtasks | Service status, config, and any lingering LHM scheduled tasks |
| `hardware.json` | PowerShell `Get-CimInstance` | OS, CPU, GPU, board, RAM modules, disks |
| `battery.json` | WMI probes + `AppState.last_battery_sample` | See battery diagnostics below |
| `environment.txt` | `std::env::var` | `USERNAME`, `USERDOMAIN`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `COMPUTERNAME`, `PROCESSOR_ARCHITECTURE` — exposes child/standard account path redirections |
| `event-log.txt` | PowerShell `Get-WinEvent` | Windows Application Event Log: rigstats errors and critical events — catches OS-level crashes not recorded in the in-app log |
| `sysinfo.json` | `AppState` + WMI shell probes | See sysinfo diagnostics below |
| `displays.json` | Monitor list from `pick_monitor()` | Resolution, position, scale, fit score, which was selected |

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
  a backend concern; the UI uses thresholds only for colour mapping.

### Window placement

- `pick_window_rect_for_profile` (in `src-egui/src/main.rs`) targets a monitor only when its resolution **matches** the active profile (both dimensions within ~10 %); among matches it picks the closest fit, so a dedicated strip/secondary that fits the profile is used automatically. When no monitor matches it falls back to the **primary** monitor (top-left at the virtual origin `0,0`), then to the first monitor. The pure selection step is `select_profile_monitor`, which is unit-tested. The egui window is positioned at the chosen monitor's top-left origin.
- **Pinned dashboard**: when `Settings::dashboard_pinned` is set, the non-floating window restores its saved position from `Settings::pinned_positions[profile]` (via `pinned_position()` / `guard_panel_position`) instead of auto-targeting — both at startup (`main()`) and on settings reload (`fixed_window_geometry`). A padlock in the fixed-mode drag strip toggles the pin and captures the current position. See CLAUDE.md → "Pinned dashboard (non-floating)".
- The egui window is borderless and undecorated — `eframe::NativeOptions` sets `decorated: false`.

### Dashboard profiles & orientation

- Profiles are named with an orientation prefix: `portrait-*` (tall, e.g. `portrait-xl` 450×1920) and `landscape-*` (wide, e.g. `landscape-xl` 1920×450). Each landscape profile is the transpose of the matching portrait profile; `*-side` portrait profiles map to `*-top` landscape profiles. `profile_is_landscape(profile)` (key prefix check) drives the orientation branches.
- **Portrait** renders all visible panels in a single vertical stack; the window width is fixed to the profile width and the height fits panel content per frame.
- **Landscape** renders panels in an **adaptive grid** (`RigStatsApp::render_landscape_grid`). The column count is chosen to maximise the per-cell content scale (ties broken toward fewer rows); every cell is the same size and the panel content scale `sc` is derived from the cell dimensions, so panels shrink/grow to fill any landscape resolution. The window is fixed to the full profile size (no per-frame content-fit). Header and clock are ordinary equal-sized cells.
- Both orientations share `draw_one_panel`, so every panel, theme, and threshold works identically; floating mode is orientation-independent (each panel is its own positioned window).

### Floating mode

- **Stats delivery** — the single egui window receives `StatsPayload` from the poll thread via `mpsc::Receiver` and passes data to each panel's `draw()` call. No separate broadcast needed.
- **Drag** — panels in floating mode render a drag-zone overlay (three dots + padlock icon) painted directly onto the panel rect via `ui.painter()`. A click on the padlock toggles lock state via an egui temp-data key `"toggle_lock"`.
- **Position persistence** — floating panel positions are stored in `settings.panel_layouts` (keyed by panel key) and written to `rigstats-settings.json` on every `Save` in Settings.
