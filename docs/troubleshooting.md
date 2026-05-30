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

## Can I Change Which Display Is Used?

Yes. Adjust the display targeting logic in `pick_target_monitor()` in `src-tauri/src/monitor.rs`.

The dashboard first targets the selected profile resolution, then falls back gracefully.

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
UI, the mismatch is in `src-tauri/src/lhm.rs` (the Rust selection logic).

Check `sidecar-log.txt` for service start/stop events and connection errors.

## How Do I Update The UI Without Rebuilding?

Edit files under `frontend/` and run:

```powershell
npm start
```

Build a new installer later with:

```powershell
npm run build
```

## Display Still Goes To Sleep

Display sleep blocking is not currently implemented in the app.

Use Windows power settings or the monitor OSD if you need the display to stay awake.
