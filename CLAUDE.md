# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build egui binary (debug)
cargo build --manifest-path src-egui/Cargo.toml

# Run egui binary directly
.\target\debug\rigstats-egui.exe

# Check egui + backend for errors
cargo check --manifest-path src-egui/Cargo.toml

# Clippy on backend + egui
cargo clippy --manifest-path rigstats-backend/Cargo.toml -- -D warnings
cargo clippy --manifest-path src-egui/Cargo.toml -- -D warnings

# Build sensor sidecar (debug, requires .NET 10 SDK)
dotnet build sensor-sidecar/sensor-sidecar.csproj

# Publish sensor sidecar as single-file self-contained exe (release)
dotnet publish sensor-sidecar/sensor-sidecar.csproj -c Release

# Run frontend unit tests only
npm test

# Run Rust tests only (requires Windows for most tests)
cargo test --manifest-path rigstats-backend/Cargo.toml

# Full verification: publish sidecar + Rust tests + clippy + fmt check + frontend tests
npm run verify

# Production build (egui release binary + sidecar)
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
cargo test --manifest-path rigstats-backend/Cargo.toml classify_system_brand
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

The repo is a Cargo workspace (`Cargo.toml` at root) with two members:

| Crate | Path | Role |
|---|---|---|
| `rigstats-backend` | `rigstats-backend/` | Shared lib — all backend modules with Tauri coupling removed (`AppHandle` → `&Path` for settings/debug/lhm functions) |
| `rigstats-egui` | `src-egui/` | Production egui binary — replaces Tauri (migration complete) |

The egui binary reads settings from `%APPDATA%\se.codeby.rigstats\`. The sidecar pipe accepts one client at a time.

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
- **`panels/`** — one file per panel (`cpu.rs`, `gpu.rs`, etc.), each rendering inside `theme::panel_frame()`. Every panel `draw()` accepts `th: &theme::AppTheme` so label/muted text colours follow the active preset; panels with temperatures additionally accept `(warn: u8, crit: u8)` from `PanelThresholds` in `main.rs`. All `draw()` functions return `egui::Rect` (the panel's painted rect); `panel_frame()` itself returns `egui::Rect`. The caller (`main.rs`) uses the returned rect to paint the drag-dots and padlock **overlay** on top of the panel in floating mode: three small dots (top-left of the panel's 24 px drag zone) and a padlock icon (right side of drag zone) are drawn with `ui.painter()` — no separate drag-strip widget. Padlock colour is `app_theme.accent`; click stored via `ui.ctx().data_mut()` temp key `"toggle_lock"`.
- **`brand.rs`** — loads 13 brand logo PNGs embedded from `src-egui/assets/` (transparent background, auto-cropped). `rig_logo(brand)` returns the rig header logo for: ROG, MSI, Alienware, Razer, Lenovo Legion, HP Omen, AORUS, Gigabyte, Acer Predator, Taurus. `cpu_logo(model)` and `gpu_logo(name)` return AMD/NVIDIA/Intel based on substring matching.
- **`windows/`** — one file per secondary window (`settings.rs`, `about.rs`, `status.rs`, `updater.rs`). All use `egui::show_viewport_deferred` and are centered via `dialog_center()` in `main.rs`. All four viewports receive the tray icon via `load_app_icon()` (loads `assets/tray.png` as `egui::IconData`). `settings.rs` holds a four-tab layout (Dashboard, Panels, Alerts, Appearance) with live preview: changes are pushed to `current_settings` every frame when `draft != last_preview`; Cancel restores the `original` snapshot captured at open; Save also persists to disk. Appearance includes the theme preset selector. **Live preview applies to:** opacity, theme, rig name, visible panels, floating mode, display profile, all threshold values. **Save-only (not previewed live):** `window_layer`.
- **`update_check.rs`** — `check()` fetches `latest.json`, compares semver. `BUNDLED_CHANGELOG` bundles `../../CHANGELOG.md` at compile time (same content as Tauri's bundled resource).
- **`win32_dark_mode.rs`** — calls `uxtheme.dll` ordinals 135 (`SetPreferredAppMode(AllowDark)`) + 104 (`RefreshImmersiveColorPolicyState`) at startup so the OS-drawn tray context menu respects dark mode.

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

- **`stats.rs`** — `StatsPayload` and all sub-structs (`CpuStats`, `GpuStats`, etc.). `HardwareInfo` holds startup-detected constants (disk model map, RAM spec, GPU VRAM, system brand, etc.) behind a `Mutex`; `AppState` holds per-tick mutable state (lhm_pipe, sysinfo handles, last samples, settings, `last_log_prune_day`).
- **`hardware.rs`** — WMI structs + all startup hardware detection: `detect_gpu_name`, `detect_gpu_vram_total_mb`, `detect_system_brand`, `classify_system_brand`, `detect_model_name`, `detect_motherboard_name`, `normalize_manufacturer`, `detect_ram_spec`, `detect_ram_details`, `detect_ping_target`, `sample_ping_ms`, `probe_wmi_status`, `detect_disk_model_map`. Each function tries WMI first, falls back to PowerShell CIM.
- **`lhm.rs`** — Named pipe client → `LhmData`. `fetch_lhm_pipe` reuses an established connection; on failure logs at most once per 30 s. `.write(false)` on `ClientOptions` (pipe is `PipeDirection.Out`). `select_gpu_idx`: preferred GPU → highest VRAM → tie-break by load.
- **`lhm_process.rs`** — `track_lhm_connection_state` (connect/disconnect logging, 30 s throttle).
- **`logging.rs`** — CSV logging: `append_stats_row`, `prune_old_logs`, `current_log_path`, `ymd_from_unix`.
- **`settings.rs`** — `Settings` struct + JSON persistence to `%APPDATA%\se.codeby.rigstats\`. Key fields: `theme` (default `"dark-cyan"`), `thresholds: HashMap<String, ComponentThresholds>`, `panel_layouts`, `floating_mode`, `floating_panel_scale`, `logging_enabled`, `log_retention_days`. `settings_version` migration sentinel (0→1).
- **`debug.rs`** — `append_debug_log`, `reset_debug_log`, `unix_now_secs`.
- **`monitor.rs`** — Profile definitions, monitor selection, `compute_panels_logical_height`.
- **`autostart.rs`** — Registry-based autostart (HKCU run key).

### Frontend (`frontend/`)

Legacy Tauri JS frontend — **no longer used at runtime**. Kept for the vitest unit test suite which tests pure logic helpers (`tempColors.js`, `vendorBranding.js`, panel formatters). The JS files themselves are not loaded by the egui app.


### Dashboard profiles

Profiles are portrait orientations with fixed pixel dimensions (e.g., `portrait-xl` = 450×1920). The profile name is stored in settings; `monitor.rs` returns the window size and `main.rs` positions the egui window accordingly. `compute_panels_logical_height` sums visible panel heights to pre-size the window before first paint.

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
