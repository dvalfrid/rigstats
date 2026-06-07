mod panels;
mod spark;
mod tempcolor;

use eframe::egui;
use rigstats_backend::{debug, hardware, lhm, lhm_process, settings};
use spark::Sparkline;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

fn app_data_dir() -> PathBuf {
  let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
  PathBuf::from(appdata).join("se.codeby.rigstats")
}

// ── Data types exchanged between poll thread and UI thread ────────────────────

#[derive(Clone, Debug, Default)]
pub struct DriveInfo {
  pub fs: String,
  pub used: u64,
  pub total: u64,
  pub pct: u8,
  pub temp: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct ProcessInfo {
  pub name: String,
  pub cpu: f32,
  pub mem_mb: u64,
}

#[derive(Clone, Debug, Default)]
pub struct PollStats {
  // CPU
  pub cpu_load: u8,
  pub cpu_temp: Option<f64>,
  pub cpu_freq_mhz: f64,
  pub cpu_power: Option<f64>,
  pub cpu_cores: Vec<u8>,
  // GPU
  pub gpu_load: Option<f64>,
  pub gpu_temp: Option<f64>,
  pub gpu_hotspot: Option<f64>,
  pub gpu_freq_mhz: Option<f64>,
  pub gpu_mem_freq_mhz: Option<f64>,
  pub gpu_vram_used_mb: Option<f64>,
  pub gpu_vram_total_mb: Option<f64>,
  pub gpu_power: Option<f64>,
  pub gpu_fan: Option<f64>,
  // RAM
  pub ram_used: u64,
  pub ram_total: u64,
  pub ram_spec: String,
  // Network
  pub net_up_mbps: f64,
  pub net_down_mbps: f64,
  pub net_iface: String,
  pub net_ping_ms: Option<f64>,
  // Disk
  pub disk_read_mbps: f64,
  pub disk_write_mbps: f64,
  pub disk_drives: Vec<DriveInfo>,
  // Motherboard (LHM)
  pub mb_fans: Vec<(String, f64)>,
  pub mb_temps: Vec<(String, f64)>,
  pub mb_voltages: Vec<(String, f64)>,
  pub mb_chip: Option<String>,
  pub mb_board: Option<String>,
  // Battery
  pub battery_present: bool,
  pub battery_charge_pct: Option<u8>,
  pub battery_charging: Option<bool>,
  pub battery_time_mins: Option<u32>,
  pub battery_power_w: Option<f64>,
  // Processes
  pub processes: Vec<ProcessInfo>,
  // System
  pub uptime_secs: u64,
  pub hostname: String,
  pub cpu_model: String,
  // Meta
  pub lhm_connected: bool,
}

// ── eframe application ────────────────────────────────────────────────────────

struct RigStatsApp {
  receiver: mpsc::Receiver<PollStats>,
  latest: PollStats,
  visible_panels: Vec<String>,
  cpu_spark: Sparkline,
  gpu_spark: Sparkline,
  ram_spark: Sparkline,
  net_up_spark: Sparkline,
  net_dn_spark: Sparkline,
  disk_page: usize,
  disk_page_tick: u32,
}

impl RigStatsApp {
  fn new(receiver: mpsc::Receiver<PollStats>, visible_panels: Vec<String>) -> Self {
    Self {
      receiver,
      latest: PollStats::default(),
      visible_panels,
      cpu_spark: Sparkline::new(60, 100.0),
      gpu_spark: Sparkline::new(60, 100.0),
      ram_spark: Sparkline::new(60, 100.0),
      net_up_spark: Sparkline::new(60, 100.0),
      net_dn_spark: Sparkline::new(60, 100.0),
      disk_page: 0,
      disk_page_tick: 0,
    }
  }

  fn visible(&self, key: &str) -> bool {
    self.visible_panels.iter().any(|p| p == key)
  }
}

impl eframe::App for RigStatsApp {
  fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
    while let Ok(stats) = self.receiver.try_recv() {
      self.latest = stats;
    }

    // Reset disk page if drive count changed (drive added/removed).
    let page_total = self.latest.disk_drives.len().div_ceil(3);
    if self.disk_page >= page_total.max(1) {
      self.disk_page = 0;
      self.disk_page_tick = 0;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
      if self.visible("header") {
        panels::header::draw(ui, &self.latest);
        ui.add_space(6.0);
      }
      if self.visible("clock") {
        panels::clock::draw(ui, self.latest.uptime_secs);
        ui.add_space(6.0);
      }
      if self.visible("cpu") {
        panels::cpu::draw(ui, &self.latest, &mut self.cpu_spark);
        ui.add_space(6.0);
      }
      if self.visible("gpu") {
        panels::gpu::draw(ui, &self.latest, &mut self.gpu_spark);
        ui.add_space(6.0);
      }
      if self.visible("ram") {
        panels::ram::draw(ui, &self.latest, &mut self.ram_spark);
        ui.add_space(6.0);
      }
      if self.visible("net") {
        panels::net::draw(ui, &self.latest, &mut self.net_up_spark, &mut self.net_dn_spark);
        ui.add_space(6.0);
      }
      if self.visible("disk") {
        panels::disk::draw(ui, &self.latest, &mut self.disk_page, &mut self.disk_page_tick);
        ui.add_space(6.0);
      }
      if self.visible("motherboard") {
        panels::motherboard::draw(ui, &self.latest);
        ui.add_space(6.0);
      }
      if self.visible("process") {
        panels::process::draw(ui, &self.latest);
        ui.add_space(6.0);
      }
      if self.visible("battery") {
        panels::battery::draw(ui, &self.latest);
        ui.add_space(6.0);
      }
    });

    ui.ctx().request_repaint_after(Duration::from_secs(1));
  }
}

// ── LHM disk temp helper ──────────────────────────────────────────────────────

fn lhm_temp_for_model(wmi_model: &str, disk_temps: &[(String, f64)]) -> Option<f64> {
  let needle = wmi_model.trim().to_lowercase();
  if needle.is_empty() {
    return None;
  }
  disk_temps.iter().find_map(|(name, temp)| {
    let hay = name.trim().to_lowercase();
    if hay == needle || hay.contains(&needle) || needle.contains(&hay) {
      Some(*temp)
    } else {
      None
    }
  })
}

// ── Poll loop (tokio runtime) ─────────────────────────────────────────────────

async fn poll_loop(tx: mpsc::SyncSender<PollStats>, dir: PathBuf) {
  // One-time blocking startup detections.
  let ram_spec = tokio::task::spawn_blocking(hardware::detect_ram_spec).await.unwrap_or_default();
  let disk_model_map: HashMap<String, String> =
    tokio::task::spawn_blocking(hardware::detect_disk_model_map).await.unwrap_or_default();
  let ping_target = hardware::detect_ping_target();
  let mb_board: Option<String> =
    tokio::task::spawn_blocking(hardware::detect_motherboard_name).await.ok().flatten();

  let s = settings::load_settings(&dir);
  let preferred_gpu = s.preferred_gpu.clone();

  let mut sys = System::new();
  // Initial full CPU refresh to populate brand string (available after first refresh).
  sys.refresh_cpu();
  let cpu_model = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
  let hostname = System::host_name().unwrap_or_else(|| "—".to_string());

  let mut disks = Disks::new_with_refreshed_list();
  let mut networks = Networks::new_with_refreshed_list();
  let pipe = tokio::sync::Mutex::new(None::<lhm::LhmPipeReader>);

  let mut last_net_instant = Instant::now();
  let mut last_ping: Option<(Instant, Option<f64>)> = None;
  type BatteryCache = (u8, bool, Option<u32>, Option<f64>);
  let mut last_battery: Option<(Instant, BatteryCache)> = None;

  loop {
    sys.refresh_specifics(
      RefreshKind::new()
        .with_cpu(CpuRefreshKind::new().with_cpu_usage().with_frequency())
        .with_memory(MemoryRefreshKind::everything()),
    );
    sys.refresh_processes();

    let cpu_load = sys.global_cpu_info().cpu_usage() as u8;
    let cpu_freq_mhz = sys.global_cpu_info().frequency() as f64;
    let cpu_cores: Vec<u8> = sys.cpus().iter().map(|c| c.cpu_usage() as u8).collect();
    let uptime_secs = System::uptime();

    // Processes — sort by CPU descending, cap at 8.
    let num_cpus = sys.cpus().len().max(1) as f32;
    let mut processes: Vec<ProcessInfo> = sys
      .processes()
      .values()
      .map(|p| ProcessInfo {
        name: p.name().to_string(),
        cpu: p.cpu_usage() / num_cpus,
        mem_mb: p.memory() / 1_048_576,
      })
      .collect();
    processes.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
    processes.truncate(8);

    // Disks.
    disks.refresh();
    let mut disk_drives: Vec<DriveInfo> = disks
      .iter()
      .filter(|d| d.total_space() > 1_000_000_000)
      .map(|d| {
        let total = d.total_space();
        let used = total.saturating_sub(d.available_space());
        let pct = if total > 0 { ((used as f64 / total as f64) * 100.0) as u8 } else { 0 };
        DriveInfo {
          fs: d.mount_point().to_string_lossy().to_string(),
          used,
          total,
          pct,
          temp: None,
        }
      })
      .collect();

    // Networks — delta-based throughput in Mbps.
    let now = Instant::now();
    let elapsed = now.duration_since(last_net_instant).as_secs_f64().max(0.5);
    last_net_instant = now;
    networks.refresh();
    let mut best_iface = "--".to_string();
    let mut best_up = 0.0f64;
    let mut best_down = 0.0f64;
    for (name, data) in networks.iter() {
      let up = data.transmitted() as f64 * 8.0 / 1_000_000.0 / elapsed;
      let down = data.received() as f64 * 8.0 / 1_000_000.0 / elapsed;
      if up + down > best_up + best_down {
        best_up = up;
        best_down = down;
        best_iface = name.clone();
      }
    }

    // Ping — cached for 5 s, measured in a blocking thread.
    let ping_ms = {
      let stale = last_ping.as_ref().map(|(t, _)| t.elapsed().as_secs_f64() >= 5.0).unwrap_or(true);
      if stale {
        let target = ping_target.clone();
        let measured = tokio::task::spawn_blocking(move || hardware::sample_ping_ms(&target))
          .await
          .unwrap_or(None);
        last_ping = Some((Instant::now(), measured));
        measured
      } else {
        last_ping.as_ref().and_then(|(_, v)| *v)
      }
    };

    // Battery — cached for 10 s.
    let (battery_present, battery_charge_pct, battery_charging, battery_time_mins, battery_power_w) = {
      let stale = last_battery.as_ref().map(|(t, _)| t.elapsed().as_secs_f64() >= 10.0).unwrap_or(true);
      if stale {
        let result = tokio::task::spawn_blocking(hardware::sample_battery_wmi).await.ok().flatten();
        let data = result;
        last_battery = data.as_ref().map(|d| (Instant::now(), *d));
        match data {
          Some((pct, charging, mins, w)) => (true, Some(pct), Some(charging), mins, w),
          None => (false, None, None, None, None),
        }
      } else {
        match last_battery.as_ref().map(|(_, d)| *d) {
          Some((pct, charging, mins, w)) => (true, Some(pct), Some(charging), mins, w),
          None => (false, None, None, None, None),
        }
      }
    };

    // LHM pipe.
    let lhm_data = lhm::fetch_lhm_pipe(&pipe, preferred_gpu.as_deref(), &dir).await;
    let lhm_connected = lhm_data.is_some();
    lhm_process::track_lhm_connection_state(&dir, lhm_connected);

    // Match disk temps from LHM by disk model name.
    if let Some(ref lhm) = lhm_data {
      for (i, drive) in disk_drives.iter_mut().enumerate() {
        let key = drive.fs.trim_end_matches(['\\', '/']).to_string();
        if let Some(model) = disk_model_map.get(&key) {
          drive.temp = lhm_temp_for_model(model, &lhm.disk_temps);
        }
        if drive.temp.is_none() && !disk_model_map.contains_key(&key) {
          drive.temp = lhm.disk_temps.get(i).map(|(_, t)| *t);
        }
      }
    }

    let stats = PollStats {
      cpu_load,
      cpu_temp: lhm_data.as_ref().and_then(|l| l.cpu_temp),
      cpu_freq_mhz,
      cpu_power: lhm_data.as_ref().and_then(|l| l.cpu_power),
      cpu_cores,
      gpu_load: lhm_data.as_ref().and_then(|l| l.gpu_load),
      gpu_temp: lhm_data.as_ref().and_then(|l| l.gpu_temp),
      gpu_hotspot: lhm_data.as_ref().and_then(|l| l.gpu_hotspot),
      gpu_freq_mhz: lhm_data.as_ref().and_then(|l| l.gpu_freq),
      gpu_mem_freq_mhz: lhm_data.as_ref().and_then(|l| l.gpu_mem_freq),
      gpu_vram_used_mb: lhm_data.as_ref().and_then(|l| l.vram_used),
      gpu_vram_total_mb: lhm_data.as_ref().and_then(|l| l.vram_total),
      gpu_power: lhm_data.as_ref().and_then(|l| l.gpu_power),
      gpu_fan: lhm_data.as_ref().and_then(|l| l.gpu_fan),
      ram_used: sys.used_memory(),
      ram_total: sys.total_memory(),
      ram_spec: ram_spec.clone(),
      net_up_mbps: best_up,
      net_down_mbps: best_down,
      net_iface: best_iface,
      net_ping_ms: ping_ms,
      disk_read_mbps: lhm_data.as_ref().map(|l| l.disk_read).unwrap_or(0.0),
      disk_write_mbps: lhm_data.as_ref().map(|l| l.disk_write).unwrap_or(0.0),
      disk_drives,
      mb_fans: lhm_data.as_ref().map(|l| l.mb_fans.clone()).unwrap_or_default(),
      mb_temps: lhm_data.as_ref().map(|l| l.mb_temps.clone()).unwrap_or_default(),
      mb_voltages: lhm_data.as_ref().map(|l| l.mb_voltages.clone()).unwrap_or_default(),
      mb_chip: lhm_data.as_ref().and_then(|l| l.mb_chip.clone()),
      mb_board: mb_board.clone(),
      battery_present,
      battery_charge_pct,
      battery_charging,
      battery_time_mins,
      battery_power_w,
      processes,
      uptime_secs,
      hostname: hostname.clone(),
      cpu_model: cpu_model.clone(),
      lhm_connected,
    };

    let _ = tx.send(stats);
    tokio::time::sleep(Duration::from_secs(1)).await;
  }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
  let dir = app_data_dir();

  debug::reset_debug_log(&dir);
  debug::append_debug_log(&dir, "rigstats-egui starting (Phase 3)");

  let s = settings::load_settings(&dir);
  let visible_panels = s.visible_panels.clone();

  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .expect("tokio runtime");

  let (tx, rx) = mpsc::sync_channel::<PollStats>(4);

  let dir_clone = dir.clone();
  runtime.spawn(async move {
    poll_loop(tx, dir_clone).await;
  });

  let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
      .with_title("RigStats")
      .with_inner_size([400.0, 780.0]),
    ..Default::default()
  };

  eframe::run_native("RigStats", options, Box::new(|cc| {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(14, 17, 23);
    cc.egui_ctx.set_visuals(visuals);
    Ok(Box::new(RigStatsApp::new(rx, visible_panels)))
  }))
  .expect("eframe");

  runtime.shutdown_background();
}
