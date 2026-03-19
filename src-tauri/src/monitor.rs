//! Dashboard profile definitions and monitor placement logic.
//!
//! A "profile" is a named portrait resolution (e.g. `portrait-xl` = 450×1920).
//! This module owns:
//! - The canonical list of valid profile names and their pixel dimensions.
//! - Monitor selection: pick the best available monitor for a given profile.
//! - Panel visibility normalisation shared by preview and save_settings paths.

use tauri::{Position, Size, WebviewWindow};

// --- Profile normalisation -------------------------------------------------

/// Returns the canonical profile name, falling back to `portrait-xl` for
/// any unrecognised input. Keeps both backend and frontend in sync on valid keys.
pub(crate) fn normalize_profile(profile: &str) -> String {
  match profile {
    "portrait-xl"
    | "portrait-slim"
    | "portrait-hd"
    | "portrait-wxga"
    | "portrait-fhd"
    | "portrait-wuxga"
    | "portrait-qhd"
    | "portrait-hdplus"
    | "portrait-900x1600"
    | "portrait-1050x1680"
    | "portrait-1600x2560"
    | "portrait-4k" => profile.to_string(),
    _ => "portrait-xl".to_string(),
  }
}

/// Returns `(width, height)` in physical pixels for a normalised profile name.
pub(crate) fn profile_dimensions(profile: &str) -> (u32, u32) {
  match normalize_profile(profile).as_str() {
    "portrait-slim" => (480, 1920),
    "portrait-hd" => (720, 1280),
    "portrait-wxga" => (800, 1280),
    "portrait-fhd" => (1080, 1920),
    "portrait-wuxga" => (1200, 1920),
    "portrait-qhd" => (1440, 2560),
    "portrait-hdplus" => (768, 1366),
    "portrait-900x1600" => (900, 1600),
    "portrait-1050x1680" => (1050, 1680),
    "portrait-1600x2560" => (1600, 2560),
    "portrait-4k" => (2160, 3840),
    _ => (450, 1920), // portrait-xl default
  }
}

fn is_portrait(width: u32, height: u32) -> bool {
  height >= width
}

// --- Panel visibility normalisation ----------------------------------------

fn is_valid_panel_key(value: &str) -> bool {
  matches!(value, "header" | "clock" | "cpu" | "gpu" | "ram" | "net" | "disk")
}

/// Validates and deduplicates a list of panel keys.
/// Returns all panels if the resulting list would be empty.
pub(crate) fn normalize_visible_panels(values: Vec<String>) -> Vec<String> {
  let mut out = Vec::new();
  for value in values {
    let key = value.trim().to_ascii_lowercase();
    if key.is_empty() || !is_valid_panel_key(&key) || out.iter().any(|v| v == &key) {
      continue;
    }
    out.push(key);
  }

  if out.is_empty() {
    vec![
      "header".to_string(),
      "clock".to_string(),
      "cpu".to_string(),
      "gpu".to_string(),
      "ram".to_string(),
      "net".to_string(),
      "disk".to_string(),
    ]
  } else {
    out
  }
}

// --- Monitor selection -----------------------------------------------------

/// Aspect-ratio and area fit score: lower = better match.
/// Weights aspect ratio 70 % and physical area 30 %.
fn fit_score(monitor_w: u32, monitor_h: u32, target_w: u32, target_h: u32) -> f64 {
  let monitor_aspect = monitor_w as f64 / monitor_h as f64;
  let target_aspect = target_w as f64 / target_h as f64;
  let aspect_cost = (monitor_aspect / target_aspect).ln().abs();

  let monitor_area = (monitor_w as f64) * (monitor_h as f64);
  let target_area = (target_w as f64) * (target_h as f64);
  let area_cost = (monitor_area / target_area).ln().abs();

  (0.7 * aspect_cost) + (0.3 * area_cost)
}

/// Moves and resizes `window` to the monitor that best fits `profile`.
///
/// Selection priority:
/// 1. Exact resolution match → fullscreen.
/// 2. Best orientation + aspect ratio match → borderless, profile size.
/// 3. Any monitor → borderless, profile size.
///
/// Returns `true` if an exact monitor match was found.
pub fn pick_target_monitor(window: &WebviewWindow, profile: &str) -> bool {
  let (target_w, target_h) = profile_dimensions(profile);
  let target_portrait = is_portrait(target_w, target_h);

  if let Ok(monitors) = window.available_monitors() {
    let exact_monitor = monitors
      .iter()
      .find(|m| m.size().width == target_w && m.size().height == target_h)
      .cloned();
    let has_exact_match = exact_monitor.is_some();

    let best_orientation_match = monitors
      .iter()
      .filter(|m| is_portrait(m.size().width, m.size().height) == target_portrait)
      .min_by(|a, b| {
        let a_score = fit_score(a.size().width, a.size().height, target_w, target_h);
        let b_score = fit_score(b.size().width, b.size().height, target_w, target_h);
        a_score.partial_cmp(&b_score).unwrap_or(std::cmp::Ordering::Equal)
      })
      .cloned();

    let best_any_match = monitors
      .iter()
      .min_by(|a, b| {
        let a_score = fit_score(a.size().width, a.size().height, target_w, target_h);
        let b_score = fit_score(b.size().width, b.size().height, target_w, target_h);
        a_score.partial_cmp(&b_score).unwrap_or(std::cmp::Ordering::Equal)
      })
      .cloned();

    let selected_monitor = exact_monitor.or(best_orientation_match).or(best_any_match);

    if let Some(monitor) = selected_monitor {
      let _ = window.set_fullscreen(false);
      let _ = window.set_position(Position::Physical(*monitor.position()));
      let _ = window.set_decorations(false);
      let _ = window.set_size(Size::Physical(tauri::PhysicalSize {
        width: target_w,
        height: target_h,
      }));

      if has_exact_match {
        let _ = window.set_fullscreen(true);
      }
    } else {
      let _ = window.set_fullscreen(false);
      let _ = window.set_decorations(false);
      let _ = window.set_size(Size::Physical(tauri::PhysicalSize {
        width: target_w,
        height: target_h,
      }));
    }

    return has_exact_match;
  }

  false
}
