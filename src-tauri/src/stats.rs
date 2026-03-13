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
  pub details: String,
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
  pub system_uptime_secs: u64,
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
  /// Cached ping sample to avoid spawning an ICMP process on every tick.
  pub last_ping_sample: Mutex<Option<(Instant, Option<f64>)>>,
  /// Last successful LHM snapshot used when live HTTP polling fails transiently.
  pub last_lhm: Mutex<Option<LhmData>>,
  /// Best-effort RAM descriptor detected on startup (e.g. DDR5 6000 MT/s).
  pub ram_spec: String,
  /// Best-effort RAM module details (e.g. 2x16 GB | Vendor | Part).
  pub ram_details: String,
  /// Best-effort VRAM total fallback in MB when live LHM data is unavailable.
  pub gpu_vram_total_mb: f64,
  /// Preferred ping target (default gateway if available, otherwise public fallback).
  pub ping_target: String,
  /// Detected system board brand (e.g. "rog", "msi", "other").
  pub system_brand: String,
}
