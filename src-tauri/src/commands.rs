//! Tauri command handlers — the public IPC surface exposed to the renderer.
//!
//! Design notes:
//! - Commands are intentionally thin wrappers around shared state and helpers.
//! - All non-trivial logic lives in the domain modules (hardware, monitor, etc.).
// Tauri command handlers take AppHandle/State/WebviewWindow by value as required
// by the IPC dispatch mechanism; suppressing the clippy lint for this module.
#![allow(clippy::needless_pass_by_value)]
//! - Settings updates are emitted to the renderer immediately after persistence.
//! - Stats collection keeps the last successful LHM sample to avoid UI flicker
//!   when LibreHardwareMonitor is temporarily unavailable.

use crate::autostart::{is_autostart_registered_with_log, register_autostart, unregister_autostart};
use crate::debug::{append_debug_log, read_debug_log_tail};
use crate::hardware::{detect_gpu_name, detect_model_name, is_placeholder_model_name, sample_ping_ms};
use crate::lhm::fetch_lhm;
use crate::lhm_process::{
  can_reach_lhm_endpoint, get_lhm_task_details, get_lhm_task_diagnosis, track_lhm_connection_state,
};
use crate::monitor::{normalize_profile, normalize_visible_panels, pick_target_monitor, profile_dimensions};
use crate::settings::{persist_settings, Settings};
use crate::stats::{AppState, CpuStats, DiskDrive, DiskStats, GpuStats, NetStats, RamStats, StatsPayload};
use serde::Serialize;
use std::time::Instant;
use tauri::{Emitter, Manager, Size, WebviewWindow};

const GITHUB_URL: &str = "https://github.com/dvalfrid/rigstats";
const CONTACT_EMAIL: &str = "daniel@valfridsson.net";
const LICENSE_NAME: &str = "MIT";
const LHM_VERSION: &str = "v0.9.6";
const SYSINFO_VERSION: &str = "0.30";
const WMI_VERSION: &str = "0.13";

// --- About -----------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutDependency {
  pub name: String,
  pub version: String,
  pub note: Option<String>,
  pub status: String,
  pub ok: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
  pub rigstats_version: String,
  pub github_url: String,
  pub license_name: String,
  pub contact_email: String,
  pub log_path: String,
  pub log_tail: String,
  pub lhm_connected: bool,
  pub lhm_task_name: Option<String>,
  pub lhm_task_status: Option<String>,
  pub lhm_task_last_result: Option<String>,
  pub lhm_task_to_run: Option<String>,
  pub lhm_task_diagnosis: String,
  pub dependencies: Vec<AboutDependency>,
}

fn build_about_dependencies(state: &AppState) -> Vec<AboutDependency> {
  let lhm_ok = can_reach_lhm_endpoint();
  vec![
    AboutDependency {
      name: "LibreHardwareMonitor".to_string(),
      version: LHM_VERSION.to_string(),
      note: Some("GPU and sensor telemetry feed".to_string()),
      status: if lhm_ok {
        "SUCCESS".to_string()
      } else {
        "FAILED".to_string()
      },
      ok: lhm_ok,
    },
    AboutDependency {
      name: "sysinfo".to_string(),
      version: SYSINFO_VERSION.to_string(),
      note: Some("CPU, RAM, disk, network stats".to_string()),
      status: if state.sysinfo_available {
        "SUCCESS".to_string()
      } else {
        "FAILED".to_string()
      },
      ok: state.sysinfo_available,
    },
    AboutDependency {
      name: "wmi".to_string(),
      version: WMI_VERSION.to_string(),
      note: Some("Windows hardware metadata".to_string()),
      status: if state.wmi_available {
        "SUCCESS".to_string()
      } else {
        "FAILED".to_string()
      },
      ok: state.wmi_available,
    },
  ]
}

#[tauri::command]
pub fn get_about_info(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> AboutInfo {
  use crate::debug::debug_log_path;
  let log_path = debug_log_path(&app);
  let (lhm_task_name, lhm_task_status, lhm_task_last_result, lhm_task_to_run) = get_lhm_task_details(&app);

  AboutInfo {
    rigstats_version: env!("CARGO_PKG_VERSION").to_string(),
    github_url: GITHUB_URL.to_string(),
    license_name: LICENSE_NAME.to_string(),
    contact_email: CONTACT_EMAIL.to_string(),
    log_path: log_path.display().to_string(),
    log_tail: read_debug_log_tail(&app, 160),
    lhm_connected: can_reach_lhm_endpoint(),
    lhm_task_name,
    lhm_task_status,
    lhm_task_last_result,
    lhm_task_to_run,
    lhm_task_diagnosis: get_lhm_task_diagnosis(&app).to_string(),
    dependencies: build_about_dependencies(&state),
  }
}

// --- Settings --------------------------------------------------------------

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle, state: tauri::State<AppState>) -> Settings {
  let mut settings = state.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
  // Override autostart_enabled with the live registry state so the toggle
  // reflects what Windows actually sees (e.g. if the user toggled it off
  // via Windows Settings > Apps > Startup).
  settings.autostart_enabled = is_autostart_registered_with_log(|msg| append_debug_log(&app, msg));
  settings
}

#[tauri::command]
pub fn preview_opacity(app: tauri::AppHandle, value: f64) -> Result<(), String> {
  if let Some(main) = app.get_webview_window("main") {
    main.emit("apply-opacity", value).map_err(|e| e.to_string())?;
  }
  Ok(())
}

#[tauri::command]
pub fn preview_profile(app: tauri::AppHandle, profile: String) -> Result<(), String> {
  let applied_profile = normalize_profile(&profile);
  let (target_w, target_h) = profile_dimensions(&applied_profile);
  if let Some(main) = app.get_webview_window("main") {
    let _ = main.set_fullscreen(false);
    let _ = main.set_decorations(false);
    let _ = main.set_size(Size::Physical(tauri::PhysicalSize {
      width: target_w,
      height: target_h,
    }));
    main.emit("apply-profile", applied_profile).map_err(|e| e.to_string())?;
  }
  Ok(())
}

#[tauri::command]
pub fn preview_visible_panels(app: tauri::AppHandle, panels: Vec<String>) -> Result<(), String> {
  if let Some(main) = app.get_webview_window("main") {
    let normalized = normalize_visible_panels(panels);
    main
      .emit("apply-visible-panels", normalized)
      .map_err(|e| e.to_string())?;
  }
  Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn save_settings(
  app: tauri::AppHandle,
  state: tauri::State<AppState>,
  opacity: f64,
  model_name: Option<String>,
  #[allow(non_snake_case)] modelName: Option<String>,
  dashboard_profile: Option<String>,
  #[allow(non_snake_case)] dashboardProfile: Option<String>,
  always_on_top: Option<bool>,
  #[allow(non_snake_case)] alwaysOnTop: Option<bool>,
  visible_panels: Option<Vec<String>>,
  #[allow(non_snake_case)] visiblePanels: Option<Vec<String>>,
  autostart_enabled: Option<bool>,
  #[allow(non_snake_case)] autostartEnabled: Option<bool>,
) -> Result<(), String> {
  let mut settings = state.settings.lock().unwrap_or_else(|e| e.into_inner());
  settings.opacity = opacity.clamp(0.0, 1.0);

  // Accept both snake_case and camelCase payload keys from the renderer.
  // If the name is empty or a known placeholder, auto-detect immediately.
  let incoming_name = model_name.or(modelName).unwrap_or_else(|| settings.model_name.clone());
  settings.model_name = if incoming_name.trim().is_empty() || is_placeholder_model_name(incoming_name.trim()) {
    detect_model_name().unwrap_or(incoming_name)
  } else {
    incoming_name
  };

  let requested_profile = dashboard_profile
    .or(dashboardProfile)
    .unwrap_or_else(|| settings.dashboard_profile.clone());
  let applied_profile = normalize_profile(&requested_profile);

  let applied_always_on_top = always_on_top.or(alwaysOnTop).unwrap_or(settings.always_on_top);

  let requested_visible_panels = visible_panels
    .or(visiblePanels)
    .unwrap_or_else(|| settings.visible_panels.clone());
  let applied_visible_panels = normalize_visible_panels(requested_visible_panels);

  let applied_autostart = autostart_enabled
    .or(autostartEnabled)
    .unwrap_or(settings.autostart_enabled);

  if let Some(main) = app.get_webview_window("main") {
    let _ = pick_target_monitor(&main, &applied_profile);
    main
      .set_always_on_top(applied_always_on_top)
      .map_err(|e| e.to_string())?;
  }

  settings.dashboard_profile = applied_profile.clone();
  settings.always_on_top = applied_always_on_top;
  settings.visible_panels = applied_visible_panels.clone();
  settings.autostart_enabled = applied_autostart;
  persist_settings(&app, &settings)?;

  // Apply autostart after settings are persisted so the preference is always saved
  // even if the registry operation encounters a transient error.
  if applied_autostart {
    if let Err(e) = register_autostart() {
      append_debug_log(&app, &format!("autostart: register failed: {e}"));
      return Err(format!("Settings saved but autostart could not be enabled: {e}"));
    }
    append_debug_log(&app, "autostart: registered");
  } else {
    if let Err(e) = unregister_autostart() {
      append_debug_log(&app, &format!("autostart: unregister failed: {e}"));
      return Err(format!("Settings saved but autostart could not be disabled: {e}"));
    }
    append_debug_log(&app, "autostart: unregistered");
  }

  if let Some(main) = app.get_webview_window("main") {
    main
      .emit("apply-opacity", settings.opacity)
      .map_err(|e| e.to_string())?;
    main
      .emit("apply-model-name", settings.model_name.clone())
      .map_err(|e| e.to_string())?;
    main.emit("apply-profile", applied_profile).map_err(|e| e.to_string())?;
    main
      .emit("apply-visible-panels", applied_visible_panels)
      .map_err(|e| e.to_string())?;
  }

  Ok(())
}

// --- Window utilities ------------------------------------------------------

#[tauri::command]
pub fn close_window(window: WebviewWindow) -> Result<(), String> {
  window.close().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_window_drag(window: WebviewWindow) -> Result<(), String> {
  window.start_dragging().map_err(|e| e.to_string())
}

// --- System info -----------------------------------------------------------

#[tauri::command]
pub fn get_system_name() -> String {
  hostname::get()
    .ok()
    .and_then(|s| s.into_string().ok())
    .unwrap_or_else(|| "RIG DASHBOARD".to_string())
}

#[tauri::command]
pub fn get_system_brand(state: tauri::State<AppState>) -> String {
  state.system_brand.clone()
}

#[tauri::command]
pub fn get_cpu_info(state: tauri::State<AppState>) -> String {
  let mut system = state.system.lock().unwrap_or_else(|e| e.into_inner());
  system.refresh_cpu();
  system
    .cpus()
    .first()
    .map(|c| c.brand().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "--".to_string())
}

#[tauri::command]
pub fn get_gpu_info() -> Option<String> {
  detect_gpu_name()
}

// --- Stats -----------------------------------------------------------------

#[tauri::command]
pub async fn get_stats(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<StatsPayload, String> {
  // Fetch a fresh LHM sample, then fall back to the last successful one if needed.
  let lhm_fresh = fetch_lhm(&state.lhm_client).await;
  let lhm = {
    let mut last_lhm = state.last_lhm.lock().unwrap_or_else(|e| {
      append_debug_log(&app, "stats: last_lhm mutex poisoned; recovering guard");
      e.into_inner()
    });
    if let Some(ref sample) = lhm_fresh {
      *last_lhm = Some(sample.clone());
    }
    (*last_lhm).clone()
  };

  let mut system = state.system.lock().unwrap_or_else(|e| {
    append_debug_log(&app, "stats: system mutex poisoned; recovering guard");
    e.into_inner()
  });
  system.refresh_cpu();
  system.refresh_memory();
  let system_uptime_secs = sysinfo::System::uptime();

  let total = system.total_memory();
  let used = system.used_memory();
  let free = system.free_memory();

  let avg_load = if system.cpus().is_empty() {
    0
  } else {
    let sum: f32 = system.cpus().iter().map(|c| c.cpu_usage()).sum();
    (sum / system.cpus().len() as f32).round() as u8
  };
  let cores: Vec<u8> = system.cpus().iter().map(|c| c.cpu_usage().round() as u8).collect();
  let freq = system
    .cpus()
    .first()
    .map(|c| c.frequency() as f64 / 1000.0)
    .unwrap_or(0.0);
  drop(system);

  let mut disks = state.disks.lock().unwrap_or_else(|e| {
    append_debug_log(&app, "stats: disks mutex poisoned; recovering guard");
    e.into_inner()
  });
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
  let mut networks = state.networks.lock().unwrap_or_else(|e| {
    append_debug_log(&app, "stats: networks mutex poisoned; recovering guard");
    e.into_inner()
  });
  let mut last_sample = state.last_net_sample.lock().unwrap_or_else(|e| {
    append_debug_log(&app, "stats: last_net_sample mutex poisoned; recovering guard");
    e.into_inner()
  });
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

  let ping_ms = {
    let mut cache = state.last_ping_sample.lock().unwrap_or_else(|e| {
      append_debug_log(&app, "stats: last_ping_sample mutex poisoned; recovering guard");
      e.into_inner()
    });
    let should_refresh = cache
      .as_ref()
      .map(|(t, _)| now.duration_since(*t).as_secs_f64() >= 5.0)
      .unwrap_or(true);

    if should_refresh {
      let measured = sample_ping_ms(&state.ping_target);
      *cache = Some((now, measured));
      measured
    } else {
      cache.as_ref().map(|(_, value)| *value).unwrap_or(None)
    }
  };

  let (disk_read, disk_write, net_up, net_down, lhm_connected) = if let Some(ref l) = lhm {
    (l.disk_read, l.disk_write, l.net_up, l.net_down, true)
  } else {
    (0.0, 0.0, best_up, best_down, false)
  };

  track_lhm_connection_state(&app, lhm_connected);

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
        .unwrap_or(state.gpu_vram_total_mb),
      fan_speed: lhm.as_ref().and_then(|l| l.gpu_fan),
      power: lhm.as_ref().and_then(|l| l.gpu_power),
    },
    ram: RamStats {
      total,
      used,
      free,
      spec: state.ram_spec.clone(),
      details: state.ram_details.clone(),
    },
    net: NetStats {
      up: net_up,
      down: net_down,
      iface: best_iface,
      ping_ms,
    },
    disk: DiskStats {
      read: disk_read,
      write: disk_write,
      drives,
    },
    system_uptime_secs,
    lhm_connected,
  })
}

// --- Changelog -------------------------------------------------------------

#[tauri::command]
pub fn get_changelog(app: tauri::AppHandle) -> String {
  use tauri::Manager;
  app
    .path()
    .resolve("CHANGELOG.md", tauri::path::BaseDirectory::Resource)
    .ok()
    .and_then(|p| std::fs::read_to_string(p).ok())
    .unwrap_or_default()
}

// --- Logging ---------------------------------------------------------------

/// Receives error reports from the renderer and writes them to the debug log
/// so they are visible in the Status dialog without opening DevTools.
#[tauri::command]
pub fn log_frontend_error(app: tauri::AppHandle, message: String) {
  let sanitized = message.chars().take(512).collect::<String>();
  append_debug_log(&app, &format!("[renderer] {}", sanitized));
}
