# Troubleshooting

## Windows Defender Alert On Service Start

Since v1.21.0, `LibreHardwareMonitorLib` uses **PawnIO** — a properly signed
kernel driver — instead of WinRing0. The installer stages PawnIO into the
Windows Driver Store via `pnputil` before starting the service, so Defender
alerts should no longer occur on fresh installs.

If you are running from source (dev environment) and see a Defender alert, it
means PawnIO is not yet installed. Install it once from an elevated prompt:

```powershell
pnputil /add-driver build\pawnio\pawnio.inf /install
```

## GPU Data Always Shows `--`

Make sure the `rigstats-sensor` Windows Service is running:

```powershell
sc.exe query rigstats-sensor
```

`STATE: 4 RUNNING` means the sidecar is active. If it is stopped or missing,
the installer may not have completed successfully — check `install.log` in the
diagnostics ZIP (Status → Collect Diagnostics).

## GPU Sensors Missing (Temp / Clock / Power / Fan) Or Sidecar Crashes

**Symptom:** The GPU panel shows core load and D3D 3D usage, but `TEMP`, `HOT`,
`FREQ`, `POWER`, `FAN` and `VRAM` stay at `--` / `0`. The Windows Event Log may
also show `rigstats-sensor.exe` crashing with:

```
System.AccessViolationException: Attempted to read or write protected memory.
   at LibreHardwareMonitor.Hardware.Gpu.AmdGpu.Update()
```

(or a crash in `atiadlxx.dll`).

**Cause:** This is an **outdated GPU driver**, not a RIGStats bug. The core GPU
sensors (temp, clock, power, fan) come from AMD's ADL library (`atiadlxx.dll`)
via LibreHardwareMonitor. Some AMD driver builds return invalid data — or
corrupt memory — on these ADL calls, which both hides the sensors and can crash
the sidecar. The `AccessViolationException` is a native-side fault that **cannot
be caught** from managed .NET, so the whole service dies until it auto-restarts.
The D3D values (3D load, VRAM-used) keep working because they come from Windows
D3D counters, not ADL.

This is a known LibreHardwareMonitor issue (see
[LibreHardwareMonitor#736](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/issues/736)).
The same RIGStats build on identical hardware works perfectly with a newer
driver — the GPU read path (`sensor-sidecar/` + `LibreHardwareMonitorLib`) has
not changed across recent app versions.

**Fix:** Update the GPU **and** chipset drivers to the latest versions:

- AMD: install the latest [AMD Software: Adrenalin Edition](https://www.amd.com/en/support).
- Note that Windows Update can silently install an older driver build; always
  prefer the vendor's latest package.

After updating, restart the `rigstats-sensor` service (or reboot) and the GPU
sensors should populate.

## Desktop Wallpaper Mode

Set **Settings → Display → Window Layer → Desktop Wallpaper** to render the
dashboard as a live wallpaper, between the desktop background and the icons. It
survives `Win+D`, is never covered by other windows, and coexists with Wallpaper
Engine / Lively.

A few behaviours are specific to this mode and are **expected**, not bugs:

- **Settings apply on Save, not as a live preview.** The wallpaper is drawn by a
  separate background process (`rigstats-wallpaper.exe`) that reads settings from
  disk about once a second, so theme/panel/threshold changes only appear after you
  press **Save**. Settings shows an amber banner reminding you of this.
- **Controls with no live effect are greyed out** while the applied layer is
  Desktop Wallpaper, and the **Display Profile selector is locked**. To change the
  profile, switch the Window Layer back to a non-wallpaper option (Normal / Always
  on Top / Always Behind), pick the profile, then return to Desktop Wallpaper and
  Save.
- **Window opacity has no effect** in wallpaper mode. `WS_EX_LAYERED` cannot be
  applied to a WorkerW child window, so the dashboard is opaque over the wallpaper.
- **The dashboard position** is the screen position the window had when you
  switched into wallpaper mode (saved as `wallpaperPosition`). Place the window
  where you want it *before* switching to Desktop Wallpaper.

If the dashboard disappears after Explorer restarts, the host process exits and is
respawned automatically by the main app within a second.

## Can I Change Which Display Is Used?

Yes — open Settings and pick a **Display Profile** that matches the target screen.
The dashboard auto-targets the connected monitor whose resolution and orientation
best match the profile (a dedicated 1920×450 strip wins for `landscape-xl`, a
portrait side-strip wins for the `*-side` profiles, etc.), and otherwise falls
back to the primary monitor — it no longer lands on an arbitrary small screen.

When you switch profiles the window keeps its current screen position if that spot
is still on a connected monitor; only when the saved position is off-screen does it
re-target the matching monitor. **Fill Screen** fills the height of the monitor the
window currently sits on, so enabling it never moves the dashboard to another
screen.

The low-level targeting logic lives in `pick_window_rect_for_profile` /
`select_profile_monitor` in `src-egui/src/geometry.rs`.

## Can I Switch Dashboard Size Manually?

Yes. Open Settings and change Display Profile. Save to apply immediately and persist the choice.

## Intel And NVIDIA Support

CPU data comes from `sysinfo` regardless of vendor.

For NVIDIA GPUs, the sidecar works as well. If labels differ on your machine, adjust the GPU sensor matching in `sensor-sidecar/SensorReader.cs`.

## How Do I Inspect Real WMI Strings?

Use PowerShell and capture these values from the actual machine:

```powershell
Get-CimInstance Win32_ComputerSystem |
  Select-Object Manufacturer, Model |
  Format-List

Get-CimInstance Win32_ComputerSystemProduct |
  Select-Object Name, Version |
  Format-List

Get-CimInstance Win32_BaseBoard |
  Select-Object Manufacturer, Product |
  Format-List
```

If you want one copy-paste friendly block for support/debugging, run:

```powershell
$cs = Get-CimInstance Win32_ComputerSystem
$csp = Get-CimInstance Win32_ComputerSystemProduct
$bb = Get-CimInstance Win32_BaseBoard

[pscustomobject]@{
  ComputerSystemManufacturer = $cs.Manufacturer
  ComputerSystemModel = $cs.Model
  ProductName = $csp.Name
  ProductVersion = $csp.Version
  BaseBoardManufacturer = $bb.Manufacturer
  BaseBoardProduct = $bb.Product
} | Format-List
```

Those six fields are the ones RIGStats now uses to classify the system brand, with product-line names like `Alienware`, `Legion`, `OMEN`, `Predator`, and `AORUS` taking priority over the generic OEM name.

## How Do I Report A Bug Or Missing Sensor Support?

Use the **Status dialog → Collect Diagnostics…** button.

It opens a native Windows save dialog. Pick a location and a ZIP file is written immediately.
No data is sent automatically — the file is written only to the path you choose.
Share it by email or attach it to a GitHub issue.

See [Diagnostics Export](../README.md#diagnostics-export) in the README for a full description of what the ZIP contains.

### What To Look For In `displays.json`

The file lists every monitor Windows reports to the app — the same data used by `pick_target_monitor()`.

Each entry shows:

- `widthPx` / `heightPx` — physical pixel resolution
- `positionX` / `positionY` — position in the virtual desktop coordinate space
- `scaleFactor` — Windows DPI scaling (e.g. `1.5` = 150 %)
- `isPortrait` — whether height ≥ width
- `fitScore` — how well this monitor matches the active profile (lower = better)
- `selected` — `true` on the monitor that was actually chosen

If the dashboard appears on the wrong monitor, compare the `fitScore` values and check whether the correct monitor has `isPortrait: true` and a lower score than the others.

### What To Look For In `sensor-tree.txt`

The file contains the full LHM hardware and sensor tree as seen by the sidecar at the last service start.
Each hardware node shows its type, identifier, and name; each sensor shows its type, identifier, name, and current value.

Use this file to identify the exact identifier path for a sensor that isn't being picked up.
For example, if a DIMM temperature reads correctly in standalone LHM but not in RIGStats, find the sensor here and compare its identifier against the filter in `sensor-sidecar/SensorReader.cs`.

The file is overwritten on every service start, so it always reflects the current hardware configuration.

### What To Look For In `sidecar-parsed.json`

The file contains the last sensor payload that the Rust backend successfully
received from the sidecar pipe. It shows the extracted values — GPU temps,
CPU temp, fan speeds, disk temps, etc. — exactly as the app uses them.

When a sensor always shows `--`, check whether the relevant field is `null`
here. If it is, the mismatch is in `sensor-sidecar/SensorReader.cs` (the C#
extraction logic). If the field is present but the wrong value appears in the
UI, the mismatch is in `rigstats-backend/src/lhm.rs` (the Rust selection logic).

Check `sidecar-log.txt` for service start/stop events and connection errors.

## How Do I Change The UI?

The dashboard is a native egui app — all UI is Rust code in `src-egui/src/`. Edit the relevant file and rebuild:

```powershell
cargo build --manifest-path src-egui/Cargo.toml
.\target\debug\rigstats.exe
```

Build a new installer after:

```powershell
cargo xtask build
```

## Ghost Entries in "Other System Tray Icons"

Windows stores one tray icon entry per unique exe path. Old entries from dev builds, renamed binaries, or previous Tauri installs linger even after those exes are gone.

Run the cleanup script (no elevation needed — run as the same user that runs RIGStats):

```powershell
pwsh -ExecutionPolicy Bypass -File tools\clean-tray-ghosts.ps1
```

The script ([`tools/clean-tray-ghosts.ps1`](../tools/clean-tray-ghosts.ps1)) finds all RIGStats-related tray entries, shows what it will keep vs delete, asks for confirmation, then restarts Explorer.

After it runs, check **Settings → Personalization → Taskbar → Other system tray icons** — only one RIGStats entry should remain.

## Display Still Goes To Sleep

Display sleep blocking is not currently implemented in the app.

Use Windows power settings or the monitor OSD if you need the display to stay awake.
