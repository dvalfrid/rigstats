use eframe::egui;
use rigstats_backend::{debug, lhm, lhm_process, settings};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

fn app_data_dir() -> PathBuf {
  let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
  PathBuf::from(appdata).join("se.codeby.rigstats")
}

#[derive(Clone, Debug, Default)]
struct Phase1Stats {
  cpu_load: u8,
  cpu_temp: Option<f64>,
  gpu_load: Option<f64>,
  gpu_temp: Option<f64>,
  ram_used_gb: f64,
  ram_total_gb: f64,
  lhm_connected: bool,
}

struct RigStatsApp {
  receiver: mpsc::Receiver<Phase1Stats>,
  latest: Phase1Stats,
}

impl RigStatsApp {
  fn new(receiver: mpsc::Receiver<Phase1Stats>) -> Self {
    Self { receiver, latest: Phase1Stats::default() }
  }
}

impl eframe::App for RigStatsApp {
  fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
    while let Ok(stats) = self.receiver.try_recv() {
      self.latest = stats;
    }

    ui.heading("RigStats — Phase 1 scaffold");
    ui.separator();

    let s = &self.latest;
    ui.label(format!("CPU load:  {}%", s.cpu_load));
    if let Some(t) = s.cpu_temp {
      ui.label(format!("CPU temp:  {t:.1} °C"));
    }
    ui.label(format!("GPU load:  {:.1}%", s.gpu_load.unwrap_or(0.0)));
    if let Some(t) = s.gpu_temp {
      ui.label(format!("GPU temp:  {t:.1} °C"));
    }
    ui.label(format!("RAM:       {:.1} / {:.1} GB", s.ram_used_gb, s.ram_total_gb));
    ui.separator();
    ui.label(if s.lhm_connected { "LHM pipe: connected ✓" } else { "LHM pipe: disconnected ✗" });

    // Sleep between repaints — this is the key to 0 % CPU at idle.
    ui.ctx().request_repaint_after(Duration::from_secs(1));
  }
}

async fn poll_loop(tx: mpsc::SyncSender<Phase1Stats>, dir: PathBuf) {
  let s = settings::load_settings(&dir);
  let preferred_gpu = s.preferred_gpu.clone();

  let pipe = tokio::sync::Mutex::new(None::<lhm::LhmPipeReader>);
  let mut sys = System::new();

  loop {
    sys.refresh_specifics(
      RefreshKind::new()
        .with_cpu(CpuRefreshKind::new().with_cpu_usage())
        .with_memory(MemoryRefreshKind::everything()),
    );

    let cpu_load = sys.global_cpu_info().cpu_usage() as u8;
    let ram_used_gb = sys.used_memory() as f64 / 1_073_741_824.0;
    let ram_total_gb = sys.total_memory() as f64 / 1_073_741_824.0;

    let lhm_data = lhm::fetch_lhm_pipe(&pipe, preferred_gpu.as_deref(), &dir).await;
    let connected = lhm_data.is_some();
    lhm_process::track_lhm_connection_state(&dir, connected);

    let (gpu_load, gpu_temp, cpu_temp) = lhm_data
      .as_ref()
      .map(|d| (d.gpu_load, d.gpu_temp, d.cpu_temp))
      .unwrap_or((None, None, None));

    let _ = tx.send(Phase1Stats {
      cpu_load,
      cpu_temp,
      gpu_load,
      gpu_temp,
      ram_used_gb,
      ram_total_gb,
      lhm_connected: connected,
    });

    tokio::time::sleep(Duration::from_secs(1)).await;
  }
}

fn main() {
  let dir = app_data_dir();

  debug::reset_debug_log(&dir);
  debug::append_debug_log(&dir, "rigstats-egui starting (Phase 1)");

  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .expect("tokio runtime");

  let (tx, rx) = mpsc::sync_channel::<Phase1Stats>(4);

  let dir_clone = dir.clone();
  runtime.spawn(async move {
    poll_loop(tx, dir_clone).await;
  });

  let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
      .with_title("RigStats")
      .with_inner_size([320.0, 280.0]),
    ..Default::default()
  };

  eframe::run_native("RigStats", options, Box::new(|_cc| Ok(Box::new(RigStatsApp::new(rx)))))
    .expect("eframe");

  runtime.shutdown_background();
}
