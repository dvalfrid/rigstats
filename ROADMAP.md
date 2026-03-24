# Roadmap

Planned features in rough priority order. Each item is scoped as a self-contained release.

---

## Auto-update ✓

**Plugin:** `tauri-plugin-updater`
**Distribution:** GitHub Releases (existing pipeline)

**Implemented.** On startup the app silently checks for updates after a 10-second
delay, then every 6 hours (handles sleep/wake). If a newer version is available
a badge appears in the dashboard header. Clicking the badge (or "Updates & Changelog"
in the tray menu) opens the updater dialog showing the new version's release notes
from GitHub, the full local version history, and a download progress bar.
After installation the app restarts automatically; the About window opens on the
first launch following an upgrade.

---

## NVMe / SSD temperatures ✓

**Panel:** Disk
**Data source:** LHM `Temperatures` section per storage device

**Implemented.** Each drive in the disk panel now shows a live temperature reading
in °C, color-coded by `resolveTempColor` (warm at 55 °C, hot at 70 °C).

LHM sensor identification uses the `SensorId` field (`/nvme/`, `/hdd/`, `/ata/`, `/scsi/`
prefixes) rather than sensor names, so motherboard and RAM thermal sensors are never
mixed in with disk readings. Warning Composite and Critical Composite threshold
sensors are excluded; the highest real temperature per device is shown.

Drive-letter-to-model mapping is resolved at startup via a WMI three-table join
(`Win32_DiskDrive → Win32_DiskDriveToDiskPartition → Win32_LogicalDiskToPartition`),
with a PowerShell CIM fallback. Temperatures are matched by model name (case-insensitive
substring match), so inserting a USB drive never shifts temperatures to the wrong
drive.

---

## Temperature threshold alerts

**Panel:** Settings (new threshold fields) + tray notifications
**Data source:** Existing CPU / GPU / disk temp fields

The dashboard is currently fully passive. A configurable alert system that fires a
Windows tray notification when a component exceeds its threshold would make the app
genuinely useful during gaming or overclocking sessions.

**Scope:**

- New optional fields in `Settings`: `alert_cpu_temp`, `alert_gpu_temp`, `alert_disk_temp` (all `Option<u8>`)
- Per-tick comparison in `commands.rs`; fire notification via `tauri-plugin-notification` with a cooldown (e.g. 60 s) to avoid spam
- Threshold sliders / inputs in the Settings window

---

## CPU fan speed

**Panel:** CPU
**Data source:** LHM `Fans` section on the CPU device node

GPU fan RPM is already displayed. Adding CPU fan speed is a trivial backend change
(one extra field in `LhmData`) mirrored in the CPU panel.

**Scope:**

- Extract `parent == "Fans"` CPU fan node in `lhm.rs`, add `cpu_fan: Option<f64>` to `LhmData`
- Propagate through `CpuStats`
- Render alongside existing CPU metrics in `panels/cpu.js`

---

## Battery panel (laptop support)

**Panel:** New `battery` panel
**Data source:** `sysinfo` battery API

Relevant for gaming laptops (ASUS ROG, Razer, Alienware). Shows charge %, charge
rate (W), and estimated time remaining. Panel is hidden automatically on systems
with no battery detected.

**Scope:**

- Query `sysinfo::Battery` on startup; store in `AppState` if present
- New `BatteryStats` struct in `stats.rs`, included in `StatsPayload`
- New `panels/battery.js` frontend panel
- Add `battery` to the valid panel keys list in `monitor.rs` and settings

---
