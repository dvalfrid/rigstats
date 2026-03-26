//! Shared data contracts between backend and renderer.
//! This module contains payload structures and mutable app state containers.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use sysinfo::{Disks, Networks, System};

use crate::lhm::LhmData;
use crate::settings::Settings;

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
  pub load: Option<f64>,
  pub temp: Option<f64>,
  pub hotspot: Option<f64>,
  pub freq: Option<f64>,
  pub vram_used: Option<f64>,
  pub vram_total: Option<f64>,
  pub fan_speed: Option<f64>,
  pub power: Option<f64>,
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

#[derive(Debug, Clone, Serialize)]
pub struct DiskDrive {
  pub fs: String,
  pub size: u64,
  pub used: u64,
  pub pct: u8,
  /// Temperature matched from LHM via disk model name; `None` when unavailable.
  pub temp: Option<f64>,
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
pub struct StatsPayload {
  pub cpu: CpuStats,
  pub gpu: GpuStats,
  pub ram: RamStats,
  pub net: NetStats,
  pub disk: DiskStats,
  pub motherboard: MotherboardStats,
  pub system_uptime_secs: u64,
  pub lhm_connected: bool,
}

pub struct AppState {
  /// Maps drive letter (e.g. `"C:"`) to physical disk model name detected at startup.
  /// Used at each tick to match LHM temperature readings to sysinfo volumes by name
  /// instead of by index, so inserting a USB drive never shifts other drives' temps.
  pub disk_model_map: std::collections::HashMap<String, String>,
  /// Reused HTTP client for LHM polling — avoids allocating a new connection pool every tick.
  pub lhm_client: reqwest::Client,
  /// Persisted UI preferences mirrored in memory for fast reads.
  pub settings: Mutex<Settings>,
  /// Reused sysinfo collector to avoid reallocating sensors every tick.
  pub system: Mutex<System>,
  pub disks: Mutex<Disks>,
  pub networks: Mutex<Networks>,
  /// Timestamp of the previous network sample for throughput delta calculations.
  pub last_net_sample: Mutex<Option<Instant>>,
  /// Cached ping sample to avoid spawning an ICMP process on every tick.
  pub last_ping_sample: Mutex<Option<(Instant, Option<f64>)>>,
  /// Last successful LHM snapshot used when live HTTP polling fails transiently.
  pub last_lhm: Mutex<Option<LhmData>>,
  /// Best-effort RAM descriptor detected on startup (e.g. DDR5 6000 MT/s).
  pub ram_spec: String,
  /// Best-effort RAM module details (e.g. 2x16 GB | Vendor | Part).
  pub ram_details: String,
  /// Best-effort VRAM total fallback in MB when live LHM data is unavailable.
  /// `None` when WMI detection failed at startup.
  pub gpu_vram_total_mb: Option<f64>,
  /// Preferred ping target (default gateway if available, otherwise public fallback).
  pub ping_target: String,
  /// Detected system board brand (e.g. "rog", "msi", "other").
  pub system_brand: String,
  /// Motherboard name detected at startup (e.g. "ASUS PRIME B650M-A AX6 II").
  pub mb_name: Option<String>,
  /// Whether sysinfo returned a usable initial snapshot on startup.
  pub sysinfo_available: bool,
  /// Whether a WMI connection could be established on startup.
  pub wmi_available: bool,
  /// Per-component alert cooldown tracker. Key: "<component>_<level>" (e.g. "cpu_warning").
  /// Stores the `Instant` of the last fired notification to enforce the 60-second cooldown.
  pub last_alert: Mutex<HashMap<String, Instant>>,
}
