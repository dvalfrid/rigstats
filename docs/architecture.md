# Architecture

## File Structure

```text
rig-dashboard/
|- frontend/
|  |- index.html
|  |- settings.html
|  |- assets/
|  \- renderer/
|- src-tauri/
|  |- src/
|  |- Cargo.toml
|  \- tauri.conf.json
|- assets/
|- vendor/lhm/
|- build/
|  |- installer.nsh
|  \- lhm-default/
|- docs/
\- package.json
```

## Backend Responsibilities

### `src-tauri/src`

- `main.rs`
  - app bootstrap, tray wiring, lifecycle setup
- `commands.rs`
  - Tauri commands, window lifecycle, stats assembly
- `settings.rs`
  - persistent settings model and file I/O
- `lhm.rs`
  - LibreHardwareMonitor JSON fetch + parsing
- `stats.rs`
  - shared payload structs and `AppState`

## Renderer Responsibilities

### `frontend/renderer`

- `app.js`
  - polling loop, payload validation, panel updates
- `environment.js`
  - Tauri bridge for invoke and events
- `systemInfo.js`
  - host, CPU model, GPU model, branding/logo wiring
- `clock.js`
  - local time and uptime rendering
- `spark.js`
  - history buffers and canvas sparklines
- `simulator.js`
  - browser preview fallback data
- `panels/*.js`
  - per-panel rendering logic

## Diagnostics Export (`collect_diagnostics`)

The `collect_diagnostics` Tauri command is invoked from the Status dialog's **Collect Diagnostics…** button.
It produces a self-contained ZIP that captures everything relevant for bug reports and sensor-support work.

### Collection flow

1. A native Windows save-file dialog is opened on a dedicated OS thread via `rfd::FileDialog` (Win32 requires STA; spawning a blocking task avoids blocking the async runtime).
2. If the user cancels, the command returns `Ok(None)` and no file is written.
3. If the user confirms a path, the following data is assembled — some synchronously, the LHM fetch asynchronously:

| Source | Function | Notes |
| --- | --- | --- |
| `manifest.json` | inline | Unix timestamp + `CARGO_PKG_VERSION` |
| `debug.log` | `std::fs::read(debug_log_path)` | Full file, not the 160-line tail shown in UI |
| `settings.json` | serde_json of current `Settings` from `AppState` | No mutation — read-only snapshot |
| `lhm-data.json` | `GET localhost:8085/data.json` via the reused `lhm_client` | 3 s timeout; error payload on failure |
| `hardware.json` | `diag_collect_hardware()` — PowerShell `Get-CimInstance` | OS, CPU, GPU, board, RAM |
| `sched-task.txt` | `diag_collect_tasks()` — `schtasks /Query /V` | Both LHM task names |
| `environment.txt` | `diag_collect_environment()` — env vars + Windows registry | Arch, build, hostname |
| `sysinfo.json` | `diag_collect_sysinfo()` — reads shared `AppState` mutexes | CPU brand, RAM totals, mount points, interfaces |

4. All entries are written into a single `zip::ZipWriter` with Deflate compression.
5. Path is logged to the debug log and returned to the renderer as `Ok(Some(path))`.

### Dependencies added

| Crate | Use |
| --- | --- |
| `zip = "2"` | Deflate ZIP writer |
| `rfd = "0.14"` | Native file-open/save dialogs |

### Privacy

Nothing is transmitted. The ZIP is written only to the user-selected path.
The LHM sensor data and hardware identifiers are only useful for diagnosing sensor compatibility; no credentials or secrets are present in any collected field.

---

## Design Decisions

- `main.rs` stays thin and delegates implementation to focused modules
- Latest successful LHM sample is kept in memory to avoid UI flicker when LHM times out
- Payloads are validated before rendering to avoid repainting with malformed transient data
- Poll ticks do not overlap, which avoids out-of-order UI updates
- `frontend` is the Tauri web root, which keeps runtime assets and HTML together
