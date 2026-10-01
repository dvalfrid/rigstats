//! `StatsPayload` and its sub-structs — the serialisable snapshot shape used by
//! session recording (`logging::append_stats_row`, built from `PollStats` in
//! `src-egui/src/poll.rs`), plus `DiskKind`.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CpuStats {
    pub load: u8,
    pub cores: Vec<u8>,
    pub temp: Option<f64>,
    pub freq: f64,
    pub power: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuStats {
    pub name: Option<String>,
    pub load: Option<f64>,
    pub temp: Option<f64>,
    pub hotspot: Option<f64>,
    pub freq: Option<f64>,
    pub mem_freq: Option<f64>,
    pub vram_used: Option<f64>,
    pub vram_total: Option<f64>,
    pub fan_speed: Option<f64>,
    pub power: Option<f64>,
    pub d3d_3d: Option<f64>,
    pub d3d_vdec: Option<f64>,
    /// Available GPU devices: `[(device_name, vram_total_mb), ...]`.
    pub available_gpus: Vec<(String, f64)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RamStats {
    pub total: u64,
    pub used: u64,
    pub free: u64,
    pub spec: String,
    pub details: String,
    pub temp: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetStats {
    pub up: f64,
    pub down: f64,
    pub iface: String,
    pub ping_ms: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiskKind {
    NVMe,
    Ssd,
    Hdd,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskDrive {
    pub fs: String,
    pub size: u64,
    pub used: u64,
    pub pct: u8,
    /// Temperature matched from LHM via disk model name; `None` when unavailable.
    pub temp: Option<f64>,
    pub model: String,
    pub kind: DiskKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskStats {
    pub read: f64,
    pub write: f64,
    pub drives: Vec<DiskDrive>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MotherboardStats {
    /// Active fan channels: `[label, rpm]`, sorted descending by RPM.
    pub fans: Vec<(String, f64)>,
    /// Temperature readings ≥ 5 °C from the Super I/O chip.
    pub temps: Vec<(String, f64)>,
    /// Named voltage rails (generic "Voltage #N" slots excluded).
    pub voltages: Vec<(String, f64)>,
    /// Super I/O chip name, e.g. "Nuvoton NCT6799D". `None` on laptops or when LHM is not running.
    pub chip: Option<String>,
    /// Motherboard name, e.g. "ASUS PRIME B650M-A AX6 II". `None` when WMI detection failed.
    pub board: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryStats {
    /// `false` on desktop systems with no battery, or when WMI query fails.
    pub present: bool,
    /// Charge percentage 0–100. `None` when not present.
    pub charge_pct: Option<u8>,
    /// `true` when on AC power (charging, full, or connected). `None` when not present.
    pub charging: Option<bool>,
    /// Estimated minutes until empty. `None` when charging, on AC, or unknown.
    pub time_remaining_mins: Option<u32>,
    /// Current power in watts: charge rate when on AC, discharge rate when on battery.
    /// `None` when the driver does not report it or no battery is present.
    pub power_w: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    pub name: String,
    /// CPU usage as a percentage of total system capacity (sum of all cores = 100%).
    pub cpu: f32,
    /// RAM consumed by this process in megabytes.
    pub mem_mb: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsPayload {
    pub cpu: CpuStats,
    pub gpu: GpuStats,
    pub ram: RamStats,
    pub net: NetStats,
    pub disk: DiskStats,
    pub motherboard: MotherboardStats,
    pub battery: BatteryStats,
    pub top_processes: Vec<ProcessEntry>,
    pub system_uptime_secs: u64,
    pub lhm_connected: bool,
    /// Control Center (#187) active profile id (`"gaming"`, `"balanced"`, …)
    /// at the moment this row was logged — `None` on the wallpaper host
    /// (never connects to the control pipe) or before the app has connected.
    pub active_profile: Option<String>,
}
