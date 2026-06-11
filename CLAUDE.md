# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# --- egui migration (feat/egui-migration branch) ---

# Build egui binary (debug)
cargo build --manifest-path src-egui/Cargo.toml

# Run egui binary directly
.\target\debug\rigstats-egui.exe

# Check egui + backend for errors
cargo check --manifest-path src-egui/Cargo.toml

# Clippy on backend + egui
cargo clippy --manifest-path rigstats-backend/Cargo.toml -- -D warnings
cargo clippy --manifest-path src-egui/Cargo.toml -- -D warnings

# --- Tauri (production, main branch) ---

# Build sensor sidecar (debug, requires .NET 10 SDK)
dotnet build sensor-sidecar/sensor-sidecar.csproj

# Publish sensor sidecar as single-file self-contained exe (release)
dotnet publish sensor-sidecar/sensor-sidecar.csproj -c Release

# Start in development mode (hot-reload frontend, debug Rust build)
npm start

# Run frontend unit tests only
npm test

# Run Rust tests only (requires Windows for most tests)
cargo test --manifest-path src-tauri/Cargo.toml

# Full verification: publish sidecar + Rust tests + check + frontend tests
npm run verify

# Production build (NSIS installer output)
npm run build

# Publish sensor sidecar as single-file exe (required before npm run build)
npm run prepare:sidecar
```

> **Local dev note:** `npm run verify` and `npm run prepare:sidecar` fail if the
> `rigstats-sensor` Windows Service is running, because the service holds the exe
> open. Stop it first (`sc.exe stop rigstats-sensor` in an elevated terminal),
> then run verify, then restart the service.

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
| Any `sensor-sidecar/*.cs` file | `dotnet build sensor-sidecar/sensor-sidecar.csproj` |
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

**Active migration:** `feat/egui-migration` branch replaces the Tauri/WebView2 frontend with a native egui UI to eliminate the 2–4 % idle CPU cost. The repo is a Cargo workspace (`Cargo.toml` at root) with three members:

| Crate | Path | Role |
|---|---|---|
| `rigstats` | `src-tauri/` | Production Tauri binary (unchanged) |
| `rigstats-backend` | `rigstats-backend/` | Shared lib — all backend modules with Tauri coupling removed (`AppHandle` → `&Path` for settings/debug/lhm functions) |
| `rigstats-egui` | `src-egui/` | New egui binary (Phase 1 scaffold; grows each phase) |

`src-tauri` builds and runs independently; existing npm scripts remain valid. The egui binary reads settings from the same `%APPDATA%\se.codeby.rigstats\` directory as the Tauri app. The sidecar pipe only accepts one client at a time, so LHM data (temps, GPU) is only available in egui when the Tauri app is not running.

### egui binary (`src-egui/src/`)

Key source files:

- **`main.rs`** — entry point: monitor selection, tray setup, settings load, poll thread, eframe `run_native`. Holds `PanelThresholds` struct (warn/crit pairs per component) initialized from `Settings` and updated on every settings reload, plus the live `AppTheme` derived from `Settings.theme` — both are passed directly to each panel `draw()` call so threshold colours and theme preview update immediately.
- **`theme.rs`** — shared constants and helpers for **both** panel cards and dialog windows:
  - `AppTheme`, `THEME_KEYS`, HSL-derived label colours (`stat_label`, `text_muted`, `mb_accent`) from a single preset accent, plus panel accent colours, `panel_frame()`, and sparkline/bar helpers.
  - **Dialog button API** (Windows 11-style with proper hover/active state):
    - `theme::dialog_btn_primary(ui, label)` — blue `#0078D4`, white text; hover lightens to `#1A86DB`, pressed darkens. Use for the main action (OK, Save, Install Now, Close, Check for Updates).
    - `theme::dialog_btn_secondary(ui, label)` — gray fill `#343434` with border; hover lightens. Use for cancel/dismiss actions (Cancel, Later).
    - `theme::dialog_btn_secondary_disabled(ui, label)` — same gray, non-interactive. Use for grayed-out actions (Update Now when already up-to-date).
  - **Button layout rule:** wrap button rows in `ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ... })`. Primary action is added first (lands on the right), secondary after (lands to its left). Always place `ui.separator()` immediately before the button row.
  - Implementation detail: hover/active colours work by temporarily overriding `ui.visuals_mut().widgets.{inactive,hovered,active}` inside a `ui.scope()` closure — the scope prevents the override from leaking to surrounding UI.
- **`panels/`** — one file per panel (`cpu.rs`, `gpu.rs`, etc.), each rendering inside `theme::panel_frame()`. Every panel `draw()` now also accepts `th: &theme::AppTheme` so label/muted text colours follow the active preset; panels with temperatures additionally accept `(warn: u8, crit: u8)` supplied from `PanelThresholds` in `main.rs`.
- **`windows/`** — one file per secondary window (`settings.rs`, `about.rs`, `status.rs`, `updater.rs`). All use `egui::show_viewport_immediate` and are centered via `dialog_center()` in `main.rs`. `settings.rs` holds a four-tab layout (Dashboard, Panels, Alerts, Appearance) with live preview: changes are pushed to `current_settings` every frame when `draft != last_preview`; Cancel restores the `original` snapshot captured at open; Save also persists to disk. Appearance includes the theme preset selector. **Live preview applies to:** opacity, theme, rig name, visible panels, floating mode, display profile, all threshold values. **Save-only (not previewed live):** `window_layer` (OS-level hint meaningless while Settings dialog is open).
- **`update_check.rs`** — `check()` fetches `latest.json`, compares semver. `BUNDLED_CHANGELOG` bundles `../../CHANGELOG.md` at compile time (same content as Tauri's bundled resource).
- **`win32_dark_mode.rs`** — calls `uxtheme.dll` ordinals 135 (`SetPreferredAppMode(AllowDark)`) + 104 (`RefreshImmersiveColorPolicyState`) at startup so the OS-drawn tray context menu respects dark mode.

### Data flow

```text
rigstats-sensor.exe  (sensor-sidecar/, .NET 10, Windows Service / LocalSystem)
    └─► LibreHardwareMonitor NuGet → PawnIO kernel driver
            └─► named pipe \\.\pipe\rigstats-sensors  (newline-delimited JSON)
                    └─► lhm.rs: pipe client → LhmData struct
sysinfo crate (CPU load/freq, RAM, disk, network)
wmi crate (GPU name, VRAM, RAM spec/details, system brand)
    └─► commands.rs: get_stats() → StatsPayload
            └─► Tauri IPC invoke("get-stats")
                    └─► frontend/renderer/app.js: tick() every 1s
                            └─► panel modules update DOM
```

### Backend (`src-tauri/src/`)

- **`main.rs`** — Tauri builder, tray icon, lifecycle. Registers two managed state types at startup: `HardwareInfo` (one-time WMI/sysinfo detection) and `AppState` (per-tick runtime state). Picks the best monitor for the profile and shows the main window.
- **`stats.rs`** — Two shared state structs and all serializable payload structs (`StatsPayload`, `CpuStats`, etc.). `HardwareInfo` holds startup-detected constants (disk model map, RAM spec, GPU VRAM, system brand, etc.) registered once and never mutated. `AppState` holds per-tick mutable state (lhm_pipe named pipe connection, sysinfo handles, last samples, alert timestamps, settings, `last_log_prune_day`) behind a `Mutex`.
- **`commands.rs`** — Thin `#[tauri::command]` handlers only. Each handler delegates to a domain module; no business logic lives here. Floating mode transitions are serialized with a mutex in `toggle_floating_mode` to prevent overlapping enable/disable races. `notify_app_ready` triggers `prewarm_panel_windows` (when not already in floating mode) so WebView2 windows are ready before the user first enables floating mode. Includes `set_gpu_preference` command (accepts both `gpu_name` and `gpuName`) used by the GPU selector dots in fixed and floating mode. `apply_tray_logging_indicator` swaps the tray icon (normal ↔ red-dot recording variant) and rebuilds the tray menu to update the Start/Stop Recording label. `open_log_folder` opens the app data directory in Explorer.
- **`debug.rs`** — `append_debug_log`, `reset_debug_log`, `run_hidden_command`, `unix_now_secs`. No deps on other crate modules — safe to import from anywhere.
- **`hardware.rs`** — WMI structs + all startup hardware detection: `detect_gpu_name`, `detect_gpu_vram_total_mb`, `detect_system_brand`, `classify_system_brand`, `detect_model_name`, `detect_motherboard_name`, `normalize_manufacturer`, `detect_ram_spec`, `detect_ram_details`, `detect_ping_target`, `sample_ping_ms`, `probe_wmi_status`, `detect_disk_model_map`. Each function tries WMI first, falls back to PowerShell CIM. `detect_disk_model_map` resolves drive letters to physical disk model names via a three-table WMI join and stores the result in `HardwareInfo` at startup for stable sidecar temperature matching. `detect_motherboard_name` queries `Win32_BaseBoard` for manufacturer + product and normalizes the manufacturer string (ASUSTeK → ASUS, Micro-Star → MSI, etc.); result stored in `HardwareInfo.mb_name`.
- **`lhm.rs`** — Named pipe client that connects to `\\.\pipe\rigstats-sensors` and deserialises the newline-delimited JSON stream into `LhmData`. `fetch_lhm_pipe` reuses an established connection stored in `AppState.lhm_pipe`; on connect failure it logs at most once every 30 s via `LAST_PIPE_FAIL_LOG_SECS`. The pipe client uses `.write(false)` on `ClientOptions` because the sidecar pipe is `PipeDirection.Out` (requesting write access returns `ERROR_ACCESS_DENIED`). GPU selection is handled by `select_gpu_idx` (pure function, testable): preferred GPU (case-insensitive substring) → highest VRAM → tie-break by load. Old HTTP parsing code (`flatten_lhm`, `parse_lhm`, `FlatNode`, etc.) is retained under `#[cfg(test)]` to keep the existing unit tests green.
- **`lhm_process.rs`** — `track_lhm_connection_state` (connect/disconnect logging with 30 s throttle). Used by `lhm.rs` to log pipe connection state changes.
- **`logging.rs`** — Opt-in stats CSV logging: `append_stats_row(&StatsPayload, dir)` appends one row per tick to a rolling daily file (`rigstats-log-YYYY-MM-DD.csv`); `prune_old_logs(dir, days)` deletes files older than the retention limit (runs at most once per calendar day via `AppState.last_log_prune_day`); `current_log_path(dir, secs)` and `ymd_from_unix` (Howard Hinnant civil_from_days algorithm, no chrono dep). The timestamp is computed once per tick to guarantee the CSV row and file path use the same second.
- **`monitor.rs`** — Profile definitions (`normalize_profile`, `profile_dimensions`), monitor selection (`pick_target_monitor`, `fit_score`), panel visibility normalisation (`normalize_visible_panels`), and `compute_panels_logical_height` (mirrors the JS `applyVisiblePanels` height formula so the backend can pre-size the window before `show()` and avoid a full-height flash). `pick_target_monitor` never uses `set_fullscreen` — borderless positioning via `set_size` + `set_decorations(false)` + `set_position` is sufficient. `set_decorations(false)` is always called after `set_size` because Windows `SetWindowPos` can restore `WS_CAPTION`/`WS_THICKFRAME`. `set_position` compensates for the DWM invisible resize border (inset = `inner_position − outer_position`) so the visible content lands flush with the monitor edge.
- **`windows.rs`** — Secondary window creation and tray-anchored positioning: `ensure_settings_window`, `ensure_about_window`, `ensure_status_window`, `ensure_updater_window`, `on_window_event`, `set_last_tray_click_position`. Floating panels are reconciled via `spawn_sync_floating_panels` (blocking-thread dispatch + queue coalescing via `FLOATING_SYNC_QUEUED`). **Critical:** `WebviewWindowBuilder::build()` must NOT run inside `run_on_main_thread` — doing so deadlocks because `build()` internally dispatches to the main event loop, which is already blocked by the closure. All panel window creation uses `tauri::async_runtime::spawn_blocking` so the event loop remains free to process WebView2 callbacks. `prewarm_panel_windows` pre-creates all panel windows (hidden) immediately after `notify_app_ready` so the first floating-mode toggle is instant. `close_floating_panels` hides windows instead of closing them to reduce WebView2 churn during rapid mode switches. Main window hide in floating mode is fail-safe: it only hides after at least one target floating panel is visible.
- **`updater.rs`** — Auto-update logic: `spawn_background_check` (6-hour loop, first check after 10 s), `check_for_update`, `install_update`, `open_updater_window` commands.
- **`diagnostics.rs`** — `collect_diagnostics` Tauri command + helpers (`diag_collect_hardware`, `diag_collect_tasks`, etc.) that gather system info into a ZIP archive for bug reports.
- **`settings.rs`** — `Settings` struct (opacity, model name, dashboard profile, `window_layer` (`"normal"` | `"on_top"` | `"behind"`), visible panels, `last_seen_version`, `thresholds: HashMap<String, ComponentThresholds>`, `alert_cooldown_secs`, `notify_on_warn`, `notify_on_crit`, `settings_version`, `preferred_gpu`, `floating_mode`, `floating_panel_scale`, `panel_layouts`, `logging_enabled`, `log_retention_days`), JSON persistence to Tauri app data dir. `logging_enabled` defaults to `false`; `log_retention_days` defaults to `7`. `ComponentThresholds { warn: Option<u8>, crit: Option<u8> }` is keyed by component (`"cpu"`, `"gpu"`, `"ram"`, `"disk"`, `"battery"`). Threshold semantics differ: temperature components fire when reading **exceeds** the threshold; battery fires when charge % **drops below** the threshold (so warn must be above crit for battery). `settings_version` is a `u8` migration sentinel: 0 = legacy flat fields (pre-1.15), 1 = current map format. `load_settings` runs `migrate_v0_thresholds` once when it reads a version-0 file, then re-persists. The eight legacy flat fields are kept as private `#[serde(default, skip_serializing)]` shims so old settings files can still be read but are never written. Floating panel scale is sanitized in command handlers to `[0.4, 1.0]` (non-finite values fallback to `1.0`).

### Sensor sidecar (`sensor-sidecar/`)

A .NET 10 C# project that replaces the standalone LibreHardwareMonitor application. Embeds the `LibreHardwareMonitor` NuGet library directly and streams sensor data over a Windows named pipe (`\\.\pipe\rigstats-sensors`). Installed and managed as a Windows Service (`rigstats-sensor`) running as LocalSystem — no scheduled task, no HTTP server, no user-session dependency.

**Why a sidecar instead of embedding in Rust:** LHM requires the `PawnIO` kernel driver for low-level hardware register access. This driver is loaded by the .NET library and requires admin privileges — there is no pure-Rust alternative that covers the same breadth of sensors (CPU temp, MB fans/voltages, disk temps, RAM DIMM temps).

**Protocol:** Newline-delimited JSON (one `SensorPayload` object per line, once per second). `lhm.rs` connects as a named pipe client with `.write(false)` and deserialises the stream into `LhmData`.

**Key files:**

- `Program.cs` — entry point; sets up the Generic Host with `AddWindowsService()` and registers `SensorWorker`
- `SensorWorker.cs` — `BackgroundService` implementation: opens `IComputer`, runs the pipe server loop, closes hardware on stop. Creates the pipe with `NamedPipeServerStreamAcl` so `BUILTIN\Users` can connect (required because the service runs as LocalSystem in session 0). Contains `UpdateVisitor` (required by LHM to trigger sensor refresh).
- `SensorReader.cs` — `SensorPayload` model records + static `Extract()` that maps LHM `IComputer` → payload. Same sensor name matching and SensorId prefix filtering as the old `lhm.rs` HTTP parser.
- `app.manifest` — requests `requireAdministrator` for interactive/debug launches; ignored by the SCM when running as a service
- Published as a self-contained single-file exe (`npm run prepare:sidecar`); no .NET runtime required on user machines

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
- **`renderer/panels/`** — One file per panel: `cpu.js`, `gpu.js`, `ram.js`, `network.js`, `disk.js`, `motherboard.js`, `process.js`, `battery.js`. Each exports an `update*Panel(stats, ...)` function. `thresholds` carries `{ warn, crit }` for temperature colour mapping; defaults apply in browser/simulator mode. `gpu.js` renders a ring gauge, 3×2 metadata grid (TEMP, HOT SPOT, CORE CLK, MEM CLK, POWER, FAN), VRAM and GPU load bars, and one optional D3D row (3D and VID bars side by side in a single `bar-row`) hidden via `display:none` when both backend fields are `null`; shown when either is non-null, with `--` for the absent value. GPU temperature uses `GPU Core` as primary and `GPU VR SoC` as fallback (covers AMD iGPUs such as Radeon 890M that only expose SoC temperature). In multi-GPU systems it also renders compact selector dots beside `GPU LOAD`; clicking a dot invokes `set_gpu_preference` and the selected dot stays highlighted. `disk.js` cycles through pages of three drives every 5 ticks when more than three drives are present; the page resets automatically when the drive count changes. `motherboard.js` renders fans/temps/voltages in a three-column layout; `shortLabel()` maps `"Temperature #N"` → `"TN"` and truncates other labels to fit the `8ch` CSS grid column. `process.js` renders the top 8 processes from `StatsPayload.top_processes`; process names are HTML-escaped and `.exe` suffix is stripped; `truncateName` and `formatRam` are pure helpers exported for unit tests. `battery.js` renders charge %, charging state, time remaining, and live power draw (W); bar colour adapts dynamically (accent when charging, green > 50 %, amber 20–50 %, red < 20 %); POWER field colour-coded by discharge rate (green < 12 W, amber 12–20 W, red > 20 W); shows "NO BATTERY" on desktops where `battery.present == false`.
- **`renderer/settings.js`** / **`renderer/about.js`** / **`renderer/status.js`** / **`renderer/updater.js`** — Entry scripts for the secondary windows. `settings.js` drives a four-tab segmented Settings UI (560×600 px, centered on the monitor containing the tray icon): **Dashboard** (display profile, floating mode), **Panels** (drag-to-reorder, toggle visibility), **Alerts** (cooldown, notify-on-critical toggle, temperature + battery thresholds), **Appearance** (rig name, opacity, always-on-top, autostart, theme). Tab state is persisted in `localStorage`. Warning-level alerts are always disabled (`notifyOnWarn: false` is always sent on save); only Critical has a user toggle. `updater.js` drives the update check, changelog rendering, and install flow.

### Dashboard profiles

Profiles are portrait orientations with fixed pixel dimensions (e.g., `portrait-xl` = 450×1920). The profile name is stored in settings; the backend calls `pick_target_monitor()` to move and resize the main window, and the frontend calls `applyProfile()` to scale CSS variables. Both sides share the same list of valid profile names. `pick_target_monitor` is only called in `save_settings` when the profile has actually changed — calling it unconditionally causes a ~3 px position drift on every save due to the DWM inset compensation.

Valid profiles: `portrait-xl` (450×1920), `portrait-slim` (480×1920), `portrait-hd` (720×1280), `portrait-wxga` (800×1280), `portrait-fhd` (1080×1920), `portrait-wuxga` (1200×1920), `portrait-qhd` (1440×2560), `portrait-hdplus` (768×1366), `portrait-900x1600`, `portrait-1050x1680`, `portrait-1600x2560`, `portrait-4k` (2160×3840), `portrait-fhd-side` (253×1080), `portrait-qhd-side` (338×1440), `portrait-4k-side` (506×2160).

Valid panel keys: `header`, `clock`, `cpu`, `gpu`, `ram`, `net`, `disk`, `motherboard`, `process`, `battery`. `motherboard`, `process`, and `battery` are opt-in (not included in the default visible set).

### Sensor sidecar integration

`rigstats-sensor.exe` runs as a Windows Service (LocalSystem, auto-start at boot). The NSIS installer registers the service with `sc create`, sets restart-on-failure policy via `sc failure`, and starts it immediately. On update: `sc stop` in PREINSTALL, then `sc delete` + `sc create` + `sc start` in POSTINSTALL. On uninstall: `sc stop` + `sc delete`.

The Rust backend connects to `\\.\pipe\rigstats-sensors` with `.write(false)` (pipe is write-only from the server side). On connect failure it falls back to the last successful sample so the UI never resets. GPU selection: preferred GPU (if set) → highest VRAM (stable default) → tie-break by load. Extracted GPU fields: core load, core temp, hotspot temp, core clock (`gpu_freq`), memory clock (`gpu_mem_freq`), power, fan speed, VRAM used/total, D3D 3D engine load (`gpu_d3d_3d`), D3D Video Decode load (`gpu_d3d_vdec`), plus `gpu_devices` for selector dots. D3D fields are `None` when idle; the frontend hides the combined D3D row when both are `null`.

### Settings persistence

Settings are stored in `%APPDATA%\se.codeby.rigstats\rigstats-settings.json`. The debug log is at `rigstats-debug.log` in the same directory.

### Testing

Frontend tests use **vitest** and are colocated with modules as `*.test.js` files (e.g., `tempColors.test.js`, `vendorBranding.test.js`). Rust tests are in `#[cfg(test)]` modules at the bottom of their respective files; most require Windows and the `wmi` feature.

### egui dialog design system

All secondary windows (Settings, About, Status, Updater) must follow this layout and colour contract. **Never deviate from these values without updating this section.**

#### Layout skeleton

Every dialog uses three egui panels:

```rust
// 1. Hero — title + optional subtitle row
egui::TopBottomPanel::top("xxx_hero")
    .frame(dialog_hero_frame())
    .show_separator_line(true)
    .show(ctx, |ui| { /* title, installed/version row if relevant */ });

// 2. Footer — status message (optional) + buttons
egui::TopBottomPanel::bottom("xxx_footer")
    .frame(dialog_footer_frame())
    .show_separator_line(true)
    .show(ctx, |ui| {
        // optional status line (small, C_DATE), then add_space(6.0)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // primary button on the right, secondary to its left
            theme::dialog_btn_primary(ui, "OK");
            ui.add_space(6.0);
            theme::dialog_btn_secondary(ui, "Cancel");
        });
    });

// 3. Central — fills all remaining space
egui::CentralPanel::default()
    .frame(dialog_central_frame())
    .show(ctx, |ui| { /* main content */ });
```

Helper frame constructors (define once in `theme.rs` when needed, or inline as shown):

```rust
// All three panels share the same background tone:
// fill = Color32::from_gray(38)  ← dialog surface colour
// hero   inner_margin: { left: 14, right: 14, top: 14, bottom: 12 }
// footer inner_margin: { left: 12, right: 12, top: 8,  bottom: 10 }
// central inner_margin: Margin::same(10)
```

#### Colour tokens (defined in `updater.rs`; will move to `theme.rs` when more dialogs need them)

| Token | Value | Usage |
|---|---|---|
| Dialog surface | `gray(38)` | Hero, footer, central background |
| Inset/scroll area | `gray(27)` | Scroll areas, code blocks — darker = inset visual |
| Section label | `gray(140)` | Small bold headings inside content (e.g. "What's New") |
| Muted text / dates | `gray(128)` | Secondary text, dates, status messages |
| Body text | `rgb(155, 180, 210)` | Primary content text |

#### Section labels and scroll areas

- A **section label** (e.g. "What's New") is rendered as a **free `ui.label()`** — never inside a frame. Font: size 11.0, strong, `gray(140)`.
- A **scroll area with inset content** uses `egui::Frame::new().fill(gray(27)).corner_radius(4)` wrapping a `ScrollArea`. **No border stroke** — the fill difference alone provides the visual distinction.
- Use `egui::Frame::new()` (not the deprecated `Frame::none()`).

#### Mutex / action pattern

Secondary windows that modify shared state must follow this pattern to avoid holding a `MutexGuard` across multiple `show()` closures (which causes borrow-checker errors):

```rust
// 1. Lock once, extract view data into locals
let st = state.lock().unwrap();
let heading = /* derive from st */;
// ...

// 2. Render all panels (read-only access via locals)
egui::TopBottomPanel::top(...).show(ctx, |ui| { /* uses heading */ });
egui::TopBottomPanel::bottom(...).show(ctx, |ui| { action_close = ...; });
egui::CentralPanel::default().show(ctx, |ui| { /* read from st */ });

// 3. Drop guard, then apply actions
drop(st);
if action_close { open.store(false, ...); }
if action_check { state.lock().unwrap().status = ...; }
```

## Kontexthantering

Efter varje svar, uppskatta hur mycket av kontextfönstret som används.
När du bedömer att ~70% är förbrukat, lägg till en varning i slutet av svaret:

⚠️ **KONTEXT ~70%** — Överväg att köra /compact eller starta ny session snart.

När du bedömer att ~90% är förbrukat:

🔴 **KONTEXT KRITISK** — Kör följande innan vi fortsätter:

1. Spara en sammanfattning till CLAUDE.md
2. Starta ny session med sammanfattningen som kickstart
