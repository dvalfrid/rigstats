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
use std::sync::atomic::{AtomicBool, Ordering};

/// Set to `true` by `notify_app_ready` when the frontend has finished
/// initialising. The startup watchdog in `main.rs` uses this flag to detect
/// WebView2 failures (common at Windows boot) and reload the webview.
pub static APP_READY: AtomicBool = AtomicBool::new(false);
use crate::hardware::{detect_gpu_name, detect_model_name, is_placeholder_model_name, sample_ping_ms};
use crate::lhm::fetch_lhm;
use crate::lhm_process::{
  can_reach_lhm_endpoint, get_lhm_task_details, get_lhm_task_diagnosis, track_lhm_connection_state,
};
use crate::monitor::{normalize_profile, normalize_visible_panels, pick_target_monitor, profile_dimensions};
use crate::settings::{persist_settings, ComponentThresholds, PanelLayout, Settings};
use crate::stats::{
  AppState, CpuStats, DiskDrive, DiskStats, GpuStats, HardwareInfo, MotherboardStats, NetStats, ProcessEntry, RamStats,
  StatsPayload,
};
use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager, Size, WebviewWindow};
use tauri_plugin_notification::NotificationExt;

const GITHUB_URL: &str = "https://github.com/dvalfrid/rigstats";
const CONTACT_EMAIL: &str = "daniel@valfridsson.net";
const LICENSE_NAME: &str = "MIT";
const LHM_VERSION: &str = "v0.9.6";
const SYSINFO_VERSION: &str = "0.30";
const WMI_VERSION: &str = "0.13";

// --- Startup readiness -----------------------------------------------------

/// Called by the renderer once the frontend has successfully initialised.
/// Clears the startup watchdog — if this is never called (e.g. because
/// WebView2 failed to load the page at boot), the watchdog will reload
/// the webview after its timeout to recover automatically.
#[tauri::command]
pub fn notify_app_ready() {
  APP_READY.store(true, Ordering::Relaxed);
}

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

fn build_about_dependencies(hw: &HardwareInfo) -> Vec<AboutDependency> {
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
      status: if hw.sysinfo_available {
        "SUCCESS".to_string()
      } else {
        "FAILED".to_string()
      },
      ok: hw.sysinfo_available,
    },
    AboutDependency {
      name: "wmi".to_string(),
      version: WMI_VERSION.to_string(),
      note: Some("Windows hardware metadata".to_string()),
      status: if hw.wmi_available {
        "SUCCESS".to_string()
      } else {
        "FAILED".to_string()
      },
      ok: hw.wmi_available,
    },
  ]
}

#[tauri::command]
pub fn get_about_info(app: tauri::AppHandle, hw: tauri::State<'_, HardwareInfo>) -> AboutInfo {
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
    dependencies: build_about_dependencies(&hw),
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
pub fn preview_profile(app: tauri::AppHandle, state: tauri::State<AppState>, profile: String) -> Result<(), String> {
  let applied_profile = normalize_profile(&profile);
  let (target_w, target_h) = profile_dimensions(&applied_profile);

  {
    let mut settings = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    settings.dashboard_profile = applied_profile.clone();
  }

  if let Some(main) = app.get_webview_window("main") {
    let _ = main.set_size(Size::Physical(tauri::PhysicalSize {
      width: target_w,
      height: target_h,
    }));
    // set_decorations must come after set_size: on Windows, SetWindowPos can
    // restore the WS_CAPTION/WS_THICKFRAME styles, so we always enforce it last.
    let _ = main.set_decorations(false);
    main.emit("apply-profile", applied_profile).map_err(|e| e.to_string())?;
  }

  let floating_mode = {
    let settings = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    settings.floating_mode
  };
  if floating_mode {
    crate::windows::sync_floating_panels(&app, &state);
  }

  Ok(())
}

#[tauri::command]
pub fn preview_visible_panels(
  app: tauri::AppHandle,
  state: tauri::State<AppState>,
  panels: Vec<String>,
) -> Result<(), String> {
  let normalized = normalize_visible_panels(panels);

  if let Some(main) = app.get_webview_window("main") {
    main
      .emit("apply-visible-panels", normalized.clone())
      .map_err(|e| e.to_string())?;
  }

  // In floating mode, preview must also open/close floating panel windows.
  let floating_mode = {
    let mut s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    if s.floating_mode {
      if s.visible_panels == normalized {
        false
      } else {
        s.visible_panels = normalized;
        true
      }
    } else {
      false
    }
  };

  if floating_mode {
    crate::windows::sync_floating_panels(&app, &state);
  }

  Ok(())
}

#[tauri::command]
pub fn set_main_height(app: tauri::AppHandle, width: f64, height: f64) -> Result<(), String> {
  if let Some(main) = app.get_webview_window("main") {
    let _ = main.set_size(Size::Logical(tauri::LogicalSize { width, height }));
    // Enforce no decorations after every resize — Windows may restore WS_CAPTION
    // via SetWindowPos when the window size changes.
    let _ = main.set_decorations(false);
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
  thresholds: Option<std::collections::HashMap<String, ComponentThresholds>>,
  #[allow(non_snake_case)] alertCooldownSecs: Option<u64>,
  #[allow(non_snake_case)] notifyOnWarn: Option<bool>,
  #[allow(non_snake_case)] notifyOnCrit: Option<bool>,
  theme: Option<String>,
  floating_mode: Option<bool>,
  #[allow(non_snake_case)] floatingMode: Option<bool>,
) -> Result<(), String> {
  let mut settings = state.settings.lock().unwrap_or_else(|e| e.into_inner());
  let previous_floating_mode = settings.floating_mode;
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
    // Only reposition/resize when the profile actually changes. Calling
    // pick_target_monitor unconditionally re-applies set_position on every Save,
    // which causes a ~3 px shift because the monitor's reported physical origin
    // differs slightly from where Windows placed the window at startup.
    if applied_profile != settings.dashboard_profile {
      let _ = pick_target_monitor(&main, &applied_profile);
    }
    main
      .set_always_on_top(applied_always_on_top)
      .map_err(|e| e.to_string())?;
  }

  settings.dashboard_profile = applied_profile.clone();
  settings.always_on_top = applied_always_on_top;
  settings.visible_panels = applied_visible_panels.clone();
  settings.autostart_enabled = applied_autostart;

  // Temperature alert thresholds: validate all pairs before writing any.
  // Reject any component where warning >= critical — both are only stored
  // when they are internally consistent.
  if let Some(ref t) = thresholds {
    for (component, thresh) in t {
      if let (Some(w), Some(c)) = (thresh.warn, thresh.crit) {
        if w >= c {
          return Err(format!(
            "{component}: warning threshold ({w}°C) must be below critical ({c}°C)"
          ));
        }
      }
    }
    settings.thresholds = t.clone();
  }
  if let Some(secs) = alertCooldownSecs {
    settings.alert_cooldown_secs = secs.max(60);
  }
  if let Some(v) = notifyOnWarn {
    settings.notify_on_warn = v;
  }
  if let Some(v) = notifyOnCrit {
    settings.notify_on_crit = v;
  }
  if let Some(t) = theme {
    settings.theme = t;
  }
  if let Some(fm) = floating_mode.or(floatingMode) {
    settings.floating_mode = fm;
  }

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

  // Capture floating mode before releasing the lock.
  let new_floating_mode = settings.floating_mode;

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
    main
      .emit("apply-thresholds", TempThresholdPayload::from(&*settings))
      .map_err(|e| e.to_string())?;
    main
      .emit("apply-theme", settings.theme.clone())
      .map_err(|e| e.to_string())?;
    // Notify main window JS so it updates floatingMode and starts/stops broadcasting.
    let _ = main.emit("apply-floating-mode", new_floating_mode);
  }

  // Release the settings lock before panel window operations — `launch_floating_panels`
  // acquires the same lock internally, so holding it here would deadlock.
  drop(settings);

  // If floating mode changed, launch or close the panel windows.
  if new_floating_mode != previous_floating_mode {
    if new_floating_mode {
      if let Some(main) = app.get_webview_window("main") {
        let _ = main.hide();
      }
      crate::windows::sync_floating_panels(&app, &state);
    } else {
      crate::windows::close_floating_panels(&app);
      if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
      }
    }
  } else if new_floating_mode {
    // Floating mode remained enabled and visible panels/profile may have changed.
    crate::windows::sync_floating_panels(&app, &state);
  }

  Ok(())
}

#[tauri::command]
pub fn preview_theme(app: tauri::AppHandle, theme: String) -> Result<(), String> {
  if let Some(main) = app.get_webview_window("main") {
    main.emit("apply-theme", theme).map_err(|e| e.to_string())?;
  }
  Ok(())
}

/// Snapshot of temperature alert thresholds emitted to the renderer after
/// `save_settings` so panel colours update immediately without a reload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempThresholdPayload {
  pub thresholds: std::collections::HashMap<String, ComponentThresholds>,
  pub alert_cooldown_secs: u64,
}

impl From<&Settings> for TempThresholdPayload {
  fn from(s: &Settings) -> Self {
    Self {
      thresholds: s.thresholds.clone(),
      alert_cooldown_secs: s.alert_cooldown_secs,
    }
  }
}

// --- Alerts ----------------------------------------------------------------

#[tauri::command]
pub fn test_temp_alert(app: tauri::AppHandle) -> Result<(), String> {
  app
    .notification()
    .builder()
    .title("RIGStats — Test Notification")
    .body("Temperature alerts are working correctly.")
    .show()
    .map_err(|e| e.to_string())
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

#[tauri::command]
pub fn open_settings_window(app: tauri::AppHandle) -> Result<(), String> {
  crate::windows::ensure_settings_window(&app)
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
pub fn get_system_brand(hw: tauri::State<HardwareInfo>) -> String {
  hw.system_brand.lock().unwrap_or_else(|e| e.into_inner()).clone()
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

/// A threshold breach detected by `pending_alerts`. Carries enough information
/// for `fire_alert_if_due` to send the notification; cooldown is checked there.
struct PendingAlert {
  component: &'static str,
  level: &'static str,
  temp: f64,
  threshold: u8,
}

/// Pushes a `PendingAlert` when `enabled`, `threshold` is set, and `temp`
/// meets or exceeds it.
fn push_if_breached(
  out: &mut Vec<PendingAlert>,
  enabled: bool,
  component: &'static str,
  level: &'static str,
  temp: f64,
  threshold: Option<u8>,
) {
  if enabled {
    if let Some(t) = threshold {
      if temp >= t as f64 {
        out.push(PendingAlert {
          component,
          level,
          temp,
          threshold: t,
        });
      }
    }
  }
}

/// Pure function: returns all threshold breaches for the given temperature
/// readings and settings. Does not touch cooldown state or fire notifications,
/// making it fully unit-testable without a Tauri context.
/// `max_disk_temp` is the pre-computed maximum temperature across all drives.
fn pending_alerts(
  cpu_temp: Option<f64>,
  gpu_temp: Option<f64>,
  ram_temp: Option<f64>,
  max_disk_temp: Option<f64>,
  settings: &crate::settings::Settings,
) -> Vec<PendingAlert> {
  let readings: &[(&str, &str, Option<f64>)] = &[
    ("cpu", "CPU", cpu_temp),
    ("gpu", "GPU", gpu_temp),
    ("ram", "RAM", ram_temp),
    ("disk", "Disk", max_disk_temp),
  ];
  let mut out = Vec::new();
  for &(key, label, temp_opt) in readings {
    if let Some(temp) = temp_opt {
      if let Some(thresh) = settings.thresholds.get(key) {
        push_if_breached(&mut out, settings.notify_on_warn, label, "WARNING", temp, thresh.warn);
        push_if_breached(&mut out, settings.notify_on_crit, label, "CRITICAL", temp, thresh.crit);
      }
    }
  }
  out
}

/// Fires a tray notification for a temperature threshold breach, subject to a
/// per-component+level cooldown. The cooldown key is derived from `component`
/// and `level`. Notification errors are written to the debug log so they are
/// visible in the Status window but never propagate to the stats tick.
fn fire_alert_if_due(
  app: &tauri::AppHandle,
  last_alert: &mut std::collections::HashMap<String, Instant>,
  component: &str,
  level: &str,
  temp: f64,
  threshold: u8,
  cooldown_secs: u64,
) {
  let key = format!("{}_{}", component.to_lowercase(), level.to_lowercase());
  let cooldown = Duration::from_secs(cooldown_secs);
  let now = Instant::now();
  let due = match last_alert.get(&key) {
    None => true,
    Some(&last) => now.duration_since(last) >= cooldown,
  };
  if due {
    let title = format!("{} Temp {} — {}°C", component, level, temp.round() as u8);
    let body = format!("Threshold: {}°C", threshold);
    if let Err(e) = app.notification().builder().title(&title).body(&body).show() {
      append_debug_log(
        app,
        &format!("notification: failed to show alert ({component} {level}): {e}"),
      );
    }
    last_alert.insert(key, now);
  }
}

/// Checks all temperature readings in `payload` against the configured thresholds
/// and fires notifications as needed. Warning and critical are independent — both
/// can fire with their own cooldown clocks. The `notify_on_warn` / `notify_on_crit`
/// flags let users suppress a whole alert level without clearing their thresholds.
fn check_temp_alerts(
  app: &tauri::AppHandle,
  payload: &StatsPayload,
  settings: &crate::settings::Settings,
  last_alert: &mut std::collections::HashMap<String, Instant>,
  cooldown_secs: u64,
) {
  // Disk: alert on the hottest drive only — per-drive alerting is not supported.
  let max_disk = payload
    .disk
    .drives
    .iter()
    .filter_map(|d| d.temp)
    .fold(f64::NEG_INFINITY, f64::max);
  let max_disk_temp = (max_disk > f64::NEG_INFINITY).then_some(max_disk);

  for alert in pending_alerts(
    payload.cpu.temp,
    payload.gpu.temp,
    payload.ram.temp,
    max_disk_temp,
    settings,
  ) {
    fire_alert_if_due(
      app,
      last_alert,
      alert.component,
      alert.level,
      alert.temp,
      alert.threshold,
      cooldown_secs,
    );
  }
}

#[cfg(test)]
mod alert_tests {
  use super::*;
  use crate::settings::Settings;

  /// Builds a minimal `Settings` with the same warn/crit applied to all four
  /// components. Keeps tests concise while covering all code paths.
  fn settings_with(warn: Option<u8>, crit: Option<u8>, notify_warn: bool, notify_crit: bool) -> Settings {
    use crate::settings::ComponentThresholds;
    let mut s = Settings::default();
    s.thresholds = ["cpu", "gpu", "ram", "disk"]
      .iter()
      .map(|&k| (k.to_string(), ComponentThresholds { warn, crit }))
      .collect();
    s.notify_on_warn = notify_warn;
    s.notify_on_crit = notify_crit;
    s
  }

  #[test]
  fn no_temp_produces_no_alerts() {
    let s = settings_with(Some(80), Some(90), true, true);
    assert!(pending_alerts(None, None, None, None, &s).is_empty());
  }

  #[test]
  fn temp_below_warn_produces_no_alerts() {
    let s = settings_with(Some(80), Some(90), true, true);
    let alerts = pending_alerts(Some(79.9), Some(79.9), Some(79.9), Some(79.9), &s);
    assert!(alerts.is_empty());
  }

  #[test]
  fn temp_at_warn_threshold_fires_warning() {
    let s = settings_with(Some(80), Some(90), true, true);
    let alerts = pending_alerts(Some(80.0), None, None, None, &s);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].component, "CPU");
    assert_eq!(alerts[0].level, "WARNING");
    assert_eq!(alerts[0].threshold, 80);
  }

  #[test]
  fn temp_above_crit_fires_both_levels() {
    let s = settings_with(Some(80), Some(90), true, true);
    let alerts = pending_alerts(Some(95.0), None, None, None, &s);
    assert_eq!(alerts.len(), 2);
    let levels: Vec<_> = alerts.iter().map(|a| a.level).collect();
    assert!(levels.contains(&"WARNING"));
    assert!(levels.contains(&"CRITICAL"));
  }

  #[test]
  fn notify_warn_disabled_suppresses_warning_only() {
    let s = settings_with(Some(80), Some(90), false, true);
    // Above warn but below crit — nothing fires.
    assert!(pending_alerts(Some(85.0), None, None, None, &s).is_empty());
    // Above crit — only critical fires.
    let alerts = pending_alerts(Some(95.0), None, None, None, &s);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].level, "CRITICAL");
  }

  #[test]
  fn notify_crit_disabled_suppresses_critical_only() {
    let s = settings_with(Some(80), Some(90), true, false);
    let alerts = pending_alerts(Some(95.0), None, None, None, &s);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].level, "WARNING");
  }

  #[test]
  fn thresholds_none_produces_no_alerts() {
    let s = settings_with(None, None, true, true);
    assert!(pending_alerts(Some(999.0), Some(999.0), Some(999.0), Some(999.0), &s).is_empty());
  }

  #[test]
  fn all_four_components_fire_independently() {
    let s = settings_with(Some(80), Some(90), true, true);
    // Each component at 85 °C: above warn (80), below crit (90) → one WARNING each.
    let alerts = pending_alerts(Some(85.0), Some(85.0), Some(85.0), Some(85.0), &s);
    assert_eq!(alerts.len(), 4);
    let components: Vec<_> = alerts.iter().map(|a| a.component).collect();
    assert!(components.contains(&"CPU"));
    assert!(components.contains(&"GPU"));
    assert!(components.contains(&"RAM"));
    assert!(components.contains(&"Disk"));
    assert!(alerts.iter().all(|a| a.level == "WARNING"));
  }

  #[test]
  fn disk_temp_none_produces_no_alert() {
    let s = settings_with(Some(55), Some(70), true, true);
    assert!(pending_alerts(None, None, None, None, &s).is_empty());
  }

  #[test]
  fn both_notify_disabled_produces_no_alerts() {
    let s = settings_with(Some(80), Some(90), false, false);
    assert!(pending_alerts(Some(999.0), Some(999.0), Some(999.0), Some(999.0), &s).is_empty());
  }
}

/// Finds the LHM temperature entry whose device name best matches `wmi_model`.
/// Matching is case-insensitive and uses substring containment so minor
/// differences in suffix (e.g. " NVMe", extra whitespace) are tolerated.
fn lhm_temp_for_model(wmi_model: &str, disk_temps: &[(String, f64)]) -> Option<f64> {
  let needle = wmi_model.trim().to_lowercase();
  if needle.is_empty() {
    return None;
  }
  disk_temps.iter().find_map(|(lhm_name, temp)| {
    let haystack = lhm_name.trim().to_lowercase();
    if haystack == needle || haystack.contains(&needle) || needle.contains(&haystack) {
      Some(*temp)
    } else {
      None
    }
  })
}

/// Sorts process entries by descending CPU usage and truncates to `limit`.
/// `NaN` values are treated as equal to preserve deterministic, non-panicking
/// behavior when platform process counters return invalid samples.
fn normalize_top_processes(mut entries: Vec<ProcessEntry>, limit: usize) -> Vec<ProcessEntry> {
  entries.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
  entries.truncate(limit);
  entries
}

#[tauri::command]
pub async fn get_stats(
  app: tauri::AppHandle,
  state: tauri::State<'_, AppState>,
  hw: tauri::State<'_, HardwareInfo>,
) -> Result<StatsPayload, String> {
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

  system.refresh_processes();
  let num_cpus = system.cpus().len().max(1) as f32;
  let proc_list_raw: Vec<ProcessEntry> = system
    .processes()
    .values()
    .map(|p| ProcessEntry {
      name: p.name().to_string(),
      cpu: p.cpu_usage() / num_cpus,
      mem_mb: p.memory() / 1_048_576,
    })
    .collect();
  let proc_list = normalize_top_processes(proc_list_raw, 8);

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
      temp: None, // filled in below once LHM data is available
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
      let measured = sample_ping_ms(&hw.ping_target);
      *cache = Some((now, measured));
      measured
    } else {
      cache.as_ref().map(|(_, value)| *value).unwrap_or(None)
    }
  };

  // Network is always sourced from sysinfo — it reads the same OS counters as
  // Task Manager and reliably tracks the active interface by traffic volume.
  // LHM's network sensors track adapters by GUID and can latch onto the wrong
  // interface (VPNs, Hyper-V bridges, etc.), producing near-zero readings.
  let (disk_read, disk_write, lhm_connected) = if let Some(ref l) = lhm {
    (l.disk_read, l.disk_write, true)
  } else {
    (0.0, 0.0, false)
  };
  let (net_up, net_down) = (best_up, best_down);

  track_lhm_connection_state(&app, lhm_connected);

  let payload = StatsPayload {
    cpu: CpuStats {
      load: avg_load,
      cores,
      temp: lhm.as_ref().and_then(|l| l.cpu_temp),
      freq,
      power: lhm.as_ref().and_then(|l| l.cpu_power),
    },
    gpu: GpuStats {
      name: lhm.as_ref().and_then(|l| l.gpu_name.clone()),
      load: lhm.as_ref().and_then(|l| l.gpu_load),
      temp: lhm.as_ref().and_then(|l| l.gpu_temp),
      hotspot: lhm.as_ref().and_then(|l| l.gpu_hotspot),
      freq: lhm.as_ref().and_then(|l| l.gpu_freq),
      mem_freq: lhm.as_ref().and_then(|l| l.gpu_mem_freq),
      vram_used: lhm.as_ref().and_then(|l| l.vram_used),
      vram_total: lhm
        .as_ref()
        .and_then(|l| l.vram_total)
        .or(*hw.gpu_vram_total_mb.lock().unwrap_or_else(|e| e.into_inner())),
      fan_speed: lhm.as_ref().and_then(|l| l.gpu_fan),
      power: lhm.as_ref().and_then(|l| l.gpu_power),
      d3d_3d: lhm.as_ref().and_then(|l| l.gpu_d3d_3d),
      d3d_vdec: lhm.as_ref().and_then(|l| l.gpu_d3d_vdec),
    },
    ram: RamStats {
      total,
      used,
      free,
      spec: hw.ram_spec.lock().unwrap_or_else(|e| e.into_inner()).clone(),
      details: hw.ram_details.lock().unwrap_or_else(|e| e.into_inner()).clone(),
      temp: lhm.as_ref().and_then(|l| l.ram_temp),
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
      drives: {
        // Match LHM temperatures to sysinfo volumes by physical disk model name.
        // This is robust against drive-letter reordering and USB drives appearing
        // in the sysinfo list without a corresponding LHM temperature entry.
        if let Some(ref l) = lhm {
          let disk_model_map = hw.disk_model_map.lock().unwrap_or_else(|e| e.into_inner());
          for (i, drive) in drives.iter_mut().enumerate() {
            let drive_key = drive.fs.trim_end_matches(['\\', '/']).to_string();
            if let Some(wmi_model) = disk_model_map.get(&drive_key) {
              drive.temp = lhm_temp_for_model(wmi_model, &l.disk_temps);
            }
            // Fallback: WMI has no record for this drive letter (map empty or drive
            // absent). Assign by position so temperatures still surface when the
            // WMI association query fails.
            if drive.temp.is_none() && !disk_model_map.contains_key(&drive_key) {
              drive.temp = l.disk_temps.get(i).map(|(_, t)| *t);
            }
          }
        }
        drives
      },
    },
    motherboard: MotherboardStats {
      fans: lhm.as_ref().map(|l| l.mb_fans.clone()).unwrap_or_default(),
      temps: lhm.as_ref().map(|l| l.mb_temps.clone()).unwrap_or_default(),
      voltages: lhm.as_ref().map(|l| l.mb_voltages.clone()).unwrap_or_default(),
      chip: lhm.as_ref().and_then(|l| l.mb_chip.clone()),
      board: hw.mb_name.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    },
    top_processes: proc_list,
    system_uptime_secs,
    lhm_connected,
  };

  // Check temperature thresholds and fire tray notifications as needed.
  {
    let settings_snap = state.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut alert_map = state.last_alert.lock().unwrap_or_else(|e| {
      append_debug_log(&app, "stats: last_alert mutex poisoned; recovering guard");
      e.into_inner()
    });
    check_temp_alerts(
      &app,
      &payload,
      &settings_snap,
      &mut alert_map,
      settings_snap.alert_cooldown_secs,
    );
  }

  Ok(payload)
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

// --- Floating panel mode ---------------------------------------------------

/// Switches between portrait and floating panel layout.
///
/// When enabled, the main window is hidden and one frameless window per visible
/// panel is opened. When disabled, all panel windows are closed and the main
/// portrait window is shown again.
#[tauri::command]
pub fn toggle_floating_mode(app: tauri::AppHandle, state: tauri::State<AppState>, enabled: bool) -> Result<(), String> {
  {
    let mut s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    s.floating_mode = enabled;
    // Self-heal older or malformed settings where visible_panels may be empty.
    s.visible_panels = normalize_visible_panels(s.visible_panels.clone());
    persist_settings(&app, &s)?;
  }
  if enabled {
    if let Some(main) = app.get_webview_window("main") {
      let _ = main.hide();
    }
    crate::windows::sync_floating_panels(&app, &state);
  } else {
    crate::windows::close_floating_panels(&app);
    if let Some(main) = app.get_webview_window("main") {
      let _ = main.show();
      let _ = main.set_focus();
    }
  }
  // Notify the main window JS so it updates floatingMode and starts/stops broadcasting.
  if let Some(main) = app.get_webview_window("main") {
    let _ = main.emit("apply-floating-mode", enabled);
  }
  Ok(())
}

/// Broadcasts a stats payload to all open floating panel windows.
///
/// Called by the main window's tick loop when floating mode is active so that
/// only one `get-stats` IPC round-trip runs per second regardless of how many
/// panels are open. The payload is received as a raw JSON value so that
/// `StatsPayload` does not need to implement `Deserialize`.
#[tauri::command]
pub fn broadcast_stats(app: tauri::AppHandle, stats: serde_json::Value) -> Result<(), String> {
  // Emit only to open panel windows — avoids delivering every stats tick to the
  // settings, about, status, and updater windows which never consume it.
  for key in crate::windows::all_panel_keys() {
    let label = format!("panel-{}", key);
    if let Some(win) = app.get_webview_window(&label) {
      win.emit("stats-broadcast", &stats).map_err(|e| e.to_string())?;
    }
  }
  Ok(())
}

/// Merges incoming panel positions into persistent settings.
///
/// Called by `panel-host.js` after the user stops dragging a panel so positions
/// survive across restarts. Incoming positions are the raw `outer_position()`
/// values — the DWM inset compensation is re-applied by `launch_floating_panels`
/// on the next startup.
#[tauri::command]
pub fn save_panel_positions(
  app: tauri::AppHandle,
  state: tauri::State<AppState>,
  positions: HashMap<String, PanelLayout>,
) -> Result<(), String> {
  let mut s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
  for (key, layout) in positions {
    s.panel_layouts.insert(key, layout);
  }
  persist_settings(&app, &s)
}

#[cfg(test)]
mod stats_helpers_tests {
  use super::*;

  #[test]
  fn lhm_temp_for_model_matches_case_insensitive_with_whitespace() {
    let disk_temps = vec![("  Samsung 990 PRO  ".to_string(), 47.0)];
    let got = lhm_temp_for_model(" samsung 990 pro ", &disk_temps);
    assert_eq!(got, Some(47.0));
  }

  #[test]
  fn lhm_temp_for_model_matches_substrings_both_directions() {
    let disk_temps = vec![
      ("WDC WDS500G2B0A-00SM50".to_string(), 33.0),
      ("Samsung SSD 980 PRO 1TB".to_string(), 45.0),
    ];

    // haystack contains needle
    assert_eq!(lhm_temp_for_model("Samsung SSD 980 PRO", &disk_temps), Some(45.0));

    // needle contains haystack
    assert_eq!(
      lhm_temp_for_model("WDC WDS500G2B0A-00SM50 SATA SSD", &disk_temps),
      Some(33.0)
    );
  }

  #[test]
  fn lhm_temp_for_model_returns_none_for_empty_or_unmatched() {
    let disk_temps = vec![("Crucial P3 Plus".to_string(), 41.0)];
    assert_eq!(lhm_temp_for_model("", &disk_temps), None);
    assert_eq!(lhm_temp_for_model("Some Other Drive", &disk_temps), None);
  }

  #[test]
  fn normalize_top_processes_sorts_descending_and_limits() {
    let entries = vec![
      ProcessEntry {
        name: "c".to_string(),
        cpu: 5.0,
        mem_mb: 300,
      },
      ProcessEntry {
        name: "a".to_string(),
        cpu: 42.0,
        mem_mb: 100,
      },
      ProcessEntry {
        name: "b".to_string(),
        cpu: 12.0,
        mem_mb: 200,
      },
    ];

    let out = normalize_top_processes(entries, 2);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].name, "a");
    assert_eq!(out[1].name, "b");
  }

  #[test]
  fn normalize_top_processes_handles_nan_cpu_without_panicking() {
    let entries = vec![
      ProcessEntry {
        name: "valid-high".to_string(),
        cpu: 70.0,
        mem_mb: 1,
      },
      ProcessEntry {
        name: "nan".to_string(),
        cpu: f32::NAN,
        mem_mb: 1,
      },
      ProcessEntry {
        name: "valid-low".to_string(),
        cpu: 10.0,
        mem_mb: 1,
      },
    ];

    let out = normalize_top_processes(entries, 3);
    assert_eq!(out.len(), 3);
    assert_eq!(out[0].name, "valid-high");
  }
}
