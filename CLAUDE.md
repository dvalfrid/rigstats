# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Skills (invoke with /name)

| Skill | When to use |
|---|---|
| `/commit-workflow` | Opening an issue, committing, roadmap sync |
| `/sensor-fixture` | Adding a hardware sensor fixture |
| `/gpu-engine-fixture` | Adding a GPU Engine (GPU Apps panel) fixture |
| `/run-rigstats` | Building and launching the app |
| `/verifier-gui` | Visually verifying a GUI change |

## Commands

```powershell
# Build egui binary (debug)
cargo build --manifest-path src-egui/Cargo.toml

# Restart the app (kill by PID — name-based kill silently fails; verify timestamp before launch)
$proc = Get-Process rigstats -ErrorAction SilentlyContinue
if ($proc) { Stop-Process -Id $proc.Id -Force }
cargo build --manifest-path src-egui/Cargo.toml
(Get-Item .\target\debug\rigstats.exe).LastWriteTime   # must have advanced
Start-Process .\target\debug\rigstats.exe

# Check egui + backend for errors
cargo check --manifest-path src-egui/Cargo.toml

# Build sensor sidecar (requires .NET 10 SDK)
dotnet build sensor-sidecar/sensor-sidecar.csproj

# Run Rust tests
cargo xtask test

# Full verification (sidecar + tests + clippy + fmt check)
cargo xtask verify

# Production build
cargo xtask build

# Setup (install git hooks, first-time only)
cargo xtask setup
```

> `cargo xtask verify` / `cargo xtask build` fail if the `rigstats-sensor` service is running (it holds the exe). Stop it first: `sc.exe stop rigstats-sensor` (elevated terminal).

Single test: `cargo test --manifest-path rigstats-backend/Cargo.toml <test_name>`

## Linting and formatting

```bash
cargo xtask fmt          # format Rust (modifies files)
cargo xtask fmt-check    # CI — no modifications
cargo xtask clippy       # must pass with zero warnings (-D warnings)
```

See [STANDARDS.md](STANDARDS.md) for the full code standards.

## After making code changes

**Always run the relevant checks before declaring a task complete.**

| Changed | Run |
| --- | --- |
| Any Rust file | `cargo xtask fmt` then `cargo xtask clippy` |
| Any `sensor-sidecar/*.cs` file | `dotnet build sensor-sidecar/sensor-sidecar.csproj` |
| Logic in Rust | `cargo xtask test` |
| Unsure | `cargo xtask verify` |

- `clippy` is `-D warnings` — zero warnings required.
- If `fmt` modifies files, include those changes in the same commit.
- Never add `#[allow(...)]` without a clear reason documented in the code.

## Commit message format

Mandatory — `release-please` parses commit subjects to generate `CHANGELOG.md` and bump the version.

```
<type>(<scope>): <subject>

<optional body>

Closes #N
```

- **type:** `feat`, `fix`, `perf`, `docs`, `refactor`, `test`, `build`, `chore`, `style`. Only `feat`/`fix`/`perf` surface in the changelog.
- **scope:** lower-case area, e.g. `cpu`, `gpu`, `settings`, `wallpaper`, `status`. Optional but expected.
- **subject:** imperative, lower-case start, no trailing period.
- Breaking change: `feat!:` / `fix!:` or `BREAKING CHANGE:` footer.
- Always include `Closes #N` so GitHub closes the issue automatically on push to main.

For the full issue → implement → test → commit → roadmap-sync workflow, run `/commit-workflow`.

## Design philosophy

Prefer the simplest solution that solves the problem. Before implementing, ask: is there a direct approach that avoids the complexity entirely? Flag files, shared state, and extra IPC are often signs that a simpler path exists. Question existing plans — a plan being written down is not a reason to follow it if a cleaner alternative is obvious.

## Architecture Overview

Windows-only egui desktop app ("RIGStats") displaying hardware telemetry on a secondary monitor (portrait or landscape), as a floating overlay, or reparented into the desktop wallpaper (WorkerW). No web frontend — all UI is native Rust/egui.

Cargo workspace with two members:

| Crate | Path | Role |
|---|---|---|
| `rigstats-backend` | `rigstats-backend/` | Shared lib — telemetry, hardware detection, settings, logging |
| `rigstats-egui` | `src-egui/` | egui library (`lib.rs`) + two binaries: `rigstats` (main app) and `rigstats-wallpaper` (WorkerW host). Both embed `dashboard::DashboardRuntime` and render via `dashboard::DashboardView`. |

Settings are read from `%APPDATA%\se.codeby.rigstats\`. The sidecar pipe accepts several clients that share one cached sample per second. In wallpaper mode the host is the dashboard's poller; the main app's `poll_loop` (`PollMode` via `poll_mode`) is `Paused`, or `Light` while the game overlay is on (overlay metrics only — no per-app lists, no session recording).

### egui binary (`src-egui/src/`)

- **`lib.rs`** — library root; re-exports all modules so both binaries share the same panel renderer
- **`main.rs`** — `RigStatsApp`/`eframe::App`: frame loop, settings reload, secondary viewports, panel rendering, wallpaper-mode supervisor (`update_wallpaper_mode`)
- **`dashboard.rs`** — `DashboardRuntime`: owned telemetry→renderer glue (sparklines, theme, thresholds, textures, `drain`/`apply_settings`/`view`); `DashboardView<'a>`: borrowed per-frame render state; `PanelThresholds` (warn/crit pairs)
- **`bin/wallpaper.rs`** — `rigstats-wallpaper` host: attaches into WorkerW, runs own `poll_loop`, exits when parent PID disappears
- **`geometry.rs`** — `profile_to_size`, monitor enumeration, pinned/auto-target position resolution; bulk of the unit tests
- **`poll.rs`** — `poll_loop` (tokio, ~1 Hz); `PollStats`/`DriveInfo`/`ProcessInfo` data types; `PollMode` (`Full`/`Light`/`Paused`, shared via `PollModeHandle`)
- **`alerts.rs`** — `pending_alerts`: pure warn/crit threshold-breach detection; `notify_on_*`, cooldowns and sending live in `main.rs`
- **`dcomp_burst.rs`** — `DcompRevealBurst`: hide-until-settled reveal policy for per-pixel-transparent (DComp) viewports — overlay and floating panels
- **`dialog_reveal.rs`** — `DialogReveal`: every dialog is created hidden, revealed after it has rendered, and hidden for one frame before teardown (no white flash, #203). See "Dialog lifecycle" in `docs/architecture.md`
- **`gpu_process.rs`** — per-process GPU engine utilisation via PDH `\GPU Engine(*)` counters + DXGI LUID→adapter map (`GpuEngineQuery`, `GpuProcessInfo`); vendor-neutral, unelevated. Pure `parse_instance`/`aggregate` are unit-tested; Win32 FFI is `#![allow(unsafe_code)]`. Real-hardware regression corpus: `src-egui/fixtures/gpu-engine/` (see its README); `dump_diagnostics()` feeds `gpu-engine.txt` into the diagnostics ZIP so a user export doubles as a fixture. For adding a fixture, run `/gpu-engine-fixture`
- **`tray.rs`** — system tray icon, `TrayCmd` enum, `load_app_icon`, `panel_label`/`panel_initial_h`; `GpuMenu` ("GPU ▸" submenu, filled in by background adapter detection; `selected_gpu_index` ticks the same adapter the poll loop displays)
- **`menu_icons.rs`** — procedurally-rasterized glyph icons for each tray context-menu row (no external image assets)
- **`lock_ext.rs`** — `LockSafe::lock_safe()`: poison-tolerant `Mutex` locking used throughout `windows/*.rs`
- **`gpu_guard.rs`** — `install_gpu_loss_guard`: wgpu device-error/device-lost callbacks that flag a fatal GPU error instead of panicking
- **`overlay.rs`** — click-through game overlay (issue #183): `ALL_OVERLAY_METRICS` registry (key/label/unit/`extract`/`color` fn) + `draw_overlay` compact chip renderer. An independent add-on driven by `Settings.overlay_enabled` — not a `window_layer` value — so it coexists with whatever the main window is doing; `RigStatsApp::render_overlay_viewport` in `main.rs` owns its own always-on-top viewport, DComp reveal-burst (white-flash fix), and resize debounce. Click-through uses `egui::ViewportCommand::MousePassthrough`, no raw Win32 needed
- **`hotkey.rs`** — global hotkey listener (`RegisterHotKey`/`WM_HOTKEY` on its own thread, wakes the main loop via a `Context` clone); fixed `Ctrl+Alt+O` shows/hides the overlay (`RigStatsApp::toggle_overlay_mode`), not remappable in v1
- **`single_instance.rs`** — `ensure_single_instance`: named-mutex guard checked first thing in `main()`; if another instance is already running, focuses its window (`FindWindowW`/`SetForegroundWindow`) and this process exits instead of starting a second one
- **`theme.rs`** — `AppTheme`, color helpers, `panel_frame()`, sparkline/bar helpers, `avail_color()`, dialog button API (`dialog_btn_primary`/`dialog_btn_secondary`)
- **`ring.rs`** — ring gauge renderer; **`spark.rs`** — sparkline ring buffer; **`tempcolor.rs`** — `temp_color()` value→green/yellow/red
- **`panels/`** — one file per panel; each `draw()` accepts `&AppTheme` and returns `egui::Rect`. `gpu_processes.rs` ("GPU APPS") renders `PollStats.gpu_processes` — top apps by GPU %, attributed to a physical adapter when >1 GPU is active
- **`brand.rs`** — embedded brand logo PNGs; `rig_logo`, `cpu_logo`, `gpu_logo`
- **`windows/`** — `settings.rs`, `about.rs`, `status.rs`, `updater.rs`, `history.rs`, `control.rs` (Control Center: profiles with Duplicate/Rename/Delete/Reset, Power/Fans/CPU/GPU/Lighting tabs (Power picks the profile's Windows power plan; Lighting previews live), fan curve editor; fan channels are mapped to fans only by measured identify results, never by name; CPU limit, Curve Optimizer and GPU changes go through `preview` with auto-revert; the CPU tab shows a red "Reverted after a restart" banner when the boot-crash guard tripped); secondary viewports via `show_viewport_immediate`. Dialog design + lifecycle contract: `src-egui/src/windows/CLAUDE.md` (new dialogs must use `DialogReveal` + `RigStatsApp::finish_dialog_frame`)
- **`win32_wallpaper.rs`** — `find_wallpaper_workerw`, `attach`/`detach`, `process_alive`; used only by the wallpaper host
- **`win32_behind.rs`** — `apply_behind`/`prepare_for_drag`/`keep_behind`: Always-Behind window layer support
- **`win32_dark_mode.rs`** — sets dark mode for OS-drawn tray menu at startup; `apply_titlebar_theme` for dialog title bars
- **`win_opacity.rs`** — raw Win32 window helpers: `SetLayeredWindowAttributes` opacity, `set_no_redirection_bitmap` (DComp), `disable_dwm_transitions`, `bring_to_foreground` (restores minimized first), `force_repaint`, `find_hwnd`
- **`update_check.rs`** — `check()` fetches `latest.json`, `download`/`launch_installer`; `BUNDLED_CHANGELOG` embeds `CHANGELOG.md`. The 10 s-then-6 h background check loop is spawned in `main.rs`

### Data flow

```text
rigstats-sensor.exe  (sensor-sidecar/, .NET 10, Windows Service / LocalSystem)
    └─► LibreHardwareMonitor NuGet → PawnIO kernel driver
            └─► named pipe \\.\pipe\rigstats-sensors  (newline-delimited JSON,
                several clients share one ≤1 Hz sample — SensorWorker.GetFreshLine)
                    └─► lhm.rs (rigstats-backend): pipe client → LhmData struct
sysinfo crate (CPU load/freq, RAM, disk, network, processes)
wmi crate (startup: GPU names, RAM spec/details, board, model, brand, ping target; per tick: battery)
PDH \GPU Engine(*) (gpu_process.rs: per-app GPU %)
    └─► poll_loop (src-egui/src/poll.rs): → PollStats → mpsc::SyncSender
            ├─► egui UI thread: DashboardRuntime::drain each ~1 s tick → all panel draw() calls
            └─► while recording: poll_stats_to_log_payload → StatsPayload → CSV row
```

### Backend (`rigstats-backend/src/`)

- **`stats.rs`** — `StatsPayload` + sub-structs: the session-recording row shape (built from `PollStats` in `poll.rs`); `DiskKind`
- **`hardware.rs`** — WMI hardware detection (PowerShell only when WMI fails): GPU names, RAM, disk, system brand, model, motherboard, ping target, battery. Typed `query::<T>()` structs need a `#[serde(rename = "Win32_…")]` container rename (#198, guarded by `wmi_classes_tests`)
- **`lhm.rs`** — named pipe client → `LhmData`; `select_gpu_idx` (preferred → highest VRAM → load tie-break), `normalize_gpu_name`/`gpu_names_match` (WMI/LHM-tolerant name matching)
- **`lhm_process.rs`** — connection state tracking (connect/disconnect logging, 30 s throttle)
- **`control.rs`** — Control Center pipe client (`\\.\pipe\rigstats-control`, duplex): `control_task` → `ControlState` (capabilities, profiles, live `fan_duty`, `fan_identified`, safety trips, running `preview`, crash-guard notice); `ControlCmd`; typed fan, CPU-limit, Curve Optimizer, GPU and lighting profile/capability structs. Service side lives in `sensor-sidecar/Control/` (`ControlBroker` incl. preview/auto-revert, `BootCrashGuard`, `FanProvider`, `FanCurveLoop`, `PowerPlanProvider`, `CpuLimitProvider` + `RyzenSmu`/`AmdSmuMap` — AMD PPT/TDC/EDC via LHM's signed RyzenSMU PawnIO module, only on hardware-verified PM table versions; `CurveOptimizerProvider` — AMD Curve Optimizer −30…0 through the same SMU, per-core only when the SMU's cores match Windows' physical cores; `GpuPowerProvider` + `AdlxGpuPower` — AMD GPU power limit via ADLX vtables, back to the pre-RIGStats value, factory reset when it was at factory; `Lighting/` — `LightingProvider` (Aura Sync over `ILightingDevice`s; native protocols only, current hardware): `AuraController`/`AuraUsb` (ASUS Aura USB motherboard), `AsusMonitorDevice` (Aura monitors + light bar), `AsusKeyboardDevice` (ASUS TUF-protocol keyboards, ROG Omni receiver), `AsusHeadsetDevice` (GearLink-protocol headsets, read-back verified), `LampArrayDevice` (any HID LampArray / Dynamic Lighting device), `SoftwareEffect` (service-drawn effects), `HidDevice`; ASUS Aura USB controllers (zones from the controller's own config table, every known controller id, yields to Armoury Crate; fixtures in `sensor-sidecar.Tests/fixtures/aura/`)). Design + phase notes: `docs/control-architecture.md`
- **`logging.rs`** — session-based CSV stats logging: `start_session`/`end_session`, `append_stats_row`, `load_sessions`/`rename_session`/`set_session_pinned`/`delete_session`, `prune_old_sessions`, `reconcile_sessions_on_startup`. Session index (`rigstats-sessions.json`) writes are guarded by a cross-process file lock (`SessionsLock`) and mirrored to a `.bak` for corruption recovery.
- **`settings.rs`** — `Settings` struct + JSON persistence to `%APPDATA%\se.codeby.rigstats\`
- **`debug.rs`** — `log_debug`/`log_warn`/`log_error`; `reset_debug_log` rotates log to `rigstats-debug-prev.log`
- **`autostart.rs`** — HKCU run key for launch-at-startup

### Dashboard profiles

Profiles are named `portrait-<size>` or `landscape-<size>` with fixed pixel dimensions (e.g. `portrait-xl` = 450×1920; landscape is always the transpose). Name stored in settings; `profile_to_size`/`profile_is_landscape` in `geometry.rs` drive window sizing and all orientation branches.

**Monitor selection:** `pick_window_rect_for_profile` picks the monitor whose resolution matches the profile (~10 %); falls back to primary. Position is carried over on profile switches when the window is still on a connected monitor.

**Portrait:** one vertical stack; window height fits content per frame.  
**Landscape:** adaptive grid (`render_landscape_grid`); column count maximises per-cell scale; window fixed to full profile size.  
**Fullscreen mode** (`fullscreen_mode`): portrait-only, fills monitor height while keeping profile width; `fullscreen_align` = `"top"` or `"center"`.  
**Pinned dashboard** (`dashboard_pinned`): locks fixed-mode window position per profile in `pinned_positions`.

Valid profile names and panel keys: see `geometry.rs` and `settings.rs`.

### Sensor sidecar integration

`rigstats-sensor.exe` runs as a Windows Service (LocalSystem, auto-start). NSIS installer uses `sc create`/`sc stop`/`sc delete` for install, update, and uninstall.

The Rust backend connects to `\\.\pipe\rigstats-sensors` (`.write(false)`). On failure it falls back to the last sample. GPU selection: preferred → highest VRAM → load tie-break. D3D fields (`gpu_d3d_3d`, `gpu_d3d_vdec`) are `None` when idle; their presence toggles the GPU panel between a two-column bar layout and single-bar default.

For adding hardware sensor fixtures, run `/sensor-fixture`.

### Session recording

Tray `Start/Stop Recording` (`TrayCmd::ToggleRecording` in `main.rs`) starts/ends a session via `logging.rs` and flips `Tray::set_recording` (menu label/icon + tray tooltip). While a session is active, `main.rs` blinks the tray icon (~600 ms, driven by the same idle-repaint tick) via `Tray::set_recording_blink`. The active session lives in the on-disk index, not in-process state, so both this app's and `rigstats-wallpaper`'s `poll_loop` — whichever is currently polling — append rows to it. `windows/history.rs` (opened via `TrayCmd::OpenHistory`) lists/pins/renames/deletes sessions and charts a selected one with `egui_plot`; it refreshes its session list on open, on any list action, and whenever recording starts/stops while it's open.

### Settings persistence

Settings: `%APPDATA%\se.codeby.rigstats\rigstats-settings.json`. Debug log: `rigstats-debug.log`; previous session: `rigstats-debug-prev.log`.

### Testing

Rust tests are `#[cfg(test)]` modules at the bottom of their files (e.g. `src-egui/src/geometry.rs`). Run with `cargo xtask test`.

.NET sidecar tests: `sensor-sidecar.Tests/` (xUnit, NSubstitute); runs as part of `cargo xtask verify`.

## Kontexthantering

Efter varje svar, uppskatta hur mycket av kontextfönstret som används.
När du bedömer att ~70% är förbrukat, lägg till en varning i slutet av svaret:

⚠️ **KONTEXT ~70%** — Överväg att köra /compact eller starta ny session snart.

När du bedömer att ~90% är förbrukat:

🔴 **KONTEXT KRITISK** — Kör följande innan vi fortsätter:

1. Spara en sammanfattning till CLAUDE.md
2. Starta ny session med sammanfattningen som kickstart
