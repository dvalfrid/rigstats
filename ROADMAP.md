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

## Temperature threshold alerts ✓

**Panel:** Settings (new threshold fields) + tray notifications
**Data source:** Existing CPU / GPU / RAM / disk temp fields

**Implemented.** A configurable alert system fires a Windows tray notification when
a component exceeds its threshold, making the app useful during gaming or overclocking.

Eight optional `Option<u8>` fields added to `Settings` (serialised as camelCase JSON):
`warningCpuTemp`, `warningGpuTemp`, `warningRamTemp`, `warningDiskTemp`,
`criticalCpuTemp`, `criticalGpuTemp`, `criticalRamTemp`, `criticalDiskTemp`.

Per-tick comparison runs in `commands.rs` inside `get_stats()` after the
`StatsPayload` is assembled. Warning and Critical are checked independently —
each has its own 60-second cooldown key (e.g. `"cpu_warning"` vs `"cpu_critical"`)
stored in `AppState.last_alert`. Disk alerts fire on the hottest drive's temperature.
Notifications are sent via `tauri-plugin-notification`; errors are silently discarded
so a failed toast never disrupts the stats tick.

The Settings window has a compact "Temp Alerts" card with number inputs for all
eight thresholds. Blank = disabled (maps to `None`). Yellow column headers for
Warning, red for Critical. Window height bumped from 620 → 700 px to accommodate
the new card.

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

## Motherboard panel (fans, temps, voltages)

**Panel:** New `motherboard` panel
**Data source:** LHM Super I/O chip node (e.g. Nuvoton NCT6799D, ITE IT87xx, Winbond W836xx)

Shows the sensors exposed by the motherboard's Super I/O chip: fan speeds, board
temperatures, and key voltage rails. Useful for monitoring system cooling without
needing to open the BIOS.

**Available sensor data (verified on ASUS PRIME B650M-A AX6 II / NCT6799D):**

- **Fans:** up to 7 channels in RPM; fan #7 on that board runs at ~2650 RPM (CPU
  cooler pump) while chassis fans sit around 900 RPM. Channels reporting 0 RPM are
  hidden automatically.
- **Fan control:** duty cycle % per channel (31–100 %). Show alongside RPM or omit
  to keep the panel compact.
- **Temperatures:** 6 unnamed slots (`Temperature #1–#6`). LHM does not label these
  — the mapping to VRM, chipset, or PCH depends on board firmware. Show as T1–T6
  and filter out sensors stuck at implausibly low values (< 5 °C sentinel).
- **Voltages:** named rails worth surfacing: `Vcore`, `+3.3V`, `AVCC`,
  `CPU Termination`. The remaining `Voltage #N` slots are unmapped and should be
  hidden by default or behind a toggle.

**Design constraints:**

The Super I/O node sits under the board name in the LHM tree, identified by a
`/lpc/` SensorId prefix. Because different chip models (NCT, ITE, Winbond) share
the same `parent == "Fans"` / `parent == "Temperatures"` structure under their
respective device node, extraction is done by SensorId prefix rather than chip name
— the same approach used for disk temperatures.

Portrait space is a concern: a naive list of 7 fans + 6 temps + 4 voltages = 17
rows. Consider grouping into two or three rows per category using the same compact
`bar-row` layout as the disk panel, and limiting fans to the top 5 active channels
(highest RPM first).

**Scope:**

- Add `/lpc/` extraction to `lhm.rs`: `mb_fans: Vec<(String, f64)>`,
  `mb_temps: Vec<f64>` (filtered, unnamed), `mb_voltages: Vec<(String, f64)>`
  (named rails only) to `LhmData`
- Propagate through a new `MotherboardStats` struct in `stats.rs` → `StatsPayload`
- New `panels/motherboard.js` frontend panel with compact multi-column layout
- Add `motherboard` to valid panel keys in `monitor.rs` and settings visibility list

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
