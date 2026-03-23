//! Secondary window creation and tray-relative positioning.
//!
//! Responsibilities:
//! - Create (or focus) the Settings, About, and Status secondary windows.
//! - Position popups relative to the tray icon click position.
//! - Handle the main window close event (hide-to-tray instead of quit).

use crate::debug::append_debug_log;
use std::sync::atomic::{AtomicI32, Ordering};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

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
  let height = 620.0;
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
