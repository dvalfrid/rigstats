//! Dashboard profile definitions and monitor placement logic.
//!
//! A "profile" is a named portrait resolution (e.g. `portrait-xl` = 450×1920).
//! This module owns:
//! - The canonical list of valid profile names and their pixel dimensions.
//! - Monitor selection: pick the best available monitor for a given profile.
//! - Panel visibility normalisation shared by preview and save_settings paths.

use tauri::{PhysicalPosition, Position, Size, WebviewWindow};

// --- Profile normalisation -------------------------------------------------

/// Returns the canonical profile name, falling back to `portrait-xl` for
/// any unrecognised input. Keeps both backend and frontend in sync on valid keys.
pub(crate) fn normalize_profile(profile: &str) -> String {
  match profile {
    "portrait-xl" | "portrait-slim" | "portrait-hd" | "portrait-wxga" | "portrait-fhd" | "portrait-wuxga"
    | "portrait-qhd" | "portrait-hdplus" | "portrait-900x1600" | "portrait-1050x1680" | "portrait-1600x2560"
    | "portrait-4k" | "portrait-fhd-side" | "portrait-qhd-side" | "portrait-4k-side" => profile.to_string(),
    _ => "portrait-xl".to_string(),
  }
}

/// Returns `(width, height)` in physical pixels for a normalised profile name.
pub(crate) fn profile_dimensions(profile: &str) -> (u32, u32) {
  match normalize_profile(profile).as_str() {
    "portrait-slim" => (480, 1920),
    "portrait-fhd-side" => (253, 1080),
    "portrait-qhd-side" => (338, 1440),
    "portrait-4k-side" => (506, 2160),
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
  matches!(
    value,
    "header" | "clock" | "cpu" | "gpu" | "ram" | "net" | "disk" | "motherboard" | "process" | "battery"
  )
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

// --- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::{
    compute_panels_logical_height, fit_score, normalize_profile, normalize_visible_panels, profile_dimensions,
  };

  #[test]
  fn normalize_profile_passes_through_all_valid_names() {
    let valid = [
      "portrait-xl",
      "portrait-slim",
      "portrait-hd",
      "portrait-wxga",
      "portrait-fhd",
      "portrait-wuxga",
      "portrait-qhd",
      "portrait-hdplus",
      "portrait-900x1600",
      "portrait-1050x1680",
      "portrait-1600x2560",
      "portrait-4k",
      "portrait-fhd-side",
      "portrait-qhd-side",
      "portrait-4k-side",
    ];
    for name in &valid {
      assert_eq!(normalize_profile(name), *name, "should pass through: {name}");
    }
  }

  #[test]
  fn normalize_profile_falls_back_to_xl_for_unknown() {
    assert_eq!(normalize_profile(""), "portrait-xl");
    assert_eq!(normalize_profile("landscape-hd"), "portrait-xl");
    assert_eq!(normalize_profile("portrait-xl-extra"), "portrait-xl");
  }

  #[test]
  fn profile_dimensions_returns_correct_pixel_sizes() {
    assert_eq!(profile_dimensions("portrait-xl"), (450, 1920));
    assert_eq!(profile_dimensions("portrait-4k"), (2160, 3840));
    assert_eq!(profile_dimensions("portrait-hd"), (720, 1280));
    assert_eq!(profile_dimensions("portrait-fhd"), (1080, 1920));
    assert_eq!(profile_dimensions("portrait-fhd-side"), (253, 1080));
    assert_eq!(profile_dimensions("portrait-qhd-side"), (338, 1440));
    assert_eq!(profile_dimensions("portrait-4k-side"), (506, 2160));
  }

  #[test]
  fn profile_dimensions_falls_back_for_unknown_profile() {
    assert_eq!(profile_dimensions("bogus"), (450, 1920));
  }

  #[test]
  fn normalize_visible_panels_accepts_valid_keys() {
    let result = normalize_visible_panels(vec!["cpu".to_string(), "gpu".to_string(), "ram".to_string()]);
    assert_eq!(result, vec!["cpu", "gpu", "ram"]);
  }

  #[test]
  fn normalize_visible_panels_strips_invalid_keys() {
    let result = normalize_visible_panels(vec!["cpu".to_string(), "bogus".to_string(), "gpu".to_string()]);
    assert_eq!(result, vec!["cpu", "gpu"]);
  }

  #[test]
  fn normalize_visible_panels_deduplicates() {
    let result = normalize_visible_panels(vec!["cpu".to_string(), "gpu".to_string(), "cpu".to_string()]);
    assert_eq!(result, vec!["cpu", "gpu"]);
  }

  #[test]
  fn normalize_visible_panels_normalizes_case_and_whitespace() {
    let result = normalize_visible_panels(vec!["  CPU  ".to_string(), "GPU".to_string()]);
    assert_eq!(result, vec!["cpu", "gpu"]);
  }

  #[test]
  fn normalize_visible_panels_returns_all_panels_when_input_is_empty() {
    let result = normalize_visible_panels(vec![]);
    assert_eq!(result, vec!["header", "clock", "cpu", "gpu", "ram", "net", "disk"]);
  }

  #[test]
  fn normalize_visible_panels_returns_all_panels_when_all_keys_are_invalid() {
    let result = normalize_visible_panels(vec!["bogus".to_string(), "fake".to_string()]);
    assert_eq!(result, vec!["header", "clock", "cpu", "gpu", "ram", "net", "disk"]);
  }

  #[test]
  fn fit_score_perfect_match_scores_zero() {
    let score = fit_score(450, 1920, 450, 1920);
    assert!(score < 1e-10, "perfect match should score ~0.0, got {score}");
  }

  #[test]
  fn fit_score_landscape_monitor_scores_worse_than_portrait_for_portrait_target() {
    let portrait_score = fit_score(720, 1280, 450, 1920);
    let landscape_score = fit_score(1920, 1080, 450, 1920);
    assert!(portrait_score < landscape_score);
  }

  #[test]
  fn fit_score_closer_size_scores_better() {
    let close = fit_score(450, 1920, 480, 1920);
    let far = fit_score(450, 1920, 2160, 3840);
    assert!(close < far);
  }

  // --- compute_panels_logical_height ---
  // These tests mirror the JS applyVisiblePanels formula. If the Rust and JS
  // sides ever drift, the window pre-sizing will be wrong and cause a height
  // flash on every floating-mode toggle.

  fn panels(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|s| s.to_string()).collect()
  }

  #[test]
  fn panel_height_single_header_at_1x_scale() {
    // logical_h = 1920, height_scale = 1.0, gap = 1.0
    // header only: (196 * 1.0).round() = 196, no gap for first panel
    let h = compute_panels_logical_height(1920, 1.0, &panels(&["header"]));
    assert_eq!(h, 196.0);
  }

  #[test]
  fn panel_height_two_panels_adds_gap() {
    // header = 196, gap = 1, clock = 148 + 1 gap = 149 → total = 345
    let h = compute_panels_logical_height(1920, 1.0, &panels(&["header", "clock"]));
    assert_eq!(h, 345.0);
  }

  #[test]
  fn panel_height_default_visible_panels_at_1x_scale() {
    // ["header","clock","cpu","gpu","ram","net","disk"] at portrait-xl (1920h) scale 1.0
    // header=196, clock=148+1=149, cpu=320+1=321, gpu=321, ram=321, net=321, disk=321 → 1950
    let h = compute_panels_logical_height(
      1920,
      1.0,
      &panels(&["header", "clock", "cpu", "gpu", "ram", "net", "disk"]),
    );
    assert_eq!(h, 1950.0);
  }

  #[test]
  fn panel_height_scales_with_higher_dpi() {
    // At 2x scale, logical_h = 1920/2 = 960, height_scale = 960/1920 = 0.5
    // header: (196*0.5).round() = 98, gap = max(1, 0.5.round()) = 1
    // clock: (148*0.5).round() + 1 = 74 + 1 = 75
    // total = 98 + 75 = 173
    let h = compute_panels_logical_height(1920, 2.0, &panels(&["header", "clock"]));
    assert_eq!(h, 173.0);
  }

  #[test]
  fn panel_height_empty_panel_list_is_zero() {
    let h = compute_panels_logical_height(1920, 1.0, &[]);
    assert_eq!(h, 0.0);
  }
}

/// Computes the logical-pixel height for a set of visible panels, matching the
/// formula used by `applyVisiblePanels` in `app.js` exactly so the backend can
/// pre-size the window before `show()` and avoid a full-height flash.
///
/// `scale_factor` is the window's current DPR (from `window.scale_factor()`).
pub(crate) fn compute_panels_logical_height(profile_h: u32, scale_factor: f64, visible_panels: &[String]) -> f64 {
  const BASE_H: f64 = 1920.0;
  let logical_h = (profile_h as f64 / scale_factor).round();
  let height_scale = logical_h / BASE_H;
  let gap = 1.0_f64.max(height_scale.round());
  let mut total = 0.0;
  for (i, key) in visible_panels.iter().enumerate() {
    let base: f64 = match key.as_str() {
      "header" => 196.0,
      "clock" => 148.0,
      _ => 320.0,
    };
    let h = (base * height_scale).round().max(1.0);
    total += h + if i > 0 { gap } else { 0.0 };
  }
  total
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
      let _ = window.set_size(Size::Physical(tauri::PhysicalSize {
        width: target_w,
        height: target_h,
      }));
      let _ = window.set_decorations(false);

      // Tauri borderless windows on Windows keep WS_THICKFRAME for resize handles.
      // This creates an invisible border: set_position places the outer rect at the
      // target but the visible content starts inset by the border width, leaving a
      // gap at the screen edge. Compute inset = inner - outer and subtract it so
      // the visible content lands exactly on the monitor origin.
      let target = *monitor.position();
      let inset_x = window
        .inner_position()
        .ok()
        .zip(window.outer_position().ok())
        .map(|(inner, outer)| inner.x - outer.x)
        .unwrap_or(0);
      let inset_y = window
        .inner_position()
        .ok()
        .zip(window.outer_position().ok())
        .map(|(inner, outer)| inner.y - outer.y)
        .unwrap_or(0);
      let _ = window.set_position(Position::Physical(PhysicalPosition {
        x: target.x - inset_x,
        y: target.y - inset_y,
      }));
    } else {
      let _ = window.set_size(Size::Physical(tauri::PhysicalSize {
        width: target_w,
        height: target_h,
      }));
      let _ = window.set_decorations(false);
    }

    return has_exact_match;
  }

  false
}
