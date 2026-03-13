//! Tauri command handlers and window event helpers.
//!
//! Design notes:
//! - Commands are intentionally thin wrappers around shared state and helpers.
//! - Settings updates are emitted to the renderer immediately after persistence.
//! - Stats collection keeps the last successful LHM sample to avoid UI flicker
//!   when LibreHardwareMonitor is temporarily unavailable.

use crate::lhm::fetch_lhm;
use crate::settings::{persist_settings, Settings};
use crate::stats::{AppState, CpuStats, DiskDrive, DiskStats, GpuStats, NetStats, RamStats, StatsPayload};
use serde::Deserialize;
use std::time::Instant;
use tauri::{GlobalWindowEvent, Manager, Position, Size, WindowBuilder, WindowEvent};

pub fn pick_target_monitor(window: &tauri::Window) {
  // Prefer the dedicated 450x1920 panel; otherwise use the secondary monitor.
  if let Ok(monitors) = window.available_monitors() {
    let target = monitors
      .iter()
      .find(|m| m.size().width == 450 && m.size().height == 1920)
      .or_else(|| monitors.get(1))
      .cloned();

    if let Some(monitor) = target {
      let _ = window.set_position(Position::Physical(*monitor.position()));
      let _ = window.set_size(Size::Physical(*monitor.size()));
    }
  }
}

#[tauri::command]
pub fn get_settings(state: tauri::State<AppState>) -> Settings {
  state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn preview_opacity(app: tauri::AppHandle, value: f64) -> Result<(), String> {
  if let Some(main) = app.get_window("main") {
    main
      .emit("apply-opacity", value)
      .map_err(|e| e.to_string())?;
  }
  Ok(())
}

#[tauri::command]
pub fn save_settings(
  app: tauri::AppHandle,
  state: tauri::State<AppState>,
  opacity: f64,
  model_name: Option<String>,
  #[allow(non_snake_case)] modelName: Option<String>,
) -> Result<(), String> {
  // Clamp opacity to a valid CSS alpha range before persistence.
  let mut settings = state.settings.lock().unwrap();
  settings.opacity = opacity.clamp(0.0, 1.0);

  // Accept both snake_case and camelCase payload keys from the renderer.
  let incoming_name = model_name.or(modelName).unwrap_or_else(|| settings.model_name.clone());
  settings.model_name = incoming_name;
  persist_settings(&app, &settings)?;

  if let Some(main) = app.get_window("main") {
    main
      .emit("apply-opacity", settings.opacity)
      .map_err(|e| e.to_string())?;
    main
      .emit("apply-model-name", settings.model_name.clone())
      .map_err(|e| e.to_string())?;
  }

  Ok(())
}

#[tauri::command]
pub fn close_window(window: tauri::Window) -> Result<(), String> {
  window.close().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_system_name() -> String {
  hostname::get()
    .ok()
    .and_then(|s| s.into_string().ok())
    .unwrap_or_else(|| "RIG DASHBOARD".to_string())
}

#[tauri::command]
pub fn get_cpu_info(state: tauri::State<AppState>) -> String {
  let mut system = state.system.lock().unwrap();
  system.refresh_cpu();
  let cpu0 = system.cpus().first();
  cpu0
    .map(|c| c.brand().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "--".to_string())
}

#[cfg(windows)]
#[derive(Deserialize, Debug)]
struct VideoController {
  #[serde(rename = "Name")]
  name: Option<String>,
}

#[tauri::command]
pub fn get_gpu_info() -> Option<String> {
  #[cfg(windows)]
  {
    let com = wmi::COMLibrary::new().ok()?;
    let conn = wmi::WMIConnection::new(com.into()).ok()?;
    let rows: Vec<VideoController> = conn.query().ok()?;
    return rows
      .into_iter()
      .filter_map(|r| r.name)
      .find(|n| !n.trim().is_empty());
  }

  #[cfg(not(windows))]
  {
    None
  }
}

#[tauri::command]
pub async fn get_stats(state: tauri::State<'_, AppState>) -> Result<StatsPayload, String> {
  // Fetch a fresh LHM sample, then fall back to the last successful one if needed.
  let lhm_fresh = fetch_lhm().await;
  let lhm = {
    let mut last_lhm = state.last_lhm.lock().unwrap();
    if let Some(ref sample) = lhm_fresh {
      *last_lhm = Some(sample.clone());
    }
    (*last_lhm).clone()
  };

  // sysinfo values are refreshed each tick from this shared System instance.
  let mut system = state.system.lock().unwrap();
  system.refresh_cpu();
  system.refresh_memory();

  let total = system.total_memory();
  let used = system.used_memory();
  let free = system.free_memory();

  let avg_load = if system.cpus().is_empty() {
    0
  } else {
    let sum: f32 = system.cpus().iter().map(|c| c.cpu_usage()).sum();
    (sum / system.cpus().len() as f32).round() as u8
  };
  let cores: Vec<u8> = system
    .cpus()
    .iter()
    .map(|c| c.cpu_usage().round() as u8)
    .collect();
  let freq = system
    .cpus()
    .first()
    .map(|c| c.frequency() as f64 / 1000.0)
    .unwrap_or(0.0);
  drop(system);

  let mut disks = state.disks.lock().unwrap();
  disks.refresh();
  let mut drives = Vec::new();
  for d in disks.iter() {
    let total_space = d.total_space();
    if total_space <= 1_000_000_000 {
      continue;
    }
    let available = d.available_space();
    let used_space = total_space.saturating_sub(available);
    let pct = if total_space == 0 {
      0
    } else {
      ((used_space as f64 / total_space as f64) * 100.0).round() as u8
    };
    drives.push(DiskDrive {
      fs: d.mount_point().to_string_lossy().to_string(),
      size: total_space,
      used: used_space,
      pct,
    });
  }
  drop(disks);

  // Network throughput is computed from deltas over elapsed time between samples.
  let mut networks = state.networks.lock().unwrap();
  let mut last_sample = state.last_net_sample.lock().unwrap();
  let now = Instant::now();
  let elapsed = last_sample
    .map(|t| now.duration_since(t).as_secs_f64())
    .unwrap_or(1.0)
    .max(0.5);
  *last_sample = Some(now);

  networks.refresh();
  let mut best_iface = "--".to_string();
  let mut best_up = 0.0;
  let mut best_down = 0.0;
  for (name, data) in networks.iter() {
    let up_mbps = (data.transmitted() as f64 * 8.0 / 1_000_000.0) / elapsed;
    let down_mbps = (data.received() as f64 * 8.0 / 1_000_000.0) / elapsed;
    if up_mbps + down_mbps > best_up + best_down {
      best_up = up_mbps;
      best_down = down_mbps;
      best_iface = name.to_string();
    }
  }

  let (disk_read, disk_write, net_up, net_down, lhm_connected) = if let Some(ref l) = lhm {
    (l.disk_read, l.disk_write, l.net_up, l.net_down, true)
  } else {
    (0.0, 0.0, best_up, best_down, false)
  };

  Ok(StatsPayload {
    cpu: CpuStats {
      load: avg_load,
      cores,
      temp: lhm.as_ref().and_then(|l| l.cpu_temp),
      freq,
      power: lhm.as_ref().and_then(|l| l.cpu_power),
    },
    gpu: GpuStats {
      load: lhm.as_ref().and_then(|l| l.gpu_load),
      temp: lhm.as_ref().and_then(|l| l.gpu_temp),
      hotspot: lhm.as_ref().and_then(|l| l.gpu_hotspot),
      freq: lhm.as_ref().and_then(|l| l.gpu_freq),
      vram_used: lhm.as_ref().and_then(|l| l.vram_used),
      vram_total: lhm
        .as_ref()
        .and_then(|l| l.vram_total)
        .unwrap_or(16384.0),
      fan_speed: lhm.as_ref().and_then(|l| l.gpu_fan),
      power: lhm.as_ref().and_then(|l| l.gpu_power),
    },
    ram: RamStats {
      total,
      used,
      free,
      spec: "RAM".to_string(),
    },
    net: NetStats {
      up: net_up,
      down: net_down,
      iface: best_iface,
    },
    disk: DiskStats {
      read: disk_read,
      write: disk_write,
      drives,
    },
    lhm_connected,
  })
}

pub fn ensure_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
  // Keep a single settings window instance; focus existing window if already open.
  if app.get_window("settings").is_some() {
    if let Some(win) = app.get_window("settings") {
      win.set_focus().map_err(|e| e.to_string())?;
    }
    return Ok(());
  }

  let main = app.get_window("main").ok_or("Main window missing")?;
  let main_pos = main.outer_position().map_err(|e| e.to_string())?;
  let main_size = main.outer_size().map_err(|e| e.to_string())?;

  let width = 300.0;
  let height = 260.0;
  let x = main_pos.x as f64 + main_size.width as f64 - width - 16.0;
  let y = main_pos.y as f64 + 16.0;

  WindowBuilder::new(
    app,
    "settings",
    tauri::WindowUrl::App("settings.html".into()),
  )
  .title("Settings")
  .inner_size(width, height)
  .position(x, y)
  .decorations(false)
  .resizable(false)
  .always_on_top(true)
  .skip_taskbar(true)
  .build()
  .map_err(|e| e.to_string())?;

  Ok(())
}

pub fn on_window_event(event: GlobalWindowEvent) {
  let win = event.window();
  if win.label() == "main" {
    // Closing the main window hides to tray instead of terminating the process.
    if let WindowEvent::CloseRequested { api, .. } = event.event() {
      api.prevent_close();
      let _ = win.hide();
    }
  }

  if win.label() == "settings" {
    // Settings behaves like a popover: close when focus is lost.
    if let WindowEvent::Focused(false) = event.event() {
      let _ = win.close();
    }
  }
}
