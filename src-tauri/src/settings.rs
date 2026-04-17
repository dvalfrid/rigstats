//! Persistent user settings model and file I/O helpers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

// --- Panel layout ----------------------------------------------------------

/// Saved screen position for a single floating panel window.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PanelLayout {
  pub x: i32,
  pub y: i32,
}

// --- Component thresholds --------------------------------------------------

/// Warn/critical temperature pair for a single hardware component.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentThresholds {
  pub warn: Option<u8>,
  pub crit: Option<u8>,
}

/// Default threshold map applied on fresh installs and as migration fallback.
pub fn default_thresholds() -> HashMap<String, ComponentThresholds> {
  [
    (
      "cpu",
      ComponentThresholds {
        warn: Some(80),
        crit: Some(90),
      },
    ),
    (
      "gpu",
      ComponentThresholds {
        warn: Some(80),
        crit: Some(90),
      },
    ),
    (
      "ram",
      ComponentThresholds {
        warn: Some(50),
        crit: Some(65),
      },
    ),
    (
      "disk",
      ComponentThresholds {
        warn: Some(55),
        crit: Some(70),
      },
    ),
  ]
  .into_iter()
  .map(|(k, v)| (k.to_string(), v))
  .collect()
}

// --- Settings struct -------------------------------------------------------

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
  /// Per-component temperature alert thresholds.
  /// Keys: "cpu", "gpu", "ram", "disk" (and any future components).
  #[serde(default)]
  pub thresholds: HashMap<String, ComponentThresholds>,
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
  /// Active colour theme key (e.g. `"dark-cyan"`).
  #[serde(default = "default_theme")]
  pub theme: String,
  /// Open each visible panel as its own frameless window instead of one portrait window.
  #[serde(default)]
  pub floating_mode: bool,
  /// Scale factor for floating panel windows in the range [0.4, 1.0].
  #[serde(default = "default_floating_panel_scale")]
  pub floating_panel_scale: f64,
  /// Last known screen position for each floating panel, keyed by panel key.
  #[serde(default)]
  pub panel_layouts: HashMap<String, PanelLayout>,
  /// Schema version used to detect and apply one-time migrations.
  /// 0 = legacy flat threshold fields (pre-map), 1 = current map format.
  #[serde(default)]
  pub settings_version: u8,

  // ---- Legacy migration shims (schema version 0) --------------------------
  // These fields existed in older settings files as eight flat values.
  // They are read from disk but never written back (`skip_serializing`).
  // `load_settings` copies them into `thresholds` exactly once, then bumps
  // `settings_version` to 1 so the migration never re-runs.
  #[serde(default, skip_serializing)]
  warning_cpu_temp: Option<u8>,
  #[serde(default, skip_serializing)]
  critical_cpu_temp: Option<u8>,
  #[serde(default, skip_serializing)]
  warning_gpu_temp: Option<u8>,
  #[serde(default, skip_serializing)]
  critical_gpu_temp: Option<u8>,
  #[serde(default, skip_serializing)]
  warning_ram_temp: Option<u8>,
  #[serde(default, skip_serializing)]
  critical_ram_temp: Option<u8>,
  #[serde(default, skip_serializing)]
  warning_disk_temp: Option<u8>,
  #[serde(default, skip_serializing)]
  critical_disk_temp: Option<u8>,
}

fn default_alert_cooldown_secs() -> u64 {
  60
}

fn default_true() -> bool {
  true
}

fn default_floating_panel_scale() -> f64 {
  1.0
}

fn default_theme() -> String {
  "dark-cyan".to_string()
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
      thresholds: default_thresholds(),
      alert_cooldown_secs: default_alert_cooldown_secs(),
      notify_on_warn: true,
      notify_on_crit: true,
      theme: default_theme(),
      floating_mode: false,
      floating_panel_scale: default_floating_panel_scale(),
      panel_layouts: HashMap::new(),
      settings_version: 1, // New installs start at current version — no migration needed.
      warning_cpu_temp: None,
      critical_cpu_temp: None,
      warning_gpu_temp: None,
      critical_gpu_temp: None,
      warning_ram_temp: None,
      critical_ram_temp: None,
      warning_disk_temp: None,
      critical_disk_temp: None,
    }
  }
}

// --- File I/O --------------------------------------------------------------

pub fn settings_path(app: &tauri::AppHandle) -> PathBuf {
  // Store settings in app data so they persist across updates.
  let app_dir = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
  app_dir.join("rigstats-settings.json")
}

pub fn load_settings(app: &tauri::AppHandle) -> Settings {
  // On parse/read failure, return defaults to keep startup robust.
  let path = settings_path(app);
  let mut settings = match fs::read_to_string(&path) {
    Ok(raw) => serde_json::from_str::<Settings>(&raw).unwrap_or_default(),
    Err(_) => Settings::default(),
  };

  // One-time migration from schema version 0 (flat threshold fields) to
  // version 1 (thresholds map). Runs once, then persists the new format.
  if settings.settings_version == 0 {
    migrate_v0_thresholds(&mut settings);
    settings.settings_version = 1;
    // Persist immediately so the migration is not repeated on the next launch.
    // Failures are non-fatal: the migrated settings are held in memory and
    // will be written again the next time the user saves settings.
    let _ = persist_settings(app, &settings);
  }

  settings
}

/// Copies schema-version-0 flat threshold fields into the `thresholds` map.
///
/// If at least one flat field was set, the user's values are preserved exactly.
/// If all flat fields are `None` (either never configured or explicitly cleared),
/// default thresholds are applied so the dashboard starts with sensible alert
/// levels rather than all alerts silently disabled.
fn migrate_v0_thresholds(s: &mut Settings) {
  let candidates = [
    ("cpu", s.warning_cpu_temp, s.critical_cpu_temp),
    ("gpu", s.warning_gpu_temp, s.critical_gpu_temp),
    ("ram", s.warning_ram_temp, s.critical_ram_temp),
    ("disk", s.warning_disk_temp, s.critical_disk_temp),
  ];
  let any_configured = candidates.iter().any(|(_, w, c)| w.is_some() || c.is_some());
  if any_configured {
    for (key, warn, crit) in candidates {
      s.thresholds.insert(key.to_string(), ComponentThresholds { warn, crit });
    }
  } else {
    s.thresholds = default_thresholds();
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
