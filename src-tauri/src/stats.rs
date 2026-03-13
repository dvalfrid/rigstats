//! Shared data contracts between backend and renderer.
//! This module contains payload structures and mutable app state containers.

use serde::Serialize;
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
  pub vram_total: f64,
  pub fan_speed: Option<f64>,
  pub power: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RamStats {
  pub total: u64,
  pub used: u64,
  pub free: u64,
  pub spec: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetStats {
  pub up: f64,
  pub down: f64,
  pub iface: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskDrive {
  pub fs: String,
  pub size: u64,
  pub used: u64,
  pub pct: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskStats {
  pub read: f64,
  pub write: f64,
  pub drives: Vec<DiskDrive>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsPayload {
  pub cpu: CpuStats,
  pub gpu: GpuStats,
  pub ram: RamStats,
  pub net: NetStats,
  pub disk: DiskStats,
  pub lhm_connected: bool,
}

pub struct AppState {
  /// Persisted UI preferences mirrored in memory for fast reads.
  pub settings: Mutex<Settings>,
  /// Reused sysinfo collector to avoid reallocating sensors every tick.
  pub system: Mutex<System>,
  pub disks: Mutex<Disks>,
  pub networks: Mutex<Networks>,
  /// Timestamp of the previous network sample for throughput delta calculations.
  pub last_net_sample: Mutex<Option<Instant>>,
  /// Last successful LHM snapshot used when live HTTP polling fails transiently.
  pub last_lhm: Mutex<Option<LhmData>>,
}
