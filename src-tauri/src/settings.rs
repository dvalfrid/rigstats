//! Persistent user settings model and file I/O helpers.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
  /// Panel alpha in the range [0.0, 1.0].
  pub opacity: f64,
  /// User-defined model label shown in the header panel.
  pub model_name: String,
  /// Active dashboard size profile.
  pub dashboard_profile: String,
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      opacity: 0.55,
      model_name: "ROG GM700TZ".to_string(),
      dashboard_profile: "portrait-xl".to_string(),
    }
  }
}

pub fn settings_path(app: &tauri::AppHandle) -> PathBuf {
  // Store settings in app data so they persist across updates.
  let app_dir = app
    .path_resolver()
    .app_data_dir()
    .unwrap_or_else(|| PathBuf::from("."));
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
