# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Start in development mode (hot-reload frontend, debug Rust build)
npm start

# Run frontend unit tests only
npm test

# Run Rust tests only (requires Windows for most tests)
cargo test --manifest-path src-tauri/Cargo.toml

# Full verification: prepare LHM assets + Rust tests + check + frontend tests
npm run verify

# Production build (NSIS installer output)
npm run build

# Prepare the LibreHardwareMonitor vendor assets (copies to build output)
npm run prepare:lhm
```

Run a single frontend test file with vitest:

```bash
npx vitest run frontend/renderer/tempColors.test.js
```

Run a single Rust test:

```bash
cargo test --manifest-path src-tauri/Cargo.toml classify_system_brand
```

## Architecture Overview

This is a **Windows-only** Tauri v2 desktop app ("RigStats") that displays hardware telemetry on a secondary portrait monitor. It has no bundler/build step for the frontend — vanilla JS ES modules are served directly from `frontend/`.

### Data flow

```text
LibreHardwareMonitor (localhost:8085/data.json)
    └─► lhm.rs: fetch + flatten JSON tree → LhmData struct
sysinfo crate (CPU load/freq, RAM, disk, network)
wmi crate (GPU name, VRAM, RAM spec/details, system brand)
    └─► commands.rs: get_stats() → StatsPayload
            └─► Tauri IPC invoke("get-stats")
                    └─► frontend/renderer/app.js: tick() every 1s
                            └─► panel modules update DOM
```

### Backend (`src-tauri/src/`)

- **`main.rs`** — Tauri builder, tray icon, lifecycle. Initializes `AppState` at startup (one-time hardware detection via WMI/sysinfo), picks the best monitor for the profile, starts LHM.
- **`stats.rs`** — `AppState` struct (shared mutable state behind `Mutex`), all serializable payload structs (`StatsPayload`, `CpuStats`, etc.).
- **`commands.rs`** — Thin `#[tauri::command]` handlers only. Each handler delegates to a domain module; no business logic lives here.
- **`debug.rs`** — `append_debug_log`, `reset_debug_log`, `run_hidden_command`, `unix_now_secs`. No deps on other crate modules — safe to import from anywhere.
- **`hardware.rs`** — WMI structs + all startup hardware detection: `detect_gpu_name`, `detect_gpu_vram_total_mb`, `detect_system_brand`, `classify_system_brand`, `detect_model_name`, `detect_ram_spec`, `detect_ram_details`, `detect_ping_target`, `sample_ping_ms`, `probe_wmi_status`. Each function tries WMI first, falls back to PowerShell CIM.
- **`lhm.rs`** — HTTP client that fetches LHM's `/data.json`, flattens the nested sensor tree into `Vec<FlatNode>`, then extracts GPU/CPU/disk/network metrics by parent+text name pairs.
- **`lhm_process.rs`** — LHM process lifecycle: `ensure_lhm_running` (scheduled task → direct spawn), `can_reach_lhm_endpoint`, `get_lhm_task_details`, `track_lhm_connection_state` (connect/disconnect logging with 30 s throttle).
- **`monitor.rs`** — Profile definitions (`normalize_profile`, `profile_dimensions`), monitor selection (`pick_target_monitor`, `fit_score`), panel visibility normalisation (`normalize_visible_panels`).
- **`windows.rs`** — Secondary window creation and tray-anchored positioning: `ensure_settings_window`, `ensure_about_window`, `ensure_status_window`, `on_window_event`, `set_last_tray_click_position`.
- **`diagnostics.rs`** — `collect_diagnostics` Tauri command + helpers (`diag_collect_hardware`, `diag_collect_tasks`, etc.) that gather system info into a ZIP archive for bug reports.
- **`settings.rs`** — `Settings` struct (opacity, model name, dashboard profile, always-on-top, visible panels), JSON persistence to Tauri app data dir.

### Frontend (`frontend/`)

No framework, no bundler. Pure ES modules. Each HTML page loads its own entry script.

- **`renderer/environment.js`** — Detects whether running inside Tauri. Exports `backend` (thin wrapper around `window.__TAURI__.core.invoke` / `.event.listen`) and `IS_DESKTOP`. All renderer modules go through this instead of accessing Tauri globals directly.
- **`renderer/app.js`** — Main dashboard orchestrator. Drives the 1-second poll loop (`tick()`), applies settings/profile/opacity from Tauri events, manages brand preview mode.
- **`renderer/panels/`** — One file per panel: `cpu.js`, `gpu.js`, `ram.js`, `network.js`, `disk.js`. Each exports an `update*Panel(stats, history, pushHistory)` function.
- **`renderer/spark.js`** — Sparkline history ring buffer and canvas drawing.
- **`renderer/vendorBranding.js`** — Pure mapping: brand key → logo asset + label. No DOM access; testable in Node.
- **`renderer/simulator.js`** — Browser-mode fake stats for developing the UI without the Tauri backend.
- **`renderer/settings.js`** / **`renderer/about.js`** / **`renderer/status.js`** — Entry scripts for the secondary windows.

### Dashboard profiles

Profiles are portrait orientations with fixed pixel dimensions (e.g., `portrait-xl` = 450×1920). The profile name is stored in settings; the backend calls `pick_target_monitor()` to move and resize the main window, and the frontend calls `applyProfile()` to scale CSS variables. Both sides share the same list of valid profile names.

Valid panel keys: `header`, `clock`, `cpu`, `gpu`, `ram`, `net`, `disk`.

### LHM integration

LibreHardwareMonitor runs as a Windows scheduled task (installed by the NSIS installer as admin). It exposes a local HTTP server on port 8085. The Rust backend polls `/data.json` every tick with an 800 ms timeout. On failure it falls back to the last successful sample. GPU data is located by finding the `GPU Memory Total` sensor (>10,000 MB) and searching a window of ±25 nodes around it.

### Settings persistence

Settings are stored in `%APPDATA%\se.codeby.rigstats\rigstats-settings.json`. The debug log is at `rigstats-debug.log` in the same directory.

### Testing

Frontend tests use **vitest** and are colocated with modules as `*.test.js` files (e.g., `tempColors.test.js`, `vendorBranding.test.js`). Rust tests are in `#[cfg(test)]` modules at the bottom of their respective files; most require Windows and the `wmi` feature.
