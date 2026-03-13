# Troubleshooting

## GPU Data Always Shows `--`

Make sure LibreHardwareMonitor is running and its web server is enabled on port `8085`.

Test in a browser:

```text
http://localhost:8085/data.json
```

It should return JSON.

## Can I Change Which Display Is Used?

Yes. Adjust the display targeting logic in `pick_target_monitor()` in `src-tauri/src/commands.rs`.

The dashboard first targets the selected profile resolution, then falls back gracefully.

## Can I Switch Dashboard Size Manually?

Yes. Open Settings and change Display Profile. Save to apply immediately and persist the choice.

## Intel And NVIDIA Support

CPU data comes from `sysinfo` regardless of vendor.

For NVIDIA GPUs, LHM works as well. If labels differ on your machine, adjust the GPU sensor matching in `src-tauri/src/lhm.rs`.

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
