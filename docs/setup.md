# Setup

## Requirements

- Windows 10/11 (x64)
- Rust: <https://rustup.rs> — the toolchain version is pinned in `rust-toolchain.toml` at the repo root, so `rustup` auto-installs the correct version (matching CI) the first time you run `cargo` here; no manual `rustup default`/`rustup override` needed
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
cargo xtask build   # dotnet publish sidecar + cargo build --release
```

The published exe is bundled into the NSIS installer automatically.

## Local Development

1. Clone the repo:

   ```powershell
   git clone https://github.com/dvalfrid/rigstats
   cd rigstats
   ```

2. Install git hooks (run once after cloning):

   ```powershell
   cargo xtask setup
   ```

3. Build and run the debug binary:

   ```powershell
   cargo build --manifest-path src-egui/Cargo.toml
   .\target\debug\rigstats.exe
   ```

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

The active profile is changed in Settings → Display tab. The dashboard
auto-targets the connected monitor that best matches the profile (resolution +
orientation), falling back to the primary monitor when none match. Landscape
profiles (the transpose of the portrait set, e.g. `landscape-xl` = 1920×450) are
also available.

The **Window Layer** control on the same tab places the dashboard as `Normal`,
`Always on Top`, `Always Behind`, or **Desktop Wallpaper** (reparented into the
desktop WorkerW layer — see [architecture.md](architecture.md) “Desktop wallpaper
mode”). In wallpaper mode the dashboard is rendered by the separate
`rigstats-wallpaper.exe` host, so settings apply on Save and the Display Profile is
locked while that layer is active.

## Local Builds

Build an installable release with:

```powershell
cargo xtask build
```

This publishes the sensor sidecar, compiles the Rust binary in release mode.
Then build the NSIS installer:

```powershell
$version = "1.27.1"
makensis /DVERSION=$version build\installer.nsi
```

On first run this can take 5–10 minutes because Rust dependencies are compiled.

Output:

```text
target\release\RIGStats_1.x.x_x64-setup.exe
```

Default install location:

```text
C:\Program Files\RIGStats\
```

## Windows Auto Start

Launch at startup is configured directly in the app — no manual steps required.

Open the Settings window (right-click the tray icon → Settings) and enable the
**Launch at Startup** toggle. The app registers itself under
`HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run`.

The sensor sidecar (`rigstats-sensor` Windows Service) is installed and started
by the NSIS installer during setup.
