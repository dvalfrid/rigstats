mod brand;
mod panels;
mod ring;
mod spark;
mod tempcolor;
mod theme;
mod windows;

use eframe::egui;
use rigstats_backend::{debug, hardware, lhm, lhm_process, settings};
use spark::Sparkline;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};
use tray_icon::{
  menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
  Icon, TrayIconBuilder,
};

/// Commands sent from the tray-polling thread to the UI thread.
enum TrayCmd {
  Toggle,
  OpenSettings,
  OpenAbout,
  OpenStatus,
  OpenUpdater,
}

fn app_data_dir() -> PathBuf {
  let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
  PathBuf::from(appdata).join("se.codeby.rigstats")
}

// ── Data types exchanged between poll thread and UI thread ────────────────────

#[derive(Clone, Debug, Default)]
pub struct DriveInfo {
  pub fs: String,
  pub used: u64,
  pub total: u64,
  pub pct: u8,
  pub temp: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct ProcessInfo {
  pub name: String,
  pub cpu: f32,
  pub mem_mb: u64,
}

#[derive(Clone, Debug, Default)]
pub struct PollStats {
  // CPU
  pub cpu_load: u8,
  pub cpu_temp: Option<f64>,
  pub cpu_freq_mhz: f64,
  pub cpu_power: Option<f64>,
  pub cpu_cores: Vec<u8>,
  // GPU
  pub gpu_load: Option<f64>,
  pub gpu_temp: Option<f64>,
  pub gpu_hotspot: Option<f64>,
  pub gpu_freq_mhz: Option<f64>,
  pub gpu_mem_freq_mhz: Option<f64>,
  pub gpu_vram_used_mb: Option<f64>,
  pub gpu_vram_total_mb: Option<f64>,
  pub gpu_power: Option<f64>,
  pub gpu_fan: Option<f64>,
  pub gpu_d3d_3d: Option<f64>,
  pub gpu_d3d_vdec: Option<f64>,
  // RAM
  pub ram_used: u64,
  pub ram_total: u64,
  pub ram_spec: String,
  // Network
  pub net_up_mbps: f64,
  pub net_down_mbps: f64,
  pub net_iface: String,
  pub net_ping_ms: Option<f64>,
  // Disk
  pub disk_read_mbps: f64,
  pub disk_write_mbps: f64,
  pub disk_drives: Vec<DriveInfo>,
  // Motherboard (LHM)
  pub mb_fans: Vec<(String, f64)>,
  pub mb_temps: Vec<(String, f64)>,
  pub mb_voltages: Vec<(String, f64)>,
  pub mb_chip: Option<String>,
  pub mb_board: Option<String>,
  // Battery
  pub battery_present: bool,
  pub battery_charge_pct: Option<u8>,
  pub battery_charging: Option<bool>,
  pub battery_time_mins: Option<u32>,
  pub battery_power_w: Option<f64>,
  // Processes
  pub processes: Vec<ProcessInfo>,
  // System
  pub uptime_secs: u64,
  pub hostname: String,
  pub cpu_model: String,
  pub model_name: String,    // product model, e.g. "ROG GM700TZ"
  pub system_brand: String,  // brand key, e.g. "asus-rog"
  pub gpu_name: String,      // GPU display name, e.g. "AMD Radeon RX 9070 XT"
  pub ram_temp: Option<f64>,
  pub gpu_devices: Vec<(String, f64)>,
  // Meta
  pub lhm_connected: bool,
}

// ── Profile window dimensions ─────────────────────────────────────────────────

fn profile_to_size(profile: &str) -> [f32; 2] {
  match profile {
    "portrait-xl" => [450.0, 1920.0],
    "portrait-slim" => [480.0, 1920.0],
    "portrait-hd" => [720.0, 1280.0],
    "portrait-wxga" => [800.0, 1280.0],
    "portrait-fhd" => [1080.0, 1920.0],
    "portrait-wuxga" => [1200.0, 1920.0],
    "portrait-qhd" => [1440.0, 2560.0],
    "portrait-hdplus" => [768.0, 1366.0],
    "portrait-900x1600" => [900.0, 1600.0],
    "portrait-1050x1680" => [1050.0, 1680.0],
    "portrait-1600x2560" => [1600.0, 2560.0],
    "portrait-4k" => [2160.0, 3840.0],
    "portrait-fhd-side" => [253.0, 1080.0],
    "portrait-qhd-side" => [338.0, 1440.0],
    "portrait-4k-side" => [506.0, 2160.0],
    _ => [400.0, 780.0],
  }
}

/// Estimated window height for the given visible panels.
///
/// Values are calibrated estimates (content + frame inner margin 16 px).
/// Fine-tune by measuring actual rendered heights in the live app.
fn compute_window_height(visible_panels: &[String]) -> f32 {
  // panel_frame inner_margin: Margin::symmetric(12, 8) → 8 top + 8 bottom = 16 px.
  const V_MARGIN: f32 = 16.0;
  let header_h = theme::PANEL_HEADER_H + V_MARGIN; // 121
  let data_h   = theme::PANEL_DATA_H   + V_MARGIN; // 216
  let n = visible_panels.len();
  let mut h = theme::DRAG_HANDLE_H;
  for key in visible_panels {
    h += match key.as_str() {
      "header" | "clock" => header_h,
      _ => data_h,
    };
  }
  // add_space(6.0) follows every panel in the render loop.
  h + (n as f32 * 6.0)
}

// ── Windows monitor enumeration ───────────────────────────────────────────────

#[cfg(windows)]
mod win_monitor {
  use winapi::shared::minwindef::LPARAM;
  use winapi::shared::windef::{HDC, HMONITOR, LPRECT};
  use winapi::um::winuser::EnumDisplayMonitors;

  /// Returns (left, top, right, bottom) for every connected display.
  #[allow(unsafe_code)]
  pub fn list() -> Vec<(i32, i32, i32, i32)> {
    #[allow(unsafe_code)]
    unsafe extern "system" fn callback(_: HMONITOR, _: HDC, lp: LPRECT, data: LPARAM) -> i32 {
      let v = &mut *(data as *mut Vec<(i32, i32, i32, i32)>);
      let r = *lp;
      v.push((r.left, r.top, r.right, r.bottom));
      1
    }

    let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
    #[allow(unsafe_code)]
    unsafe {
      EnumDisplayMonitors(
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        Some(callback),
        &mut rects as *mut _ as LPARAM,
      );
    }
    rects
  }
}

/// Returns the (x, y) top-left position to center a window of `w × h` on the
/// first landscape monitor (or first monitor if all are portrait).
fn dialog_center(w: f32, h: f32) -> [f32; 2] {
  #[cfg(windows)]
  {
    let monitors = win_monitor::list();
    let picked = monitors
      .iter()
      .find(|&&(l, t, r, b)| (r - l) >= (b - t)) // landscape first
      .or_else(|| monitors.first());
    if let Some(&(l, t, r, b)) = picked {
      let cx = (l + r) as f32 / 2.0;
      let cy = (t + b) as f32 / 2.0;
      return [cx - w / 2.0, cy - h / 2.0];
    }
  }
  [100.0, 100.0]
}

/// Returns (x, y) position for the window — top-left of the best portrait monitor.
/// Falls back to (0, 0) if no portrait monitor is found or on non-Windows.
fn pick_window_position() -> [f32; 2] {
  #[cfg(windows)]
  {
    let monitors = win_monitor::list();
    // Prefer portrait (height > width), else use first monitor.
    let picked = monitors
      .iter()
      .find(|&&(l, t, r, b)| (b - t) > (r - l))
      .or_else(|| monitors.first());
    if let Some(&(x, y, _, _)) = picked {
      return [x as f32, y as f32];
    }
  }
  [0.0, 0.0]
}

// ── Tray icon ─────────────────────────────────────────────────────────────────

struct Tray {
  _icon: tray_icon::TrayIcon,
  show_id: tray_icon::menu::MenuId,
  settings_id: tray_icon::menu::MenuId,
  about_id: tray_icon::menu::MenuId,
  status_id: tray_icon::menu::MenuId,
  updater_id: tray_icon::menu::MenuId,
  quit_id: tray_icon::menu::MenuId,
}

fn load_tray_icon() -> Icon {
  let bytes = include_bytes!("../../assets/tray.png");
  let img = image::load_from_memory(bytes).expect("tray.png").to_rgba8();
  let (w, h) = img.dimensions();
  Icon::from_rgba(img.into_raw(), w, h).expect("tray icon rgba")
}

fn build_tray() -> Tray {
  let show_item = MenuItem::new("Show / Hide", true, None);
  let settings_item = MenuItem::new("Settings", true, None);
  let about_item = MenuItem::new("About", true, None);
  let status_item = MenuItem::new("Status", true, None);
  let updater_item = MenuItem::new("Check for Updates", true, None);
  let quit_item = MenuItem::new("Quit", true, None);

  let show_id = show_item.id().clone();
  let settings_id = settings_item.id().clone();
  let about_id = about_item.id().clone();
  let status_id = status_item.id().clone();
  let updater_id = updater_item.id().clone();
  let quit_id = quit_item.id().clone();

  let menu = Menu::new();
  let _ = menu.append(&show_item);
  let _ = menu.append(&PredefinedMenuItem::separator());
  let _ = menu.append(&settings_item);
  let _ = menu.append(&about_item);
  let _ = menu.append(&status_item);
  let _ = menu.append(&updater_item);
  let _ = menu.append(&PredefinedMenuItem::separator());
  let _ = menu.append(&quit_item);

  let icon = load_tray_icon();
  let tray_icon = TrayIconBuilder::new()
    .with_menu(Box::new(menu))
    .with_icon(icon)
    .with_tooltip("RigStats")
    .build()
    .expect("tray icon");

  Tray { _icon: tray_icon, show_id, settings_id, about_id, status_id, updater_id, quit_id }
}

// ── eframe application ────────────────────────────────────────────────────────

struct RigStatsApp {
  receiver: mpsc::Receiver<PollStats>,
  tray_rx: mpsc::Receiver<TrayCmd>,
  latest: PollStats,
  visible_panels: Vec<String>,
  opacity: f32,
  _tray: Tray,
  window_visible: bool,
  // Sparklines
  cpu_spark: Sparkline,
  gpu_spark: Sparkline,
  net_up_spark: Sparkline,
  net_dn_spark: Sparkline,
  // Brand textures (loaded once at startup)
  textures: brand::Textures,
  // Secondary windows
  settings_open: Arc<AtomicBool>,
  about_open: Arc<AtomicBool>,
  status_open: Arc<AtomicBool>,
  updater_open: Arc<AtomicBool>,
  // Set to true when a dialog is opened; cleared on first callback frame to send Focus.
  settings_focus: Arc<AtomicBool>,
  about_focus: Arc<AtomicBool>,
  status_focus: Arc<AtomicBool>,
  updater_focus: Arc<AtomicBool>,
  settings_win: Arc<Mutex<windows::settings::SettingsWindow>>,
  status_win: Arc<Mutex<windows::status::StatusState>>,
  updater_win: Arc<Mutex<windows::updater::UpdaterState>>,
  // Shared settings (updated on save, applied each frame)
  current_settings: Arc<Mutex<settings::Settings>>,
  settings_reload: Arc<AtomicBool>,
  preferred_gpu: Arc<Mutex<Option<String>>>,
  dir: Arc<PathBuf>,
}

impl RigStatsApp {
  #[allow(clippy::too_many_arguments)]
  fn new(
    receiver: mpsc::Receiver<PollStats>,
    tray_rx: mpsc::Receiver<TrayCmd>,
    visible_panels: Vec<String>,
    opacity: f32,
    tray: Tray,
    current_settings: Arc<Mutex<settings::Settings>>,
    settings_reload: Arc<AtomicBool>,
    dir: Arc<PathBuf>,
    textures: brand::Textures,
    preferred_gpu: Arc<Mutex<Option<String>>>,
  ) -> Self {
    let init_settings = current_settings.lock().unwrap().clone();
    Self {
      receiver,
      tray_rx,
      latest: PollStats::default(),
      visible_panels,
      opacity,
      _tray: tray,
      window_visible: true,
      cpu_spark: Sparkline::new(60),
      gpu_spark: Sparkline::new(60),
      net_up_spark: Sparkline::new(60),
      net_dn_spark: Sparkline::new(60),
      textures,
      settings_open: Arc::new(AtomicBool::new(false)),
      about_open: Arc::new(AtomicBool::new(false)),
      status_open: Arc::new(AtomicBool::new(false)),
      updater_open: Arc::new(AtomicBool::new(false)),
      settings_focus: Arc::new(AtomicBool::new(false)),
      about_focus: Arc::new(AtomicBool::new(false)),
      status_focus: Arc::new(AtomicBool::new(false)),
      updater_focus: Arc::new(AtomicBool::new(false)),
      settings_win: Arc::new(Mutex::new(
        windows::settings::SettingsWindow::from_settings(&init_settings),
      )),
      status_win: Arc::new(Mutex::new(windows::status::StatusState::load(&dir))),
      updater_win: Arc::new(Mutex::new(windows::updater::UpdaterState::default())),
      current_settings,
      settings_reload,
      preferred_gpu,
      dir,
    }
  }

}

impl eframe::App for RigStatsApp {
  fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
    // Background #0e1117 = (14/255, 17/255, 23/255); pre-multiplied with opacity.
    let a = self.opacity;
    [0.0549 * a, 0.0667 * a, 0.0902 * a, a]
  }

  fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
    // Pull latest stats from poll thread; push to sparklines only on new data.
    while let Ok(stats) = self.receiver.try_recv() {
      self.cpu_spark.push(stats.cpu_load as f32);
      self.gpu_spark.push(stats.gpu_load.unwrap_or(0.0) as f32);
      self.net_up_spark.push(stats.net_up_mbps as f32);
      self.net_dn_spark.push(stats.net_down_mbps as f32);
      self.latest = stats;
    }

    // Apply settings saved from the settings window.
    if self.settings_reload.swap(false, Ordering::Relaxed) {
      let s = self.current_settings.lock().unwrap();
      self.visible_panels = s.visible_panels.clone();
      self.opacity = s.opacity.clamp(0.1, 1.0) as f32;
      let level = match s.window_layer.as_str() {
        "on_top" => egui::WindowLevel::AlwaysOnTop,
        "behind" => egui::WindowLevel::AlwaysOnBottom,
        _ => egui::WindowLevel::Normal,
      };
      let [w, _] = profile_to_size(&s.dashboard_profile);
      let h = compute_window_height(&self.visible_panels);
      ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
      ui.ctx().send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
    }

    // Handle tray commands forwarded by the background polling thread.
    while let Ok(cmd) = self.tray_rx.try_recv() {
      match cmd {
        TrayCmd::Toggle => {
          self.window_visible = !self.window_visible;
          ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(self.window_visible));
        }
        TrayCmd::OpenSettings => {
          // Re-initialise draft from current settings each time the window opens.
          let s = self.current_settings.lock().unwrap().clone();
          *self.settings_win.lock().unwrap() =
            windows::settings::SettingsWindow::from_settings(&s);
          self.settings_open.store(true, Ordering::Relaxed);
          self.settings_focus.store(true, Ordering::Relaxed);
        }
        TrayCmd::OpenAbout => {
          self.about_open.store(true, Ordering::Relaxed);
          self.about_focus.store(true, Ordering::Relaxed);
        }
        TrayCmd::OpenStatus => {
          *self.status_win.lock().unwrap() = windows::status::StatusState::load(&self.dir);
          self.status_open.store(true, Ordering::Relaxed);
          self.status_focus.store(true, Ordering::Relaxed);
        }
        TrayCmd::OpenUpdater => {
          self.updater_open.store(true, Ordering::Relaxed);
          self.updater_focus.store(true, Ordering::Relaxed);
        }
      }
    }

    // ── Secondary windows ─────────────────────────────────────────────────

    let main_ctx = ui.ctx().clone();

    if self.settings_open.load(Ordering::Relaxed) {
      let open = self.settings_open.clone();
      let focus = self.settings_focus.clone();
      let state = self.settings_win.clone();
      let dir = self.dir.clone();
      let saved = self.current_settings.clone();
      let reload = self.settings_reload.clone();
      let mctx = main_ctx.clone();
      let [px, py] = dialog_center(560.0, 600.0);
      ui.ctx().show_viewport_deferred(
        egui::ViewportId::from_hash_of("settings"),
        egui::ViewportBuilder::default()
          .with_title("RigStats — Settings")
          .with_inner_size([560.0, 600.0])
          .with_position([px, py])
          .with_resizable(false)
          .with_always_on_top(),
        move |ctx, _class| {
          windows::settings::show(ctx, &mctx, &open, &focus, &state, &dir, &saved, &reload);
        },
      );
    }

    if self.about_open.load(Ordering::Relaxed) {
      let open = self.about_open.clone();
      let focus = self.about_focus.clone();
      let dir = self.dir.clone();
      let mctx = main_ctx.clone();
      let [px, py] = dialog_center(360.0, 280.0);
      ui.ctx().show_viewport_deferred(
        egui::ViewportId::from_hash_of("about"),
        egui::ViewportBuilder::default()
          .with_title("About RigStats")
          .with_inner_size([360.0, 280.0])
          .with_position([px, py])
          .with_resizable(false)
          .with_always_on_top(),
        move |ctx, _class| {
          windows::about::show(ctx, &mctx, &open, &focus, &dir);
        },
      );
    }

    if self.status_open.load(Ordering::Relaxed) {
      let open = self.status_open.clone();
      let focus = self.status_focus.clone();
      let state = self.status_win.clone();
      let dir = self.dir.clone();
      let mctx = main_ctx.clone();
      let [px, py] = dialog_center(520.0, 500.0);
      ui.ctx().show_viewport_deferred(
        egui::ViewportId::from_hash_of("status"),
        egui::ViewportBuilder::default()
          .with_title("RigStats — Status")
          .with_inner_size([520.0, 500.0])
          .with_position([px, py])
          .with_always_on_top(),
        move |ctx, _class| {
          windows::status::show(ctx, &mctx, &open, &focus, &state, &dir);
        },
      );
    }

    if self.updater_open.load(Ordering::Relaxed) {
      let open = self.updater_open.clone();
      let focus = self.updater_focus.clone();
      let state = self.updater_win.clone();
      let mctx = main_ctx.clone();
      let [px, py] = dialog_center(340.0, 220.0);
      ui.ctx().show_viewport_deferred(
        egui::ViewportId::from_hash_of("updater"),
        egui::ViewportBuilder::default()
          .with_title("RigStats — Updates")
          .with_inner_size([340.0, 220.0])
          .with_position([px, py])
          .with_resizable(false)
          .with_always_on_top(),
        move |ctx, _class| {
          windows::updater::show(ctx, &mctx, &open, &focus, &state);
        },
      );
    }

    // Drag handle — thin invisible strip at top for moving the borderless window.
    let drag_w = {
      let w = ui.available_width();
      if w.is_finite() && w > 0.0 { w } else { ui.ctx().content_rect().width().max(1.0) }
    };
    let (drag_rect, drag_resp) = ui.allocate_exact_size(
      egui::Vec2::new(drag_w, theme::DRAG_HANDLE_H),
      egui::Sense::drag(),
    );
    if drag_resp.dragged() {
      ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if drag_resp.hovered() {
      let painter = ui.painter();
      let cy = drag_rect.center().y;
      let cx = drag_rect.center().x;
      for i in [-5.0f32, 0.0, 5.0] {
        painter.circle_filled(
          egui::Pos2::new(cx + i, cy),
          1.5,
          egui::Color32::from_gray(160),
        );
      }
    }

    // Panels in the order defined by visible_panels (respects user reordering).
    let panels_to_draw = self.visible_panels.clone();
    let mut new_preferred_gpu: Option<String> = None;
    for panel in &panels_to_draw {
      match panel.as_str() {
        "header" => panels::header::draw(ui, &self.latest, &self.textures),
        "clock" => panels::clock::draw(ui, self.latest.uptime_secs),
        "cpu" => panels::cpu::draw(ui, &self.latest, &self.cpu_spark, &self.textures),
        "gpu" => {
          if let Some(p) = panels::gpu::draw(ui, &self.latest, &self.gpu_spark, &self.textures) {
            new_preferred_gpu = Some(p);
          }
        }
        "ram" => panels::ram::draw(ui, &self.latest),
        "net" => {
          panels::net::draw(ui, &self.latest, &self.net_up_spark, &self.net_dn_spark)
        }
        "disk" => {
          panels::disk::draw(ui, &self.latest)
        }
        "motherboard" => panels::motherboard::draw(ui, &self.latest),
        "process" => panels::process::draw(ui, &self.latest),
        "battery" => panels::battery::draw(ui, &self.latest),
        _ => {}
      }
      ui.add_space(6.0);
    }
    // Fit window height to actual rendered content every frame so no black gap
    // appears regardless of panel set, spacing, or egui version.
    let used_h = ui.min_rect().height();
    if used_h > 10.0 {
      let [w, _] = profile_to_size(&self.current_settings.lock().unwrap().dashboard_profile);
      ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
        egui::Vec2::new(w - 2.0, used_h),
      ));
    }

    if let Some(new_pref) = new_preferred_gpu {
      *self.preferred_gpu.lock().unwrap() = Some(new_pref.clone());
      let mut s = self.current_settings.lock().unwrap();
      s.preferred_gpu = Some(new_pref);
      let _ = settings::persist_settings(&self.dir, &s);
    }

    // Repaint faster when a secondary window is open so it closes within ~100 ms
    // of the user clicking X/Save/Cancel (the open flag is set in the callback
    // which runs after ui(), so the NEXT frame sees open=false and stops calling
    // show_viewport_deferred, which is what actually destroys the viewport).
    let any_dialog_open = self.settings_open.load(Ordering::Relaxed)
      || self.about_open.load(Ordering::Relaxed)
      || self.status_open.load(Ordering::Relaxed)
      || self.updater_open.load(Ordering::Relaxed);
    ui.ctx().request_repaint_after(if any_dialog_open {
      Duration::from_millis(100)
    } else {
      Duration::from_secs(1)
    });
  }
}

// ── LHM disk temp helper ──────────────────────────────────────────────────────

fn lhm_temp_for_model(wmi_model: &str, disk_temps: &[(String, f64)]) -> Option<f64> {
  let needle = wmi_model.trim().to_lowercase();
  if needle.is_empty() {
    return None;
  }
  disk_temps.iter().find_map(|(name, temp)| {
    let hay = name.trim().to_lowercase();
    if hay == needle || hay.contains(&needle) || needle.contains(&hay) {
      Some(*temp)
    } else {
      None
    }
  })
}

// ── Poll loop (tokio runtime) ─────────────────────────────────────────────────

async fn poll_loop(
  tx: mpsc::SyncSender<PollStats>,
  dir: PathBuf,
  preferred_gpu: Arc<Mutex<Option<String>>>,
) {
  let ram_spec = tokio::task::spawn_blocking(hardware::detect_ram_spec).await.unwrap_or_default();
  let disk_model_map: HashMap<String, String> =
    tokio::task::spawn_blocking(hardware::detect_disk_model_map).await.unwrap_or_default();
  let ping_target = hardware::detect_ping_target();
  let mb_board: Option<String> =
    tokio::task::spawn_blocking(hardware::detect_motherboard_name).await.ok().flatten();
  let model_name: String =
    tokio::task::spawn_blocking(hardware::detect_model_name).await.ok().flatten().unwrap_or_default();
  let system_brand: String =
    tokio::task::spawn_blocking(hardware::detect_system_brand).await.unwrap_or_default();
  let gpu_name: String =
    tokio::task::spawn_blocking(hardware::detect_gpu_name).await.ok().flatten().unwrap_or_default();

  let mut sys = System::new();
  sys.refresh_cpu();
  let cpu_model = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
  let hostname = System::host_name().unwrap_or_else(|| "—".to_string());

  let mut disks = Disks::new_with_refreshed_list();
  let mut networks = Networks::new_with_refreshed_list();
  let pipe = tokio::sync::Mutex::new(None::<lhm::LhmPipeReader>);

  let mut last_net_instant = Instant::now();
  let mut last_ping: Option<(Instant, Option<f64>)> = None;
  type BatteryCache = (u8, bool, Option<u32>, Option<f64>);
  let mut last_battery: Option<(Instant, BatteryCache)> = None;

  loop {
    sys.refresh_specifics(
      RefreshKind::new()
        .with_cpu(CpuRefreshKind::new().with_cpu_usage().with_frequency())
        .with_memory(MemoryRefreshKind::everything()),
    );
    sys.refresh_processes_specifics(sysinfo::ProcessRefreshKind::new().with_cpu().with_memory().with_disk_usage());

    let cpu_load = sys.global_cpu_info().cpu_usage() as u8;
    // global_cpu_info().frequency() returns 0 on Windows in sysinfo 0.30; use per-core average.
    let cpu_freq_mhz = {
      let cpus = sys.cpus();
      if cpus.is_empty() { 0.0 } else { cpus.iter().map(|c| c.frequency() as f64).sum::<f64>() / cpus.len() as f64 }
    };
    let cpu_cores: Vec<u8> = sys.cpus().iter().map(|c| c.cpu_usage() as u8).collect();
    let uptime_secs = System::uptime();

    let num_cpus = sys.cpus().len().max(1) as f32;
    let mut processes: Vec<ProcessInfo> = sys
      .processes()
      .values()
      .map(|p| ProcessInfo {
        name: p.name().to_string(),
        cpu: p.cpu_usage() / num_cpus,
        mem_mb: p.memory() / 1_048_576,
      })
      .collect();
    processes.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
    processes.truncate(8);

    disks.refresh();
    let mut disk_drives: Vec<DriveInfo> = disks
      .iter()
      .filter(|d| d.total_space() > 1_000_000_000)
      .map(|d| {
        let total = d.total_space();
        let used = total.saturating_sub(d.available_space());
        let pct = if total > 0 { ((used as f64 / total as f64) * 100.0) as u8 } else { 0 };
        DriveInfo { fs: d.mount_point().to_string_lossy().to_string(), used, total, pct, temp: None }
      })
      .collect();

    let now = Instant::now();
    let elapsed = now.duration_since(last_net_instant).as_secs_f64().max(0.5);
    last_net_instant = now;

    // disk_usage().read_bytes / written_bytes are per-tick deltas from sysinfo processes.
    let total_read_bytes: u64 = sys.processes().values().map(|p| p.disk_usage().read_bytes).sum();
    let total_write_bytes: u64 = sys.processes().values().map(|p| p.disk_usage().written_bytes).sum();
    let disk_read_mbps = total_read_bytes as f64 / elapsed / 1_048_576.0;
    let disk_write_mbps = total_write_bytes as f64 / elapsed / 1_048_576.0;
    networks.refresh();
    let mut best_iface = "--".to_string();
    let mut best_up = 0.0f64;
    let mut best_down = 0.0f64;
    for (name, data) in networks.iter() {
      let up = data.transmitted() as f64 * 8.0 / 1_000_000.0 / elapsed;
      let down = data.received() as f64 * 8.0 / 1_000_000.0 / elapsed;
      if up + down > best_up + best_down {
        best_up = up;
        best_down = down;
        best_iface = name.clone();
      }
    }

    let ping_ms = {
      let stale = last_ping.as_ref().map(|(t, _)| t.elapsed().as_secs_f64() >= 5.0).unwrap_or(true);
      if stale {
        let target = ping_target.clone();
        let measured = tokio::task::spawn_blocking(move || hardware::sample_ping_ms(&target))
          .await
          .unwrap_or(None);
        last_ping = Some((Instant::now(), measured));
        measured
      } else {
        last_ping.as_ref().and_then(|(_, v)| *v)
      }
    };

    let (battery_present, battery_charge_pct, battery_charging, battery_time_mins, battery_power_w) = {
      let stale =
        last_battery.as_ref().map(|(t, _)| t.elapsed().as_secs_f64() >= 10.0).unwrap_or(true);
      if stale {
        let result = tokio::task::spawn_blocking(hardware::sample_battery_wmi).await.ok().flatten();
        last_battery = result.as_ref().map(|d| (Instant::now(), *d));
        match result {
          Some((pct, charging, mins, w)) => (true, Some(pct), Some(charging), mins, w),
          None => (false, None, None, None, None),
        }
      } else {
        match last_battery.as_ref().map(|(_, d)| *d) {
          Some((pct, charging, mins, w)) => (true, Some(pct), Some(charging), mins, w),
          None => (false, None, None, None, None),
        }
      }
    };

    let pref = preferred_gpu.lock().unwrap().clone();
    let lhm_data = lhm::fetch_lhm_pipe(&pipe, pref.as_deref(), &dir).await;
    let lhm_connected = lhm_data.is_some();
    lhm_process::track_lhm_connection_state(&dir, lhm_connected);

    if let Some(ref lhm) = lhm_data {
      for (i, drive) in disk_drives.iter_mut().enumerate() {
        let key = drive.fs.trim_end_matches(['\\', '/']).to_string();
        if let Some(model) = disk_model_map.get(&key) {
          drive.temp = lhm_temp_for_model(model, &lhm.disk_temps);
        }
        if drive.temp.is_none() && !disk_model_map.contains_key(&key) {
          drive.temp = lhm.disk_temps.get(i).map(|(_, t)| *t);
        }
      }
    }

    let stats = PollStats {
      cpu_load,
      cpu_temp: lhm_data.as_ref().and_then(|l| l.cpu_temp),
      cpu_freq_mhz,
      cpu_power: lhm_data.as_ref().and_then(|l| l.cpu_power),
      cpu_cores,
      gpu_load: lhm_data.as_ref().and_then(|l| l.gpu_load),
      gpu_temp: lhm_data.as_ref().and_then(|l| l.gpu_temp),
      gpu_hotspot: lhm_data.as_ref().and_then(|l| l.gpu_hotspot),
      gpu_freq_mhz: lhm_data.as_ref().and_then(|l| l.gpu_freq),
      gpu_mem_freq_mhz: lhm_data.as_ref().and_then(|l| l.gpu_mem_freq),
      gpu_vram_used_mb: lhm_data.as_ref().and_then(|l| l.vram_used),
      gpu_vram_total_mb: lhm_data.as_ref().and_then(|l| l.vram_total),
      gpu_power: lhm_data.as_ref().and_then(|l| l.gpu_power),
      gpu_fan: lhm_data.as_ref().and_then(|l| l.gpu_fan),
      gpu_d3d_3d: lhm_data.as_ref().and_then(|l| l.gpu_d3d_3d),
      gpu_d3d_vdec: lhm_data.as_ref().and_then(|l| l.gpu_d3d_vdec),
      ram_used: sys.used_memory(),
      ram_total: sys.total_memory(),
      ram_spec: ram_spec.clone(),
      net_up_mbps: best_up,
      net_down_mbps: best_down,
      net_iface: best_iface,
      net_ping_ms: ping_ms,
      disk_read_mbps,
      disk_write_mbps,
      disk_drives,
      mb_fans: lhm_data.as_ref().map(|l| l.mb_fans.clone()).unwrap_or_default(),
      mb_temps: lhm_data.as_ref().map(|l| l.mb_temps.clone()).unwrap_or_default(),
      mb_voltages: lhm_data.as_ref().map(|l| l.mb_voltages.clone()).unwrap_or_default(),
      mb_chip: lhm_data.as_ref().and_then(|l| l.mb_chip.clone()),
      mb_board: mb_board.clone(),
      battery_present,
      battery_charge_pct,
      battery_charging,
      battery_time_mins,
      battery_power_w,
      processes,
      uptime_secs,
      hostname: hostname.clone(),
      cpu_model: cpu_model.clone(),
      model_name: model_name.clone(),
      system_brand: system_brand.clone(),
      gpu_name: lhm_data.as_ref().and_then(|l| l.gpu_name.clone()).unwrap_or_else(|| gpu_name.clone()),
      ram_temp: lhm_data.as_ref().and_then(|l| l.ram_temp),
      gpu_devices: lhm_data.as_ref().map(|l| l.gpu_devices.clone()).unwrap_or_default(),
      lhm_connected,
    };

    let _ = tx.send(stats);
    tokio::time::sleep(Duration::from_secs(1)).await;
  }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
  let dir = app_data_dir();
  debug::reset_debug_log(&dir);
  debug::append_debug_log(&dir, "rigstats-egui starting (Phase 6)");

  let s = settings::load_settings(&dir);
  let visible_panels = s.visible_panels.clone();
  let opacity = s.opacity.clamp(0.1, 1.0) as f32;
  let always_on_top = s.window_layer == "on_top";
  let [win_w, _] = profile_to_size(&s.dashboard_profile);
  let win_h = compute_window_height(&visible_panels);
  let [pos_x, pos_y] = pick_window_position();

  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .expect("tokio runtime");

  let preferred_gpu_arc: Arc<Mutex<Option<String>>> =
    Arc::new(Mutex::new(s.preferred_gpu.clone()));
  let (tx, rx) = mpsc::sync_channel::<PollStats>(4);
  let dir_clone = dir.clone();
  let pref_poll = preferred_gpu_arc.clone();
  runtime.spawn(async move { poll_loop(tx, dir_clone, pref_poll).await });

  let mut viewport = egui::ViewportBuilder::default()
    .with_title("RigStats")
    .with_inner_size([win_w - 2.0, win_h])
    .with_position([pos_x, pos_y])
    .with_decorations(false)
    .with_transparent(true);
  if always_on_top {
    viewport = viewport.with_always_on_top();
  }

  let options = eframe::NativeOptions { viewport, ..Default::default() };

  eframe::run_native(
    "RigStats",
    options,
    Box::new(|cc| {
      let mut visuals = egui::Visuals::dark();
      // Transparent background — colour comes from clear_color(); panels draw their own fill.
      visuals.panel_fill = egui::Color32::TRANSPARENT;
      visuals.window_fill = egui::Color32::TRANSPARENT;
      // Default text colour matches Tauri --text: #b8cce8
      visuals.override_text_color = Some(theme::C_TEXT);
      cc.egui_ctx.set_visuals(visuals);

      // Larger font sizes for readability on a portrait monitor.
      {
        use egui::{FontFamily, FontId, TextStyle};
        let mut style = (*cc.egui_ctx.global_style()).clone();
        style.text_styles = [
          (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
          (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
          (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
          (TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional)),
          (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();
        cc.egui_ctx.set_global_style(style);
      }

      let tray = build_tray();
      let (tray_tx, tray_rx) = mpsc::channel::<TrayCmd>();

      // Spawn a thread that polls tray events at 50 ms intervals and wakes the
      // egui event loop via request_repaint().  Quit is handled here directly
      // with process::exit so it is never delayed by a missed repaint.
      let ctx = cc.egui_ctx.clone();
      let quit_id = tray.quit_id.clone();
      let show_id = tray.show_id.clone();
      let settings_id = tray.settings_id.clone();
      let about_id = tray.about_id.clone();
      let status_id = tray.status_id.clone();
      let updater_id = tray.updater_id.clone();
      std::thread::spawn(move || loop {
        let mut repaint = false;
        if let Ok(ev) = MenuEvent::receiver().try_recv() {
          // Grant all processes foreground rights immediately while our process is
          // still active from the tray-menu interaction.  This allows the subsequent
          // ViewportCommand::Focus (sent on the first callback frame of each dialog)
          // to call SetForegroundWindow successfully — without this call Windows blocks
          // focus changes from background processes.
          #[cfg(windows)]
          #[allow(unsafe_code)]
          unsafe {
            winapi::um::winuser::AllowSetForegroundWindow(0xFFFF_FFFFu32); // ASFW_ANY
          }

          let cmd = if ev.id == quit_id {
            std::process::exit(0);
          } else if ev.id == show_id {
            Some(TrayCmd::Toggle)
          } else if ev.id == settings_id {
            Some(TrayCmd::OpenSettings)
          } else if ev.id == about_id {
            Some(TrayCmd::OpenAbout)
          } else if ev.id == status_id {
            Some(TrayCmd::OpenStatus)
          } else if ev.id == updater_id {
            Some(TrayCmd::OpenUpdater)
          } else {
            None
          };
          if let Some(c) = cmd {
            let _ = tray_tx.send(c);
            repaint = true;
          }
        }
        if let Ok(ev) = tray_icon::TrayIconEvent::receiver().try_recv() {
          if matches!(ev, tray_icon::TrayIconEvent::DoubleClick { .. }) {
            let _ = tray_tx.send(TrayCmd::Toggle);
            repaint = true;
          }
        }
        if repaint {
          ctx.request_repaint();
        }
        std::thread::sleep(Duration::from_millis(50));
      });

      let current_settings = Arc::new(Mutex::new(settings::load_settings(&dir)));
      let settings_reload = Arc::new(AtomicBool::new(false));
      let dir_arc = Arc::new(dir.clone());
      let textures = brand::Textures::load(&cc.egui_ctx);

      Ok(Box::new(RigStatsApp::new(
        rx,
        tray_rx,
        visible_panels,
        opacity,
        tray,
        current_settings,
        settings_reload,
        dir_arc,
        textures,
        preferred_gpu_arc,
      )))
    }),
  )
  .expect("eframe");

  runtime.shutdown_background();
}
