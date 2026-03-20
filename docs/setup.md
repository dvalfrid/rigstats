# Setup

## Requirements

- Windows 10/11 (x64)
- Node.js LTS: <https://nodejs.org>
- Rust: <https://rustup.rs>
- Visual Studio 2022 Build Tools with Desktop development with C++
- Tauri CLI (installed automatically via `npm install`)

## LibreHardwareMonitor

LHM is bundled and runs via a scheduled task with highest privileges.

This means:

- The dashboard runs as a normal user without an admin prompt on every start
- LHM starts at logon and can read sensors with proper permissions
- The installer uses an existing LHM installation if found, otherwise the bundled version
- LHM is configured automatically during installation with the web server on port `8085`
- The pinned LHM release is downloaded automatically into `vendor/lhm/` when you run `npm run build`

The repo does not check in the LHM binaries. Instead, builds use a pinned release:

1. Version: `v0.9.6`
2. Asset: `LibreHardwareMonitor.zip`
3. Source: <https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases/tag/v0.9.6>

You can fetch it manually at any time with:

```powershell
npm run prepare:lhm
```

That script downloads the pinned ZIP and extracts the full contents into `vendor/lhm/`.

Expected layout:

```text
vendor/
└── lhm/
    ├── LibreHardwareMonitor.exe
    ├── LibreHardwareMonitorLib.dll
    └── ...
```

During installation:

- The installer first looks for an existing `LibreHardwareMonitor.exe`
- If none is found, it uses the bundled version in the app's `lhm` folder
- Default config from `build/lhm-default/LibreHardwareMonitor.config` is applied
- If a config already exists, it is backed up as `LibreHardwareMonitor.config.backup`
- Scheduled task `LibreHardwareMonitor` is created or updated with the selected exe path
- The task is started once immediately after install

If the LHM download fails and `vendor/lhm/` is still missing, `npm run build` will fail instead of producing an installer without bundled sensor support.

## Local Development

1. Extract or clone the repo, for example to:

   ```text
   C:\Users\YourName\rig-dashboard\
   ```

2. Open a terminal in the project folder.
3. Install dependencies:

   ```powershell
   npm install
   ```

4. Start development mode:

   ```powershell
   npm start
   ```

The Tauri backend will compile and the dashboard window will open.

## Display Profiles

Built-in profiles:

1. `portrait-xl` -> `450x1920` (default)
2. `portrait-slim` -> `480x1920`
3. `portrait-hd` -> `720x1280`
4. `portrait-wxga` -> `800x1280`

How it works:

- The app loads your saved profile at startup
- The backend resizes the main window to that profile size
- Monitor targeting prefers an exact resolution match for that profile
- If no exact match exists, the selected size is still applied and the window can be moved manually

You can change the profile in the Settings window.

## Local Builds

Build an installable release with:

```powershell
npm run build
```

This automatically prepares the pinned LibreHardwareMonitor bundle before `tauri build` runs.

On first run this can take 5 to 10 minutes because Rust dependencies are compiled.

Output goes to:

```text
src-tauri\target\release\bundle\
  nsis\
    RIGStats_1.0.0_x64-setup.exe
```

Default install location:

```text
C:\Program Files\RIGStats\
```

## Windows Auto Start

To auto-start the dashboard itself with Task Scheduler:

1. Open Task Scheduler
2. Click Create Basic Task
3. Trigger: At log on
4. Action: Start a program
5. Program: `C:\Program Files\RIGStats\RIGStats.exe`

LHM startup is handled separately by the installer-created scheduled task.
