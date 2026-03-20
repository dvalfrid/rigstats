# Troubleshooting

## GPU Data Always Shows `--`

Make sure LibreHardwareMonitor is running and its web server is enabled on port `8085`.

Test in a browser:

```text
http://localhost:8085/data.json
```

It should return JSON.

## Can I Change Which Display Is Used?

Yes. Adjust the display targeting logic in `pick_target_monitor()` in `src-tauri/src/monitor.rs`.

The dashboard first targets the selected profile resolution, then falls back gracefully.

## Can I Switch Dashboard Size Manually?

Yes. Open Settings and change Display Profile. Save to apply immediately and persist the choice.

## Intel And NVIDIA Support

CPU data comes from `sysinfo` regardless of vendor.

For NVIDIA GPUs, LHM works as well. If labels differ on your machine, adjust the GPU sensor matching in `src-tauri/src/lhm.rs`.

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

### What To Look For In `lhm-data.json`

The file is the raw JSON from `http://localhost:8085/data.json`.
It contains a nested `Children` tree. Each leaf node has `Text` (the sensor name) and `Value` (the current reading).
When a sensor in the dashboard always shows `--`, compare the `Text` values in this file against the expected strings in `src-tauri/src/lhm.rs`.
The mismatch is the fix location.

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
