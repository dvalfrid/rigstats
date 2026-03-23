# Roadmap

Planned features in rough priority order. Each item is scoped as a self-contained release.

---

## v1.6 — Auto-update ✓

**Plugin:** `tauri-plugin-updater`
**Distribution:** GitHub Releases (existing pipeline)

**Implemented.** On startup the app silently checks for updates after a 10-second
delay. If a newer version is available a badge appears in the dashboard header.
Clicking the badge (or "Check for Updates" in the tray menu) opens an update
dialog showing the new version, release notes, and a download progress bar.
After installation the NSIS installer restarts the app; the About window
opens automatically on the first launch following an upgrade.

**Setup required (one-time, not yet done):**

1. Generate a signing keypair:

   ```bash
   npx @tauri-apps/cli signer generate -w ./rigstats-update.key
   ```

   Copy the printed **public key** to `tauri.conf.json` → `plugins.updater.pubkey`.

2. Add two GitHub Actions secrets:
   - `TAURI_SIGNING_PRIVATE_KEY` — the base64-encoded private key content
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the key password (empty string if none)

---

## v1.7 — NVMe / SSD temperatures

**Panel:** Disk
**Data source:** LHM `Temperatures` section per storage device

LHM already exposes per-drive temperature sensors. The disk panel currently shows
throughput and usage but no thermal data. NVMe drives throttle silently at high
temperatures, making this a high-value, low-effort addition.

**Scope:**

- Parse `Temperatures` nodes per disk device in `lhm.rs`, add `disk_temps: Vec<(String, f64)>` to `LhmData`
- Propagate through `DiskStats` / `StatsPayload`
- Render a °C indicator per drive in `panels/disk.js` with `resolveTempColor` highlighting

---

## v1.8 — Temperature threshold alerts

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

## v1.9 — CPU fan speed

**Panel:** CPU
**Data source:** LHM `Fans` section on the CPU device node

GPU fan RPM is already displayed. Adding CPU fan speed is a trivial backend change
(one extra field in `LhmData`) mirrored in the CPU panel.

**Scope:**

- Extract `parent == "Fans"` CPU fan node in `lhm.rs`, add `cpu_fan: Option<f64>` to `LhmData`
- Propagate through `CpuStats`
- Render alongside existing CPU metrics in `panels/cpu.js`

---

## v2.0 — Battery panel (laptop support)

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
