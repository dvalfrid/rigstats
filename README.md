# RigStats (rig-dashboard)

A gaming stats dashboard optimized for a vertical secondary display (450×1920).
Shows CPU, GPU (AMD RX 9070 XT), RAM, network, and disk in real time.

Computer name, CPU model, and GPU model are detected automatically at startup.
Display sleep is blocked while the app is running.

---

## Stack

| Component | Role |
|---|---|
| **Tauri v1** | App framework (native window, IPC, system tray) |
| **Rust / sysinfo** | CPU, RAM, disk, network data |
| **LibreHardwareMonitor** | GPU temperature, load, fan speed, power (AMD) |
| **HTML / CSS / JS** | Dashboard UI (renderer) |

---

## Dependencies

## CI and Merge Safety

This repository has a Verify workflow at .github/workflows/verify.yml.
It runs on Windows for push and pull_request and executes:

- cargo test
- cargo check
- vitest (frontend unit tests)

To make this required before merge, enable branch protection in GitHub:

1. Open repository Settings -> Branches
2. Add a branch protection rule for main
3. Enable Require a pull request before merging
4. Enable Require status checks to pass before merging
5. Select the status check named Verify (Windows)
6. Save the rule

After this, PRs cannot be merged unless Verify passes.

### LibreHardwareMonitor Integration

LHM is bundled and runs via a scheduled task with highest privileges.
This means:

- The dashboard runs as a normal user (no UAC prompt every start)
- LHM starts at logon and can read sensors with proper permissions
- The installer uses an existing LHM installation if found, otherwise the bundled version
- LHM is configured automatically during installation (web server on port `8085`)

You only need to place the files in the right folder **once** before building:

1. Download the latest release (ZIP):
   <https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases>
2. Create `vendor/lhm/` in the project root
3. Extract the **entire ZIP contents** into `vendor/lhm/`, like this:

   ```
   vendor/
   └── lhm/
       ├── LibreHardwareMonitor.exe
       ├── LibreHardwareMonitorLib.dll
       └── (other files from the ZIP)
   ```

4. During installation:
   - The installer first looks for an already installed `LibreHardwareMonitor.exe`
   - If none is found, it uses the bundled version in `resources/lhm`
   - Default config (`build/lhm-default/LibreHardwareMonitor.config`) is applied to the selected LHM installation (existing or bundled)
   - If a config already exists, a backup is saved as `LibreHardwareMonitor.config.backup`
  - Scheduled task `RigStats\\LibreHardwareMonitor` is created/updated with the selected exe path
   - The task is started once immediately after install

No manual LHM configuration is required.

> **Elevation:** The installer requests admin once. The dashboard app itself then runs without admin prompts.

If `vendor/lhm/` is missing, the app still runs normally, but GPU sensors show `--`.

---

## Part 1: Project Setup

### Step 1: Requirements

- **Windows 10/11** (x64)
- **Node.js LTS** — <https://nodejs.org>
- **Rust** — <https://rustup.rs> (install with default options)
- **MSVC Build Tools** — Visual Studio 2022 Build Tools with the "Desktop development with C++" workload  
  <https://visualstudio.microsoft.com/visual-cpp-build-tools/>
- **Tauri CLI** (installed automatically via `npm install`)

> The first `cargo` build downloads and compiles Rust crates (~5-10 minutes). Subsequent builds are much faster.

### Step 2: Extract the project

Extract the ZIP (or clone the repo) to any folder, for example:

```
C:\Users\YourName\rig-dashboard\
```

### Step 3: Open a terminal in the project folder

Right-click the folder in Explorer and choose "Open in Terminal" (or PowerShell).

### Step 4: Install dependencies

```powershell
npm install
```

### Step 5: Start the app (development)

```powershell
npm start
```

Tauri compiles the Rust backend and opens the dashboard window.
If a 450×1920 display is connected, the window is placed there automatically.
If not, it falls back to the secondary display, or the primary display if only one exists.

### Display Profiles

RigStats now supports 4 built-in dashboard size profiles:

1. `portrait-xl` -> `450x1920` (default)
2. `portrait-slim` -> `480x1920`
3. `portrait-hd` -> `720x1280`
4. `portrait-wxga` -> `800x1280`

How profile selection works:

- On startup, the app loads your saved profile from settings.
- The backend resizes the main window to that profile size.
- Monitor targeting prefers an exact resolution match for the selected profile.
- If no exact match exists, the selected profile is still kept and only the size is applied.
- In that case, the window can be moved manually to any monitor.

You can change profile manually in the **Settings** window using the **Display Profile** dropdown.
The selected profile is persisted and applied on next start.

---

## Part 2: Build an installable `.exe`

```powershell
npm run build
```

Takes about 5-10 minutes on first run (Rust compilation). Output goes to `src-tauri\target\release\bundle\`:

```
src-tauri\target\release\bundle\
  nsis\
    RigStats_1.0.0_x64-setup.exe   <- NSIS installer
  msi\
    RigStats_1.0.0_x64_en-US.msi   <- MSI installer
```

Run the installer and follow the wizard.
Default install location:

```
C:\Program Files\RigStats\
```

---

## Part 3: Windows Auto Start

Enable auto start using Task Scheduler:

1. Search for "Task Scheduler" in Start
2. Click "Create Basic Task..."
3. Trigger: **At log on**
4. Action: **Start a program**
5. Program: `C:\Program Files\RigStats\RigStats.exe`
6. Done — the dashboard starts automatically on next login

> Note: LHM startup is handled by the installer-created scheduled task.

---

## File Structure

```
rig-dashboard/
|- frontend/
|  |- index.html          <- Dashboard UI
|  |- settings.html       <- Settings window
|  |- assets/             <- Runtime image assets used by the renderer
|  \- renderer/           <- JS modules (panels, app logic)
|- src-tauri/
|  |- src/main.rs         <- Rust backend (commands, tray, system stats)
|  |- Cargo.toml          <- Rust dependencies
|  \- tauri.conf.json     <- Tauri app configuration
|- assets/                <- Images and icons
|- vendor/lhm/            <- Bundled LibreHardwareMonitor (place files here)
|- build/
|  |- installer.nsh       <- NSIS installer script
|  \- lhm-default/        <- Default LHM config applied during install
\- package.json           <- npm scripts + Tauri CLI
```

## Code Ownership And Responsibilities

### Backend (`src-tauri/src`)

- `main.rs`
  - App bootstrap only (wiring, tray setup, lifecycle).
  - Keeps startup code thin; implementation lives in focused modules.
- `commands.rs`
  - Tauri command handlers, settings-window lifecycle, and window events.
  - Collects and shapes the full stats payload sent to the renderer.
- `settings.rs`
  - Persistent settings model + load/save from app data directory.
  - Single source of truth for defaults and settings file format.
- `lhm.rs`
  - Fetches and parses LibreHardwareMonitor JSON (`http://localhost:8085/data.json`).
  - Converts nested sensor tree into normalized metrics.
- `stats.rs`
  - Shared payload structs (`StatsPayload`, panel models) and `AppState`.

### Renderer (`frontend/renderer`)

- `app.js`
  - Runtime orchestrator: polling loop, payload validation, panel updates.
  - Contains anti-flicker logic (`isTicking`, `lastValidStats`).
- `environment.js`
  - Thin Tauri backend bridge for command invocation and event subscription.
- `systemInfo.js`
  - One-shot static identity labels (host, CPU model, GPU model).
- `clock.js`
  - Local clock and session uptime rendering.
- `spark.js`
  - History buffers + canvas sparkline drawing.
- `simulator.js`
  - Browser-preview fallback data source.
- `panels/*.js`
  - Panel-specific rendering only (CPU/GPU/RAM/Network/Disk).

## Why Key Decisions Were Made

- Split `main.rs` into modules:
  - Reduced coupling and made debugging safer (window behavior vs stats parsing vs settings I/O).
- Keep latest successful LHM sample in memory:
  - Prevents temporary LHM timeouts from causing `--`/0 flashes in the UI.
- Validate payloads before rendering in `app.js`:
  - Rejects malformed transient samples instead of repainting panels with invalid values.
- Prevent overlapping poll ticks:
  - Avoids out-of-order response races that can make bars jump backwards.
- Use `src` as Tauri web root:
  - Minimizes dev-time reload noise from unrelated file changes outside the renderer root.

---

## FAQ

**GPU data always shows `--`**
Make sure LibreHardwareMonitor is running and the web server is enabled on port `8085`.
Test in a browser: `http://localhost:8085/data.json` should return JSON.

**Can I change which display is used?**
Yes. In `src-tauri/src/commands.rs`, adjust the display-detection logic in `pick_target_monitor()`.
The dashboard targets the selected profile resolution first, then falls back gracefully.

**Can I switch dashboard size manually?**
Yes. Open **Settings** and select a value in **Display Profile**. Save to apply immediately and persist.

**Intel/NVIDIA support?**
The Rust backend via `sysinfo` handles CPU regardless of vendor.
For NVIDIA GPUs, LHM works as well. Adjust the GPU sensor matching logic in `main.rs` to match your sensor naming.

**How do I update the UI without rebuilding?**
Edit files under `frontend/` and run `npm start` to preview changes.
Build a new installer with `npm run build` when ready.

**Display still goes to sleep**
The app calls the Windows `SetThreadExecutionState` API at startup to block display sleep.
If sleep still happens, check display-level power settings in the monitor OSD menu.
