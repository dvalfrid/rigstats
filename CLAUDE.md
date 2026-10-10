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
| `/add-control-provider` | Adding a hardware-control domain (IControlProvider) to the Control Center service |
| `/add-lighting-device` | Supporting a new lighting device or model, incl. regenerating the supported-devices list |
| `/live-test` | Building and handing over a sidecar change for a live test on the owner's hardware (dev sidecar + debug app), recording the result |
| `/diagnostics-triage` | Finding why a device/sensor/feature doesn't work from the service log, `lighting-devices.json` or a user's diagnostics ZIP (read-only) |

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

> The installed `rigstats-sensor` service runs from `C:Program FilesRIGStats` and does not block builds. The dev sidecar (`toolsdev-sidecar.ps1`) runs `sensor-sidecarinDebug…igstats-sensor.exe` and locks it — building the sidecar fails until the owner stops it with Ctrl+C (a project hook blocks those commands meanwhile).

**Live-testing Control Center changes on real hardware:** the developer runs `pwsh -File tools\dev-sidecar.ps1 -Live` in an elevated window (debug sidecar in place of the service; Ctrl+C restores everything). Rebuild the sidecar only after Ctrl+C. Full loop and protocol-finding methods: "Live testing on real hardware" in `docs/control-architecture.md`.

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

Settings are read from `%APPDATA%\se.codeby.rigstats\`. The sidecar pipe accepts several clients that share one cached sample per second. In wallpaper mode the host is the dashboard's poller; the main app's `poll_loop` (`PollMode` via `poll_mode`) runs `Light` — it still feeds the game overlay, the tray hover card and the alerts (no per-app lists, no session recording; never paused, #299).

### Where the details live

Module catalogues sit next to the code and load when you work there:

| Area | File |
|---|---|
| egui app (`src-egui/src/`): modules, dashboard profiles, session recording | `src-egui/CLAUDE.md` |
| Dialogs: design system (`ui_kit.rs`), where a setting lives, lifecycle contract — read before any UI work | `src-egui/src/windows/CLAUDE.md` |
| Backend (`rigstats-backend/src/`) | `rigstats-backend/CLAUDE.md` |
| Control Center service side + **hardware-write safety rules** | `sensor-sidecar/Control/CLAUDE.md` |
| Full architecture, design decisions | `docs/architecture.md`, `docs/control-architecture.md` |

Profiles are named `portrait-<size>` / `landscape-<size>` (landscape = transpose); sizing lives in `geometry.rs`.

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

### Sensor sidecar integration

`rigstats-sensor.exe` runs as a Windows Service (LocalSystem, auto-start). NSIS installer uses `sc create`/`sc stop`/`sc delete` for install, update, and uninstall.

The Rust backend connects to `\\.\pipe\rigstats-sensors` (`.write(false)`). On failure it falls back to the last sample. GPU selection: preferred → highest VRAM → load tie-break. D3D fields (`gpu_d3d_3d`, `gpu_d3d_vdec`) are `None` when idle; their presence toggles the GPU panel between a two-column bar layout and single-bar default.

For adding hardware sensor fixtures, run `/sensor-fixture`.

### Settings persistence

Settings: `%APPDATA%\se.codeby.rigstats\rigstats-settings.json`. Debug log: `rigstats-debug.log`; previous session: `rigstats-debug-prev.log`.

### Testing

Rust tests are `#[cfg(test)]` modules at the bottom of their files (e.g. `src-egui/src/geometry.rs`). Run with `cargo xtask test`.

.NET sidecar tests: `sensor-sidecar.Tests/` (xUnit, NSubstitute); runs as part of `cargo xtask verify`.
