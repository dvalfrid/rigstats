#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod brand;
mod panels;
mod ring;
mod spark;
mod tempcolor;
mod theme;
mod update_check;
#[cfg(windows)]
mod win32_behind;
#[cfg(windows)]
mod win32_dark_mode;
#[cfg(windows)]
mod win_opacity;
mod windows;

use eframe::egui;
use rigstats_backend::{debug, hardware, lhm, lhm_process, logging, settings};
use spark::Sparkline;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, MouseButtonState, TrayIconBuilder,
};

/// Commands sent from the tray-polling thread to the UI thread.
enum TrayCmd {
    Toggle,
    OpenSettings,
    OpenAbout,
    OpenStatus,
    OpenUpdater,
    ToggleFloating,
    ToggleRecording,
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
    pub model_name: String,   // product model, e.g. "ROG GM700TZ"
    pub system_brand: String, // brand key, e.g. "asus-rog"
    pub gpu_name: String,     // GPU display name, e.g. "AMD Radeon RX 9070 XT"
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
    let data_h = theme::PANEL_DATA_H + V_MARGIN; // 216
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
    use winapi::um::shellscalingapi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use winapi::um::winuser::EnumDisplayMonitors;

    struct MonitorData {
        rects: Vec<(i32, i32, i32, i32)>,
    }

    /// Returns (left, top, right, bottom) for every connected display in **logical pixels**.
    /// Physical pixel coordinates from EnumDisplayMonitors are divided by the monitor's
    /// effective DPI scale so that egui window positions (which are in logical pixels) land
    /// on the correct screen position regardless of per-monitor DPI scaling.
    #[allow(unsafe_code)]
    pub fn list() -> Vec<(i32, i32, i32, i32)> {
        #[allow(unsafe_code)]
        unsafe extern "system" fn callback(hm: HMONITOR, _: HDC, lp: LPRECT, data: LPARAM) -> i32 {
            let d = &mut *(data as *mut MonitorData);
            let r = *lp;
            let mut dpi_x: u32 = 96;
            let mut dpi_y: u32 = 96;
            let _ = GetDpiForMonitor(hm, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let sx = dpi_x as f32 / 96.0;
            let sy = dpi_y as f32 / 96.0;
            d.rects.push((
                (r.left as f32 / sx) as i32,
                (r.top as f32 / sy) as i32,
                (r.right as f32 / sx) as i32,
                (r.bottom as f32 / sy) as i32,
            ));
            1
        }

        let mut data = MonitorData { rects: Vec::new() };
        #[allow(unsafe_code)]
        unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Some(callback),
                &mut data as *mut _ as LPARAM,
            );
        }
        data.rects
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
    icon: tray_icon::TrayIcon,
    show_id: tray_icon::menu::MenuId,
    settings_id: tray_icon::menu::MenuId,
    about_id: tray_icon::menu::MenuId,
    status_id: tray_icon::menu::MenuId,
    updater_id: tray_icon::menu::MenuId,
    quit_id: tray_icon::menu::MenuId,
    floating_id: tray_icon::menu::MenuId,
    recording_id: tray_icon::menu::MenuId,
    recording_item: tray_icon::menu::MenuItem,
}

fn load_tray_icon() -> Icon {
    let bytes = include_bytes!("../../assets/tray.png");
    let img = image::load_from_memory(bytes).expect("tray.png").to_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).expect("tray icon rgba")
}

/// Load tray.png as egui IconData for dialog viewport windows.
fn load_app_icon() -> egui::IconData {
    let bytes = include_bytes!("../../assets/tray.png");
    let img = image::load_from_memory(bytes).expect("tray.png").to_rgba8();
    let (w, h) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    }
}

fn build_tray(logging_enabled: bool) -> Tray {
    let show_item = MenuItem::new("Show / Hide", true, None);
    let floating_item = MenuItem::new("Toggle Floating Mode", true, None);
    let recording_label = if logging_enabled {
        "Stop Recording"
    } else {
        "Start Recording"
    };
    let recording_item = MenuItem::new(recording_label, true, None);
    let settings_item = MenuItem::new("Settings", true, None);
    let about_item = MenuItem::new("About", true, None);
    let status_item = MenuItem::new("Status", true, None);
    let updater_item = MenuItem::new("Check for Updates", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let show_id = show_item.id().clone();
    let floating_id = floating_item.id().clone();
    let recording_id = recording_item.id().clone();
    let settings_id = settings_item.id().clone();
    let about_id = about_item.id().clone();
    let status_id = status_item.id().clone();
    let updater_id = updater_item.id().clone();
    let quit_id = quit_item.id().clone();

    let menu = Menu::new();
    let _ = menu.append(&show_item);
    let _ = menu.append(&floating_item);
    let _ = menu.append(&recording_item);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&settings_item);
    let _ = menu.append(&about_item);
    let _ = menu.append(&status_item);
    let _ = menu.append(&updater_item);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit_item);

    let icon = if logging_enabled {
        let bytes = include_bytes!("../../assets/tray-recording.png");
        let img = image::load_from_memory(bytes)
            .expect("tray-recording.png")
            .to_rgba8();
        let (w, h) = img.dimensions();
        Icon::from_rgba(img.into_raw(), w, h).expect("tray recording icon rgba")
    } else {
        load_tray_icon()
    };
    let tooltip = if logging_enabled {
        "RigStats \u{2014} Recording"
    } else {
        "RigStats"
    };
    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip(tooltip)
        .build()
        .expect("tray icon");

    Tray {
        icon: tray_icon,
        show_id,
        settings_id,
        about_id,
        status_id,
        updater_id,
        quit_id,
        floating_id,
        recording_id,
        recording_item,
    }
}

impl Tray {
    fn set_recording(&self, enabled: bool) {
        let label = if enabled {
            "Stop Recording"
        } else {
            "Start Recording"
        };
        self.recording_item.set_text(label);
        let bytes: &[u8] = if enabled {
            include_bytes!("../../assets/tray-recording.png")
        } else {
            include_bytes!("../../assets/tray.png")
        };
        if let Ok(img) = image::load_from_memory(bytes) {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            if let Ok(icon) = Icon::from_rgba(rgba.into_raw(), w, h) {
                let _ = self.icon.set_icon(Some(icon));
            }
        }
        let tooltip = if enabled {
            "RigStats \u{2014} Recording"
        } else {
            "RigStats"
        };
        let _ = self.icon.set_tooltip(Some(tooltip));
    }
}

// ── Panel helpers ─────────────────────────────────────────────────────────────

fn panel_label(key: &str) -> &'static str {
    match key {
        "header" => "Header",
        "clock" => "Clock",
        "cpu" => "CPU",
        "gpu" => "GPU",
        "ram" => "RAM",
        "net" => "Network",
        "disk" => "Disk",
        "motherboard" => "Motherboard",
        "process" => "Processes",
        "battery" => "Battery",
        _ => "Panel",
    }
}

/// Initial window height estimate for a panel (content + frame inner margin 16 px).
fn panel_initial_h(key: &str) -> f32 {
    match key {
        "header" | "clock" => theme::PANEL_HEADER_H + 16.0,
        _ => theme::PANEL_DATA_H + 16.0,
    }
}

// ── eframe application ────────────────────────────────────────────────────────

struct RigStatsApp {
    receiver: mpsc::Receiver<PollStats>,
    tray_rx: mpsc::Receiver<TrayCmd>,
    latest: PollStats,
    visible_panels: Vec<String>,
    opacity: f32,
    tray: Tray,
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
    // true while a manual check/download is in flight (prevents double-trigger)
    updater_busy: Arc<AtomicBool>,
    // Shared settings (updated on save, applied each frame)
    current_settings: Arc<Mutex<settings::Settings>>,
    settings_reload: Arc<AtomicBool>,
    preferred_gpu: Arc<Mutex<Option<String>>>,
    dir: Arc<PathBuf>,
    // Win32 HWND stored as isize for window-level opacity (SetLayeredWindowAttributes).
    // Found via FindWindowW on the first ui() frame; 0 until then.
    hwnd: isize,
    // ── Floating mode ──────────────────────────────────────────────────────
    floating_mode: bool,
    floating_panels_locked: bool,
    floating_panel_scale: f32,
    /// Last-known screen positions for each panel key, keyed by panel key.
    /// Loaded from settings at startup; updated on drag; persisted on change.
    floating_positions: Arc<Mutex<HashMap<String, [f32; 2]>>>,
    /// Set true inside a floating panel viewport when its position changes.
    /// Consumed in `ui()` to debounce settings writes to once per tick.
    positions_dirty: Arc<AtomicBool>,
    /// Receives a new preferred-GPU name when the user clicks a GPU dot
    /// inside the floating GPU panel viewport.
    float_new_pref_gpu: Arc<Mutex<Option<String>>>,
    /// Live lock state toggled from the padlock icon in the drag handle.
    /// Propagated back to `floating_panels_locked` and persisted in `update()`.
    floating_lock_arc: Arc<AtomicBool>,
    /// Guards the one-time initial hide of the main window when the app
    /// starts with floating_mode already enabled.
    initial_floating_applied: bool,
    /// Tracks which floating panel viewports have already had their initial
    /// position applied.  Once a panel is in this set, `with_position` is
    /// NOT included in the ViewportBuilder — the OS owns the position from
    /// that point on, which prevents the builder diff from continuously
    /// sending SetOuterPosition and causing sub-pixel blur.
    /// Cleared whenever floating mode transitions from off → on so positions
    /// are restored from the saved layout on next activation.
    panels_positioned: HashSet<String>,
    /// Shared with the heartbeat thread so it knows whether to drive parent repaints.
    floating_mode_arc: Arc<AtomicBool>,
    /// Live thresholds for temperature colour coding — updated on settings reload.
    thresholds: PanelThresholds,
    /// Active panel theme derived from `Settings.theme`.
    app_theme: theme::AppTheme,
}

/// Per-component warn/crit thresholds (°C) used for temperature colour coding.
#[derive(Clone)]
struct PanelThresholds {
    cpu: (u8, u8),
    gpu: (u8, u8),
    gpu_hotspot: (u8, u8),
    ram: (u8, u8),
    disk: (u8, u8),
    mb: (u8, u8),
}

impl Default for PanelThresholds {
    fn default() -> Self {
        Self {
            cpu: (80, 90),
            gpu: (80, 90),
            gpu_hotspot: (90, 105),
            ram: (60, 70),
            disk: (50, 60),
            mb: (70, 90),
        }
    }
}

impl PanelThresholds {
    fn from_settings(s: &settings::Settings) -> Self {
        let get = |key: &str, default: (u8, u8)| -> (u8, u8) {
            s.thresholds.get(key).map_or(default, |t| {
                (t.warn.unwrap_or(default.0), t.crit.unwrap_or(default.1))
            })
        };
        let def = Self::default();
        Self {
            cpu: get("cpu", def.cpu),
            gpu: get("gpu", def.gpu),
            gpu_hotspot: def.gpu_hotspot, // not user-configurable
            ram: get("ram", def.ram),
            disk: get("disk", def.disk),
            mb: def.mb, // not user-configurable
        }
    }
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
        floating_mode_arc: Arc<AtomicBool>,
        updater_win: Arc<Mutex<windows::updater::UpdaterState>>,
        updater_open: Arc<AtomicBool>,
        updater_focus: Arc<AtomicBool>,
    ) -> Self {
        let init_settings = current_settings.lock().unwrap().clone();
        let init_positions: HashMap<String, [f32; 2]> = init_settings
            .panel_layouts
            .iter()
            .map(|(k, v)| (k.clone(), [v.x as f32, v.y as f32]))
            .collect();
        Self {
            receiver,
            tray_rx,
            latest: PollStats::default(),
            visible_panels,
            opacity,
            tray,
            window_visible: true,
            cpu_spark: Sparkline::new(60),
            gpu_spark: Sparkline::new(60),
            net_up_spark: Sparkline::new(60),
            net_dn_spark: Sparkline::new(60),
            textures,
            settings_open: Arc::new(AtomicBool::new(false)),
            about_open: Arc::new(AtomicBool::new(false)),
            status_open: Arc::new(AtomicBool::new(false)),
            updater_open,
            settings_focus: Arc::new(AtomicBool::new(false)),
            about_focus: Arc::new(AtomicBool::new(false)),
            status_focus: Arc::new(AtomicBool::new(false)),
            updater_focus,
            settings_win: Arc::new(Mutex::new(
                windows::settings::SettingsWindow::from_settings(&init_settings),
            )),
            status_win: Arc::new(Mutex::new(windows::status::StatusState::load(&dir, false))),
            updater_win,
            updater_busy: Arc::new(AtomicBool::new(false)),
            current_settings,
            settings_reload,
            preferred_gpu,
            dir,
            hwnd: 0,
            floating_mode: init_settings.floating_mode,
            floating_panels_locked: init_settings.floating_panels_locked,
            floating_panel_scale: init_settings.floating_panel_scale.clamp(0.4, 1.0) as f32,
            floating_positions: Arc::new(Mutex::new(init_positions)),
            positions_dirty: Arc::new(AtomicBool::new(false)),
            float_new_pref_gpu: Arc::new(Mutex::new(None)),
            floating_lock_arc: Arc::new(AtomicBool::new(init_settings.floating_panels_locked)),
            initial_floating_applied: false,
            panels_positioned: HashSet::new(),
            floating_mode_arc,
            thresholds: PanelThresholds::from_settings(&init_settings),
            app_theme: theme::AppTheme::from_key(&init_settings.theme),
        }
    }
}

impl eframe::App for RigStatsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Solid dark background matching PANEL_FILL. Window-level opacity is
        // applied by SetLayeredWindowAttributes so the swap chain stays opaque.
        let c = theme::PANEL_FILL;
        [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            1.0,
        ]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // On the first frame: locate the HWND and apply the initial window opacity.
        #[cfg(windows)]
        if self.hwnd == 0 {
            self.hwnd = win_opacity::find_hwnd("RigStats");
            if self.hwnd != 0 {
                win_opacity::set_opacity(self.hwnd, self.opacity);
            }
        }

        // Pull latest stats from poll thread; push to sparklines only on new data.
        while let Ok(stats) = self.receiver.try_recv() {
            let cpu_v = stats.cpu_load as f32;
            let gpu_v = stats.gpu_load.unwrap_or(0.0) as f32;
            let nu_v = stats.net_up_mbps as f32;
            let nd_v = stats.net_down_mbps as f32;
            self.cpu_spark.push(cpu_v);
            self.gpu_spark.push(gpu_v);
            self.net_up_spark.push(nu_v);
            self.net_dn_spark.push(nd_v);
            self.latest = stats;
        }

        // Apply settings saved from the settings window.
        if self.settings_reload.swap(false, Ordering::Relaxed) {
            let s = self.current_settings.lock().unwrap();
            let prev_visible = self.visible_panels.clone();
            self.visible_panels = s.visible_panels.clone();
            // Any panel that was visible before but is now hidden must be removed
            // from panels_positioned so its saved position is re-applied when it
            // reappears (e.g. after cancel reverts a live preview toggle).
            for key in &prev_visible {
                if !self.visible_panels.contains(key) {
                    self.panels_positioned.remove(key);
                }
            }
            self.opacity = s.opacity.clamp(0.1, 1.0) as f32;
            self.thresholds = PanelThresholds::from_settings(&s);
            self.app_theme = theme::AppTheme::from_key(&s.theme);
            let level = match s.window_layer.as_str() {
                "on_top" => egui::WindowLevel::AlwaysOnTop,
                "behind" => egui::WindowLevel::AlwaysOnBottom,
                _ => egui::WindowLevel::Normal,
            };
            let was_floating = self.floating_mode;
            self.floating_mode = s.floating_mode;
            self.floating_mode_arc
                .store(self.floating_mode, Ordering::Relaxed);
            self.floating_panels_locked = s.floating_panels_locked;
            self.floating_panel_scale = s.floating_panel_scale.clamp(0.4, 1.0) as f32;
            drop(s);
            let [w, _] = profile_to_size(&self.current_settings.lock().unwrap().dashboard_profile);
            let h = compute_window_height(&self.visible_panels);
            // Only resize/reposition the main window when in fixed (non-floating) mode.
            if !self.floating_mode {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
            }
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            #[cfg(windows)]
            win_opacity::set_opacity(self.hwnd, self.opacity);
            // Toggle main window position when floating mode changes.
            // We move it off-screen instead of hiding it — a hidden window is not
            // ticked by eframe, so the floating panels would not update.
            if was_floating != self.floating_mode {
                if self.floating_mode {
                    // Clear positioned-set so restored positions are re-applied
                    // from the saved layout the first time each panel is shown.
                    self.panels_positioned.clear();
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                            -32000.0, -32000.0,
                        )));
                } else {
                    // Restore to the correct portrait monitor position.
                    let [px, py] = pick_window_position();
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                            px, py,
                        )));
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
                }
            }
        }

        // Handle tray commands forwarded by the background polling thread.
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCmd::Toggle => {
                    self.window_visible = !self.window_visible;
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Visible(self.window_visible));
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
                    *self.status_win.lock().unwrap() =
                        windows::status::StatusState::load(&self.dir, self.latest.lhm_connected);
                    self.status_open.store(true, Ordering::Relaxed);
                    self.status_focus.store(true, Ordering::Relaxed);
                }
                TrayCmd::OpenUpdater => {
                    self.updater_open.store(true, Ordering::Relaxed);
                    self.updater_focus.store(true, Ordering::Relaxed);
                }
                TrayCmd::ToggleFloating => {
                    let new_mode = {
                        let mut s = self.current_settings.lock().unwrap();
                        s.floating_mode = !s.floating_mode;
                        let _ = settings::persist_settings(&self.dir, &s);
                        s.floating_mode
                    };
                    let was_floating = self.floating_mode;
                    self.floating_mode = new_mode;
                    self.floating_mode_arc.store(new_mode, Ordering::Relaxed);
                    if was_floating != new_mode {
                        if new_mode {
                            self.panels_positioned.clear();
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::Pos2::new(-32000.0, -32000.0),
                                ));
                        } else {
                            let [px, py] = pick_window_position();
                            let s = self.current_settings.lock().unwrap();
                            let [w, _] = profile_to_size(&s.dashboard_profile);
                            let h = compute_window_height(&self.visible_panels);
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::Pos2::new(px, py),
                                ));
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                egui::Vec2::new(w, h),
                            ));
                        }
                    }
                }
                TrayCmd::ToggleRecording => {
                    let (new_enabled, retention_days) = {
                        let mut s = self.current_settings.lock().unwrap();
                        s.logging_enabled = !s.logging_enabled;
                        let _ = settings::persist_settings(&self.dir, &s);
                        (s.logging_enabled, s.log_retention_days)
                    };
                    if new_enabled {
                        logging::prune_old_logs(&self.dir, retention_days);
                    }
                    self.tray.set_recording(new_enabled);
                }
            }
        }

        // Sync floating lock state toggled by padlock icon in drag handle.
        let arc_locked = self.floating_lock_arc.load(Ordering::Relaxed);
        if arc_locked != self.floating_panels_locked {
            self.floating_panels_locked = arc_locked;
            let mut s = self.current_settings.lock().unwrap();
            s.floating_panels_locked = arc_locked;
            let _ = settings::persist_settings(&self.dir, &s);
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
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("settings"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Settings")
                    .with_inner_size([560.0, 600.0])
                    .with_position([px, py])
                    .with_resizable(false)
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    // Capture HWND while we're inside the callback — window definitely exists here.
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("RigStats \u{2014} Settings");
                    }
                    windows::settings::show(
                        child_ui.ctx(),
                        &mctx,
                        &open,
                        &focus,
                        &state,
                        &dir,
                        &saved,
                        &reload,
                    );
                },
            );
            // Bring dialog to foreground now that we have the HWND.
            // If found_hwnd == 0 the window wasn't ready yet; restore the focus flag
            // so we retry on the next frame (which fires within 100 ms since a dialog is open).
            #[cfg(windows)]
            if wants_focus {
                if found_hwnd != 0 {
                    win_opacity::bring_to_foreground(found_hwnd);
                } else {
                    focus.store(true, Ordering::Relaxed);
                }
            }
        }

        if self.about_open.load(Ordering::Relaxed) {
            let open = self.about_open.clone();
            let focus = self.about_focus.clone();
            let dir = self.dir.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(360.0, 280.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("about"),
                egui::ViewportBuilder::default()
                    .with_title("About RigStats")
                    .with_inner_size([380.0, 420.0])
                    .with_position([px, py])
                    .with_resizable(false)
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("About RigStats");
                    }
                    windows::about::show(child_ui.ctx(), &mctx, &open, &focus, &dir);
                },
            );
            #[cfg(windows)]
            if wants_focus {
                if found_hwnd != 0 {
                    win_opacity::bring_to_foreground(found_hwnd);
                } else {
                    focus.store(true, Ordering::Relaxed);
                }
            }
        }

        if self.status_open.load(Ordering::Relaxed) {
            let open = self.status_open.clone();
            let focus = self.status_focus.clone();
            let state = self.status_win.clone();
            let dir = self.dir.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(680.0, 720.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let lhm_connected = self.latest.lhm_connected;
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("status"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Status")
                    .with_inner_size([680.0, 720.0])
                    .with_position([px, py])
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("RigStats \u{2014} Status");
                    }
                    windows::status::show(
                        child_ui.ctx(),
                        &mctx,
                        &open,
                        &focus,
                        &state,
                        &dir,
                        lhm_connected,
                    );
                },
            );
            #[cfg(windows)]
            if wants_focus {
                if found_hwnd != 0 {
                    win_opacity::bring_to_foreground(found_hwnd);
                } else {
                    focus.store(true, Ordering::Relaxed);
                }
            }
        }

        if self.updater_open.load(Ordering::Relaxed) {
            let open = self.updater_open.clone();
            let focus = self.updater_focus.clone();
            let state = self.updater_win.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(490.0, 560.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("updater"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats Update")
                    .with_inner_size([490.0, 560.0])
                    .with_position([px, py])
                    .with_resizable(false)
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("RigStats Update");
                    }
                    windows::updater::show(child_ui.ctx(), &mctx, &open, &focus, &state);
                },
            );
            #[cfg(windows)]
            if wants_focus {
                if found_hwnd != 0 {
                    win_opacity::bring_to_foreground(found_hwnd);
                } else {
                    focus.store(true, Ordering::Relaxed);
                }
            }

            // When the window sets status to Checking (manual button click),
            // kick off a check+download task on the tokio runtime.
            let is_checking = matches!(
                self.updater_win.lock().unwrap().status,
                windows::updater::UpdateStatus::Checking
            );
            if is_checking && !self.updater_busy.swap(true, Ordering::Relaxed) {
                let win = self.updater_win.clone();
                let busy = self.updater_busy.clone();
                let ctx = ui.ctx().clone();
                tokio::spawn(async move {
                    let result = tokio::task::spawn_blocking(update_check::check)
                        .await
                        .unwrap_or_else(|_| Err("task panic".to_string()));
                    match result {
                        Ok(update_check::CheckResult::UpdateAvailable(info)) => {
                            let version = info.version.clone();
                            let url = info.url.clone();
                            let dest = update_check::installer_temp_path(&version);
                            {
                                let mut s = win.lock().unwrap();
                                s.status = windows::updater::UpdateStatus::Downloading {
                                    downloaded: 0,
                                    total: 0,
                                };
                            }
                            ctx.request_repaint();
                            let win2 = win.clone();
                            let ctx2 = ctx.clone();
                            let dest2 = dest.clone();
                            let dl_result = tokio::task::spawn_blocking(move || {
                                update_check::download(&url, &dest2, |downloaded, total| {
                                    let mut s = win2.lock().unwrap();
                                    s.status = windows::updater::UpdateStatus::Downloading {
                                        downloaded,
                                        total,
                                    };
                                    ctx2.request_repaint();
                                })
                            })
                            .await
                            .unwrap_or_else(|_| Err("download task panic".to_string()));
                            let mut s = win.lock().unwrap();
                            s.status = match dl_result {
                                Ok(()) => windows::updater::UpdateStatus::Ready {
                                    info,
                                    installer_path: dest,
                                },
                                Err(e) => windows::updater::UpdateStatus::Error(e),
                            };
                        }
                        Ok(update_check::CheckResult::UpToDate) => {
                            win.lock().unwrap().status = windows::updater::UpdateStatus::UpToDate;
                        }
                        Err(e) => {
                            win.lock().unwrap().status = windows::updater::UpdateStatus::Error(e);
                        }
                    }
                    ctx.request_repaint();
                    busy.store(false, Ordering::Relaxed);
                });
            }
        }

        // On the first frame: move main window off-screen when floating mode is active.
        // We NEVER use Visible(false) in floating mode because a hidden window is not
        // ticked by eframe — show_viewport_immediate would stop being called and all
        // floating panels would freeze.
        if !self.initial_floating_applied {
            self.initial_floating_applied = true;
            if self.floating_mode {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                        -32000.0, -32000.0,
                    )));
            }
        }

        let any_dialog_open = self.settings_open.load(Ordering::Relaxed)
            || self.about_open.load(Ordering::Relaxed)
            || self.status_open.load(Ordering::Relaxed)
            || self.updater_open.load(Ordering::Relaxed);

        if self.floating_mode {
            // ── Floating mode — each panel in its own borderless viewport ─────
            self.render_floating_panels(ui);

            // Persist positions when any panel was dragged.
            if self.positions_dirty.swap(false, Ordering::Relaxed) {
                self.persist_floating_positions();
            }

            // Apply GPU preference change made from the floating GPU panel.
            if let Some(new_pref) = self.float_new_pref_gpu.lock().unwrap().take() {
                *self.preferred_gpu.lock().unwrap() = Some(new_pref.clone());
                let mut s = self.current_settings.lock().unwrap();
                s.preferred_gpu = Some(new_pref);
                let _ = settings::persist_settings(&self.dir, &s);
            }
        } else {
            // ── Fixed mode — all panels in one portrait window ────────────────

            // Drag handle — thin invisible strip at top for moving the borderless window.
            let drag_w = {
                let w = ui.available_width();
                if w.is_finite() && w > 0.0 {
                    w
                } else {
                    ui.ctx().content_rect().width().max(1.0)
                }
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
            // Extract update version once per frame (cheap lock read).
            let update_ver: Option<String> = {
                use windows::updater::UpdateStatus;
                let st = self.updater_win.lock().unwrap();
                if let UpdateStatus::Ready { info, .. } = &st.status {
                    Some(info.version.clone())
                } else {
                    None
                }
            };
            for panel in &panels_to_draw {
                match panel.as_str() {
                    // Panels always render at full opacity; window-level transparency is
                    // applied by SetLayeredWindowAttributes (win_opacity module).
                    "header" => {
                        let _ = panels::header::draw(
                            ui,
                            &self.latest,
                            &self.textures,
                            1.0,
                            &self.app_theme,
                            1.0,
                        );
                    }
                    "clock" => {
                        let _ = panels::clock::draw(
                            ui,
                            self.latest.uptime_secs,
                            1.0,
                            &self.app_theme,
                            update_ver.as_deref(),
                            1.0,
                        );
                        // Badge click → open updater dialog.
                        if ui
                            .ctx()
                            .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("open_updater")))
                            .unwrap_or(false)
                        {
                            self.updater_open.store(true, Ordering::Relaxed);
                        }
                    }
                    "cpu" => {
                        let _ = panels::cpu::draw(
                            ui,
                            &self.latest,
                            &self.cpu_spark,
                            &self.textures,
                            1.0,
                            self.thresholds.cpu.0,
                            self.thresholds.cpu.1,
                            &self.app_theme,
                            1.0,
                        );
                    }
                    "gpu" => {
                        if let Some(p) = panels::gpu::draw(
                            ui,
                            &self.latest,
                            &self.gpu_spark,
                            &self.textures,
                            1.0,
                            &self.app_theme,
                            self.thresholds.gpu.0,
                            self.thresholds.gpu.1,
                            1.0,
                        )
                        .0
                        {
                            new_preferred_gpu = Some(p);
                        }
                    }
                    "ram" => {
                        let _ = panels::ram::draw(
                            ui,
                            &self.latest,
                            1.0,
                            self.thresholds.ram.0,
                            self.thresholds.ram.1,
                            &self.app_theme,
                            1.0,
                        );
                    }
                    "net" => {
                        let _ = panels::net::draw(
                            ui,
                            &self.latest,
                            &self.net_up_spark,
                            &self.net_dn_spark,
                            1.0,
                            &self.app_theme,
                            1.0,
                        );
                    }
                    "disk" => {
                        let _ = panels::disk::draw(
                            ui,
                            &self.latest,
                            1.0,
                            self.thresholds.disk.0,
                            self.thresholds.disk.1,
                            &self.app_theme,
                            1.0,
                        );
                    }
                    "motherboard" => {
                        let _ = panels::motherboard::draw(
                            ui,
                            &self.latest,
                            1.0,
                            self.thresholds.mb.0,
                            self.thresholds.mb.1,
                            &self.app_theme,
                            1.0,
                        );
                    }
                    "process" => {
                        let _ = panels::process::draw(ui, &self.latest, 1.0, &self.app_theme, 1.0);
                    }
                    "battery" => {
                        let _ = panels::battery::draw(ui, &self.latest, 1.0, &self.app_theme, 1.0);
                    }
                    _ => {}
                }
                ui.add_space(6.0);
            }
            // Fit window height to actual rendered content every frame so no black gap
            // appears regardless of panel set, spacing, or egui version.
            let used_h = ui.min_rect().height();
            if used_h > 10.0 {
                let [w, _] =
                    profile_to_size(&self.current_settings.lock().unwrap().dashboard_profile);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                        w - 2.0,
                        used_h,
                    )));
            }

            if let Some(new_pref) = new_preferred_gpu {
                *self.preferred_gpu.lock().unwrap() = Some(new_pref.clone());
                let mut s = self.current_settings.lock().unwrap();
                s.preferred_gpu = Some(new_pref);
                let _ = settings::persist_settings(&self.dir, &s);
            }
        }

        // Repaint faster when a secondary window is open so it closes within ~100 ms
        // of the user clicking X/Save/Cancel (the open flag is set in the callback
        // which runs after ui(), so the NEXT frame sees open=false and stops calling
        // show_viewport_immediate, which is what actually destroys the viewport).
        ui.ctx().request_repaint_after(if any_dialog_open {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(1)
        });
    }
}

// ── Floating panel helpers ────────────────────────────────────────────────────

/// Returns the accent colour for a given floating panel key.
/// Draw a padlock icon centred on `center` (fits inside a ~10 × 12 px area).
///
/// * **locked**: symmetric arch, both shackle arms enter the body.
/// * **unlocked**: arch is lifted with a clear gap; only the right arm reaches
///   the body — the left end hangs free, clearly showing the lock is open.
fn draw_padlock(painter: &egui::Painter, center: egui::Pos2, locked: bool, color: egui::Color32) {
    let body_w = 8.0_f32;
    let body_h = 4.5_f32;
    let sr = 2.6_f32; // shackle half-width = radius of semicircle
    let stroke = egui::Stroke::new(1.5, color);

    // Body — filled rect in the lower portion.
    let body_cy = center.y + 2.2;
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(center.x, body_cy),
            egui::Vec2::new(body_w, body_h),
        ),
        1.0,
        color,
    );

    let body_top = body_cy - body_h / 2.0;
    let left_x = center.x - sr;
    let right_x = center.x + sr;

    if locked {
        // Arch sits tightly on body; both arms just touch body_top.
        let arc_cy = body_top - sr;
        painter.line_segment(
            [egui::pos2(left_x, body_top), egui::pos2(left_x, arc_cy)],
            stroke,
        );
        painter.line_segment(
            [egui::pos2(right_x, body_top), egui::pos2(right_x, arc_cy)],
            stroke,
        );
        let pts: Vec<egui::Pos2> = (0..=10)
            .map(|i| {
                let a = std::f32::consts::PI * i as f32 / 10.0;
                egui::pos2(center.x - sr * a.sin(), arc_cy - sr * a.cos())
            })
            .collect();
        painter.add(egui::Shape::line(pts, stroke));
    } else {
        // Arch is raised well above body — clear gap shows it is open.
        // Only the right arm extends down to body_top; left end hangs free.
        let arc_cy = body_top - sr * 2.6; // raised ~2.5× compared to locked
        painter.line_segment(
            [egui::pos2(right_x, body_top), egui::pos2(right_x, arc_cy)],
            stroke,
        );
        // Left arm: short stub at arc end (makes the opening obvious).
        painter.line_segment(
            [
                egui::pos2(left_x, arc_cy),
                egui::pos2(left_x, arc_cy + sr * 0.7),
            ],
            stroke,
        );
        let pts: Vec<egui::Pos2> = (0..=10)
            .map(|i| {
                let a = std::f32::consts::PI * i as f32 / 10.0;
                egui::pos2(center.x - sr * a.sin(), arc_cy - sr * a.cos())
            })
            .collect();
        painter.add(egui::Shape::line(pts, stroke));
    }
}

impl RigStatsApp {
    /// Render every visible panel as its own borderless OS window using
    /// `show_viewport_immediate`.  Called each frame when `floating_mode` is true.
    ///
    /// `show_viewport_immediate` renders each child synchronously as part of the
    /// parent frame — no deferred callbacks, no separate event loops.  The parent
    /// ticks at ~1 fps (via `request_repaint_after(1 s)` in `update()`), so all
    /// panels naturally update at ~1 fps without any Win32 tricks.
    fn render_floating_panels(&mut self, ui: &mut egui::Ui) {
        let s = self.current_settings.lock().unwrap();
        let profile_w = profile_to_size(&s.dashboard_profile)[0];
        let window_level = match s.window_layer.as_str() {
            "on_top" => egui::WindowLevel::AlwaysOnTop,
            "behind" => egui::WindowLevel::AlwaysOnBottom,
            _ => egui::WindowLevel::Normal,
        };
        let scale = self.floating_panel_scale;
        drop(s);

        let panels_to_draw = self.visible_panels.clone();
        let opacity = self.opacity;

        for (idx, panel_key) in panels_to_draw.iter().enumerate() {
            let key = panel_key.clone();

            let init_pos: [f32; 2] = {
                let positions = self.floating_positions.lock().unwrap();
                positions
                    .get(&key)
                    .copied()
                    .unwrap_or([100.0 + idx as f32 * 20.0, 80.0 + idx as f32 * 30.0])
            };

            let panel_w = profile_w * scale;
            let initial_h = panel_initial_h(&key) * scale;

            // Only set window position on first creation.  After that the OS
            // owns the position (via drag); re-sending with_position every frame
            // causes egui to diff-and-dispatch SetOuterPosition continuously,
            // which fights the OS and produces sub-pixel blur.
            let needs_position = !self.panels_positioned.contains(&key);
            if needs_position {
                self.panels_positioned.insert(key.clone());
            }

            let mut vp_builder = egui::ViewportBuilder::default()
                .with_title(format!("RigStats \u{2014} {}", panel_label(&key)))
                .with_inner_size([panel_w, initial_h])
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_window_level(window_level);

            // Capture title string for Win32 FindWindowW lookup inside the callback.
            let win_title = format!("RigStats \u{2014} {}", panel_label(&key));
            let is_behind = window_level == egui::WindowLevel::AlwaysOnBottom;

            if needs_position {
                vp_builder = vp_builder.with_position(init_pos);
            }

            // `show_viewport_immediate` is FnMut with no Send/'static bound —
            // we can borrow self fields directly instead of going through Arc.
            let positions_arc = &self.floating_positions;
            let dirty = &self.positions_dirty;
            let new_pref_arc = &self.float_new_pref_gpu;
            let lock_arc = &self.floating_lock_arc;
            let stats = &self.latest;
            let cspark = &self.cpu_spark;
            let gspark = &self.gpu_spark;
            let nuspark = &self.net_up_spark;
            let ndspark = &self.net_dn_spark;
            let tex = &self.textures;
            let app_theme = self.app_theme;
            let float_update_ver: Option<String> = {
                use windows::updater::UpdateStatus;
                let st = self.updater_win.lock().unwrap();
                if let UpdateStatus::Ready { info, .. } = &st.status {
                    Some(info.version.clone())
                } else {
                    None
                }
            };
            let updater_open_arc = &self.updater_open;
            // On the very first frame a viewport is shown, `outer_rect` reports the
            // egui-default position (before the OS has honoured `with_position`).
            // Saving that would overwrite the loaded position, so we skip tracking
            // on the first frame — `needs_position` was true iff this is that frame.
            let skip_pos_tracking = needs_position;

            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of(format!("float_{key}")),
                vp_builder,
                |child_ui, _class| {
                    let ctx = child_ui.ctx();

                    // ── Track window position for persistence ─────────────────
                    if !skip_pos_tracking {
                        if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                            let new_pos = [outer.left().round(), outer.top().round()];
                            let mut pos = positions_arc.lock().unwrap();
                            let stored = pos.entry(key.clone()).or_insert([f32::NAN, f32::NAN]);
                            if stored[0].is_nan()
                                || stored[1].is_nan()
                                || ((*stored)[0] - new_pos[0]).abs() > 0.5
                                || ((*stored)[1] - new_pos[1]).abs() > 0.5
                            {
                                *stored = new_pos;
                                dirty.store(true, Ordering::Relaxed);
                            }
                        }
                    }

                    // ── "Always Behind" enforcement ───────────────────────────
                    // SC_MOVE (StartDrag) requires the window to be active.
                    // WS_EX_NOACTIVATE prevents activation, so we only enforce
                    // "behind" when the primary button is NOT pressed on this
                    // panel.  prepare_for_drag() strips WS_EX_NOACTIVATE and
                    // activates the window just before sending StartDrag, then
                    // apply_behind() re-arms on the next idle frame.
                    let primary_down = ctx.input(|i| i.pointer.primary_down());
                    if is_behind && !primary_down {
                        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                            egui::WindowLevel::AlwaysOnBottom,
                        ));
                        #[cfg(windows)]
                        win32_behind::apply_behind(&win_title);
                    }

                    #[allow(deprecated)] // CentralPanel::show is correct in viewport callbacks
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none().fill(theme::PANEL_FILL))
                        .show(ctx, |ui| {
                            // ── Drag & lock state ─────────────────────────────
                            let locked = lock_arc.load(Ordering::Relaxed);
                            let hover_pos = ctx.input(|i| i.pointer.hover_pos());
                            let just_pressed = ctx.input(|i| i.pointer.primary_pressed());

                            // ── Panel content ─────────────────────────────────
                            // Each draw() returns the panel's outer Rect so we can
                            // overlay the drag dots and padlock without extra height.
                            let mut new_pref: Option<String> = None;
                            let panel_rect = match key.as_str() {
                                "header" => {
                                    panels::header::draw(ui, stats, tex, 1.0, &app_theme, scale)
                                }
                                "clock" => {
                                    let r = panels::clock::draw(
                                        ui,
                                        stats.uptime_secs,
                                        1.0,
                                        &app_theme,
                                        float_update_ver.as_deref(),
                                        scale,
                                    );
                                    if ui
                                        .ctx()
                                        .data_mut(|d| {
                                            d.remove_temp::<bool>(egui::Id::new("open_updater"))
                                        })
                                        .unwrap_or(false)
                                    {
                                        updater_open_arc.store(true, Ordering::Relaxed);
                                    }
                                    r
                                }
                                "cpu" => panels::cpu::draw(
                                    ui,
                                    stats,
                                    cspark,
                                    tex,
                                    1.0,
                                    self.thresholds.cpu.0,
                                    self.thresholds.cpu.1,
                                    &app_theme,
                                    scale,
                                ),
                                "gpu" => {
                                    let r = panels::gpu::draw(
                                        ui,
                                        stats,
                                        gspark,
                                        tex,
                                        1.0,
                                        &app_theme,
                                        self.thresholds.gpu.0,
                                        self.thresholds.gpu.1,
                                        scale,
                                    );
                                    new_pref = r.0;
                                    r.1
                                }
                                "ram" => panels::ram::draw(
                                    ui,
                                    stats,
                                    1.0,
                                    self.thresholds.ram.0,
                                    self.thresholds.ram.1,
                                    &app_theme,
                                    scale,
                                ),
                                "net" => panels::net::draw(
                                    ui, stats, nuspark, ndspark, 1.0, &app_theme, scale,
                                ),
                                "disk" => panels::disk::draw(
                                    ui,
                                    stats,
                                    1.0,
                                    self.thresholds.disk.0,
                                    self.thresholds.disk.1,
                                    &app_theme,
                                    scale,
                                ),
                                "motherboard" => panels::motherboard::draw(
                                    ui,
                                    stats,
                                    1.0,
                                    self.thresholds.mb.0,
                                    self.thresholds.mb.1,
                                    &app_theme,
                                    scale,
                                ),
                                "process" => {
                                    panels::process::draw(ui, stats, 1.0, &app_theme, scale)
                                }
                                "battery" => {
                                    panels::battery::draw(ui, stats, 1.0, &app_theme, scale)
                                }
                                _ => egui::Rect::NOTHING,
                            };

                            if let Some(p) = new_pref {
                                *new_pref_arc.lock().unwrap() = Some(p);
                            }

                            // ── Drag zone: top 24 px of the panel inner area ──
                            // inner_margin top = 8 px; title row is ~20 px tall,
                            // so top+24 covers the title row comfortably.
                            let drag_zone = egui::Rect::from_min_max(
                                panel_rect.min,
                                egui::pos2(panel_rect.right(), panel_rect.top() + 24.0),
                            );
                            // Padlock hit area: right 20 px of the drag zone.
                            let padlock_cx = drag_zone.right() - 14.0;
                            let padlock_cy = drag_zone.center().y;
                            let padlock_center = egui::pos2(padlock_cx, padlock_cy);
                            let padlock_hit = egui::Rect::from_center_size(
                                padlock_center,
                                egui::Vec2::new(22.0, drag_zone.height()),
                            );

                            let in_drag_zone =
                                hover_pos.map(|p| drag_zone.contains(p)).unwrap_or(false);
                            let in_padlock =
                                hover_pos.map(|p| padlock_hit.contains(p)).unwrap_or(false);

                            // Drag trigger (whole drag zone minus padlock area).
                            if !locked && just_pressed && in_drag_zone && !in_padlock {
                                #[cfg(windows)]
                                if is_behind {
                                    win32_behind::prepare_for_drag(&win_title);
                                }
                                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                            }

                            // Padlock interaction.
                            let padlock_resp = ui.interact(
                                padlock_hit,
                                ui.id().with("padlock"),
                                egui::Sense::click(),
                            );
                            if padlock_resp.clicked() {
                                lock_arc.store(!locked, Ordering::Relaxed);
                            }
                            if padlock_resp.hovered() {
                                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                            }

                            // ── Overlay: dots + padlock painted on the panel ──
                            let painter = ui.painter();

                            // Three dots — shown when unlocked and hovering the drag zone.
                            if in_drag_zone && !locked {
                                let cy = drag_zone.center().y;
                                let cx = drag_zone.center().x;
                                for i in [-5.0f32, 0.0, 5.0] {
                                    painter.circle_filled(
                                        egui::pos2(cx + i, cy),
                                        1.5,
                                        egui::Color32::from_gray(160),
                                    );
                                }
                            }

                            // Padlock: full icon on hover; tiny dot when locked + not hovering.
                            if in_drag_zone {
                                let padlock_color = if locked {
                                    app_theme.accent
                                } else if padlock_resp.hovered() {
                                    egui::Color32::from_gray(130)
                                } else {
                                    egui::Color32::from_gray(100)
                                };
                                draw_padlock(painter, padlock_center, locked, padlock_color);
                            } else if locked {
                                // Subtle locked indicator when not hovering — just a small dot.
                                painter.circle_filled(
                                    padlock_center,
                                    2.5,
                                    egui::Color32::from_rgba_unmultiplied(
                                        app_theme.accent.r(),
                                        app_theme.accent.g(),
                                        app_theme.accent.b(),
                                        120,
                                    ),
                                );
                            }

                            // Auto-resize height to content, but only when it actually
                            // changes — sending InnerSize every frame causes sub-pixel
                            // jitter that makes the panel blurry after dragging.
                            let used_h = ui.min_rect().height().round();
                            if used_h > 10.0 {
                                let current_h = ctx
                                    .input(|i| i.viewport().inner_rect)
                                    .map_or(0.0, |r| r.height().round());
                                if (used_h - current_h).abs() > 0.5 {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                        egui::Vec2::new(panel_w, used_h),
                                    ));
                                }
                            }

                            // Apply opacity to this panel's OS window.
                            #[cfg(windows)]
                            {
                                let title = format!("RigStats \u{2014} {}", panel_label(&key));
                                let hwnd = win_opacity::find_hwnd(&title);
                                win_opacity::set_opacity(hwnd, opacity);
                            }
                        });
                },
            );
        }
    }

    /// Flush `floating_positions` → `panel_layouts` in settings and persist to disk.
    fn persist_floating_positions(&self) {
        let positions = self.floating_positions.lock().unwrap();
        let mut s = self.current_settings.lock().unwrap();
        for (key, &[x, y]) in positions.iter() {
            s.panel_layouts.insert(
                key.clone(),
                settings::PanelLayout {
                    x: x as i32,
                    y: y as i32,
                },
            );
        }
        let _ = settings::persist_settings(&self.dir, &s);
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

// ── Logging helpers ───────────────────────────────────────────────────────────

fn poll_stats_to_log_payload(s: &PollStats) -> rigstats_backend::stats::StatsPayload {
    use rigstats_backend::stats::{
        BatteryStats, CpuStats, DiskDrive, DiskStats, GpuStats, MotherboardStats, NetStats,
        ProcessEntry, RamStats, StatsPayload,
    };
    StatsPayload {
        cpu: CpuStats {
            load: s.cpu_load,
            cores: s.cpu_cores.clone(),
            temp: s.cpu_temp,
            freq: s.cpu_freq_mhz,
            power: s.cpu_power,
        },
        gpu: GpuStats {
            name: Some(s.gpu_name.clone()),
            load: s.gpu_load,
            temp: s.gpu_temp,
            hotspot: s.gpu_hotspot,
            freq: s.gpu_freq_mhz,
            mem_freq: s.gpu_mem_freq_mhz,
            vram_used: s.gpu_vram_used_mb,
            vram_total: s.gpu_vram_total_mb,
            fan_speed: s.gpu_fan,
            power: s.gpu_power,
            d3d_3d: s.gpu_d3d_3d,
            d3d_vdec: s.gpu_d3d_vdec,
            available_gpus: s.gpu_devices.clone(),
        },
        ram: RamStats {
            total: s.ram_total,
            used: s.ram_used,
            free: s.ram_total.saturating_sub(s.ram_used),
            spec: s.ram_spec.clone(),
            details: String::new(),
            temp: s.ram_temp,
        },
        net: NetStats {
            up: s.net_up_mbps,
            down: s.net_down_mbps,
            iface: s.net_iface.clone(),
            ping_ms: s.net_ping_ms,
        },
        disk: DiskStats {
            read: s.disk_read_mbps,
            write: s.disk_write_mbps,
            drives: s
                .disk_drives
                .iter()
                .map(|d| DiskDrive {
                    fs: d.fs.clone(),
                    size: d.total,
                    used: d.used,
                    pct: d.pct,
                    temp: d.temp,
                })
                .collect(),
        },
        motherboard: MotherboardStats {
            fans: s.mb_fans.clone(),
            temps: s.mb_temps.clone(),
            voltages: s.mb_voltages.clone(),
            chip: s.mb_chip.clone(),
            board: s.mb_board.clone(),
        },
        battery: BatteryStats {
            present: s.battery_present,
            charge_pct: s.battery_charge_pct,
            charging: s.battery_charging,
            time_remaining_mins: s.battery_time_mins,
            power_w: s.battery_power_w,
        },
        top_processes: s
            .processes
            .iter()
            .map(|p| ProcessEntry {
                name: p.name.clone(),
                cpu: p.cpu,
                mem_mb: p.mem_mb,
            })
            .collect(),
        system_uptime_secs: s.uptime_secs,
        lhm_connected: s.lhm_connected,
    }
}

// ── Poll loop (tokio runtime) ─────────────────────────────────────────────────

async fn poll_loop(
    tx: mpsc::SyncSender<PollStats>,
    dir: PathBuf,
    preferred_gpu: Arc<Mutex<Option<String>>>,
    settings_arc: Arc<Mutex<settings::Settings>>,
) {
    let ram_spec = tokio::task::spawn_blocking(hardware::detect_ram_spec)
        .await
        .unwrap_or_default();
    debug::append_debug_log(&dir, &format!("hardware: ram_spec={ram_spec}"));
    let disk_model_map: HashMap<String, String> =
        tokio::task::spawn_blocking(hardware::detect_disk_model_map)
            .await
            .unwrap_or_default();
    debug::append_debug_log(
        &dir,
        &format!("hardware: disk_model_map entries={}", disk_model_map.len()),
    );
    let ping_target = hardware::detect_ping_target();
    debug::append_debug_log(&dir, &format!("hardware: ping_target={ping_target}"));
    let mb_board: Option<String> = tokio::task::spawn_blocking(hardware::detect_motherboard_name)
        .await
        .ok()
        .flatten();
    debug::append_debug_log(&dir, &format!("hardware: mb_board={mb_board:?}"));
    let model_name: String = tokio::task::spawn_blocking(hardware::detect_model_name)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    debug::append_debug_log(&dir, &format!("hardware: model_name={model_name}"));
    let system_brand: String = tokio::task::spawn_blocking(hardware::detect_system_brand)
        .await
        .unwrap_or_default();
    debug::append_debug_log(&dir, &format!("hardware: system_brand={system_brand}"));
    let gpu_name: String = tokio::task::spawn_blocking(hardware::detect_gpu_name)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    debug::append_debug_log(&dir, &format!("hardware: gpu_name={gpu_name}"));

    let mut sys = System::new();
    sys.refresh_cpu();
    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_default();
    let hostname = System::host_name().unwrap_or_else(|| "—".to_string());
    debug::append_debug_log(&dir, &format!("hardware: cpu_model={cpu_model}"));
    debug::append_debug_log(&dir, &format!("hardware: hostname={hostname}"));

    let mut disks = Disks::new_with_refreshed_list();
    let mut networks = Networks::new_with_refreshed_list();
    let pipe = tokio::sync::Mutex::new(None::<lhm::LhmPipeReader>);

    let mut last_net_instant = Instant::now();
    let mut last_ping: Option<(Instant, Option<f64>)> = None;
    type BatteryCache = (u8, bool, Option<u32>, Option<f64>);
    let mut last_battery: Option<(Instant, BatteryCache)> = None;
    let mut last_prune_day: Option<u64> = None;
    let mut first_tick_logged = false;

    loop {
        sys.refresh_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::new().with_cpu_usage().with_frequency())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_processes_specifics(
            sysinfo::ProcessRefreshKind::new()
                .with_cpu()
                .with_memory()
                .with_disk_usage(),
        );

        let cpu_load = sys.global_cpu_info().cpu_usage() as u8;
        // global_cpu_info().frequency() returns 0 on Windows in sysinfo 0.30; use per-core average.
        let cpu_freq_mhz = {
            let cpus = sys.cpus();
            if cpus.is_empty() {
                0.0
            } else {
                cpus.iter().map(|c| c.frequency() as f64).sum::<f64>() / cpus.len() as f64
            }
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
        processes.sort_by(|a, b| {
            b.cpu
                .partial_cmp(&a.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        processes.truncate(8);

        disks.refresh();
        let mut disk_drives: Vec<DriveInfo> = disks
            .iter()
            .filter(|d| d.total_space() > 1_000_000_000)
            .map(|d| {
                let total = d.total_space();
                let used = total.saturating_sub(d.available_space());
                let pct = if total > 0 {
                    ((used as f64 / total as f64) * 100.0) as u8
                } else {
                    0
                };
                DriveInfo {
                    fs: d.mount_point().to_string_lossy().to_string(),
                    used,
                    total,
                    pct,
                    temp: None,
                }
            })
            .collect();

        let now = Instant::now();
        let elapsed = now.duration_since(last_net_instant).as_secs_f64().max(0.5);
        last_net_instant = now;

        // disk_usage().read_bytes / written_bytes are per-tick deltas from sysinfo processes.
        let total_read_bytes: u64 = sys
            .processes()
            .values()
            .map(|p| p.disk_usage().read_bytes)
            .sum();
        let total_write_bytes: u64 = sys
            .processes()
            .values()
            .map(|p| p.disk_usage().written_bytes)
            .sum();
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
            let stale = last_ping
                .as_ref()
                .map(|(t, _)| t.elapsed().as_secs_f64() >= 5.0)
                .unwrap_or(true);
            if stale {
                let target = ping_target.clone();
                let measured =
                    tokio::task::spawn_blocking(move || hardware::sample_ping_ms(&target))
                        .await
                        .unwrap_or(None);
                last_ping = Some((Instant::now(), measured));
                measured
            } else {
                last_ping.as_ref().and_then(|(_, v)| *v)
            }
        };

        let (
            battery_present,
            battery_charge_pct,
            battery_charging,
            battery_time_mins,
            battery_power_w,
        ) = {
            let stale = last_battery
                .as_ref()
                .map(|(t, _)| t.elapsed().as_secs_f64() >= 10.0)
                .unwrap_or(true);
            if stale {
                let result = match tokio::task::spawn_blocking(hardware::sample_battery_wmi).await {
                    Ok(result) => result,
                    Err(err) => {
                        debug::append_debug_log(
                            &dir,
                            &format!("battery: sample_battery_wmi join error: {err}"),
                        );
                        None
                    }
                };
                if result.is_none() {
                    match tokio::task::spawn_blocking(hardware::probe_wmi_status).await {
                        Ok(Err(err)) => debug::append_debug_log(
                            &dir,
                            &format!("battery: WMI probe failed after battery read miss: {err}"),
                        ),
                        Err(err) => debug::append_debug_log(
                            &dir,
                            &format!("battery: probe_wmi_status join error: {err}"),
                        ),
                        Ok(Ok(())) => {}
                    }
                }
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
            mb_fans: lhm_data
                .as_ref()
                .map(|l| l.mb_fans.clone())
                .unwrap_or_default(),
            mb_temps: lhm_data
                .as_ref()
                .map(|l| l.mb_temps.clone())
                .unwrap_or_default(),
            mb_voltages: lhm_data
                .as_ref()
                .map(|l| l.mb_voltages.clone())
                .unwrap_or_default(),
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
            gpu_name: lhm_data
                .as_ref()
                .and_then(|l| l.gpu_name.clone())
                .unwrap_or_else(|| gpu_name.clone()),
            ram_temp: lhm_data.as_ref().and_then(|l| l.ram_temp),
            gpu_devices: lhm_data
                .as_ref()
                .map(|l| l.gpu_devices.clone())
                .unwrap_or_default(),
            lhm_connected,
        };

        let _ = tx.send(stats.clone());
        if !first_tick_logged {
            debug::append_debug_log(
                &dir,
                &format!("poll: first tick complete lhm_connected={lhm_connected}"),
            );
            first_tick_logged = true;
        }

        // CSV logging — reads settings each tick so changes take effect immediately.
        let (log_enabled, retention_days) = {
            let s = settings_arc.lock().unwrap();
            (s.logging_enabled, s.log_retention_days)
        };
        if log_enabled {
            let payload = poll_stats_to_log_payload(&stats);
            let _ = logging::append_stats_row(&payload, &dir);
            let today = logging::unix_now_secs() / 86400;
            if last_prune_day != Some(today) {
                logging::prune_old_logs(&dir, retention_days);
                last_prune_day = Some(today);
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    // Opt the process into dark mode for OS-drawn UI elements (tray context menu).
    #[cfg(windows)]
    win32_dark_mode::enable();

    let dir = app_data_dir();
    debug::reset_debug_log(&dir);
    debug::append_debug_log(&dir, "rigstats-egui starting");

    let s = settings::load_settings(&dir);
    let visible_panels = s.visible_panels.clone();
    let opacity = s.opacity.clamp(0.1, 1.0) as f32;
    let always_on_top = s.window_layer == "on_top";
    let [win_w, _] = profile_to_size(&s.dashboard_profile);
    let win_h = compute_window_height(&visible_panels);
    let [pos_x, pos_y] = pick_window_position();
    debug::append_debug_log(
        &dir,
        &format!(
            "settings: profile={} panels={} opacity={opacity:.2} floating_mode={}",
            s.dashboard_profile,
            visible_panels.join(","),
            s.floating_mode
        ),
    );
    debug::append_debug_log(
        &dir,
        &format!("window: initial_position=({pos_x:.1}, {pos_y:.1})"),
    );

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let preferred_gpu_arc: Arc<Mutex<Option<String>>> =
        Arc::new(Mutex::new(s.preferred_gpu.clone()));
    let current_settings_shared = Arc::new(Mutex::new(settings::load_settings(&dir)));
    let (tx, rx) = mpsc::sync_channel::<PollStats>(4);
    let dir_clone = dir.clone();
    let pref_poll = preferred_gpu_arc.clone();
    let settings_poll = current_settings_shared.clone();
    runtime.spawn(async move { poll_loop(tx, dir_clone, pref_poll, settings_poll).await });

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("RigStats")
        .with_inner_size([win_w - 2.0, win_h])
        .with_position([pos_x, pos_y])
        .with_decorations(false)
        .with_taskbar(false); // app is tray-only, never show in taskbar
    if always_on_top {
        viewport = viewport.with_always_on_top();
    }
    if s.floating_mode {
        viewport = viewport.with_taskbar(false);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "RigStats",
        options,
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            // Suppress egui's built-in panel/window backgrounds; clear_color provides the base fill.
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
                    (
                        TextStyle::Small,
                        FontId::new(12.0, FontFamily::Proportional),
                    ),
                    (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
                    (
                        TextStyle::Button,
                        FontId::new(14.0, FontFamily::Proportional),
                    ),
                    (
                        TextStyle::Heading,
                        FontId::new(18.0, FontFamily::Proportional),
                    ),
                    (
                        TextStyle::Monospace,
                        FontId::new(13.0, FontFamily::Monospace),
                    ),
                ]
                .into();
                cc.egui_ctx.set_global_style(style);
            }

            let tray = build_tray(s.logging_enabled);
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
            let floating_id = tray.floating_id.clone();
            let recording_id = tray.recording_id.clone();
            std::thread::spawn(move || loop {
                let mut repaint = false;
                if let Ok(ev) = MenuEvent::receiver().try_recv() {
                    // Use the foreground rights that come with the tray-menu interaction.
                    // We immediately bring the (off-screen) parent window to the foreground
                    // so that our process owns foreground when the dialog is created a few
                    // milliseconds later.  Without this the dialog window would be created
                    // as a background window and SetForegroundWindow would be refused.
                    #[cfg(windows)]
                    #[allow(unsafe_code)]
                    unsafe {
                        winapi::um::winuser::AllowSetForegroundWindow(0xFFFF_FFFFu32); // ASFW_ANY
                        let parent_hwnd = win_opacity::find_hwnd("RigStats");
                        if parent_hwnd != 0 {
                            winapi::um::winuser::SetForegroundWindow(parent_hwnd as _);
                        }
                    }

                    let cmd = if ev.id == quit_id {
                        std::process::exit(0);
                    } else if ev.id == show_id {
                        Some(TrayCmd::Toggle)
                    } else if ev.id == floating_id {
                        Some(TrayCmd::ToggleFloating)
                    } else if ev.id == recording_id {
                        Some(TrayCmd::ToggleRecording)
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
                if let Ok(tray_icon::TrayIconEvent::Click {
                    button,
                    button_state,
                    ..
                }) = tray_icon::TrayIconEvent::receiver().try_recv()
                {
                    if button == MouseButton::Left && button_state == MouseButtonState::Up {
                        let _ = tray_tx.send(TrayCmd::Toggle);
                        repaint = true;
                    }
                }
                if repaint {
                    ctx.request_repaint();
                }
                std::thread::sleep(Duration::from_millis(50));
            });

            let current_settings = current_settings_shared;
            let settings_reload = Arc::new(AtomicBool::new(false));
            let dir_arc = Arc::new(dir.clone());
            let textures = brand::Textures::load(&cc.egui_ctx);

            // Heartbeat: wakes the parent eframe loop at ~1 fps when floating mode is
            // active.  With `show_viewport_immediate`, all panels are rendered
            // synchronously as part of the parent frame, so one `request_repaint()`
            // per second is enough to drive all panel updates.
            let fm_arc_hb = Arc::new(AtomicBool::new(
                current_settings.lock().unwrap().floating_mode,
            ));
            let ctx_hb = cc.egui_ctx.clone();
            let fm_arc_hb2 = fm_arc_hb.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(950));
                if fm_arc_hb2.load(Ordering::Relaxed) {
                    ctx_hb.request_repaint();
                }
            });

            // Background auto-update check: fires 10 s after startup, then every 6 h.
            // When a newer version is found and downloaded, opens the updater window.
            let updater_win_bg: Arc<Mutex<windows::updater::UpdaterState>> =
                Arc::new(Mutex::new(windows::updater::UpdaterState::default()));
            let updater_open_bg = Arc::new(AtomicBool::new(false));
            let updater_focus_bg = Arc::new(AtomicBool::new(false));
            {
                let win = updater_win_bg.clone();
                let open = updater_open_bg.clone();
                let focus = updater_focus_bg.clone();
                let ctx = cc.egui_ctx.clone();
                runtime.spawn(async move {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    loop {
                        let result = tokio::task::spawn_blocking(update_check::check)
                            .await
                            .unwrap_or_else(|_| Err("task panic".to_string()));
                        match result {
                            Ok(update_check::CheckResult::UpdateAvailable(info)) => {
                                let version = info.version.clone();
                                let url = info.url.clone();
                                let dest = update_check::installer_temp_path(&version);
                                {
                                    let mut s = win.lock().unwrap();
                                    s.status = windows::updater::UpdateStatus::Downloading {
                                        downloaded: 0,
                                        total: 0,
                                    };
                                }
                                ctx.request_repaint();
                                let win2 = win.clone();
                                let ctx2 = ctx.clone();
                                let dest2 = dest.clone();
                                let dl_result = tokio::task::spawn_blocking(move || {
                                    update_check::download(&url, &dest2, |downloaded, total| {
                                        let mut s = win2.lock().unwrap();
                                        s.status = windows::updater::UpdateStatus::Downloading {
                                            downloaded,
                                            total,
                                        };
                                        ctx2.request_repaint();
                                    })
                                })
                                .await
                                .unwrap_or_else(|_| Err("download task panic".to_string()));
                                match dl_result {
                                    Ok(()) => {
                                        let mut s = win.lock().unwrap();
                                        s.status = windows::updater::UpdateStatus::Ready {
                                            info,
                                            installer_path: dest,
                                        };
                                        drop(s);
                                        open.store(true, Ordering::Relaxed);
                                        focus.store(true, Ordering::Relaxed);
                                        ctx.request_repaint();
                                    }
                                    Err(e) => {
                                        win.lock().unwrap().status =
                                            windows::updater::UpdateStatus::Error(e);
                                    }
                                }
                            }
                            Ok(update_check::CheckResult::UpToDate) => {}
                            Err(_) => {}
                        }
                        tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
                    }
                });
            }

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
                fm_arc_hb,
                updater_win_bg,
                updater_open_bg,
                updater_focus_bg,
            )))
        }),
    )
    .expect("eframe");

    runtime.shutdown_background();
}
