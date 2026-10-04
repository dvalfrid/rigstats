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
| `rigstats-egui` | `src-egui/` | egui library + two binaries: `rigstats` (main app — panels, tray, settings windows) and `rigstats-wallpaper` (the WorkerW desktop-wallpaper host process). Both embed `dashboard::DashboardRuntime` and render via `dashboard::DashboardView` |

### Layered separation: shared core, swappable shell

The codebase is structured as a **shared telemetry+render core** with a thin,
per-target **application shell** on top:

```
rigstats-sensor.exe   ── Sensor   (separate .NET process, named pipe)
        │
rigstats-backend      ── Backend  (hardware, settings, logging, lhm pipe)
   poll.rs            ── poll_loop (the ~1 Hz telemetry loop)          ← shared
   dashboard.rs       ── DashboardRuntime (telemetry glue) + DashboardView (render core) ← shared
        │
   ┌────┴───────────────────────────┐
rigstats (main.rs)            rigstats-wallpaper (bin/wallpaper.rs)
RigStatsApp shell             WallpaperHost shell
```

Both binaries link the same library, embed the same `DashboardRuntime` (which
owns sparklines, theme, thresholds, textures, and the `drain`/`view` wiring),
and render through the same `DashboardView` driven by the same `poll_loop` —
so the wallpaper host is not a fork of the dashboard, it is the *same*
dashboard with a different shell.

What differs between the two is only the **app shell**, and the difference is
deliberate (not historical):

- **`rigstats` (main)** — interactive: mouse, floating-panel drag/lock, the tray
  menu, the Settings/About/Status/Updater dialogs, live preview, and the
  wallpaper supervisor.
- **`rigstats-wallpaper` (host)** — display-only: no mouse, reads settings from
  disk (~1 Hz), and reparents into the desktop `WorkerW` layer.

A **separate process is mandatory**, not a convenience: a WorkerW child window is
destroyed when Explorer restarts, which must never be allowed to take the main
app down with it. So the two shells cannot be collapsed into one process; the
boundary is a correctness requirement (see *Desktop wallpaper mode* below).

The telemetry→renderer glue that was previously duplicated across both shells
(sparklines, theme, thresholds, textures, `drain()`, `view()`, settings
mapping) now lives in a single `DashboardRuntime` struct (issue #135). Each
shell embeds one instance and keeps only its own concerns on top.

---

## Data Flow

```text
rigstats-sensor.exe  (sensor-sidecar/, .NET 10, Windows Service / LocalSystem)
    └─► LibreHardwareMonitor NuGet → PawnIO kernel driver
            └─► named pipe \\.\pipe\rigstats-sensors  (newline-delimited JSON)
                    └─► lhm.rs (rigstats-backend): pipe client → LhmData struct
    └─► Control Center (same service, same LHM Computer + hardware lock):
            ControlBroker + providers (power plan, fans, CPU limits, Curve Optimizer, GPU power, lighting), FanCurveLoop (~1 Hz)
            └─► named pipe \\.\pipe\rigstats-control  (duplex, request/response + events)
                    └─► control.rs (rigstats-backend): control_task → ControlState

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

**Overlay mode** (`Settings.overlay_enabled`, issue #183): an independent
add-on, not a `window_layer` value — it renders in its own always-on-top
viewport (`RigStatsApp::render_overlay_viewport` in `main.rs`) showing a
compact, optionally click-through strip of user-selected metrics
(`overlay.rs`), corner-anchored or free-dragged, and coexists with whatever
the main window (portrait/landscape/floating/wallpaper) is doing underneath.
Click-through uses `egui::ViewportCommand::MousePassthrough` — no raw Win32
FFI is needed for it, unlike Always-Behind (`win32_behind.rs`). Per-pixel
transparency (where DComp is available) required a hide → reapply-burst →
reveal sequence to avoid a transient white box on activation/resize (the
DWM redirection-bitmap binding doesn't reliably follow a live resize), tuned
to a handful of frames after measuring that a hidden window's paint/present
cadence is throttled to a fixed ~100 ms/frame regardless of work done inside
it. A live Scale-slider drag debounces before committing to a new size, to
avoid re-triggering that burst on every intermediate value. A fixed global
hotkey (`hotkey.rs`, `Ctrl+Alt+O`) shows/hides the overlay without leaving
the game — the conventional in-game-overlay toggle; locking/unlocking
click-through is a separate, less-frequent action left to the tray row
and Settings switch only.

---

## File Structure

```text
rig-dashboard/
├── src-egui/               egui library + binaries (rigstats.exe, rigstats-wallpaper.exe)
│   ├── src/
│   │   ├── lib.rs          Shared library root — re-exports the modules below
│   │   ├── main.rs         `rigstats` bin: eframe run_native, RigStatsApp, wallpaper supervisor
│   │   ├── bin/wallpaper.rs  `rigstats-wallpaper` bin: WorkerW desktop-wallpaper host
│   │   ├── dashboard.rs    Shared DashboardView render core + PanelThresholds
│   │   ├── geometry.rs     Profile dimensions, monitor selection, pinned position
│   │   ├── poll.rs         Poll thread (PollMode Full/Light/Paused), PollStats/DriveInfo/ProcessInfo
│   │   ├── alerts.rs       Pure threshold-breach detection (pending_alerts); cooldowns/notify in main.rs
│   │   ├── dcomp_burst.rs  DcompRevealBurst — hidden reveal policy for per-pixel-transparent viewports
│   │   ├── dialog_reveal.rs DialogReveal — dialogs hidden until rendered, hidden before torn down
│   │   ├── gpu_process.rs  Per-process GPU engine % via PDH \GPU Engine counters + DXGI LUID→adapter map
│   │   ├── gpu_guard.rs    wgpu device-loss guard (uncaptured-error/device-lost callbacks)
│   │   ├── overlay.rs      Overlay mode: selectable metric registry + compact chip renderer
│   │   ├── hotkey.rs       Global hotkey listener (fixed Ctrl+Alt+O — overlay show/hide toggle)
│   │   ├── tray.rs         System tray icon, menu (incl. GPU submenu), TrayCmd, panel-label helpers
│   │   ├── menu_icons.rs   Procedurally-drawn tray context-menu glyph icons
│   │   ├── lock_ext.rs     LockSafe — poison-tolerant `.lock_safe()` mutex helper
│   │   ├── theme.rs        AppTheme, colours, panel_frame(), dialog button API
│   │   ├── brand.rs        Brand logo PNG loading
│   │   ├── tempcolor.rs    temp_color() — value → green/yellow/red
│   │   ├── ring.rs         Ring gauge renderer
│   │   ├── spark.rs        Sparkline ring buffer
│   │   ├── update_check.rs Update detection, download, installer launch
│   │   ├── win_opacity.rs  Window opacity, DComp/DWM window styles, foreground/restore, force_repaint
│   │   ├── win32_dark_mode.rs  Dark-mode tray context menu
│   │   ├── win32_wallpaper.rs  Progman/WorkerW discovery + SetParent reparenting
│   │   ├── win32_behind.rs Always-Behind window layer: apply_behind/prepare_for_drag/keep_behind
│   │   ├── single_instance.rs  Named-mutex + FindWindowW guard: focuses an already-running instance
│   │   ├── panels/         One file per panel (cpu, gpu, ram, net, disk, …)
│   │   └── windows/        Secondary windows (settings, about, status, updater, history)
│   ├── assets/             Embedded PNGs (brand logos, tray icon)
│   └── Cargo.toml
├── rigstats-backend/       Shared Rust lib (telemetry, hardware, settings)
│   └── src/
│       ├── stats.rs        StatsPayload + sub-structs (session-recording row shape), DiskKind
│       ├── hardware.rs     WMI/PowerShell hardware detection at startup
│       ├── lhm.rs          Named pipe client → LhmData; GPU selection
│       ├── lhm_process.rs  LHM connection state tracking
│       ├── settings.rs     Settings struct, JSON persistence, atomic_write
│       ├── autostart.rs    Windows startup registry management
│       ├── logging.rs      Session-based CSV stats logging, sessions.json index
│       └── debug.rs        Debug log helpers
├── sensor-sidecar/         .NET 10 C# sidecar (rigstats-sensor.exe)
│   ├── Program.cs          Entry point (Windows service host)
│   ├── SensorWorker.cs     LHM Computer, multi-client telemetry pipe server, shared ≤1 Hz sample
│   ├── SensorReader.cs     SensorPayload model + Extract() mapping
│   └── sensor-sidecar.csproj
├── sensor-sidecar.Tests/   xUnit tests: SensorReader.Extract rules + JSON contract
├── docs/
├── website/
├── assets/                 Screenshot PNGs for website/README
└── build/
    ├── installer.nsi       NSIS installer script
    └── pawnio/             Official PawnIO setup (pinned, SHA-256 checked)
```

---

## Backend Modules

### Quick reference

**`rigstats-backend/src/`** — shared lib consumed by the egui binary:

| Module | Responsibility |
| --- | --- |
| `stats.rs` | `StatsPayload` and its sub-structs — the serialisable row shape for session recording (built from `PollStats`); `DiskKind` |
| `hardware.rs` | Hardware detection via WMI (PowerShell only as a fallback when WMI fails) |
| `lhm.rs` | Named pipe client → `LhmData`; GPU selection and sensor extraction |
| `lhm_process.rs` | `track_lhm_connection_state` — sidecar pipe connect/disconnect logging, 30 s "still offline" throttle |
| `control.rs` | Control Center pipe client (`control_task`) → `ControlState` (capabilities, profiles, active profile, live `fan_duty`, `fan_identified` channel→fan mapping, safety trips, running preview, crash-guard notice); `ControlCmd` (apply/save profile, preview/confirm, identify fan, release to firmware); typed `FanPart`/`FanHeaderConfig`/`FanCaps`, `CpuLimitPart`/`AmdCpuLimit`/`CpuLimitCaps`, `GpuPart`/`GpuAdapterConfig`/`GpuCaps`, `CurveOptPart`/`CurveOptCaps`, `AuraPart`/`AuraCaps`; `active_ppt_limit()` and `active_gpu_limit(name)` feed the CPU panel's `PPT … W` and the GPU panel's `PL … %` lines. Design: `docs/control-architecture.md` |
| `settings.rs` | `Settings` struct, JSON persistence, `atomic_write` |
| `autostart.rs` | Windows startup registry management (HKCU run key) |
| `logging.rs` | Session-based CSV stats logging — `start_session`/`end_session`/`append_stats_row`, `load_sessions`/`rename_session`/`set_session_pinned`/`delete_session`/`prune_old_sessions`, `reconcile_sessions_on_startup`; `sessions.json` index guarded by `SessionsLock` (cross-process file lock) and a `.bak` for corruption recovery |
| `debug.rs` | Debug log helpers — no deps on other modules |

**`src-egui/src/`** — egui library + binaries:

| Module | Responsibility |
| --- | --- |
| `lib.rs` | Shared library root — declares + re-exports the modules below so both bins link them |
| `main.rs` | `rigstats` bin: eframe `run_native`, `RigStatsApp` + `eframe::App` impl, panel rendering, wallpaper-mode supervisor |
| `bin/wallpaper.rs` | `rigstats-wallpaper` bin: minimal eframe host that renders the dashboard into the desktop WorkerW layer |
| `dashboard.rs` | `DashboardRuntime` (owned telemetry→renderer glue: sparklines, theme, thresholds, textures, `drain`/`apply_settings`/`view`); `DashboardView` (borrowed per-frame render state + `draw_one_panel`/`render_landscape_grid`); `PanelThresholds` |
| `geometry.rs` | Profile dimensions (`profile_to_size`), monitor enumeration/selection, pinned-position resolution (unit-tested) |
| `poll.rs` | Background `poll_loop` (tokio), `PollStats`/`DriveInfo`/`ProcessInfo` data types, CSV log payload mapping. `PollMode` (shared via `PollModeHandle`): `Full` normally; in wallpaper mode (the host is the dashboard's poller) `Light` while the overlay is on — overlay metrics only, no per-app lists, no session recording — else `Paused` |
| `alerts.rs` | `pending_alerts` — pure warn/crit threshold-breach detection (no I/O, unit-tested); `notify_on_*` checks, per-alert cooldowns and sending the notification are the caller's job in `main.rs` |
| `dcomp_burst.rs` | `DcompRevealBurst` — decision logic for revealing a per-pixel-transparent (DComp) viewport only after its style/resize burst settles; used by the overlay and floating panels |
| `dialog_reveal.rs` | `DialogReveal` — each dialog window is created hidden and revealed after it has rendered, and hidden for one frame before it's torn down (no white flash on open/close); see the dialog lifecycle under `windows/` below |
| `gpu_process.rs` | `GpuEngineQuery` — persistent PDH query on `\GPU Engine(*)\Utilization Percentage` (Task Manager's data source); `adapter_luid_map()` maps each sample's LUID to a physical GPU via DXGI. Vendor-neutral, unelevated. Pure `parse_instance`/`aggregate` are unit-tested; the Win32 FFI carries a scoped `#![allow(unsafe_code)]` |
| `gpu_guard.rs` | `install_gpu_loss_guard` — wgpu `on_uncaptured_error`/`set_device_lost_callback` handlers that flag a fatal device error instead of letting wgpu panic the process |
| `overlay.rs` | `ALL_OVERLAY_METRICS` registry (key/label/unit/`extract` fn/`color` fn) and `draw_overlay` — the compact chip renderer for the overlay add-on |
| `hotkey.rs` | Global hotkey listener (`RegisterHotKey`/`WM_HOTKEY` on a dedicated thread) — fixed `Ctrl+Alt+O`, shows/hides the overlay |
| `tray.rs` | System tray icon + menu, `TrayCmd` channel, `load_app_icon`, `panel_label`/`panel_initial_h`. `GpuMenu`: the "GPU ▸" submenu (Automatic + one check row per adapter), filled in once background adapter detection reports back; rows use deterministic ids decoded by `gpu_choice_from_menu_id`. "Toggle Desk Lamp" (`TrayCmd::ToggleLamp` → `ControlCmd::ToggleLamp`) is inserted into the menu only while a lighting device with a lamp is connected (`set_lamp_available`, from `ControlState::has_lamp`) |
| `menu_icons.rs` | Procedurally-rasterized glyph icons (circle/ring/triangle/rect/line primitives, supersampled) for each tray context-menu row — no external image assets |
| `lock_ext.rs` | `LockSafe` trait — `.lock_safe()` recovers a poisoned `Mutex` instead of panicking; used throughout `windows/*.rs` |
| `theme.rs` | `AppTheme`, colour constants, `panel_frame()`, sparkline/bar helpers, dialog button API, `apply_dashboard_fonts` |
| `brand.rs` | Brand logo PNG loading (13 logos embedded at compile time) |
| `tempcolor.rs` | `temp_color(value, warn, crit)` → green/yellow/red |
| `update_check.rs` | Update detection (`check()`), download with progress, `launch_installer()` |
| `win_opacity.rs` | Raw Win32 window helpers: `SetLayeredWindowAttributes` opacity, `set_no_redirection_bitmap` (DComp), `disable_dwm_transitions`, `bring_to_foreground` (restores a minimized window first), `force_repaint`, `find_hwnd` |
| `win32_dark_mode.rs` | Dark-mode tray context menu via `uxtheme.dll` ordinals; `apply_titlebar_theme` (dark DWM title bar per dialog) |
| `win32_wallpaper.rs` | Progman/WorkerW discovery, `SetParent` reparenting, attach/detach/`is_attached`, parent-process liveness |
| `win32_behind.rs` | Always-Behind window layer: `apply_behind`, `prepare_for_drag` (called before a floating-panel drag so `SC_MOVE` works under `WS_EX_NOACTIVATE`), `keep_behind` |
| `single_instance.rs` | `ensure_single_instance` — named kernel mutex (`CreateMutexW`) acquired first thing in `main()`; if already held, `FindWindowW` + `SetForegroundWindow` focuses the running instance's window and this process exits |
| `panels/` | One file per panel — each exports `draw(ui, stats, opacity, th, sc, ...)` returning `egui::Rect`. Panels: `cpu`, `gpu`, `ram`, `net`, `disk`, `motherboard`, `process`, `gpu_processes`, `power`, `battery`, `clock`, `header` |
| `windows/` | Secondary windows: `settings.rs`, `about.rs`, `status.rs`, `updater.rs`, `history.rs`, `control.rs` (Control Center: profiles with Duplicate / Rename / Delete or Reset, Power tab choosing the profile's Windows power plan with a shortcut to Windows' Power Options, Fans tab with curve editor, identify, shared curves, CPU tab with PPT/TDC/EDC sliders, Eco presets and a Curve Optimizer card (all-core, per-core), GPU tab with a power-limit slider per adapter, Lighting tab (Aura Sync to ASUS Aura motherboards, Aura monitors + light bar, ROG keyboards, the ROG Delta II and any Dynamic Lighting device: effect, colour (picker plus fixed quick-pick swatches), brightness, live preview of only what changed (`preview_delta`); a Desk lamp row — on/off, brightness, colour temperature — for the light bar's lamp, with a note when the lamp's real state (`lamp_on`, read at open) differs from the profile; per-device "blocked" reasons); limit changes go Try → Keep/Undo). Opened from the tray, the header's profile chip, or a fan name in the Motherboard panel — which opens the curve of the channel that drives that fan, as measured by identify (never matched by name) |

### Module details

#### `main.rs`

Entry point: settings load, window placement, and `eframe::run_native`, hosting `RigStatsApp` and its per-frame `ui()` loop. Monitor selection lives in `geometry.rs`, tray setup in `tray.rs`, and the poll thread in `poll.rs`. Starts a tokio runtime for the poll loop and the background auto-update check (first check after 10 s, then every 6 h). The poll loop samples hardware once per second and sends a `PollStats` snapshot to the UI thread via `mpsc::SyncSender`. On startup, checks for `--just-updated=VERSION` argument (set by the NSIS in-app updater) and opens the updater dialog in `JustUpdated` state if present.

#### `stats.rs`

Defines the serialisable `StatsPayload` and its sub-structs — the row shape
written by session recording (`logging::append_stats_row`), built from each
tick's `PollStats` by `poll_stats_to_log_payload` in `src-egui/src/poll.rs`.
Live per-tick state (sysinfo collectors, the sensor-pipe reader, ping/battery
caches, startup detection results) is held as locals of each `poll_loop`,
not in a shared struct.

**Payload structs:**

| Struct | Contents |
| --- | --- |
| `StatsPayload` | Top-level snapshot (one CSV row per tick while recording) |
| `CpuStats` | Load, per-core loads, temp, freq, power |
| `GpuStats` | Load, temps, clocks, VRAM, fan, power, D3D, and `available_gpus` selector metadata |
| `RamStats` | Used/free/total, spec string, DIMM temp |
| `NetStats` | Up/down throughput, interface name, ping |
| `DiskStats` | Read/write throughput, per-drive entries |
| `DiskDrive` | Filesystem label, size, used, pct, temp |
| `MotherboardStats` | Fans, temps, voltages, chip name, board name |
| `ProcessEntry` | Process name, CPU % of total system, RAM in MB |
| `BatteryStats` | `present`, charge %, charging state, time remaining, power draw (W) |

The `power` panel (`panels/power.rs`) is opt-in and derives its estimate entirely from fields already in `PollStats` — no new sidecar data. It computes `cpu_power + gpu_power + platform_overhead` (25 W desktop / 10 W laptop) and exposes CPU/GPU breakdown bars plus a bottom gauge. The gauge ceiling is `Settings.psu_watts` when set by the user, otherwise the auto reference.

`StatsPayload.top_processes` is a `Vec<ProcessEntry>` pre-sorted by CPU usage
and capped at 8 entries before serialisation.


#### `hardware.rs`

All startup hardware detection. Each function queries WMI (the `wmi` crate) and
only falls back to a PowerShell `Get-CimInstance` process (~1 s each) when the
WMI query itself fails. Row structs used with a typed `conn.query::<T>()` must
carry a container `#[serde(rename = "Win32_…")]` — `wmi` derives the `FROM`
class from the serde name, so a missing rename queries a non-existent class and
silently forces the slow fallback (#198); `wmi_classes_tests` guards this.
Callers run these off the UI thread (`spawn_blocking` in `poll_loop`, or a
startup background thread in `main.rs`) and keep the results.

| Function | What it detects |
| --- | --- |
| `detect_gpu_name` | Primary discrete GPU name |
| `detect_gpu_names` | All physical adapter names (virtual/basic adapters filtered, deduped by normalized name) — the choices for the tray "GPU ▸" submenu and Settings → Display → GPU |
| `detect_battery_present` | Whether a battery exists (gates the Battery panel / `battery_pct` overlay metric in Settings); a WMI failure counts as "present" |
| `detect_gpu_drivers` | Installed GPU driver name/version/date per adapter (`Vec<GpuDriverInfo>`); `driver_age_days` derives age from `DriverDate` |
| `detect_system_brand` | Brand key: `rog`, `msi`, `alienware`, etc. |
| `classify_system_brand` | Brand classification logic |
| `detect_model_name` | System model name |
| `detect_motherboard_name` | Board manufacturer + product (normalised) |
| `detect_ram_spec` | Type + speed string, e.g. "DDR5 6000 MT/s" |
| `detect_ram_details` | Stick count, capacity, vendor, part number |
| `detect_disk_model_map` | `HashMap<drive_letter, model_name>` via WMI join |
| `detect_ping_target` | First IPv4 default gateway (`Win32_NetworkAdapterConfiguration`), else `1.1.1.1` |
| `probe_wmi_status` | Checks whether WMI is reachable |
| `sample_battery_wmi` | Per-tick (cached 10 s) battery query via `Win32_Battery` + `root\wmi BatteryStatus` (charge/discharge rate in mW) |

`detect_disk_model_map` builds its map via a three-table WMI join:
`Win32_DiskDrive → Win32_DiskDriveToDiskPartition → Win32_LogicalDiskToPartition`.
`poll_loop` keeps the map for its lifetime so LHM temperatures can be matched by
model name rather than by index (stable when USB drives are inserted/removed).

#### `lhm.rs`

Named pipe client that connects to `\\.\pipe\rigstats-sensors` (written by the
`sensor-sidecar` process) and deserialises the newline-delimited JSON stream
into `LhmData`.

`fetch_lhm_pipe` is called once per tick. It reuses an established connection
held by the calling `poll_loop` (`tokio::sync::Mutex<Option<LhmPipeReader>>`).
The sidecar serves several clients at once (e.g. the wallpaper host plus the
main app feeding the overlay) from one shared, at most ~1 Hz LHM sample.
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
index:

- Use `preferred_gpu` if it matches a candidate — exact match on the
  normalized name first (`normalize_gpu_name`: case-insensitive, `(TM)`/`(R)`/
  `™`/`®` stripped, whitespace collapsed, so WMI and LHM spellings agree), then
  a normalized substring match (`gpu_names_match`)
- Otherwise pick the highest VRAM GPU (stable default)
- Tie-break by load

The tray "GPU ▸" submenu and Settings' GPU list tick the same adapter via
`tray::selected_gpu_index`, which uses the same matching.

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

Just one function: `track_lhm_connection_state(dir, connected)`, called each
stats tick. Logs a message when the sidecar pipe transitions
connected→disconnected or back, and throttles the repeated "still offline"
warning to once per 30 s so a prolonged outage doesn't spam the debug log.
Diagnostics-ZIP collection (including the LHM sensor tree) lives in
`src-egui/src/windows/status.rs` (`collect_and_open_diagnostics`), not a
separate module.

#### `settings.rs`

`Settings` struct persisted as JSON to
`%APPDATA%\se.codeby.rigstats\rigstats-settings.json`.

All fields use `#[serde(default)]` for backwards-compatible schema evolution —
new fields deserialise cleanly from older settings files. `last_seen_version`
is compared against `CARGO_PKG_VERSION` at startup to detect the first launch
after an upgrade.

Alert thresholds are stored as `thresholds: HashMap<String, ComponentThresholds>`
where `ComponentThresholds { warn: Option<u8>, crit: Option<u8> }` and the keys
are `"cpu"`, `"gpu"`, `"ram"`, `"disk"`, `"cpu_load"`, `"gpu_load"`,
`"ram_load"`, `"disk_usage"`, `"battery"`, `"battery_power"`.

Threshold semantics differ by key:

- **Temperature keys** (`cpu`/`gpu`/`ram`/`disk`): alert fires when the reading
  **exceeds** the threshold. `warn < crit` is enforced.
- **Load/usage percentage keys** (`cpu_load`/`gpu_load`/`ram_load`/
  `disk_usage`): same "exceeds" direction as temperature, but a distinct key
  since the values are percentages, not °C — kept separate from `cpu`/`gpu`/
  `ram`/`disk` (which are temperature-only) so a load % reading is never
  colored against a °C-calibrated cutoff. Read into `PanelThresholds` in
  `dashboard.rs` and consumed by the overlay's per-metric `color` functions
  (`overlay.rs`) and the main Disk panel's per-drive usage bar.
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
  all panel windows. The per-panel padlock toggles a shared `floating_lock_arc`
  (`AtomicBool`); each frame's `update()` compares it against
  `self.floating_panels_locked` and persists on change (`main.rs`, ~line 1002) —
  there is no single `toggle_floating_lock` function.
- **`panel_layouts: HashMap<String, PanelLayout>`** — last known `outer_position`
  (`x: i32, y: i32`) per panel key. Positions are tracked directly in
  `render_floating_panels` (`src-egui/src/main.rs`) from each panel viewport's
  `outer_rect` and persisted to `rigstats-settings.json`; no JavaScript or
  debounce is involved.

Multi-GPU pinning adds one field:

- **`preferred_gpu: Option<String>`** — user-selected GPU device name for stable
  display across ticks; `None` means use backend stable default (highest VRAM).
  Set from three places, all through `RigStatsApp::select_gpu`: the GPU panel's
  click dots (fixed/floating mode), the tray "GPU ▸" submenu, and Settings →
  Display → GPU (only shown with more than one adapter). The tray and Settings
  are the only ways in Desktop Wallpaper mode, where the dashboard is
  click-through. The main app pushes it into its `poll_loop` on every settings
  reload; the wallpaper host re-reads it from disk in `refresh_settings`, so a
  change applies to the dashboard and the overlay within ~1 s.

Fullscreen (fill-screen) mode adds two fields — both `#[serde(default ...)]`, no
migration needed:

- **`fullscreen_mode: bool`** — when true (and not floating), the fixed window
  fills the portrait monitor's height instead of fitting panel content; the
  dashboard background fills the rest. Width stays at the profile width so panel
  proportions never stretch.
- **`fullscreen_align: String`** — `"top"` or `"center"` (default `"center"`):
  where the panel stack sits within the filled window.

#### `windows/` (`settings.rs`, `about.rs`, `status.rs`, `updater.rs`, `history.rs`)

Secondary egui windows, each rendered via `show_viewport_immediate` from
`RigStatsApp::ui` in `main.rs` while its `*_open` flag is set (tray commands set
the flags). Every dialog is centred with `geometry::dialog_center(w, h)`, which
enumerates real monitors via `geometry::win_monitor::list()` (falls back to
`[100.0, 100.0]` when none are found) rather than tracking a tray-click
position. Settings is 560×600; About, Status, History and Updater have their own
fixed sizes set at their `show_viewport_immediate` call sites. Visual design:
`src-egui/src/windows/CLAUDE.md`.

**Dialog lifecycle** (every dialog must follow this — see #203):

- **Open hidden, reveal when rendered.** The builder gets
  `.with_visible(self.dialog_reveal.visible(id))`; the window renders
  `DialogReveal::FRAMES` frames hidden before it's shown. A window shown before
  its first wgpu present flashes white (worst on a slow/power-saving GPU).
- **Hide before teardown.** `DialogReveal::track` queues a just-closed dialog;
  it gets one content-less `with_visible(false)` frame, so eframe drops its GPU
  surface from an already-hidden window.
- **`finish_dialog_frame`** (after each dialog's `show_viewport_immediate`)
  applies the dark title bar, disables DWM transitions
  (`win_opacity::disable_dwm_transitions` — DWM's close animation used a white
  snapshot of the flip-model swap chain), forces repaints while hidden, and
  brings the dialog to the foreground once visible.
- **Reopen restores.** Dialogs have no taskbar button, so
  `win_opacity::bring_to_foreground` restores a minimized dialog (e.g. after
  Win+D) before focusing it.

A new dialog is added by giving it an `id`, a `*_open`/`*_focus` flag pair, a
`dialog_reveal.track` entry, `.with_visible(visible)` on its builder, and a
`finish_dialog_frame` call.

Floating panel management lives in `render_floating_panels` (`main.rs`), not a
separate `windows.rs` module:

- Iterates `self.runtime.visible_panels` and opens one frameless,
  `WindowLevel`-matched viewport per panel via `show_viewport_immediate`,
  sized from `panel_initial_h(key)` scaled by `floating_panel_scale`.
- A panel's position is only set on first creation (`with_position`); after
  that the OS owns it via drag, and the viewport's `outer_rect` is read back
  each frame to update `floating_positions` (persisted to
  `settings.panel_layouts` on Save). Panels without a saved position stagger
  diagonally from `[100.0, 80.0]`.
- Each panel draws its own drag zone (top 24 px) and padlock hit-rect inline;
  dragging is triggered by a raw `just_pressed` check in that zone rather than
  `egui::Sense::drag()`, so it can call `win32_behind::prepare_for_drag` first
  when the window layer is "Always Behind" (`SC_MOVE` needs the window
  active, which `WS_EX_NOACTIVATE` normally prevents).
- The per-panel lock toggle flips `floating_lock_arc`, shared across all
  panel viewports.

#### Updates (`update_check.rs` + `windows/updater.rs`)

`update_check.rs` is plain blocking I/O: `check()` fetches `latest.json` and
compares versions, `download()` streams the installer with a progress
callback, `installer_temp_path()` / `launch_installer()` run it, and
`BUNDLED_CHANGELOG` embeds `CHANGELOG.md`. The background check is a tokio task
spawned in `main.rs` (first check after 10 s, then every 6 h) that runs these
via `spawn_blocking`, downloads an available update, and drives
`windows/updater.rs`'s `UpdaterState`/`UpdateStatus` (shown by the Update
dialog and the clock panel's update badge).

#### `history.rs`

Session History window, opened via `TrayCmd::OpenHistory`. Left panel lists
sessions (name, time range, duration, avg CPU/GPU) with Pin/Rename/Reveal/Delete
row actions — buttons share one fixed size per row and wrap onto a second line
rather than overflow a narrow panel. Renaming swaps the row into an inline
`TextEdit` with Save/Cancel (Enter/Escape also commit/cancel). Selecting a
session loads its CSV rows on a background thread
(`spawn_load_rows`/`spawn_load_sessions`, guarded by an `AtomicBool` so a
second load while one is in flight is a no-op) and renders CPU/GPU/RAM/
network/disk/ping charts via `egui_plot`, one `Plot` per metric group linked
by a shared group id so hovering any chart shows a synced crosshair and
per-curve value readout across all of them. The list refreshes on open, after
any pin/rename/delete action, and whenever `TrayCmd::ToggleRecording` fires
while the window is open (otherwise a session ending while History is open
would keep showing it as still recording until a manual Refresh).

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
| `gpu_processes` | GPU Apps | opt-in | PDH `\GPU Engine` · DXGI · sysinfo |
| `battery` | Battery | opt-in | sidecar · WMI |
| `power` | System Power | opt-in | sidecar (derived, no new sensors) |

Panel visibility and order are saved as a plain `Vec<String>` of keys in
`Settings.visible_panels` (`rigstats-backend/src/settings.rs`) — there is no
separate validation function; an unrecognized key is simply ignored by the
renderer's key→panel match.

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
| `settings.json` | `rigstats-settings.json` read from disk | All persisted user settings |
| `sidecar-log.txt` | `%PROGRAMDATA%\se.codeby.rigstats\rigstats-sensor.log` | Sidecar file log: start/stop, per-client connect/disconnect, and Control Center outcomes (#207): every profile apply/rollback with its reason, failed fan verifies (header, commanded vs. read-back duty), fan identify results (`control/1 -> Fan #2 (...), Fan #5 (...)`), safety overrides. No parsed sensor values. |
| `control-capabilities.json` | `ControlState.capabilities` (UI snapshot) | What the board supports: writable fan channels with min/max and their measured `drives` (channel→fan mapping, persisted in `fan-channels.json`), available curve sources, power schemes, CPU limit support (generation, PM table version, BIOS/min/current limits — or why unsupported), GPU power-limit adapters (name, PNP id, driver range, default/original/current), Curve Optimizer support (cores, per-core, BIOS/current offsets), lighting controller (firmware, zones — or why unavailable, e.g. Armoury Crate) |
| `lighting-devices.json` | `%PROGRAMDATA%\se.codeby.rigstats\lighting-devices.json` (written by the service after every device discovery — at start and when devices change) | Lighting devices RIGStats drives (Aura USB firmware + config table, Aura monitor config reply, LampArray kind/lamp count/update interval/report layout), conflict state (OpenRGB, Armoury Crate, Windows Dynamic Lighting), and a scan of every HID collection (VID/PID, interface, usage page/usage, report lengths, product name; `known_as` null = not supported yet). No HID paths or serials. Enough to add or fix a device from a user's export (#212) |
| `profiles.json` | `%PROGRAMDATA%\se.codeby.rigstats\profiles.json` | The service-owned profile store (power plan, fan curves, CPU limits, Curve Optimizer, GPU power limit, lighting incl. desk lamp) |
| `sensor-tree.txt` | `%PROGRAMDATA%\se.codeby.rigstats\sensor-tree.txt` | Full LHM hardware/sensor tree written by the sidecar at service start — the raw input for `/sensor-fixture` |
| `sidecar-service.txt` | `sc query` + `sc qc` + legacy schtasks | Service status, config, and any lingering LHM scheduled tasks |
| `hardware.json` | PowerShell `Get-CimInstance` | OS, CPU, GPU, board, RAM modules, disks |
| `environment.txt` | `std::env::var` | `USERNAME`, `USERDOMAIN`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `COMPUTERNAME`, `PROCESSOR_ARCHITECTURE` — exposes child/standard account path redirections |
| `event-log.txt` | PowerShell `Get-WinEvent` | Windows Application Event Log: rigstats errors and critical events — catches OS-level crashes not recorded in the in-app log |
| `sysinfo.json` | sysinfo snapshot + WMI probe + RAM shell probe | See sysinfo diagnostics below |
| `displays.json` | `geometry::win_monitor::list()` + `pick_window_rect_for_profile()` | Each monitor's position/resolution, `is_primary`, and `is_selected` for the active dashboard profile |
| `gpu-engine.txt` | `gpu_process::dump_diagnostics()` | Raw PDH `\GPU Engine(*)` instance names/values + the full DXGI adapter list. The exact input `gpu_process::parse_instance`/`aggregate` consume — doubles as a ready-made fixture for `src-egui/fixtures/gpu-engine/` (see its README, and `/gpu-engine-fixture`). Blocks ~1s (PDH needs two collects spaced in time). |

### `sysinfo.json` — key fields

| Field | What it tells you |
| --- | --- |
| `cpu_brand`, `cpu_count` | sysinfo's view of the CPU at collection time |
| `total_memory_mb`, `used_memory_mb` | sysinfo memory snapshot |
| `wmi_available` | `probe_wmi_status()` at collection time (`SELECT Caption FROM Win32_OperatingSystem`). `false` means WMI is unreachable, so every detection falls back to PowerShell (slow) or comes back empty |
| `sysinfo_available` | Always `true` (kept for format compatibility) |
| `ram_spec_probe` | Stdout of an independent PowerShell `Win32_PhysicalMemory` query ("<type> <speed> MT/s (<n> DIMMs)"), to compare with the `hardware: ram_spec=` line in `debug.log` when RAM detection looks wrong |

The detected startup values themselves (`ram_spec`, `ram_details`, `mb_board`,
`model_name`, `system_brand`, `gpu_name`, `gpu_names`, `ping_target`,
`battery_present`, `disk_model_map entries`) are logged as `hardware: …` lines in
`debug.log`.

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

- **No tick overlap by construction** — `poll_loop` is a single `loop { ...;
  tokio::time::sleep(1s).await }` on one task, not a re-entrant timer callback,
  so there is no possibility of two ticks running concurrently or out of order.
- **Sidecar pipe read failures are per-tick, not sticky** — `fetch_lhm_pipe`
  returns `Option<LhmData>`; on a failed/timed-out read it returns `None` for
  *that* tick only (logged via `lhm_process::track_lhm_connection_state`, throttled
  to once per 30 s). There is currently no last-known-good LHM sample kept
  across ticks, so LHM-derived fields go blank for the duration of an outage
  rather than holding the previous value — non-LHM data (sysinfo, WMI) is
  unaffected since it's sourced independently each tick.
- **Threshold-based alert notifications are not currently wired up.**
  `Settings` has full support for this — per-component warn/crit thresholds,
  `notify_on_warn`/`notify_on_crit` toggles, `alert_cooldown_secs`, and a
  working "Test Notification" button (`windows/settings.rs`,
  `send_test_notification` — shows a real Windows balloon tip via a hidden
  PowerShell `NotifyIcon` script) — but nothing in `poll_loop` or `main.rs`
  currently compares a live reading against its threshold and fires that
  notification automatically. Thresholds are otherwise fully used for panel
  colour-coding (warn = yellow, crit = red). Tracked as a bug — see
  [#179](https://github.com/dvalfrid/rigstats/issues/179).

### Window placement

- `pick_window_rect_for_profile` (in `src-egui/src/geometry.rs`) targets a monitor only when its resolution **matches** the active profile (both dimensions within ~10 %); among matches it picks the closest fit, so a dedicated strip/secondary that fits the profile is used automatically. When no monitor matches it falls back to the **primary** monitor (top-left at the virtual origin `0,0`), then to the first monitor. Both orientations use this path, so a portrait/side profile lands on a resolution-matching screen or the main screen rather than any small portrait monitor. The pure selection step is `select_profile_monitor`, which is unit-tested. The egui window is positioned at the chosen monitor's top-left origin.
- **Position carry-over on profile change**: switching display profiles keeps the window at its current position when it is still on a connected monitor, instead of re-targeting a monitor. `fixed_window_geometry` (settings reload) and startup return `last_fixed_pos` (the window's current outer position, tracked each fixed-mode frame and validated by `guard_panel_position`) and only auto-target the profile-matching monitor when that spot is off-screen. Skipped in fullscreen (which snaps to the filled monitor) and overridden by a pin; `last_fixed_pos` is frozen while the window is parked off-screen for wallpaper mode.
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

### Desktop wallpaper mode (WorkerW)

- **Why a separate process** — `window_layer == "wallpaper"` reparents a window into the desktop `WorkerW` (between wallpaper and icons) so it survives `Win+D`. A child window is destroyed when its parent is — cross-process included — so an Explorer restart would destroy the reparented window. Reparenting the *main* window would therefore kill the app; instead a dedicated **`rigstats-wallpaper`** host process owns the wallpaper window, and the main app supervises it.
- **Supervisor** (`RigStatsApp::update_wallpaper_mode`, called each frame) — on entering wallpaper mode the main app parks its own window off-screen and spawns the host, which becomes the dashboard's poller. Each frame it also sets its own `poll_loop`'s `PollMode`: `Paused` in wallpaper mode (sensor pipe released, no work), or `Light` while the game overlay is on (the overlay is rendered by the main app, so it keeps polling overlay metrics — the sidecar serves both processes from one shared sample; per-app lists and session recording are skipped so the host alone records), and `Full` outside wallpaper mode. It relaunches the host if it exits (covers an Explorer restart that destroyed the host's window) and kills it on leaving the mode or quitting. Mutually exclusive with floating mode (floating wins).
- **Host** (`bin/wallpaper.rs`) — a minimal eframe app whose window fits the dashboard rather than filling the monitor (that would break panel proportions): **landscape** uses the fixed profile size (the adaptive grid fills it), **portrait** fits the panel-stack content height (`compute_window_height` at creation, then a per-frame fit to `ui.min_rect()` exactly as the main app does in normal mode — otherwise the leftover of the full profile height renders as a black gap below the stack). Centred on the matching monitor, so the wallpaper shows around it. Renders the shared `DashboardView`. On the first frame it caches its HWND (via `FindWindow`, before reparenting — afterwards the window is a WorkerW *child* and `FindWindow` can no longer find it) and attaches. It re-attaches each tick if `is_attached` reports it was detached, and exits if its parent PID (`RIGSTATS_PARENT_PID`) disappears so it never orphans.
- **Positioning** — the host window is placed at `Settings::wallpaper_position` (absolute screen coords, so it encodes both *which* monitor and *where*) when that point is still on a connected monitor, else centred on the profile-matching monitor. The supervisor captures that position from the main window's last on-screen position when **entering** wallpaper mode and persists it before spawning the host, so the workflow is: position the window in a normal layer, then switch to wallpaper. Leaving wallpaper mode restores the window to that spot. Verified to coexist with Wallpaper Engine (WE paints the wallpaper into the WorkerW; our child window sits above it).
- **WorkerW discovery** (`win32_wallpaper::find_wallpaper_workerw`) — Windows 11 keeps the wallpaper `WorkerW` as a *child* of `Progman` (checked first, no spawn); older Windows 10 needs `SendMessageTimeout(Progman, 0x052C)` then the top-level `WorkerW` sibling after Progman; a legacy `SHELLDLL_DefView`-sibling enumeration is the final fallback. Reparented coordinates are translated to WorkerW-client space so the window stays on the correct monitor.
- **v1 is display-only** — no mouse hook; drag/padlock are N/A, and the displayed GPU is chosen from the tray "GPU ▸" submenu or Settings → Display → GPU (the host picks the change up on its next settings refresh). The host binary is also the artifact a future Wallpaper Engine "Application wallpaper" integration (ROADMAP, v3.0) will reuse. While `window_layer == "wallpaper"` is selected, Settings disables the controls that have no effect in this mode — Floating Mode and Fill Screen + Alignment — with an explanatory note. Opacity is supported (see below).
- **Settings apply on Save, not live preview** — the host is a separate process that reads `rigstats-settings.json` from disk (~1 Hz), while Settings live-preview only pushes the draft to the main app's in-memory `current_settings`. So theme/panel/threshold changes appear on **Save**, not while dragging. (A future enhancement could persist the draft to disk during preview in wallpaper mode.) A **display-profile** change on Save is applied by the host **self-exiting** when its `refresh_settings` sees the disk `dashboard_profile` differ from the one it started with; the supervisor's respawn-if-exited path then relaunches a fresh host for the new profile (new size/orientation/monitor), so a profile change in wallpaper mode takes effect on Save with a brief ~1 s relaunch flicker.
- **Opacity in wallpaper mode is per-pixel, via DirectComposition, not `WS_EX_LAYERED`** — `WS_EX_LAYERED`/`LWA_ALPHA` (used for window opacity in the normal/behind modes) cannot be applied to a WorkerW *child* window (`SetParent` strips the layered ex-style; setting it on a child is rejected — verified). Instead, the host forces the DX12 backend with `wgpu::Dx12SwapchainKind::DxgiFromVisual` (`NativeOptions.wgpu_options` in `bin/wallpaper.rs`), which makes wgpu create a DirectComposition-backed, per-pixel-alpha swap chain from the window's HWND automatically, and applies `WS_EX_NOREDIRECTIONBITMAP` after window creation (`win_opacity::set_no_redirection_bitmap`) — required for the swap chain to actually composite over the wallpaper instead of being masked by DWM's own opaque redirection bitmap. `clear_color()` and `theme::panel_frame()` premultiply the dashboard's fill/border colors by the opacity setting to match the swap chain's `PreMultiplied` composite alpha mode. Verified empirically (issue #131) that `WS_EX_NOREDIRECTIONBITMAP` can be applied *after* the wgpu surface already exists — despite Microsoft's docs suggesting creation-time-only — so no eframe replacement was needed.
