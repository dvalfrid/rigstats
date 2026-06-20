# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build egui binary (debug)
cargo build --manifest-path src-egui/Cargo.toml

# Run egui binary directly
.\target\debug\rigstats.exe

# Restart the app reliably (Windows locks the exe while it runs — kill by PID first)
# Step 1: kill by PID (Stop-Process -Name may silently fail)
Stop-Process -Id (Get-Process rigstats -ErrorAction Stop).Id -Force
# Step 2: build (cargo will fail silently if exe is still locked)
cargo build --manifest-path src-egui/Cargo.toml
# Step 3: VERIFY the exe timestamp changed before launching — if not, the process was still running
Start-Process .\target\debug\rigstats.exe

# Check egui + backend for errors
cargo check --manifest-path src-egui/Cargo.toml

# Build sensor sidecar (debug, requires .NET 10 SDK)
dotnet build sensor-sidecar/sensor-sidecar.csproj

# Run Rust tests (egui binary + backend)
cargo xtask test

# Full verification: sidecar + Rust tests + clippy + fmt check
cargo xtask verify

# Production build (egui release binary + sidecar)
cargo xtask build
```

> **Local dev note:** `cargo xtask verify` and `cargo xtask build` fail if the
> `rigstats-sensor` Windows Service is running, because the service holds the exe
> open. Stop it first (`sc.exe stop rigstats-sensor` in an elevated terminal),
> then run verify, then restart the service.

First-time setup (install git hooks):

```bash
cargo xtask setup
```

Run a single Rust test:

```bash
cargo test --manifest-path rigstats-backend/Cargo.toml classify_system_brand
```

## Linting and formatting

```bash
# Format Rust (modifies files)
cargo xtask fmt

# Check Rust formatting without modifying (CI)
cargo xtask fmt-check

# Clippy
cargo xtask clippy
```

See [STANDARDS.md](STANDARDS.md) for the full code standards.

## After making code changes

**Always run the relevant checks before declaring a task complete.** Do not wait to be asked.

| Changed | Run |
| --- | --- |
| Any Rust file | `cargo xtask fmt` then `cargo xtask clippy` |
| Any `sensor-sidecar/*.cs` file | `dotnet build sensor-sidecar/sensor-sidecar.csproj` |
| Logic in Rust | `cargo xtask test` |
| Unsure | `cargo xtask verify` |

## Issue tracking and commit workflow

Every bug fix and feature must follow this sequence — do not skip steps or reorder them.

### 1. Open a GitHub issue before starting work

```powershell
& "C:\Program Files\GitHub CLI\gh.exe" issue create --title "..." --body "..." --label bug   # or: --label enhancement
```

`gh` is installed but not on PATH in shell sessions — always use the full path above.

For a **bug**: describe the incorrect behaviour, steps to reproduce, and expected behaviour.
For a **feature**: describe the user-visible change and why it is needed.

### 2. Implement the fix or feature

### 3. Test in the running app — required before any commit

Run the app and verify the golden path **and** the edge cases for the change:

```powershell
# Kill → build → verify exe timestamp → launch
Stop-Process -Id (Get-Process rigstats -ErrorAction Stop).Id -Force
cargo build --manifest-path src-egui/Cargo.toml
Start-Process .\target\debug\rigstats.exe
```

**Do not commit until the fix or feature has been confirmed working in the running app.**
Passing tests and a clean clippy are necessary but not sufficient — they verify code correctness, not behaviour.

### 4. Run checks, then commit with `Closes #N`

```bash
cargo xtask fmt
cargo xtask clippy        # zero warnings required
```

**Commit message format — [Conventional Commits](https://www.conventionalcommits.org/).**
This is mandatory: `release-please` (`.github/workflows/release-please.yml`) parses
commit subjects to generate `CHANGELOG.md` and bump the version. A commit that does
not follow this format is silently dropped from the changelog.

```
<type>(<scope>): <subject>

<optional body>

Closes #N
```

- **type** — one of: `feat` (new feature), `fix` (bug fix), `perf` (performance),
  `docs`, `refactor`, `test`, `build`, `chore`, `style`. Only `feat`, `fix`, and
  `perf` surface in the changelog; the rest are still required to be valid types.
- **scope** — the area changed, lower-case: e.g. `status`, `updater`, `cpu`, `gpu`,
  `settings`, `disk`, `debug-log`, `readme`, `roadmap`, `claude`. Optional but
  expected.
- **subject** — imperative, no trailing period, lower-case start.
- A breaking change uses `feat!:` / `fix!:` or a `BREAKING CHANGE:` body footer.

Reference the issue in the commit message so GitHub closes it automatically when pushed to main:

```
fix(updater): reset to Idle on close so Check for Updates reappears

Closes #77
```

### 5. If the commit was made without `Closes #N`, close manually

```powershell
& "C:\Program Files\GitHub CLI\gh.exe" issue close 77 --comment "Fixed in commit abc1234."
```

---

## Documentation and website updates

**Every feature change must also update all three of these — do not wait to be asked:**

| What changed | Where to update |
| --- | --- |
| New panel, data field, or backend module | `docs/architecture.md` — backend modules + renderer modules sections |
| New panel or user-visible feature | `website/index.html` — panel count in `<h2>`, panel card in `.panels-grid`, hero description if relevant |
| Feature complete or scope change | `ROADMAP.md` — mark ✓ and add implementation summary |
| New behaviour or architectural rule | `CLAUDE.md` — Architecture Overview section |

These four files must be consistent with the code at all times. Check all four before declaring a task done.

- `cargo xtask clippy` is configured with `-D warnings` — zero warnings is the bar, not a goal.
- If `cargo xtask fmt` modifies files, include those changes in the same commit.
- If a check fails, fix the issue. Do not skip checks or add `#[allow(...)]` without a clear reason documented in the code.

## Design philosophy

Prefer the simplest solution that solves the problem. Before implementing, ask: is there a direct approach that avoids the complexity entirely? Flag files, shared state, and extra IPC are often signs that a simpler path exists. Question existing plans — a plan being written down is not a reason to follow it if a cleaner alternative is obvious.

## Architecture Overview

This is a **Windows-only** egui desktop app ("RIGStats") that displays hardware telemetry on a secondary portrait monitor. There is no web frontend — all UI is native Rust/egui.

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
  - **Sensor availability convention:** `C_UNAVAILABLE = gray(80)` is the colour for sensors not supported by the current hardware. `avail_color(val: &Option<T>, active: Color32) -> Color32` returns `active` when `Some`, `C_UNAVAILABLE` when `None`. Use this for every stat label and value in every panel so missing sensors dim consistently across the app.
  - **Dialog button API** (Windows 11-style with proper hover/active state):
    - `theme::dialog_btn_primary(ui, label)` — blue `#0078D4`, white text; hover lightens to `#1A86DB`, pressed darkens. Use for the main action (OK, Save, Install Now, Close, Check for Updates).
    - `theme::dialog_btn_secondary(ui, label)` — gray fill `#343434` with border; hover lightens. Use for cancel/dismiss actions (Cancel, Later).
    - `theme::dialog_btn_secondary_disabled(ui, label)` — same gray, non-interactive. Use for grayed-out actions (Update Now when already up-to-date).
  - **Button layout rule:** wrap button rows in `ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ... })`. Primary action is added first (lands on the right), secondary after (lands to its left). Always place `ui.separator()` immediately before the button row.
  - Implementation detail: hover/active colours work by temporarily overriding `ui.visuals_mut().widgets.{inactive,hovered,active}` inside a `ui.scope()` closure — the scope prevents the override from leaking to surrounding UI.
- **`panels/`** — one file per panel (`cpu.rs`, `gpu.rs`, etc.), each rendering inside `theme::panel_frame()`. Every panel `draw()` accepts `th: &theme::AppTheme` so label/muted text colours follow the active preset; panels with temperatures additionally accept `(warn: u8, crit: u8)` from `PanelThresholds` in `main.rs`. All `draw()` functions return `egui::Rect` (the panel's painted rect); `panel_frame()` itself returns `egui::Rect`. The caller (`main.rs`) uses the returned rect to paint the drag-dots and padlock **overlay** on top of the panel in floating mode: three small dots (top-left of the panel's 24 px drag zone) and a padlock icon (right side of drag zone) are drawn with `ui.painter()` — no separate drag-strip widget. Padlock colour is `app_theme.accent`; click stored via `ui.ctx().data_mut()` temp key `"toggle_lock"`.
- **`brand.rs`** — loads 13 brand logo PNGs embedded from `src-egui/assets/` (transparent background, auto-cropped). `rig_logo(brand)` returns the rig header logo for: ROG, MSI, Alienware, Razer, Lenovo Legion, HP Omen, AORUS, Gigabyte, Acer Predator, Taurus. `cpu_logo(model)` and `gpu_logo(name)` return AMD/NVIDIA/Intel based on substring matching.
- **`windows/`** — one file per secondary window (`settings.rs`, `about.rs`, `status.rs`, `updater.rs`). All use `egui::show_viewport_deferred` and are centered via `dialog_center()` in `main.rs`. All four viewports receive the tray icon via `load_app_icon()` (loads `assets/tray.png` as `egui::IconData`). `settings.rs` holds a four-tab layout (Dashboard, Panels, Alerts, Appearance) with live preview: changes are pushed to `current_settings` every frame when `draft != last_preview`; Cancel restores the `original` snapshot captured at open; Save also persists to disk. Appearance includes the theme preset selector. **Live preview applies to:** opacity, theme, rig name, visible panels, floating mode, display profile, all threshold values. **Save-only (not previewed live):** `window_layer`. `status.rs` shows a two-column Components row (`render_components`): a Dependencies card (PawnIO, sidecar service, WMI) on the left and a GPU Drivers card on the right listing each adapter's installed driver version + date, an age-based "stale driver" warning (`DRIVER_STALE_DAYS = 270`), and a per-adapter right-aligned "↗ Latest driver" link to the vendor support page (AMD/NVIDIA/Intel). `render_components` measures the Dependencies card height (`min_rect` delta) and forces the Drivers card to the same height; the driver list sits inside an `egui::ScrollArea` (unique `id_salt("gpu_drivers_scroll")` to avoid an ID clash with the debug-log scroll area) so extra GPUs scroll while the card height stays fixed. Driver data comes from `hardware::detect_gpu_drivers()`. Outdated GPU drivers are the known cause of missing ADL GPU sensors (LHM #736) — see `docs/troubleshooting.md`.
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
- **`settings.rs`** — `Settings` struct + JSON persistence to `%APPDATA%\se.codeby.rigstats\`. Key fields: `theme` (default `"dark-cyan"`), `thresholds: HashMap<String, ComponentThresholds>`, `panel_layouts`, `floating_mode`, `floating_panel_scale`, `fullscreen_mode`, `fullscreen_align` (`"top"`/`"center"`, default `"center"`), `logging_enabled`, `log_retention_days`. `settings_version` migration sentinel (0→1).
- **`debug.rs`** — `append_debug_log` (INFO) plus `log_debug`/`log_warn`/`log_error` level variants, all built on `append_debug_log_lvl` + the `LogLevel` enum. Lines are written as `[YYYY-MM-DD HH:MM:SS] [LEVEL] message` (local time via `chrono`, the `[LEVEL]` tag padded to `[WARNING]` width so message columns align in the Status view). Also `reset_debug_log` (rotates current log to `rigstats-debug-prev.log` before starting fresh — preserves crash evidence), `unix_now_secs`.
- **`monitor.rs`** — Profile definitions, monitor selection, `compute_panels_logical_height`.
- **`autostart.rs`** — Registry-based autostart (HKCU run key).

### Frontend (`frontend/`)

Legacy Tauri JS frontend — **no longer used at runtime**. Kept for the vitest unit test suite which tests pure logic helpers (`tempColors.js`, `vendorBranding.js`, panel formatters). The JS files themselves are not loaded by the egui app.


### Dashboard profiles

Profiles have an orientation prefix and fixed pixel dimensions. **Portrait** profiles (e.g. `portrait-xl` = 450×1920) are tall; **landscape** profiles (e.g. `landscape-xl` = 1920×450) are the transpose. The profile name is stored in settings; `profile_to_size` (in `src-egui/src/main.rs`) returns the window size and `main.rs` positions the egui window accordingly. `profile_is_landscape(profile)` (key-prefix check) drives every orientation branch.

**Monitor selection:** `pick_window_rect_for_profile` filters to monitors matching the profile orientation (landscape → `width >= height`, portrait → `height > width`), then picks the one whose resolution is closest to the profile dimensions, so the dashboard auto-lands on the matching screen (a dedicated 1920×450 strip wins for `landscape-xl`).

**Layout:**
- **Portrait** renders all visible panels in one vertical stack; the window width is fixed to the profile width and the height fits panel content per frame (`draw_one_panel` per panel).
- **Landscape** renders panels in an **adaptive grid** (`render_landscape_grid`): the column count is chosen to maximise the per-cell content scale (ties broken toward fewer rows); every cell is the same size and the panel content scale `sc` is derived from the cell dimensions, so panels shrink/grow to fill any landscape resolution. The window is fixed to the full profile size (no per-frame content-fit). Header and clock are ordinary equal-sized cells. Both orientations share `draw_one_panel`, so panels/themes/thresholds are identical; floating mode is orientation-independent.

**Fullscreen (fill-screen) mode** (`fullscreen_mode`): in non-floating **portrait** mode the window normally fits its content height (per-frame fit in `main.rs`). When `fullscreen_mode` is on, `fixed_window_geometry` instead fills the portrait monitor's height (keeping the profile width so panel proportions never stretch — intended for a screen whose resolution matches the profile), the per-frame content-fit is skipped, and the panel stack is placed per `fullscreen_align` (`"center"` adds top padding to vertically center it; `"top"` leaves it at the top). The dashboard background fills the rest of the window via `clear_color`. Fullscreen has no effect in floating mode or in landscape (landscape always uses the fixed profile geometry).

Valid portrait profiles: `portrait-xl` (450×1920), `portrait-slim` (480×1920), `portrait-hd` (720×1280), `portrait-wxga` (800×1280), `portrait-fhd` (1080×1920), `portrait-wuxga` (1200×1920), `portrait-qhd` (1440×2560), `portrait-hdplus` (768×1366), `portrait-900x1600`, `portrait-1050x1680`, `portrait-1600x2560`, `portrait-4k` (2160×3840), `portrait-fhd-side` (253×1080), `portrait-qhd-side` (338×1440), `portrait-4k-side` (506×2160).

Valid landscape profiles (transpose of the portrait set): `landscape-xl` (1920×450), `landscape-slim` (1920×480), `landscape-hd` (1280×720), `landscape-wxga` (1280×800), `landscape-fhd` (1920×1080), `landscape-wuxga` (1920×1200), `landscape-qhd` (2560×1440), `landscape-hdplus` (1366×768), `landscape-1600x900`, `landscape-1680x1050`, `landscape-2560x1600`, `landscape-4k` (3840×2160), `landscape-fhd-top` (1080×253), `landscape-qhd-top` (1440×338), `landscape-4k-top` (2160×506).

Valid panel keys: `header`, `clock`, `cpu`, `gpu`, `ram`, `net`, `disk`, `motherboard`, `process`, `battery`. `motherboard`, `process`, and `battery` are opt-in (not included in the default visible set).

### Sensor sidecar integration

`rigstats-sensor.exe` runs as a Windows Service (LocalSystem, auto-start at boot). The NSIS installer registers the service with `sc create`, sets restart-on-failure policy via `sc failure`, and starts it immediately. On update: `sc stop` in PREINSTALL, then `sc delete` + `sc create` + `sc start` in POSTINSTALL. On uninstall: `sc stop` + `sc delete`.

The Rust backend connects to `\\.\pipe\rigstats-sensors` with `.write(false)` (pipe is write-only from the server side). On connect failure it falls back to the last successful sample so the UI never resets. GPU selection: preferred GPU (if set) → highest VRAM (stable default) → tie-break by load. Extracted GPU fields: core load, core temp, hotspot temp, core clock (`gpu_freq`), memory clock (`gpu_mem_freq`), power, fan speed, VRAM used/total, D3D 3D engine load (`gpu_d3d_3d`), D3D Video Decode load (`gpu_d3d_vdec`), plus `gpu_devices` for selector dots. D3D fields are `None` when idle. When `gpu_d3d_3d` is present the GPU panel shows a two-column bar layout (VRAM|3D, FAN|VDEC); `gpu_d3d_vdec` renders dimmed at 0 % when `None`. Without `gpu_d3d_3d` the panel falls back to full-width VRAM and FAN bars.

### Settings persistence

Settings are stored in `%APPDATA%\se.codeby.rigstats\rigstats-settings.json`. The debug log is at `rigstats-debug.log` in the same directory; the previous session's log is preserved as `rigstats-debug-prev.log` (see `reset_debug_log`).

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
