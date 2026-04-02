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

## Linting and formatting

```bash
# Format Rust (modifies files)
npm run fmt:rs

# Check Rust formatting without modifying (CI)
npm run fmt:rs:check

# Rust clippy
npm run clippy

# Lint JavaScript
npm run lint

# Auto-fix JavaScript
npm run lint:fix

# Lint Markdown
npm run lint:md
```

See [STANDARDS.md](STANDARDS.md) for the full code standards.

## After making code changes

**Always run the relevant checks before declaring a task complete.** Do not wait to be asked.

| Changed | Run |
| --- | --- |
| Any Rust file | `npm run fmt:rs` then `npm run clippy` |
| Any `.js` file | `npm run lint` |
| Any `.md` file | `npm run lint:md` |
| Logic in Rust or JS | `npm test` (or the single-file variant) |
| Unsure | `npm run verify` (runs everything, including markdown lint) |

## Documentation and website updates

**Every feature change must also update all three of these — do not wait to be asked:**

| What changed | Where to update |
| --- | --- |
| New panel, data field, or backend module | `docs/architecture.md` — backend modules + renderer modules sections |
| New panel or user-visible feature | `website/index.html` — panel count in `<h2>`, panel card in `.panels-grid`, hero description if relevant |
| Feature complete or scope change | `ROADMAP.md` — mark ✓ and add implementation summary |
| New behaviour or architectural rule | `CLAUDE.md` — Architecture Overview section |

These four files must be consistent with the code at all times. Check all four before declaring a task done.

- `npm run clippy` is configured with `-D warnings` — zero warnings is the bar, not a goal.
- `npm run lint` must exit clean — fix all errors and warnings before finishing.
- If `fmt:rs` modifies files, include those changes in the same commit.
- If a check fails, fix the issue. Do not skip checks or add `#[allow(...)]` without a clear reason documented in the code.

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

- **`main.rs`** — Tauri builder, tray icon, lifecycle. Registers two managed state types at startup: `HardwareInfo` (one-time WMI/sysinfo detection) and `AppState` (per-tick runtime state). Picks the best monitor for the profile and starts LHM.
- **`stats.rs`** — Two shared state structs and all serializable payload structs (`StatsPayload`, `CpuStats`, etc.). `HardwareInfo` holds startup-detected constants (disk model map, RAM spec, GPU VRAM, system brand, etc.) registered once and never mutated. `AppState` holds per-tick mutable state (LHM client, sysinfo handles, last samples, alert timestamps, settings) behind a `Mutex`.
- **`commands.rs`** — Thin `#[tauri::command]` handlers only. Each handler delegates to a domain module; no business logic lives here.
- **`debug.rs`** — `append_debug_log`, `reset_debug_log`, `run_hidden_command`, `unix_now_secs`. No deps on other crate modules — safe to import from anywhere.
- **`hardware.rs`** — WMI structs + all startup hardware detection: `detect_gpu_name`, `detect_gpu_vram_total_mb`, `detect_system_brand`, `classify_system_brand`, `detect_model_name`, `detect_motherboard_name`, `normalize_manufacturer`, `detect_ram_spec`, `detect_ram_details`, `detect_ping_target`, `sample_ping_ms`, `probe_wmi_status`, `detect_disk_model_map`. Each function tries WMI first, falls back to PowerShell CIM. `detect_disk_model_map` resolves drive letters to physical disk model names via a three-table WMI join and stores the result in `HardwareInfo` at startup for stable LHM temperature matching. `detect_motherboard_name` queries `Win32_BaseBoard` for manufacturer + product and normalizes the manufacturer string (ASUSTeK → ASUS, Micro-Star → MSI, etc.); result stored in `HardwareInfo.mb_name`.
- **`lhm.rs`** — HTTP client that fetches LHM's `/data.json`, flattens the nested sensor tree into `Vec<FlatNode>` (each node carries `text`, `value`, `parent`, `grandparent`, `sensor_id`), then extracts GPU/CPU/disk/network/motherboard metrics. Disk temperatures are identified by `SensorId` prefix (`/nvme/`, `/hdd/`, `/ata/`, `/scsi/`, `/ssd/`); Warning/Critical threshold sensors are excluded; the highest real temperature per device is stored in `LhmData.disk_temps`. RAM temperature uses `SensorId` prefix `/memory/dimm/` with suffix `/temperature/0` (the actual reading; indices 1–5 are resolution and Low/High/CriticalLow/CriticalHigh limits which are excluded); max across all populated DIMM slots is stored in `LhmData.ram_temp`. CPU temperature is matched by name (`"Core (Tctl/Tdie)"` for AMD, `"CPU Package"` / `"Core Average"` for Intel) restricted to `parent == "Temperatures"` — this prevents the Intel `"CPU Package"` power sensor (same name, parent `"Powers"`) from being returned instead. CPU package power is matched as `"CPU Package"` (Intel) or `"Package"` (AMD) under parent `"Powers"`. Motherboard Super I/O sensors use the `/lpc/` SensorId prefix (chip-agnostic, works on NCT, ITE, Winbond, etc.): fans filtered to RPM > 0 sorted descending (`LhmData.mb_fans`), temperatures filtered ≥ 5 °C (`LhmData.mb_temps`), named voltage rails only — generic `Voltage #N` unmapped slots excluded — filtered > 0.1 V (`LhmData.mb_voltages`), chip name from the grandparent of the first `/lpc/` node (`LhmData.mb_chip`).
- **`lhm_process.rs`** — LHM process lifecycle: `ensure_lhm_running` (scheduled task → direct spawn), `can_reach_lhm_endpoint`, `get_lhm_task_details`, `track_lhm_connection_state` (connect/disconnect logging with 30 s throttle).
- **`monitor.rs`** — Profile definitions (`normalize_profile`, `profile_dimensions`), monitor selection (`pick_target_monitor`, `fit_score`), panel visibility normalisation (`normalize_visible_panels`). `pick_target_monitor` never uses `set_fullscreen` — borderless positioning via `set_size` + `set_decorations(false)` + `set_position` is sufficient. `set_decorations(false)` is always called after `set_size` because Windows `SetWindowPos` can restore `WS_CAPTION`/`WS_THICKFRAME`. `set_position` compensates for the DWM invisible resize border (inset = `inner_position − outer_position`) so the visible content lands flush with the monitor edge.
- **`windows.rs`** — Secondary window creation and tray-anchored positioning: `ensure_settings_window`, `ensure_about_window`, `ensure_status_window`, `ensure_updater_window`, `on_window_event`, `set_last_tray_click_position`.
- **`updater.rs`** — Auto-update logic: `spawn_background_check` (6-hour loop, first check after 10 s), `check_for_update`, `install_update`, `open_updater_window` commands.
- **`diagnostics.rs`** — `collect_diagnostics` Tauri command + helpers (`diag_collect_hardware`, `diag_collect_tasks`, etc.) that gather system info into a ZIP archive for bug reports.
- **`settings.rs`** — `Settings` struct (opacity, model name, dashboard profile, always-on-top, visible panels, `last_seen_version`, `thresholds: HashMap<String, ComponentThresholds>`, `alert_cooldown_secs`, `notify_on_warn`, `notify_on_crit`, `settings_version`), JSON persistence to Tauri app data dir. `ComponentThresholds { warn: Option<u8>, crit: Option<u8> }` is keyed by component (`"cpu"`, `"gpu"`, `"ram"`, `"disk"`). `settings_version` is a `u8` migration sentinel: 0 = legacy flat fields (pre-1.15), 1 = current map format. `load_settings` runs `migrate_v0_thresholds` once when it reads a version-0 file, then re-persists. The eight legacy flat fields are kept as private `#[serde(default, skip_serializing)]` shims so old settings files can still be read but are never written.

### Frontend (`frontend/`)

No framework, no bundler. Pure ES modules. Each HTML page loads its own entry script.

- **`renderer/environment.js`** — Detects whether running inside Tauri. Exports `backend` (thin wrapper around `window.__TAURI__.core.invoke` / `.event.listen`) and `IS_DESKTOP`. All renderer modules go through this instead of accessing Tauri globals directly.
- **`renderer/app.js`** — Main dashboard orchestrator. Drives the 1-second poll loop (`tick()`), applies settings/profile/opacity from Tauri events, manages brand preview mode. `applyThresholds(s)` builds per-component `{ warn, crit }` objects and stores them in the module-level `thresholds` variable; called at startup and on every `apply-thresholds` event so panel colours update instantly after saving settings.
- **`renderer/systemInfo.js`** — Host name, CPU model, GPU model, and branding/logo wiring.
- **`renderer/clock.js`** — Local time and uptime rendering.
- **`renderer/spark.js`** — Sparkline history ring buffer and canvas drawing.
- **`renderer/tempColors.js`** — Maps temperature values to color thresholds for heat indicators.
- **`renderer/vendorBranding.js`** — Pure mapping: brand key → logo asset + label. No DOM access; testable in Node.
- **`renderer/simulator.js`** — Browser-mode fake stats for developing the UI without the Tauri backend.
- **`renderer/panels/`** — One file per panel: `cpu.js`, `gpu.js`, `ram.js`, `network.js`, `disk.js`, `motherboard.js`, `process.js`. Each exports an `update*Panel(stats, ...)` function. `thresholds` carries `{ warn, crit }` for temperature colour mapping; defaults apply in browser/simulator mode. `gpu.js` renders a ring gauge, 3×2 metadata grid (TEMP, HOT SPOT, CORE CLK, MEM CLK, POWER, FAN), VRAM and GPU load bars, and two optional D3D bars (3D engine, Video Decode) that are hidden via `display:none` when the backend returns `null` for those fields. `disk.js` cycles through pages of three drives every 5 ticks when more than three drives are present; the page resets automatically when the drive count changes. `motherboard.js` renders fans/temps/voltages in a three-column layout; `shortLabel()` maps `"Temperature #N"` → `"TN"` and truncates other labels to fit the `8ch` CSS grid column. `process.js` renders the top 8 processes from `StatsPayload.top_processes`; process names are HTML-escaped and `.exe` suffix is stripped; `truncateName` and `formatRam` are pure helpers exported for unit tests.
- **`renderer/settings.js`** / **`renderer/about.js`** / **`renderer/status.js`** / **`renderer/updater.js`** — Entry scripts for the secondary windows. `updater.js` drives the update check, changelog rendering, and install flow.

### Dashboard profiles

Profiles are portrait orientations with fixed pixel dimensions (e.g., `portrait-xl` = 450×1920). The profile name is stored in settings; the backend calls `pick_target_monitor()` to move and resize the main window, and the frontend calls `applyProfile()` to scale CSS variables. Both sides share the same list of valid profile names. `pick_target_monitor` is only called in `save_settings` when the profile has actually changed — calling it unconditionally causes a ~3 px position drift on every save due to the DWM inset compensation.

Valid profiles: `portrait-xl` (450×1920), `portrait-slim` (480×1920), `portrait-hd` (720×1280), `portrait-wxga` (800×1280), `portrait-fhd` (1080×1920), `portrait-wuxga` (1200×1920), `portrait-qhd` (1440×2560), `portrait-hdplus` (768×1366), `portrait-900x1600`, `portrait-1050x1680`, `portrait-1600x2560`, `portrait-4k` (2160×3840), `portrait-fhd-side` (253×1080), `portrait-qhd-side` (338×1440), `portrait-4k-side` (506×2160).

Valid panel keys: `header`, `clock`, `cpu`, `gpu`, `ram`, `net`, `disk`, `motherboard`, `process`. Both `motherboard` and `process` are opt-in (not included in the default visible set).

### LHM integration

LibreHardwareMonitor runs as a Windows scheduled task (installed by the NSIS installer as admin). It exposes a local HTTP server on port 8085. The Rust backend polls `/data.json` every tick with an 800 ms timeout. On failure it falls back to the last successful sample. GPU data is located by finding the `GPU Memory Total` sensor with the highest value (handles iGPU+dGPU configs), then collecting all sensors whose `grandparent` matches the anchor node's `grandparent` (the GPU device name). A fixed ±25 window was dropped because GPUs like the RTX 4090 expose 19+ D3D load sensors that pushed temperature/clock/power sensors outside any reasonable window. Extracted GPU fields: core load, core temp, hotspot temp, core clock (`gpu_freq`), memory clock (`gpu_mem_freq`), power, fan speed, VRAM used/total, D3D 3D engine load (`gpu_d3d_3d`), and D3D Video Decode load (`gpu_d3d_vdec`). D3D fields are `None` when the GPU is idle or the driver does not report them; the frontend hides their bar rows in that case.

### Settings persistence

Settings are stored in `%APPDATA%\se.codeby.rigstats\rigstats-settings.json`. The debug log is at `rigstats-debug.log` in the same directory.

### Testing

Frontend tests use **vitest** and are colocated with modules as `*.test.js` files (e.g., `tempColors.test.js`, `vendorBranding.test.js`). Rust tests are in `#[cfg(test)]` modules at the bottom of their respective files; most require Windows and the `wmi` feature.
