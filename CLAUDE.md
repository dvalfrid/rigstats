# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Skills (invoke with /name)

| Skill | When to use |
|---|---|
| `/commit-workflow` | Opening an issue, committing, roadmap sync |
| `/sensor-fixture` | Adding a hardware sensor fixture |
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

Settings are read from `%APPDATA%\se.codeby.rigstats\`. The sidecar pipe accepts one client at a time — in wallpaper mode only the host polls; the main app pauses its `poll_loop` via `poll_paused`.

### egui binary (`src-egui/src/`)

- **`lib.rs`** — library root; re-exports all modules so both binaries share the same panel renderer
- **`main.rs`** — `RigStatsApp`/`eframe::App`: frame loop, settings reload, secondary viewports, panel rendering, wallpaper-mode supervisor (`update_wallpaper_mode`)
- **`dashboard.rs`** — `DashboardRuntime`: owned telemetry→renderer glue (sparklines, theme, thresholds, textures, `drain`/`apply_settings`/`view`); `DashboardView<'a>`: borrowed per-frame render state; `PanelThresholds` (warn/crit pairs)
- **`bin/wallpaper.rs`** — `rigstats-wallpaper` host: attaches into WorkerW, runs own `poll_loop`, exits when parent PID disappears
- **`geometry.rs`** — `profile_to_size`, monitor enumeration, pinned/auto-target position resolution; bulk of the unit tests
- **`poll.rs`** — `poll_loop` (tokio, ~1 Hz); `PollStats`/`DriveInfo`/`ProcessInfo` data types
- **`tray.rs`** — system tray icon, `TrayCmd` enum, `load_app_icon`, `panel_label`/`panel_initial_h`
- **`menu_icons.rs`** — procedurally-rasterized glyph icons for each tray context-menu row (no external image assets)
- **`gpu_guard.rs`** — `install_gpu_loss_guard`: wgpu device-error/device-lost callbacks that flag a fatal GPU error instead of panicking
- **`theme.rs`** — `AppTheme`, color helpers, `panel_frame()`, sparkline/bar helpers, `avail_color()`, dialog button API (`dialog_btn_primary`/`dialog_btn_secondary`)
- **`panels/`** — one file per panel; each `draw()` accepts `&AppTheme` and returns `egui::Rect`
- **`brand.rs`** — embedded brand logo PNGs; `rig_logo`, `cpu_logo`, `gpu_logo`
- **`windows/`** — `settings.rs`, `about.rs`, `status.rs`, `updater.rs`, `history.rs`; secondary viewports via `show_viewport_deferred`. Dialog design contract: `src-egui/src/windows/CLAUDE.md`
- **`win32_wallpaper.rs`** — `find_wallpaper_workerw`, `attach`/`detach`, `process_alive`; used only by the wallpaper host
- **`win32_dark_mode.rs`** — sets dark mode for OS-drawn tray menu at startup
- **`update_check.rs`** — `check()` fetches `latest.json`; `BUNDLED_CHANGELOG` embeds `CHANGELOG.md`

### Data flow

```text
rigstats-sensor.exe  (sensor-sidecar/, .NET 10, Windows Service / LocalSystem)
    └─► LibreHardwareMonitor NuGet → PawnIO kernel driver
            └─► named pipe \\.\pipe\rigstats-sensors  (newline-delimited JSON)
                    └─► lhm.rs (rigstats-backend): pipe client → LhmData struct
sysinfo crate (CPU load/freq, RAM, disk, network)
wmi crate (GPU name, VRAM, RAM spec/details, system brand)
    └─► poll_loop (src-egui/main.rs): get_stats() → StatsPayload → mpsc::Sender
            └─► egui UI thread: receives payload each 1 s tick → all panel draw() calls
```

### Backend (`rigstats-backend/src/`)

- **`stats.rs`** — `StatsPayload` + sub-structs; `HardwareInfo` (startup constants behind `Mutex`), `AppState` (per-tick mutable state)
- **`hardware.rs`** — WMI/CIM hardware detection: GPU, RAM, disk, system brand, model, motherboard, ping
- **`lhm.rs`** — named pipe client → `LhmData`; `select_gpu_idx` (preferred → highest VRAM → load tie-break)
- **`lhm_process.rs`** — connection state tracking (connect/disconnect logging, 30 s throttle)
- **`logging.rs`** — session-based CSV stats logging: `start_session`/`end_session`, `append_stats_row`, `load_sessions`/`rename_session`/`set_session_pinned`/`delete_session`, `prune_old_sessions`, `reconcile_sessions_on_startup`. Session index (`rigstats-sessions.json`) writes are guarded by a cross-process file lock (`SessionsLock`) and mirrored to a `.bak` for corruption recovery.
- **`settings.rs`** — `Settings` struct + JSON persistence to `%APPDATA%\se.codeby.rigstats\`
- **`debug.rs`** — `log_debug`/`log_warn`/`log_error`; `reset_debug_log` rotates log to `rigstats-debug-prev.log`
- **`monitor.rs`** — profile definitions, monitor selection, `compute_panels_logical_height`
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
