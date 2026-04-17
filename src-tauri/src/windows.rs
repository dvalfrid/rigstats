//! Secondary window creation and tray-relative positioning.
//!
//! Responsibilities:
//! - Create (or focus) the Settings, About, and Status secondary windows.
//! - Position popups relative to the tray icon click position.
//! - Handle the main window close event (hide-to-tray instead of quit).

use crate::debug::append_debug_log;
use crate::monitor::{normalize_visible_panels, profile_dimensions};
use crate::stats::AppState;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use tauri::utils::config::Color;
use tauri::{AppHandle, Manager, PhysicalPosition, Position, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

/// Last recorded tray icon click position, used to anchor popups.
static LAST_TRAY_CLICK_X: AtomicI32 = AtomicI32::new(i32::MIN);
static LAST_TRAY_CLICK_Y: AtomicI32 = AtomicI32::new(i32::MIN);
// Prevent flooding main-thread window creation with duplicate sync tasks.
// We only need one queued sync because each run reads the latest settings.
static FLOATING_SYNC_QUEUED: AtomicBool = AtomicBool::new(false);

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
  let height = 860.0;
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

fn panel_base_size(key: &str, dashboard_profile: &str, user_scale: f64) -> (f64, f64) {
  // Match main-dashboard scaling so floating panels keep the same physical size
  // as in fixed mode for the selected profile, then apply user_scale.
  const BASE_PROFILE_H: f64 = 1920.0;

  let (profile_w, profile_h) = profile_dimensions(dashboard_profile);
  let height_scale = profile_h as f64 / BASE_PROFILE_H;
  let scale = user_scale.clamp(0.4, 1.0);
  let panel_h = (panel_base_height(key) * height_scale * scale).round().max(1.0);
  let panel_w = (profile_w as f64 * scale).round().max(1.0);
  (panel_w, panel_h)
}

pub fn all_panel_keys() -> &'static [&'static str] {
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

fn resize_existing_panel_window(app: &AppHandle, key: &str, dashboard_profile: &str, user_scale: f64) {
  let label = format!("panel-{}", key);
  if let Some(win) = app.get_webview_window(&label) {
    let (w, h) = panel_base_size(key, dashboard_profile, user_scale);
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
pub fn launch_floating_panels(app: &AppHandle, state: &tauri::State<AppState>) {
  let (visible_panels, panel_layouts, dashboard_profile, user_scale) = {
    let s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    (
      normalize_visible_panels(s.visible_panels.clone()),
      s.panel_layouts.clone(),
      s.dashboard_profile.clone(),
      s.floating_panel_scale,
    )
  };

  let visible_set: HashSet<&str> = visible_panels.iter().map(String::as_str).collect();

  for (i, key) in all_panel_keys().iter().enumerate() {
    let label = format!("panel-{}", key);
    let desired_visible = visible_set.contains(*key);
    let (w, h) = panel_base_size(key, &dashboard_profile, user_scale);

    // If already open, reconcile size/visibility instead of skipping.
    if let Some(win) = app.get_webview_window(&label) {
      let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize { width: w, height: h }));
      let _ = win.set_background_color(Some(Color(0, 0, 0, 0)));
      let _ = win.set_decorations(false);
      if desired_visible {
        let _ = win.show();
      } else {
        let _ = win.hide();
      }
      continue;
    }

    let url = format!("panel-{}.html", key);

    // Saved position or staggered default.
    let (saved_x, saved_y) = panel_layouts
      .get(*key)
      .map(|p| (p.x, p.y))
      .unwrap_or_else(|| (80 + i as i32 * 24, 80 + i as i32 * 24));

    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
      .title(*key)
      .inner_size(w, h)
      .position(saved_x as f64, saved_y as f64)
      .background_color(Color(0, 0, 0, 0))
      .decorations(false)
      .transparent(true)
      .resizable(false)
      .always_on_top(true)
      .skip_taskbar(true)
      .visible(desired_visible);

    // WebView2 can panic instead of returning Err when in a degraded state
    // (e.g. after the startup watchdog reloads the main webview). Catch the
    // panic so the process survives and the remaining panels can still open.
    let build_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| builder.build()));
    let win = match build_result {
      Ok(Ok(w)) => w,
      Ok(Err(e)) => {
        append_debug_log(app, &format!("panel window '{}' build failed: {}", label, e));
        continue;
      }
      Err(_) => {
        append_debug_log(
          app,
          &format!("panel window '{}' build panicked (WebView2 degraded)", label),
        );
        continue;
      }
    };

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
    if desired_visible {
      let _ = win.show();
    } else {
      let _ = win.hide();
    }
  }
}

/// Reconciles open floating panel windows with the current settings without
/// tearing down every panel window on each preview interaction.
pub fn sync_floating_panels(app: &AppHandle, state: &tauri::State<AppState>) {
  // Guard against stale queued sync tasks: if floating mode was disabled
  // before this sync runs, ensure all panel windows are closed and exit.
  let floating_enabled = {
    let s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    s.floating_mode
  };
  if !floating_enabled {
    close_floating_panels(app);
    return;
  }

  let (visible_panels, dashboard_profile, user_scale) = {
    let s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    (
      normalize_visible_panels(s.visible_panels.clone()),
      s.dashboard_profile.clone(),
      s.floating_panel_scale,
    )
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
      resize_existing_panel_window(app, key, &dashboard_profile, user_scale);
    }
  }

  append_debug_log(app, "floating sync: ensure missing windows");
  launch_floating_panels(app, state);

  // Transition safety: only hide the main window after at least one desired
  // floating panel is actually visible. If panel creation failed (e.g. WebView2
  // degraded), keep main visible so the app never appears to have crashed.
  let mut any_visible_panel = false;
  for key in &desired {
    let label = format!("panel-{}", key);
    if let Some(win) = app.get_webview_window(&label) {
      if win.is_visible().unwrap_or(false) {
        any_visible_panel = true;
        break;
      }
    }
  }

  if let Some(main) = app.get_webview_window("main") {
    if any_visible_panel {
      let _ = main.hide();
    } else {
      append_debug_log(app, "floating sync: no visible panel windows; keeping main visible");
      let _ = main.show();
      let _ = main.set_focus();
    }
  }
}

/// Schedules `sync_floating_panels` on the main event thread and returns
/// immediately. `WebviewWindowBuilder::build()` must run on the main thread;
/// calling it from an IPC handler thread blocks until each window is ready,
/// freezing the JS `await` in the settings window. Fire-and-forget via
/// `run_on_main_thread` keeps the IPC call responsive.
pub fn spawn_sync_floating_panels(app: &AppHandle) {
  if FLOATING_SYNC_QUEUED.swap(true, Ordering::SeqCst) {
    return;
  }

  let app2 = app.clone();
  if let Err(e) = app.run_on_main_thread(move || {
    FLOATING_SYNC_QUEUED.store(false, Ordering::SeqCst);

    let state = app2.state::<crate::stats::AppState>();
    // If floating mode was disabled before this closure ran (race between
    // enable and disable), do nothing — close_floating_panels already ran.
    let floating = {
      let s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
      s.floating_mode
    };
    if !floating {
      return;
    }
    sync_floating_panels(&app2, &state);
  }) {
    FLOATING_SYNC_QUEUED.store(false, Ordering::SeqCst);
    append_debug_log(app, &format!("floating sync: run_on_main_thread failed: {}", e));
  }
}

/// Hides all open floating panel windows.
///
/// We intentionally keep the webviews alive instead of destroying them to
/// avoid repeated WebView2 create/destroy churn during rapid mode toggles.
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
  match event {
    WindowEvent::CloseRequested { api, .. } if win.label() == "main" => {
      api.prevent_close();
      let _ = win.hide();
    }
    // Re-apply borderless on every move: Windows can restore WS_CAPTION when a
    // frameless window is dragged between monitors with different DPI settings.
    // Only applies to the main window and floating panel windows; settings/about/
    // status/updater windows have decorations and must keep them.
    WindowEvent::Moved(_) if win.label() == "main" || win.label().starts_with("panel-") => {
      let _ = win.set_decorations(false);
    }
    _ => {}
  }
}
