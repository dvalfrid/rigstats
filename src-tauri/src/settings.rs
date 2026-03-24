//! Persistent user settings model and file I/O helpers.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
  /// Panel alpha in the range [0.0, 1.0].
  #[serde(default = "default_opacity")]
  pub opacity: f64,
  /// User-defined model label shown in the header panel.
  #[serde(default = "default_model_name")]
  pub model_name: String,
  /// Active dashboard size profile.
  #[serde(default = "default_dashboard_profile")]
  pub dashboard_profile: String,
  /// Keep the dashboard window above other windows.
  #[serde(default)]
  pub always_on_top: bool,
  /// Ordered list of visible dashboard panels.
  #[serde(default = "default_visible_panels")]
  pub visible_panels: Vec<String>,
  /// Launch the dashboard automatically when the user logs in.
  #[serde(default)]
  pub autostart_enabled: bool,
  /// Version seen on last launch, used to detect first run after an update.
  #[serde(default)]
  pub last_seen_version: String,
  /// Warning temperature threshold for CPU in °C. `None` = alert disabled.
  #[serde(default)]
  pub warning_cpu_temp: Option<u8>,
  /// Warning temperature threshold for GPU in °C. `None` = alert disabled.
  #[serde(default)]
  pub warning_gpu_temp: Option<u8>,
  /// Warning temperature threshold for RAM in °C. `None` = alert disabled.
  #[serde(default)]
  pub warning_ram_temp: Option<u8>,
  /// Warning temperature threshold for disk in °C. `None` = alert disabled.
  #[serde(default)]
  pub warning_disk_temp: Option<u8>,
  /// Critical temperature threshold for CPU in °C. `None` = alert disabled.
  #[serde(default)]
  pub critical_cpu_temp: Option<u8>,
  /// Critical temperature threshold for GPU in °C. `None` = alert disabled.
  #[serde(default)]
  pub critical_gpu_temp: Option<u8>,
  /// Critical temperature threshold for RAM in °C. `None` = alert disabled.
  #[serde(default)]
  pub critical_ram_temp: Option<u8>,
  /// Critical temperature threshold for disk in °C. `None` = alert disabled.
  #[serde(default)]
  pub critical_disk_temp: Option<u8>,
  /// Minimum seconds between repeated notifications for the same component+level.
  /// Floored at 60 s on save to prevent notification spam.
  #[serde(default = "default_alert_cooldown_secs")]
  pub alert_cooldown_secs: u64,
  /// Whether to send notifications when a WARNING threshold is crossed.
  #[serde(default = "default_true")]
  pub notify_on_warn: bool,
  /// Whether to send notifications when a CRITICAL threshold is crossed.
  #[serde(default = "default_true")]
  pub notify_on_crit: bool,
}

fn default_alert_cooldown_secs() -> u64 {
  60
}

fn default_true() -> bool {
  true
}

fn default_opacity() -> f64 {
  0.55
}

fn default_model_name() -> String {
  String::new()
}

fn default_dashboard_profile() -> String {
  "portrait-xl".to_string()
}

fn default_visible_panels() -> Vec<String> {
  vec![
    "header".to_string(),
    "clock".to_string(),
    "cpu".to_string(),
    "gpu".to_string(),
    "ram".to_string(),
    "net".to_string(),
    "disk".to_string(),
  ]
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      opacity: default_opacity(),
      model_name: default_model_name(),
      dashboard_profile: default_dashboard_profile(),
      always_on_top: false,
      visible_panels: default_visible_panels(),
      autostart_enabled: false,
      last_seen_version: String::new(),
      warning_cpu_temp: Some(80),
      warning_gpu_temp: Some(80),
      warning_ram_temp: Some(50),
      warning_disk_temp: Some(55),
      critical_cpu_temp: Some(90),
      critical_gpu_temp: Some(90),
      critical_ram_temp: Some(65),
      critical_disk_temp: Some(70),
      alert_cooldown_secs: default_alert_cooldown_secs(),
      notify_on_warn: true,
      notify_on_crit: true,
    }
  }
}

pub fn settings_path(app: &tauri::AppHandle) -> PathBuf {
  // Store settings in app data so they persist across updates.
  let app_dir = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
  app_dir.join("rigstats-settings.json")
}

pub fn load_settings(app: &tauri::AppHandle) -> Settings {
  // On parse/read failure, return defaults to keep startup robust.
  let path = settings_path(app);
  match fs::read_to_string(path) {
    Ok(raw) => serde_json::from_str::<Settings>(&raw).unwrap_or_default(),
    Err(_) => Settings::default(),
  }
}

pub fn persist_settings(app: &tauri::AppHandle, settings: &Settings) -> Result<(), String> {
  // Ensure parent directories exist before writing the settings file.
  let path = settings_path(app);
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
  }
  let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
  fs::write(path, json).map_err(|e| e.to_string())
}
