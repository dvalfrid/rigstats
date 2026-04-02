//! Secondary window creation and tray-relative positioning.
//!
//! Responsibilities:
//! - Create (or focus) the Settings, About, and Status secondary windows.
//! - Position popups relative to the tray icon click position.
//! - Handle the main window close event (hide-to-tray instead of quit).

use crate::debug::append_debug_log;
use crate::monitor::profile_dimensions;
use crate::stats::AppState;
use std::collections::HashSet;
use std::sync::atomic::{AtomicI32, Ordering};
use tauri::utils::config::Color;
use tauri::{AppHandle, Manager, PhysicalPosition, Position, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

/// Last recorded tray icon click position, used to anchor popups.
static LAST_TRAY_CLICK_X: AtomicI32 = AtomicI32::new(i32::MIN);
static LAST_TRAY_CLICK_Y: AtomicI32 = AtomicI32::new(i32::MIN);

pub fn set_last_tray_click_position(x: f64, y: f64) {
  LAST_TRAY_CLICK_X.store(x.round() as i32, Ordering::Relaxed);
  LAST_TRAY_CLICK_Y.store(y.round() as i32, Ordering::Relaxed);
}

// --- Popup positioning -----------------------------------------------------

/// Clamps a popup to stay fully inside the monitor work area.
#[allow(clippy::too_many_arguments)]
fn monitor_work_area(
  origin_x: i32,
  origin_y: i32,
  monitor_w: u32,
  monitor_h: u32,
  popup_w: f64,
  popup_h: f64,
  preferred_x: i32,
  preferred_y: i32,
) -> (f64, f64) {
  let popup_w_px = popup_w.round() as i32;
  let popup_h_px = popup_h.round() as i32;
  let max_x = origin_x + monitor_w as i32 - popup_w_px;
  let max_y = origin_y + monitor_h as i32 - popup_h_px;
  let x = preferred_x.clamp(origin_x, max_x);
  let y = preferred_y.clamp(origin_y, max_y);
  (x as f64, y as f64)
}

/// Computes a position for a popup that is anchored just above the tray icon.
/// Falls back to the bottom-right corner of the primary monitor.
fn tray_anchor_position(app: &AppHandle, width: f64, height: f64) -> Option<(f64, f64)> {
  let margin_px = 12i32;
  let tray_x = LAST_TRAY_CLICK_X.load(Ordering::Relaxed);
  let tray_y = LAST_TRAY_CLICK_Y.load(Ordering::Relaxed);

  if tray_x != i32::MIN && tray_y != i32::MIN {
    if let Ok(monitors) = app.available_monitors() {
      if let Some(monitor) = monitors.into_iter().find(|monitor| {
        let pos = monitor.position();
        let size = monitor.size();
        let within_x = tray_x >= pos.x && tray_x < pos.x + size.width as i32;
        let within_y = tray_y >= pos.y && tray_y < pos.y + size.height as i32;
        within_x && within_y
      }) {
        let pos = monitor.position();
        let size = monitor.size();
        let preferred_x = tray_x - width.round() as i32 + 26;
        let preferred_y = tray_y - height.round() as i32 - margin_px;
        return Some(monitor_work_area(
          pos.x,
          pos.y,
          size.width,
          size.height,
          width,
          height,
          preferred_x,
          preferred_y,
        ));
      }
    }
  }

  // Fallback: bottom-right of the primary monitor.
  let monitor = app.primary_monitor().ok().flatten()?;
  let origin = monitor.position();
  let size = monitor.size();
  Some(monitor_work_area(
    origin.x,
    origin.y,
    size.width,
    size.height,
    width,
    height,
    origin.x + size.width as i32 - width.round() as i32 - margin_px,
    origin.y + size.height as i32 - height.round() as i32 - margin_px,
  ))
}

// --- Window constructors ---------------------------------------------------

/// Opens the Settings window, or focuses it if already open.
pub fn ensure_settings_window(app: &AppHandle) -> Result<(), String> {
  append_debug_log(app, "Settings window requested from tray/menu");

  if let Some(win) = app.get_webview_window("settings") {
    win.show().map_err(|e| {
      let msg = e.to_string();
      append_debug_log(app, &format!("Settings window show failed: {}", msg));
      msg
    })?;
    win.set_focus().map_err(|e| {
      let msg = e.to_string();
      append_debug_log(app, &format!("Settings window focus failed: {}", msg));
      msg
    })?;
    append_debug_log(app, "Settings window reused successfully");
    return Ok(());
  }

  let width = 640.0;
  let height = 760.0;
  let (x, y) = tray_anchor_position(app, width, height).unwrap_or((40.0, 40.0));

  let window = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
    .title("Settings")
    .inner_size(width, height)
    .position(x, y)
    .decorations(true)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .build()
    .map_err(|e| {
      let msg = e.to_string();
      append_debug_log(app, &format!("Settings window build failed: {}", msg));
      msg
    })?;

  window.show().map_err(|e| {
    let msg = e.to_string();
    append_debug_log(app, &format!("Settings window initial show failed: {}", msg));
    msg
  })?;
  window.set_focus().map_err(|e| {
    let msg = e.to_string();
    append_debug_log(app, &format!("Settings window initial focus failed: {}", msg));
    msg
  })?;
  append_debug_log(app, "Settings window created successfully");
  Ok(())
}

/// Opens the About window, or focuses it if already open.
pub fn ensure_about_window(app: &AppHandle) -> Result<(), String> {
  append_debug_log(app, "About window requested from tray/menu");

  if let Some(win) = app.get_webview_window("about") {
    win.show().map_err(|e| {
      let msg = e.to_string();
      append_debug_log(app, &format!("About window show failed: {}", msg));
      msg
    })?;
    win.set_focus().map_err(|e| {
      let msg = e.to_string();
      append_debug_log(app, &format!("About window focus failed: {}", msg));
      msg
    })?;
    append_debug_log(app, "About window reused successfully");
    return Ok(());
  }

  let width = 640.0;
  let height = 380.0;
  let (x, y) = tray_anchor_position(app, width, height).unwrap_or((56.0, 56.0));

  let window = WebviewWindowBuilder::new(app, "about", WebviewUrl::App("about.html".into()))
    .title("About RIGStats")
    .inner_size(width, height)
    .position(x, y)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(true)
    .build()
    .map_err(|e| {
      let msg = e.to_string();
      append_debug_log(app, &format!("About window build failed: {}", msg));
      msg
    })?;

  window.show().map_err(|e| {
    let msg = e.to_string();
    append_debug_log(app, &format!("About window initial show failed: {}", msg));
    msg
  })?;
  window.set_focus().map_err(|e| {
    let msg = e.to_string();
    append_debug_log(app, &format!("About window initial focus failed: {}", msg));
    msg
  })?;
  append_debug_log(app, "About window created successfully");
  Ok(())
}

/// Opens the Status window, or focuses it if already open.
pub fn ensure_status_window(app: &AppHandle) -> Result<(), String> {
  append_debug_log(app, "Status window requested from tray/menu");

  if let Some(win) = app.get_webview_window("status") {
    win.show().map_err(|e| {
      let msg = e.to_string();
      append_debug_log(app, &format!("Status window show failed: {}", msg));
      msg
    })?;
    win.set_focus().map_err(|e| {
      let msg = e.to_string();
      append_debug_log(app, &format!("Status window focus failed: {}", msg));
      msg
    })?;
    append_debug_log(app, "Status window reused successfully");
    return Ok(());
  }

  let width = 700.0;
  let height = 760.0;
  let (x, y) = tray_anchor_position(app, width, height).unwrap_or((56.0, 56.0));

  let window = WebviewWindowBuilder::new(app, "status", WebviewUrl::App("status.html".into()))
    .title("RIGStats Status")
    .inner_size(width, height)
    .position(x, y)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(true)
    .build()
    .map_err(|e| {
      let msg = e.to_string();
      append_debug_log(app, &format!("Status window build failed: {}", msg));
      msg
    })?;

  window.show().map_err(|e| {
    let msg = e.to_string();
    append_debug_log(app, &format!("Status window initial show failed: {}", msg));
    msg
  })?;
  window.set_focus().map_err(|e| {
    let msg = e.to_string();
    append_debug_log(app, &format!("Status window initial focus failed: {}", msg));
    msg
  })?;
  append_debug_log(app, "Status window created successfully");
  Ok(())
}

/// Opens the Updater window, or focuses it if already open.
pub fn ensure_updater_window(app: &AppHandle) -> Result<(), String> {
  append_debug_log(app, "Updater window requested");

  if let Some(win) = app.get_webview_window("updater") {
    win.show().map_err(|e| e.to_string())?;
    win.set_focus().map_err(|e| e.to_string())?;
    append_debug_log(app, "Updater window reused successfully");
    return Ok(());
  }

  let width = 500.0;
  let height = 560.0;
  let (x, y) = tray_anchor_position(app, width, height).unwrap_or((60.0, 60.0));

  let window = WebviewWindowBuilder::new(app, "updater", WebviewUrl::App("updater.html".into()))
    .title("RIGStats Update")
    .inner_size(width, height)
    .position(x, y)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(true)
    .build()
    .map_err(|e| {
      let msg = e.to_string();
      append_debug_log(app, &format!("Updater window build failed: {}", msg));
      msg
    })?;

  window.show().map_err(|e| e.to_string())?;
  window.set_focus().map_err(|e| e.to_string())?;
  append_debug_log(app, "Updater window created successfully");
  Ok(())
}

// --- Floating panel windows ------------------------------------------------

/// Base dimensions (logical pixels at portrait-xl 1x scale) for each panel key.
fn panel_base_height(key: &str) -> f64 {
  match key {
    "header" => 196.0,
    "clock" => 148.0,
    "cpu" => 420.0,
    "gpu" => 320.0,
    "ram" => 315.0,
    "net" => 260.0,
    "disk" => 295.0,
    "motherboard" => 260.0,
    "process" => 260.0,
    _ => 260.0,
  }
}

fn panel_base_size(key: &str, dashboard_profile: &str) -> (f64, f64) {
  // Match main-dashboard scaling so floating panels keep the same physical size
  // as in fixed mode for the selected profile.
  const BASE_PROFILE_H: f64 = 1920.0;

  let (profile_w, profile_h) = profile_dimensions(dashboard_profile);
  let height_scale = profile_h as f64 / BASE_PROFILE_H;
  let panel_h = (panel_base_height(key) * height_scale).round().max(1.0);
  (profile_w as f64, panel_h)
}

fn all_panel_keys() -> &'static [&'static str] {
  &[
    "header",
    "clock",
    "cpu",
    "gpu",
    "ram",
    "net",
    "disk",
    "motherboard",
    "process",
  ]
}

fn resize_existing_panel_window(app: &AppHandle, key: &str, dashboard_profile: &str) {
  let label = format!("panel-{}", key);
  if let Some(win) = app.get_webview_window(&label) {
    let (w, h) = panel_base_size(key, dashboard_profile);
    let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize { width: w, height: h }));
    let _ = win.set_background_color(Some(Color(0, 0, 0, 0)));
    let _ = win.set_decorations(false);
    let _ = win.show();
  }
}

/// Opens one frameless window per visible panel.
///
/// Positions are loaded from `settings.panel_layouts`.  Panels without a saved
/// position are staggered diagonally so they do not all land on top of each other.
pub fn launch_floating_panels(app: &AppHandle, state: &tauri::State<AppState>) -> Result<(), String> {
  let (visible_panels, panel_layouts, dashboard_profile) = {
    let s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    (
      s.visible_panels.clone(),
      s.panel_layouts.clone(),
      s.dashboard_profile.clone(),
    )
  };

  let visible_set: HashSet<&str> = visible_panels.iter().map(String::as_str).collect();

  for (i, key) in all_panel_keys().iter().enumerate() {
    let label = format!("panel-{}", key);

    // Skip if already open.
    if app.get_webview_window(&label).is_some() {
      continue;
    }

    let url = format!("panel-{}.html", key);
    let (w, h) = panel_base_size(key, &dashboard_profile);

    // Saved position or staggered default.
    let (saved_x, saved_y) = panel_layouts
      .get(*key)
      .map(|p| (p.x, p.y))
      .unwrap_or_else(|| (80 + i as i32 * 24, 80 + i as i32 * 24));

    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
      .title(*key)
      .inner_size(w, h)
      .position(saved_x as f64, saved_y as f64)
      .background_color(Color(0, 0, 0, 0))
      .decorations(false)
      .transparent(true)
      .resizable(false)
      .always_on_top(true)
      .skip_taskbar(true)
      .visible(visible_set.contains(*key))
      .build()
      .map_err(|e| {
        let msg = format!("panel window '{}' build failed: {}", label, e);
        append_debug_log(app, &msg);
        msg
      })?;

    // Apply DWM invisible resize border compensation so the saved position
    // lands flush with the screen edge (same logic as pick_target_monitor).
    let inset_x = win
      .inner_position()
      .ok()
      .zip(win.outer_position().ok())
      .map(|(i, o)| i.x - o.x)
      .unwrap_or(0);
    let inset_y = win
      .inner_position()
      .ok()
      .zip(win.outer_position().ok())
      .map(|(i, o)| i.y - o.y)
      .unwrap_or(0);
    let _ = win.set_position(Position::Physical(PhysicalPosition {
      x: saved_x - inset_x,
      y: saved_y - inset_y,
    }));

    let _ = win.set_decorations(false);
    if visible_set.contains(*key) {
      let _ = win.show();
    } else {
      let _ = win.hide();
    }
  }

  Ok(())
}

/// Reconciles open floating panel windows with the current settings without
/// tearing down every panel window on each preview interaction.
pub fn sync_floating_panels(app: &AppHandle, state: &tauri::State<AppState>) -> Result<(), String> {
  let (visible_panels, dashboard_profile) = {
    let s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    (s.visible_panels.clone(), s.dashboard_profile.clone())
  };

  let desired: HashSet<&str> = visible_panels.iter().map(String::as_str).collect();

  for key in all_panel_keys() {
    let label = format!("panel-{}", key);
    if !desired.contains(key) {
      if let Some(win) = app.get_webview_window(&label) {
        append_debug_log(app, &format!("floating sync: hide {label}"));
        let _ = win.hide();
      }
      continue;
    }

    if app.get_webview_window(&label).is_some() {
      append_debug_log(app, &format!("floating sync: show/resize {label}"));
      resize_existing_panel_window(app, key, &dashboard_profile);
    }
  }

  append_debug_log(app, "floating sync: ensure missing windows");
  launch_floating_panels(app, state)
}

/// Closes all open floating panel windows.
pub fn close_floating_panels(app: &AppHandle) {
  for key in all_panel_keys() {
    let label = format!("panel-{}", key);
    if let Some(win) = app.get_webview_window(&label) {
      let _ = win.hide();
    }
  }
}

// --- Window event handler --------------------------------------------------

/// Intercepts the main window close event and hides to tray instead of quitting.
/// Also reapplies decorations=false on move, as Windows can re-enable the title bar
/// when a window is dragged between monitors with different DPI or configurations.
pub fn on_window_event(win: &Window, event: &WindowEvent) {
  if win.label() == "main" {
    match event {
      WindowEvent::CloseRequested { api, .. } => {
        api.prevent_close();
        let _ = win.hide();
      }
      WindowEvent::Moved(_) => {
        let _ = win.set_decorations(false);
      }
      _ => {}
    }
  }
}
