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
