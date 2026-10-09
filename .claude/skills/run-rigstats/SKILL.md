---
name: run-rigstats
description: Build and launch the RigStats egui binary on Windows. Use when asked to run, start, restart, or build the app.
---

RigStats is a Windows-native egui binary (`target\debug\rigstats.exe`). There is no web frontend — UI is pure Rust/egui. Use PowerShell for all steps.

## Kill → Build → Launch

Always follow this sequence. Never skip the timestamp check — cargo silently no-ops if the exe is locked.

```powershell
# 1. Kill by PID — Stop-Process -Name silently fails on locked exes
$proc = Get-Process rigstats -ErrorAction SilentlyContinue
if ($proc) { Stop-Process -Id $proc.Id -Force; Write-Host "killed PID $($proc.Id)" }

# 2. Build
cargo build --manifest-path src-egui/Cargo.toml

# 3. Verify timestamp changed — if it didn't, the exe was still locked
(Get-Item .\target\debug\rigstats.exe).LastWriteTime

# 4. Launch
Start-Process .\target\debug\rigstats.exe

# 5. Confirm it started
Start-Sleep 2
Get-Process rigstats -ErrorAction SilentlyContinue | Select-Object Id, StartTime
```

## Monitor layout (known-good, verify with AllScreens if it changes)

| Display | X | Y | W | H | Notes |
|---|---|---|---|---|---|
| DISPLAY1 (primary) | 0 | 0 | 2560 | 1440 | Main desktop — Settings/About windows appear here |
| DISPLAY2 | -2560 | 0 | 2560 | 1440 | Secondary left monitor |
| DISPLAY6 (portrait) | 2560 | 0 | 450 | 1920 | The RigStats dashboard lives here |

```powershell
# Re-enumerate if layout seems wrong:
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Screen]::AllScreens | ForEach-Object {
    Write-Host "$($_.DeviceName) Bounds=$($_.Bounds) Primary=$($_.Primary)"
}
```

## Screenshot helpers

```powershell
# Portrait dashboard (full)
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$bmp = New-Object System.Drawing.Bitmap(450, 1920)
$g   = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen([System.Drawing.Point]::new(2560, 0), [System.Drawing.Point]::Empty, [System.Drawing.Size]::new(450, 1920))
$bmp.Save("$env:TEMP\rigstats_portrait.png"); $g.Dispose(); $bmp.Dispose()

# Portrait header only (top 300 px — hostname + model_name + clock)
$bmp = New-Object System.Drawing.Bitmap(450, 300)
$g   = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen([System.Drawing.Point]::new(2560, 0), [System.Drawing.Point]::Empty, [System.Drawing.Size]::new(450, 300))
$bmp.Save("$env:TEMP\rigstats_header.png"); $g.Dispose(); $bmp.Dispose()
```

## Gotchas

- **`Stop-Process -Name rigstats` silently fails** if the exe is in use. Always use `-Id`.
- **Cargo no-ops silently** if the exe is still locked by a running process. Check the timestamp.
- **The app window is on DISPLAY6 (X=2560)**, not the primary monitor. Primary screenshots will not show the dashboard.
- **Settings/About/Updater viewports** open as secondary egui windows on the primary monitor. The process `MainWindowTitle` becomes `"RigStats — Settings"` (etc.) while they are open.
