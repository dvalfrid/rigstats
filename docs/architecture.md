# Architecture

## File Structure

```text
rig-dashboard/
|- frontend/
|  |- index.html
|  |- settings.html
|  |- status.html
|  |- about.html
|  |- updater.html
|  |- assets/
|  \- renderer/
|     |- panels/
|     \- *.js
|- src-tauri/
|  |- src/
|  |- Cargo.toml
|  \- tauri.conf.json
|- assets/
|- vendor/lhm/
|- build/
|  |- installer.nsh
|  \- lhm-default/
|- docs/
\- package.json
```

## Backend Modules (`src-tauri/src/`)

- **`main.rs`** — Tauri builder, tray icon, lifecycle. Initializes `AppState` at startup (one-time hardware detection via WMI/sysinfo), picks the best monitor for the profile, starts LHM.
- **`stats.rs`** — `AppState` struct (shared mutable state behind `Mutex`), all serializable payload structs (`StatsPayload`, `CpuStats`, etc.).
- **`commands.rs`** — Thin `#[tauri::command]` handlers only. Each handler delegates to a domain module; no business logic lives here.
- **`debug.rs`** — `append_debug_log`, `reset_debug_log`, `run_hidden_command`, `unix_now_secs`. No dependencies on other crate modules — safe to import from anywhere.
- **`hardware.rs`** — WMI structs and all startup hardware detection: `detect_gpu_name`, `detect_gpu_vram_total_mb`, `detect_system_brand`, `classify_system_brand`, `detect_model_name`, `detect_ram_spec`, `detect_ram_details`, `detect_ping_target`, `sample_ping_ms`, `probe_wmi_status`, `detect_disk_model_map`. Each function tries WMI first, falls back to PowerShell CIM. `detect_disk_model_map` builds a `HashMap<drive_letter, model_name>` via a three-table WMI join (`Win32_DiskDrive → Win32_DiskDriveToDiskPartition → Win32_LogicalDiskToPartition`); this mapping is stored in `AppState` at startup so that LHM disk temperatures can be matched by model name rather than index (index-based matching would shift temperatures to wrong drives when a USB drive is inserted).
- **`lhm.rs`** — HTTP client that fetches LHM's `/data.json`, flattens the nested sensor tree into `Vec<FlatNode>` (each node carries `text`, `value`, `parent`, `grandparent`, and `sensor_id`), then extracts GPU/CPU/disk/network/RAM metrics. Disk temperatures are identified by `SensorId` prefix (`/nvme/`, `/hdd/`, `/ata/`, `/scsi/`) rather than by sensor name, preventing motherboard or RAM thermal sensors from leaking into disk readings. Warning Composite and Critical Composite threshold sensors are excluded; the highest real temperature per device is stored as `disk_temps: Vec<(device_name, °C)>` in `LhmData`. RAM temperature uses `SensorId` prefix `/memory/dimm/` with suffix `/temperature/0` — index 0 is the actual reading, while indices 1–5 are resolution and Low/High/CriticalLow/CriticalHigh limits; the max reading across all populated DIMM slots is stored as `ram_temp: Option<f64>` (DDR5 always has sensors; DDR4 coverage varies by module).
- **`lhm_process.rs`** — LHM process lifecycle: `ensure_lhm_running` (scheduled task → direct spawn), `can_reach_lhm_endpoint`, `get_lhm_task_details`, `track_lhm_connection_state` (connect/disconnect logging with 30 s throttle).
- **`monitor.rs`** — Profile definitions (`normalize_profile`, `profile_dimensions`), monitor selection (`pick_target_monitor`, `fit_score`), panel visibility normalisation (`normalize_visible_panels`).
- **`windows.rs`** — Secondary window creation and tray-anchored positioning: `ensure_settings_window`, `ensure_about_window`, `ensure_status_window`, `ensure_updater_window`, `on_window_event`, `set_last_tray_click_position`.
- **`updater.rs`** — Auto-update logic: `spawn_background_check` starts a background loop that checks for updates every 6 hours (first check after 10 s); emits `update-available` event to the frontend when a newer version is found. Also exposes `check_for_update`, `install_update`, and `open_updater_window` commands.
- **`diagnostics.rs`** — `collect_diagnostics` Tauri command and helpers that gather system info into a ZIP archive for bug reports.
- **`autostart.rs`** — Per-user Windows autostart via `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run`. Uses `winreg` for direct registry access (no subprocesses). Also manages `StartupApproved\Run` to stay in sync with Windows Settings > Apps > Startup.
- **`settings.rs`** — `Settings` struct (opacity, model name, dashboard profile, always-on-top, autostart enabled, visible panels, last seen version, eight optional temperature thresholds for CPU/GPU/RAM/Disk, alert cooldown, and `notify_on_warn`/`notify_on_crit` flags), JSON persistence to Tauri app data dir. `last_seen_version` is compared against `CARGO_PKG_VERSION` at startup to detect the first launch after an upgrade. All threshold fields use `#[serde(default)]` so existing settings files without those keys deserialise cleanly.

## Renderer Modules (`frontend/renderer/`)

- **`environment.js`** — Detects whether running inside Tauri. Exports `backend` (thin wrapper around `window.__TAURI__`) and `IS_DESKTOP`. All renderer modules go through this instead of accessing Tauri globals directly.
- **`app.js`** — Main dashboard orchestrator. Drives the 1-second poll loop (`tick()`), applies settings/profile/opacity from Tauri events, manages brand preview mode. `applyVisiblePanels` hides/shows panels and reorders them in the DOM to match the saved order.
- **`systemInfo.js`** — Host name, CPU model, GPU model, and branding/logo wiring.
- **`clock.js`** — Local time and uptime rendering.
- **`spark.js`** — Sparkline history ring buffer and canvas drawing.
- **`tempColors.js`** — Maps temperature values to color thresholds for heat indicators.
- **`vendorBranding.js`** — Pure mapping: brand key → logo asset + label. No DOM access; testable in Node.
- **`simulator.js`** — Browser-mode fake stats for developing the UI without the Tauri backend.
- **`panels/`** — One file per panel: `cpu.js`, `gpu.js`, `ram.js`, `network.js`, `disk.js`. Each exports an `update*Panel(stats, history, pushHistory, thresholds)` function. `thresholds` carries `{ warn, crit }` values for temperature colour mapping; defaults are applied when the argument is absent so panels work in browser/simulator mode without backend settings.
- **`app.js`** — `applyThresholds(s)` builds per-component `{ warn, crit }` objects from a settings or `TempThresholdPayload` snapshot and stores them in the module-level `thresholds` variable. Called once at startup from the `get-settings` response and then on every `apply-thresholds` event emitted by the backend after `save_settings`. This keeps panel colours in sync without requiring a full settings reload.
- **`settings.js`** — Settings window entry script. Manages panel visibility and order: `panelOrder` tracks all panels (visible + hidden) in user-defined sequence; `hiddenPanels` is a `Set` of keys the user has unchecked. `renderPanelToggles` re-renders the list from those two structures. Drag-to-reorder uses the Pointer Events API (`pointerdown`/`pointermove`/`pointerup` on each `≡` handle with `setPointerCapture`) and a fixed-position ghost element to work around WebView2's HTML5 drag incompatibility.
- **`about.js`** / **`status.js`** / **`updater.js`** — Entry scripts for the About, Status, and Updates & Changelog secondary windows. `updater.js` invokes `check-for-update` on load, renders release notes from `latest.json` when an update is available (combined with the bundled CHANGELOG.md for full history), and drives the `install-update` download + progress flow.

## Diagnostics Export (`collect_diagnostics`)

The `collect_diagnostics` Tauri command is invoked from the Status dialog's **Collect Diagnostics…** button.
It produces a self-contained ZIP for bug reports and sensor-support work.

### Collection flow

1. A native Windows save-file dialog is opened on a dedicated OS thread via `rfd::FileDialog` (Win32 requires STA; spawning a blocking task avoids blocking the async runtime).
2. If the user cancels, the command returns `Ok(None)` and no file is written.
3. If the user confirms a path, the following data is assembled and written into a single `zip::ZipWriter` with Deflate compression. The path is logged to the debug log and returned to the renderer as `Ok(Some(path))`.

| File in ZIP | Source | Notes |
| --- | --- | --- |
| `manifest.json` | inline | Unix timestamp + `CARGO_PKG_VERSION` |
| `debug.log` | `std::fs::read(debug_log_path)` | Full file, not the tail shown in UI |
| `settings.json` | serde_json of current `Settings` from `AppState` | Read-only snapshot |
| `lhm-data.json` | `GET localhost:8085/data.json` | 3 s timeout; error payload on failure |
| `hardware.json` | `diag_collect_hardware()` — PowerShell `Get-CimInstance` | OS, CPU, GPU, board, RAM |
| `sched-task.txt` | `diag_collect_tasks()` — `schtasks /Query /V` | Both LHM task names |
| `environment.txt` | `diag_collect_environment()` — env vars + Windows registry | Arch, build, hostname |
| `install.log` | `diag_collect_installer_log()` — reads `rigstats-install.log` from app data dir | Written by the NSIS installer; contains LHM exe path and task registration exit codes |
| `sysinfo.json` | `diag_collect_sysinfo()` — reads shared `AppState` mutexes | CPU brand, RAM totals, mount points, interfaces |
| `displays.json` | `diag_collect_displays()` — reads available monitors via Tauri | Each monitor's resolution, position, scale factor, fit score, and which one was selected for the current profile |

---

## Design Decisions

- Disk temperatures are matched to drive letters by physical disk model name (startup WMI query) rather than by index. Index-based matching would silently assign temperatures to the wrong drives when a USB device is inserted and shifts sysinfo's volume list. Model-name matching is stable regardless of insertion order.
- LHM disk sensor identification uses the `SensorId` field (`/nvme/`, `/hdd/`, `/ata/`, `/scsi/` prefixes) instead of sensor text. Filtering by text alone would pick up motherboard chip sensors (e.g. `Temperature #1` on Nuvoton NCT6799D) and RAM DIMM sensors that happen to share the same parent-category name.
- RAM DIMM temperature identification uses both `SensorId` prefix `/memory/dimm/` and suffix `/temperature/0`. Each DIMM slot exposes 6 temperature-category sensors (actual reading at index 0, resolution at 1, and four threshold limits at 2–5). Filtering to index 0 alone is robust — no text matching needed, no risk of picking up threshold values regardless of locale or LHM version.
- `main.rs` stays thin and delegates implementation to focused modules.
- `#[tauri::command]` functions live only in `commands.rs` — domain modules contain no Tauri command annotations.
- Latest successful LHM sample is kept in memory to avoid UI flicker when LHM times out.
- Payloads are validated before rendering to avoid repainting with malformed transient data.
- Poll ticks do not overlap, which avoids out-of-order UI updates.
- `frontend/` is the Tauri web root, keeping runtime assets and HTML together.
- No bundler or framework — vanilla ES modules are served directly by Tauri's asset server.
- The dashboard uses CSS flexbox (not grid) so panels can be reordered in the DOM via `appendChild`. Each panel class (`panel-header`, `panel-cpu`, etc.) carries its own fixed height via a CSS variable, decoupling height from DOM position.
- Drag-to-reorder in the Settings window uses the Pointer Events API with `setPointerCapture` instead of the HTML5 Drag API, which shows a prohibition cursor inside WebView2.
- Temperature alerts use a `Mutex<HashMap<String, Instant>>` in `AppState` to track the last fire time per component+level key (e.g. `"cpu_warning"`). Cooldown is enforced entirely in the backend so the frontend never needs to reason about timing. `notify_on_warn`/`notify_on_crit` flags gate whole alert levels independently, allowing colour indicators to remain active while notifications are silenced. Disk alerts fire only on the hottest drive; per-drive alerting is not supported.
- `TempThresholdPayload` (the `apply-thresholds` event payload) carries only the numeric thresholds, not the notify flags. The frontend uses thresholds exclusively for colour mapping; whether a notification fires is a backend concern.
