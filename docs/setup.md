# Setup

## Requirements

- Windows 10/11 (x64)
- Node.js LTS: <https://nodejs.org>
- Rust: <https://rustup.rs>
- .NET 10 SDK: `winget install Microsoft.DotNet.SDK.10`
- Visual Studio 2022 Build Tools with Desktop development with C++
- NSIS (for building the installer): `choco install nsis -y`

## Sensor Sidecar

Hardware sensor data (GPU temp, CPU temp, fan speeds, voltages, disk temps) is
collected by `rigstats-sensor.exe` — a self-contained .NET 10 executable that
embeds `LibreHardwareMonitorLib` and streams readings over a Windows named pipe.

The sidecar is installed and managed as a **Windows Service** (`rigstats-sensor`,
LocalSystem, auto-start at boot). The NSIS installer registers it with `sc create`
and starts it immediately. It restarts automatically on crash (5 s / 10 s / 30 s).

The repo does not check in the sidecar binary. It is built from source as part of
the build pipeline:

```powershell
npm run prepare:sidecar   # dotnet publish → sensor-sidecar/bin/Release/.../publish/
npm run build             # calls prepare:sidecar, then cargo build --release + makensis
```

The published exe is bundled into the NSIS installer automatically.

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

The egui binary will compile and the dashboard window will open.

## Display Profiles

Built-in profiles:

| Profile | Resolution | Notes |
| --- | --- | --- |
| `portrait-xl` | 450×1920 | Default |
| `portrait-slim` | 480×1920 | |
| `portrait-hd` | 720×1280 | |
| `portrait-wxga` | 800×1280 | |
| `portrait-fhd` | 1080×1920 | |
| `portrait-wuxga` | 1200×1920 | |
| `portrait-qhd` | 1440×2560 | |
| `portrait-4k` | 2160×3840 | |
| `portrait-hdplus` | 768×1366 | |
| `portrait-900x1600` | 900×1600 | |
| `portrait-1050x1680` | 1050×1680 | |
| `portrait-1600x2560` | 1600×2560 | |
| `portrait-fhd-side` | 253×1080 | Landscape monitor sidebar |
| `portrait-qhd-side` | 338×1440 | Landscape monitor sidebar |
| `portrait-4k-side` | 506×2160 | Landscape monitor sidebar |

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

This publishes the sensor sidecar, compiles the Rust binary in release mode, and runs `makensis` to produce the installer.

On first run this can take 5 to 10 minutes because Rust dependencies are compiled.

Output goes to:

```text
target\release\
  RIGStats_1.0.0_x64-setup.exe
```

Default install location:

```text
C:\Program Files\RIGStats\
```

## Windows Auto Start

Launch at startup is configured directly in the app — no manual steps required.

Open the Settings window (right-click the tray icon → Settings) and enable the **Launch at Startup** toggle. The app registers itself under `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run` and keeps the `StartupApproved\Run` entry in sync so the toggle reflects the actual state shown in Windows Settings → Apps → Startup.

The sensor sidecar (`rigstats-sensor` Windows Service) is installed and started
by the NSIS installer during setup.
