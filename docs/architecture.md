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

## Design Decisions

- `main.rs` stays thin and delegates implementation to focused modules
- Latest successful LHM sample is kept in memory to avoid UI flicker when LHM times out
- Payloads are validated before rendering to avoid repainting with malformed transient data
- Poll ticks do not overlap, which avoids out-of-order UI updates
- `frontend` is the Tauri web root, which keeps runtime assets and HTML together
