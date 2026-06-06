mod panels;
mod spark;
mod tempcolor;

use eframe::egui;
use rigstats_backend::{debug, hardware, lhm, lhm_process, settings};
use spark::Sparkline;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

fn app_data_dir() -> PathBuf {
  let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
  PathBuf::from(appdata).join("se.codeby.rigstats")
}

// ── Stats payload sent from poll thread to UI thread ──────────────────────────

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
  // Meta
  pub lhm_connected: bool,
}

// ── eframe application ────────────────────────────────────────────────────────

struct RigStatsApp {
  receiver: mpsc::Receiver<PollStats>,
  latest: PollStats,
  cpu_spark: Sparkline,
  gpu_spark: Sparkline,
  ram_spark: Sparkline,
}

impl RigStatsApp {
  fn new(receiver: mpsc::Receiver<PollStats>) -> Self {
    Self {
      receiver,
      latest: PollStats::default(),
      cpu_spark: Sparkline::new(60, 100.0),
      gpu_spark: Sparkline::new(60, 100.0),
      ram_spark: Sparkline::new(60, 100.0),
    }
  }
}

impl eframe::App for RigStatsApp {
  fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
    // Drain all pending updates — take only the newest.
    while let Ok(stats) = self.receiver.try_recv() {
      self.latest = stats;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
      panels::cpu::draw(ui, &self.latest, &mut self.cpu_spark);
      ui.add_space(8.0);
      panels::gpu::draw(ui, &self.latest, &mut self.gpu_spark);
      ui.add_space(8.0);
      panels::ram::draw(ui, &self.latest, &mut self.ram_spark);
    });

    ui.ctx().request_repaint_after(Duration::from_secs(1));
  }
}

// ── Poll loop (runs on the tokio runtime) ─────────────────────────────────────

async fn poll_loop(tx: mpsc::SyncSender<PollStats>, dir: PathBuf) {
  // One-time startup detections (blocking WMI calls run on the blocking thread pool).
  let ram_spec = tokio::task::spawn_blocking(hardware::detect_ram_spec).await.unwrap_or_default();

  let s = settings::load_settings(&dir);
  let preferred_gpu = s.preferred_gpu.clone();

  let pipe = tokio::sync::Mutex::new(None::<lhm::LhmPipeReader>);
  let mut sys = System::new();

  loop {
    sys.refresh_specifics(
      RefreshKind::new()
        .with_cpu(CpuRefreshKind::new().with_cpu_usage().with_frequency())
        .with_memory(MemoryRefreshKind::everything()),
    );

    let cpu_load = sys.global_cpu_info().cpu_usage() as u8;
    let cpu_freq_mhz = sys.global_cpu_info().frequency() as f64;
    let cpu_cores: Vec<u8> = sys.cpus().iter().map(|c| c.cpu_usage() as u8).collect();

    let lhm_data = lhm::fetch_lhm_pipe(&pipe, preferred_gpu.as_deref(), &dir).await;
    let connected = lhm_data.is_some();
    lhm_process::track_lhm_connection_state(&dir, connected);

    let stats = if let Some(ref d) = lhm_data {
      PollStats {
        cpu_load,
        cpu_temp: d.cpu_temp,
        cpu_freq_mhz,
        cpu_power: d.cpu_power,
        cpu_cores,
        gpu_load: d.gpu_load,
        gpu_temp: d.gpu_temp,
        gpu_hotspot: d.gpu_hotspot,
        gpu_freq_mhz: d.gpu_freq,
        gpu_mem_freq_mhz: d.gpu_mem_freq,
        gpu_vram_used_mb: d.vram_used,
        gpu_vram_total_mb: d.vram_total,
        gpu_power: d.gpu_power,
        gpu_fan: d.gpu_fan,
        ram_used: sys.used_memory(),
        ram_total: sys.total_memory(),
        ram_spec: ram_spec.clone(),
        lhm_connected: true,
      }
    } else {
      PollStats {
        cpu_load,
        cpu_freq_mhz,
        cpu_cores,
        ram_used: sys.used_memory(),
        ram_total: sys.total_memory(),
        ram_spec: ram_spec.clone(),
        lhm_connected: false,
        ..Default::default()
      }
    };

    let _ = tx.send(stats);

    tokio::time::sleep(Duration::from_secs(1)).await;
  }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
  let dir = app_data_dir();

  debug::reset_debug_log(&dir);
  debug::append_debug_log(&dir, "rigstats-egui starting (Phase 2)");

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
    // Dark theme with a slight blue tint for labels.
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(14, 17, 23);
    cc.egui_ctx.set_visuals(visuals);
    Ok(Box::new(RigStatsApp::new(rx)))
  }))
  .expect("eframe");

  runtime.shutdown_background();
}
