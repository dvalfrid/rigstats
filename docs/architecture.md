# Architecture

## File Structure

```text
rig-dashboard/
|- frontend/
|  |- index.html
|  |- settings.html
|  |- status.html
|  |- about.html
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
- **`hardware.rs`** — WMI structs and all startup hardware detection: `detect_gpu_name`, `detect_gpu_vram_total_mb`, `detect_system_brand`, `classify_system_brand`, `detect_model_name`, `detect_ram_spec`, `detect_ram_details`, `detect_ping_target`, `sample_ping_ms`, `probe_wmi_status`. Each function tries WMI first, falls back to PowerShell CIM.
- **`lhm.rs`** — HTTP client that fetches LHM's `/data.json`, flattens the nested sensor tree into `Vec<FlatNode>`, then extracts GPU/CPU/disk/network metrics by parent+text name pairs.
- **`lhm_process.rs`** — LHM process lifecycle: `ensure_lhm_running` (scheduled task → direct spawn), `can_reach_lhm_endpoint`, `get_lhm_task_details`, `track_lhm_connection_state` (connect/disconnect logging with 30 s throttle).
- **`monitor.rs`** — Profile definitions (`normalize_profile`, `profile_dimensions`), monitor selection (`pick_target_monitor`, `fit_score`), panel visibility normalisation (`normalize_visible_panels`).
- **`windows.rs`** — Secondary window creation and tray-anchored positioning: `ensure_settings_window`, `ensure_about_window`, `ensure_status_window`, `on_window_event`, `set_last_tray_click_position`.
- **`diagnostics.rs`** — `collect_diagnostics` Tauri command and helpers that gather system info into a ZIP archive for bug reports.
- **`settings.rs`** — `Settings` struct (opacity, model name, dashboard profile, always-on-top, visible panels), JSON persistence to Tauri app data dir.

## Renderer Modules (`frontend/renderer/`)

- **`environment.js`** — Detects whether running inside Tauri. Exports `backend` (thin wrapper around `window.__TAURI__`) and `IS_DESKTOP`. All renderer modules go through this instead of accessing Tauri globals directly.
- **`app.js`** — Main dashboard orchestrator. Drives the 1-second poll loop (`tick()`), applies settings/profile/opacity from Tauri events, manages brand preview mode.
- **`systemInfo.js`** — Host name, CPU model, GPU model, and branding/logo wiring.
- **`clock.js`** — Local time and uptime rendering.
- **`spark.js`** — Sparkline history ring buffer and canvas drawing.
- **`tempColors.js`** — Maps temperature values to color thresholds for heat indicators.
- **`vendorBranding.js`** — Pure mapping: brand key → logo asset + label. No DOM access; testable in Node.
- **`simulator.js`** — Browser-mode fake stats for developing the UI without the Tauri backend.
- **`panels/`** — One file per panel: `cpu.js`, `gpu.js`, `ram.js`, `network.js`, `disk.js`. Each exports an `update*Panel(stats, history, pushHistory)` function.
- **`settings.js`** / **`about.js`** / **`status.js`** — Entry scripts for the secondary windows.

## Diagnostics Export (`collect_diagnostics`)

The `collect_diagnostics` Tauri command is invoked from the Status dialog's **Collect Diagnostics…** button.
It produces a self-contained ZIP for bug reports and sensor-support work.

### Collection flow

1. A native Windows save-file dialog is opened on a dedicated OS thread via `rfd::FileDialog` (Win32 requires STA; spawning a blocking task avoids blocking the async runtime).
2. If the user cancels, the command returns `Ok(None)` and no file is written.
3. If the user confirms a path, the following data is assembled:

| File in ZIP | Source | Notes |
| --- | --- | --- |
| `manifest.json` | inline | Unix timestamp + `CARGO_PKG_VERSION` |
| `debug.log` | `std::fs::read(debug_log_path)` | Full file, not the tail shown in UI |
| `settings.json` | serde_json of current `Settings` from `AppState` | Read-only snapshot |
| `lhm-data.json` | `GET localhost:8085/data.json` | 3 s timeout; error payload on failure |
| `hardware.json` | `diag_collect_hardware()` — PowerShell `Get-CimInstance` | OS, CPU, GPU, board, RAM |
| `sched-task.txt` | `diag_collect_tasks()` — `schtasks /Query /V` | Both LHM task names |
| `environment.txt` | `diag_collect_environment()` — env vars + Windows registry | Arch, build, hostname |
| `sysinfo.json` | `diag_collect_sysinfo()` — reads shared `AppState` mutexes | CPU brand, RAM totals, mount points, interfaces |

4. All entries are written into a single `zip::ZipWriter` with Deflate compression.
5. Path is logged to the debug log and returned to the renderer as `Ok(Some(path))`.

---

## Design Decisions

- `main.rs` stays thin and delegates implementation to focused modules.
- `#[tauri::command]` functions live only in `commands.rs` — domain modules contain no Tauri command annotations.
- Latest successful LHM sample is kept in memory to avoid UI flicker when LHM times out.
- Payloads are validated before rendering to avoid repainting with malformed transient data.
- Poll ticks do not overlap, which avoids out-of-order UI updates.
- `frontend/` is the Tauri web root, keeping runtime assets and HTML together.
- No bundler or framework — vanilla ES modules are served directly by Tauri's asset server.
