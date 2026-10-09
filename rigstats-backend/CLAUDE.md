# rigstats-backend

Shared library used by both binaries (`rigstats`, `rigstats-wallpaper`). Domain logic belongs here — `src-egui/` is UI and wiring. Full module detail: `docs/architecture.md` ("Backend Modules").

- **`stats.rs`** — `StatsPayload` + sub-structs: the session-recording row shape (built from `PollStats` in `poll.rs`); `DiskKind`
- **`hardware.rs`** — WMI hardware detection (PowerShell only when WMI fails): GPU names, RAM, disk, system brand, model, motherboard, ping target, battery. Typed `query::<T>()` structs need a `#[serde(rename = "Win32_…")]` container rename (#198, guarded by `wmi_classes_tests`)
- **`lhm.rs`** — named pipe client → `LhmData`; `select_gpu_idx` (preferred → highest VRAM → load tie-break), `normalize_gpu_name`/`gpu_names_match` (WMI/LHM-tolerant name matching)
- **`lhm_process.rs`** — connection state tracking (connect/disconnect logging, 30 s throttle)
- **`pipe_server.rs`** — `is_service_pipe`: both pipe clients only accept a server running in session 0 (the service), so another process that took the pipe name is ignored; debug builds accept any server (dev sidecar). The service side: `sensor-sidecar/Control/DataDirectory.cs` locks `%ProgramData%\se.codeby.rigstats` to SYSTEM/Administrators before anything is read from it
- **`control.rs`** — Control Center pipe client (`\\.\pipe\rigstats-control`, duplex): `control_task` → `ControlState` (capabilities, profiles, live `fan_duty`, `fan_identified`, safety trips, running `preview`, crash-guard notice); `ControlCmd`; typed fan, CPU-limit, Curve Optimizer, GPU and lighting profile/capability structs. Service side: `sensor-sidecar/Control/CLAUDE.md`.
- **`logging.rs`** — session-based CSV stats logging: `start_session`/`end_session`, `append_stats_row`, `load_sessions`/`rename_session`/`set_session_pinned`/`delete_session`, `prune_old_sessions`, `reconcile_sessions_on_startup`. Session index (`rigstats-sessions.json`) writes are guarded by a cross-process file lock (`SessionsLock`) and mirrored to a `.bak` for corruption recovery.
- **`settings.rs`** — `Settings` struct + JSON persistence to `%APPDATA%\se.codeby.rigstats\`
- **`debug.rs`** — `log_debug`/`log_warn`/`log_error`; `reset_debug_log` rotates log to `rigstats-debug-prev.log`; `install_panic_logger` (both binaries: every panic → log with thread, file:line, backtrace); `supervise` restarts `poll_loop`/`control_task` after a panic (2 s, doubling to 60 s). The sidecar logs unhandled exceptions, start failures and unexpected pipe errors with stack traces; both logs use local time (sidecar with UTC offset) (#219)
- **`autostart.rs`** — HKCU run key for launch-at-startup

