#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod brand;
mod lock_ext;
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
use lock_ext::LockSafe;
use rigstats_backend::{debug, hardware, lhm, lhm_process, logging, settings};
use spark::Sparkline;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
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
    pub model: String,
    pub kind: rigstats_backend::stats::DiskKind,
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
    pub ram_details: String,
    // Network
    pub net_up_mbps: f64,
    pub net_down_mbps: f64,
    pub net_iface: String,
    pub net_ping_ms: Option<f64>,
    pub net_ping_wan_ms: Option<f64>,
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
        // Landscape profiles — the portrait dimensions transposed (w > h).
        "landscape-xl" => [1920.0, 450.0],
        "landscape-slim" => [1920.0, 480.0],
        "landscape-hd" => [1280.0, 720.0],
        "landscape-wxga" => [1280.0, 800.0],
        "landscape-fhd" => [1920.0, 1080.0],
        "landscape-wuxga" => [1920.0, 1200.0],
        "landscape-qhd" => [2560.0, 1440.0],
        "landscape-hdplus" => [1366.0, 768.0],
        "landscape-1600x900" => [1600.0, 900.0],
        "landscape-1680x1050" => [1680.0, 1050.0],
        "landscape-2560x1600" => [2560.0, 1600.0],
        "landscape-4k" => [3840.0, 2160.0],
        "landscape-fhd-top" => [1080.0, 253.0],
        "landscape-qhd-top" => [1440.0, 338.0],
        "landscape-4k-top" => [2160.0, 506.0],
        _ => [400.0, 780.0],
    }
}

/// True when the profile is a landscape (wide) layout. Landscape profiles use a
/// `landscape-` key prefix and render panels in a grid rather than a vertical stack.
pub(crate) fn profile_is_landscape(profile: &str) -> bool {
    profile.starts_with("landscape-")
}

/// Content scale for a profile. Uses portrait-xl (450 px) as the 1.0 reference.
/// Narrow profiles (side panels) scale down; wider profiles stay at 1.0.
fn profile_scale(profile: &str) -> f32 {
    const REF_W: f32 = 450.0;
    (profile_to_size(profile)[0] / REF_W).clamp(0.4, 1.0)
}

/// Estimated window height for the given visible panels at scale `sc`.
///
/// Values are calibrated estimates (content + frame inner margin 16 px).
/// Fine-tune by measuring actual rendered heights in the live app.
fn compute_window_height(visible_panels: &[String], sc: f32) -> f32 {
    // panel_frame inner_margin: Margin::symmetric(12, 8) → 8 top + 8 bottom = 16 px.
    const V_MARGIN: f32 = 16.0;
    let header_h = (theme::PANEL_HEADER_H + V_MARGIN) * sc;
    let data_h = (theme::PANEL_DATA_H + V_MARGIN) * sc;
    let n = visible_panels.len();
    let mut h = theme::DRAG_HANDLE_H;
    for key in visible_panels {
        h += match key.as_str() {
            "header" | "clock" => header_h,
            _ => data_h,
        };
    }
    // add_space(6.0 * sc) follows every panel in the render loop.
    h + (n as f32 * 6.0 * sc)
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

/// Returns `pos` if it is within any currently connected monitor (allowing 60 px
/// overhang so a panel that straddles a monitor edge is still considered visible),
/// otherwise returns `fallback`.  Call this before applying a saved floating-panel
/// position so panels that were left on a disconnected monitor snap back on-screen.
fn guard_panel_position(pos: [f32; 2], fallback: [f32; 2]) -> [f32; 2] {
    #[cfg(windows)]
    {
        if position_on_any_monitor(pos, &win_monitor::list()) {
            pos
        } else {
            fallback
        }
    }
    #[cfg(not(windows))]
    pos
}

/// True when `pos` lies on (or within the 60 px overhang margin of) any monitor
/// in `monitors`. Pure core of [`guard_panel_position`] and the pinned-position
/// check, split out so it can be unit-tested without enumerating real monitors.
fn position_on_any_monitor(pos: [f32; 2], monitors: &[(i32, i32, i32, i32)]) -> bool {
    let margin = 60.0_f32;
    monitors.iter().any(|&(l, t, r, b)| {
        pos[0] >= l as f32 - margin
            && pos[0] <= r as f32 + margin
            && pos[1] >= t as f32 - margin
            && pos[1] <= b as f32 + margin
    })
}

/// Pure decision for where a pinned dashboard should be placed.
///
/// Returns the saved position only when the dashboard is `pinned`, a position is
/// `saved` for the current profile, and that position is still on a connected
/// monitor; otherwise `None`, so the caller auto-targets the matching monitor.
fn resolve_pinned_position(
    pinned: bool,
    saved: Option<[i32; 2]>,
    monitors: &[(i32, i32, i32, i32)],
) -> Option<[f32; 2]> {
    if !pinned {
        return None;
    }
    let p = saved?;
    let pos = [p[0] as f32, p[1] as f32];
    position_on_any_monitor(pos, monitors).then_some(pos)
}

/// Returns `[x, y, w, h]` for the best monitor to host `profile`.
///
/// A monitor is only chosen as a dedicated target when its resolution *matches*
/// the profile (both dimensions within ~10 %), so a small strip/secondary screen
/// is used only when its size actually fits the profile. When no monitor matches,
/// the window goes to the **primary** monitor (top-left at the virtual origin) so
/// e.g. a 1080×253 profile lands on the main screen rather than a 1920×450 strip.
/// Falls back to the first monitor, then `[0, 0, 0, 0]`.
fn pick_window_rect_for_profile(profile: &str) -> [f32; 4] {
    #[cfg(windows)]
    {
        let [pw, ph] = profile_to_size(profile);
        let monitors = win_monitor::list();
        if let Some(idx) = select_profile_monitor(&monitors, pw, ph) {
            let (l, t, r, b) = monitors[idx];
            return [l as f32, t as f32, (r - l) as f32, (b - t) as f32];
        }
    }
    #[cfg(not(windows))]
    let _ = profile;
    [0.0, 0.0, 0.0, 0.0]
}

/// Pure monitor-selection logic for [`pick_window_rect_for_profile`], split out so
/// it can be unit-tested without enumerating real monitors.
///
/// Returns the index into `monitors` of the screen that should host a profile of
/// size `pw`×`ph`:
/// 1. A monitor whose resolution *matches* the profile (both dims within ~10 %);
///    among matches, the closest fit. This keeps a small strip/secondary screen
///    reserved for the profile that actually fits it.
/// 2. Otherwise the **primary** monitor (top-left at the virtual origin `0,0`), so
///    e.g. a 1080×253 profile lands on the main screen rather than a 1920×450 strip.
/// 3. Otherwise the first monitor. `None` only when `monitors` is empty.
fn select_profile_monitor(monitors: &[(i32, i32, i32, i32)], pw: f32, ph: f32) -> Option<usize> {
    let dim = |&(l, t, r, b): &(i32, i32, i32, i32)| ((r - l) as f32, (b - t) as f32);

    // 1. A monitor whose resolution matches the profile, closest fit first.
    let tol_w = (pw * 0.10).max(8.0);
    let tol_h = (ph * 0.10).max(8.0);
    let matched = monitors
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            let (w, h) = dim(m);
            (w - pw).abs() <= tol_w && (h - ph).abs() <= tol_h
        })
        .min_by(|(_, a), (_, b)| {
            let (aw, ah) = dim(a);
            let (bw, bh) = dim(b);
            let da = (aw - pw).abs() + (ah - ph).abs();
            let db = (bw - pw).abs() + (bh - ph).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i);
    if matched.is_some() {
        return matched;
    }

    // 2. No dedicated match → the primary monitor (origin at 0,0).
    if let Some(i) = monitors.iter().position(|&(l, t, _, _)| l == 0 && t == 0) {
        return Some(i);
    }

    // 3. Fallback: first monitor.
    if monitors.is_empty() {
        None
    } else {
        Some(0)
    }
}

/// Returns `[x, y, w, h]` (logical pixels) for the best monitor matching the
/// dashboard `orientation`. For portrait targets, prefers a portrait monitor
/// (height > width); for landscape targets, prefers a landscape monitor
/// (width >= height). Falls back to the first monitor, then `[0, 0, 0, 0]`.
fn pick_window_rect_for(landscape: bool) -> [f32; 4] {
    #[cfg(windows)]
    {
        let monitors = win_monitor::list();
        let picked = monitors
            .iter()
            .find(|&&(l, t, r, b)| {
                if landscape {
                    (r - l) >= (b - t)
                } else {
                    (b - t) > (r - l)
                }
            })
            .or_else(|| monitors.first());
        if let Some(&(l, t, r, b)) = picked {
            return [l as f32, t as f32, (r - l) as f32, (b - t) as f32];
        }
    }
    #[cfg(not(windows))]
    let _ = landscape;
    [0.0, 0.0, 0.0, 0.0]
}

/// Returns `[x, y, w, h]` for the best portrait monitor (back-compat helper).
fn pick_window_rect() -> [f32; 4] {
    pick_window_rect_for(false)
}

/// Returns (x, y) position for the window — top-left of the best portrait monitor.
/// Falls back to (0, 0) if no portrait monitor is found or on non-Windows.
fn pick_window_position() -> [f32; 2] {
    let [x, y, _, _] = pick_window_rect();
    [x, y]
}

// ── Tray icon ─────────────────────────────────────────────────────────────────

struct Tray {
    icon: tray_icon::TrayIcon,
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

    let floating_id = floating_item.id().clone();
    let recording_id = recording_item.id().clone();
    let settings_id = settings_item.id().clone();
    let about_id = about_item.id().clone();
    let status_id = status_item.id().clone();
    let updater_id = updater_item.id().clone();
    let quit_id = quit_item.id().clone();

    let menu = Menu::new();
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
        "RIGStats \u{2014} Recording"
    } else {
        "RIGStats"
    };
    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip(tooltip)
        .build()
        .expect("tray icon");

    Tray {
        icon: tray_icon,
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
            "RIGStats \u{2014} Recording"
        } else {
            "RIGStats"
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
    /// Cached from settings — "on_top", "behind", or "normal".
    window_layer: String,
    tray: Tray,
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
    status_refreshing: Arc<AtomicBool>,
    status_collecting: Arc<AtomicBool>,
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
    // ── Fullscreen (fill-screen) mode ──────────────────────────────────────
    /// When true (and not floating), the fixed window fills the whole monitor
    /// instead of fitting panel content; the dashboard background fills the rest.
    fullscreen_mode: bool,
    /// Vertical placement of the panel stack when fullscreen: `"top"` | `"center"`.
    fullscreen_align: String,
    /// Measured panel-stack content height (excluding drag handle + centering pad),
    /// cached from the previous frame so fullscreen centering is exact. `None`
    /// until the first fullscreen frame; `compute_window_height` is the fallback.
    fullscreen_content_h: Option<f32>,
    // ── Pinned (non-floating) dashboard ────────────────────────────────────
    /// When true, the fixed-mode dashboard window is pinned: it cannot be dragged
    /// and its position is restored from `Settings::pinned_positions` across
    /// restarts instead of auto-targeting the matching monitor.
    dashboard_pinned: bool,
    /// Last outer position observed for the fixed-mode window this session, used
    /// to capture the spot to pin when the padlock is clicked. `None` until the
    /// first fixed-mode frame reports a position.
    last_fixed_pos: Option<[f32; 2]>,
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
    /// Per-panel "always behind" enforcement state (key → last enforce time +
    /// previous primary-button state). Used to throttle the Win32 Z-order
    /// re-push so floating "behind" panels don't re-assert every frame — which
    /// would create a SetWindowPos → repaint → SetWindowPos spin loop and burn
    /// CPU. Enforcement happens on creation, in a short burst after a drag, and
    /// then ~1/s as an idle safety net.
    behind_enforce: RefCell<HashMap<String, BehindEnforce>>,
    /// Shared with the heartbeat thread so it knows whether to drive parent repaints.
    floating_mode_arc: Arc<AtomicBool>,
    /// Live thresholds for temperature colour coding — updated on settings reload.
    thresholds: PanelThresholds,
    /// Active panel theme derived from `Settings.theme`.
    app_theme: theme::AppTheme,
    /// Colour palette for dialog windows — switches between dark/light based on OS theme.
    dialog_colors: theme::DialogColors,
    /// Cached OS dark-mode flag; checked each frame to detect live theme switches.
    os_dark_mode: bool,
    /// Whether any dialog was open last frame; used to restore dark visuals when the
    /// last dialog closes (avoids calling set_visuals every frame).
    any_dialog_open_prev: bool,
    /// When > 0, re-applies WindowLevel + opacity for this many more frames.
    /// Used after floating→non-floating transitions where winit may reset the
    /// window level when the window is moved back on-screen.
    reapply_window_props_frames: u8,
    /// Last [w, h] sent via InnerSize — avoids spurious resize events when
    /// only opacity or theme changed (which would cause a visible jump).
    last_applied_window_size: Option<[f32; 2]>,
    /// Last content height fitted in fixed mode — avoids dispatching an
    /// InnerSize viewport command every frame when the height is unchanged
    /// (which during interaction runs at display refresh rate, causing
    /// needless WM_SIZE churn and sub-pixel jitter).
    last_fitted_height: Option<f32>,
    /// Counts down over the first docked frames after launch. Early `InnerSize`
    /// viewport commands can be dropped before the window is fully realized,
    /// which leaves the bottom panel clipped until the user toggles floating
    /// mode. While this is > 0 we force the fit-to-content path to re-snap (and
    /// drive fast repaints) so the true content height reliably sticks.
    startup_fit_frames: u8,
}

/// Per-panel state for throttling "always behind" Z-order enforcement.
struct BehindEnforce {
    /// When the panel was last pushed to the bottom of the Z-order.
    last_enforce: Instant,
    /// Primary-button state on the previous frame — used to detect drag release.
    prev_primary_down: bool,
    /// While `Instant::now() < force_until`, enforce every frame (short burst
    /// after a drag finishes so the panel snaps back behind promptly).
    force_until: Instant,
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
    battery: (u8, u8),
    battery_power: (u8, u8),
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
            battery: (20, 10),
            battery_power: (15, 25),
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
            battery: get("battery", def.battery),
            battery_power: get("battery_power", def.battery_power),
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
        let init_settings = current_settings.lock_safe().clone();
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
            window_layer: init_settings.window_layer.clone(),
            tray,
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
            status_win: Arc::new(Mutex::new(windows::status::StatusState::placeholder())),
            status_refreshing: Arc::new(AtomicBool::new(false)),
            status_collecting: Arc::new(AtomicBool::new(false)),
            updater_win,
            updater_busy: Arc::new(AtomicBool::new(false)),
            current_settings,
            settings_reload,
            preferred_gpu,
            dir,
            hwnd: 0,
            fullscreen_mode: init_settings.fullscreen_mode,
            fullscreen_align: init_settings.fullscreen_align.clone(),
            fullscreen_content_h: None,
            dashboard_pinned: init_settings.dashboard_pinned,
            last_fixed_pos: None,
            floating_mode: init_settings.floating_mode,
            floating_panels_locked: init_settings.floating_panels_locked,
            floating_panel_scale: init_settings.floating_panel_scale.clamp(0.4, 1.0) as f32,
            floating_positions: Arc::new(Mutex::new(init_positions)),
            positions_dirty: Arc::new(AtomicBool::new(false)),
            float_new_pref_gpu: Arc::new(Mutex::new(None)),
            floating_lock_arc: Arc::new(AtomicBool::new(init_settings.floating_panels_locked)),
            initial_floating_applied: false,
            panels_positioned: HashSet::new(),
            behind_enforce: RefCell::new(HashMap::new()),
            floating_mode_arc,
            thresholds: PanelThresholds::from_settings(&init_settings),
            app_theme: theme::AppTheme::from_key(&init_settings.theme),
            os_dark_mode: {
                #[cfg(windows)]
                {
                    win32_dark_mode::is_system_dark_mode()
                }
                #[cfg(not(windows))]
                {
                    true
                }
            },
            dialog_colors: {
                #[cfg(windows)]
                {
                    if win32_dark_mode::is_system_dark_mode() {
                        theme::DialogColors::dark()
                    } else {
                        theme::DialogColors::light()
                    }
                }
                #[cfg(not(windows))]
                {
                    theme::DialogColors::dark()
                }
            },
            any_dialog_open_prev: false,
            // Apply WindowLevel + opacity for the first few frames at startup when in
            // non-floating mode: the viewport builder only handles "on_top", so "behind"
            // must be sent via viewport command, and opacity needs a valid HWND which
            // may not be available until after the first paint.
            reapply_window_props_frames: if !init_settings.floating_mode { 4 } else { 0 },
            last_applied_window_size: None,
            last_fitted_height: None,
            startup_fit_frames: 12,
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

        // Re-apply WindowLevel + opacity for a few frames after a floating→non-floating
        // transition, because winit may reset the window level when it processes the move.
        if self.reapply_window_props_frames > 0 {
            self.reapply_window_props_frames -= 1;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    Self::window_level_from_layer(&self.window_layer),
                ));
            #[cfg(windows)]
            {
                win_opacity::set_opacity(self.hwnd, self.opacity);
                // For "behind" mode, also enforce HWND_BOTTOM directly — ViewportCommand
                // alone is not reliable on Windows. Only needed at startup/transition;
                // normal operation won't bring the window back to front on its own.
                if self.window_layer == "behind" && !self.floating_mode {
                    win32_behind::keep_behind("RigStats");
                }
            }
        }

        // Detect OS dark/light mode changes and refresh dialog colours accordingly.
        #[cfg(windows)]
        {
            let dark = win32_dark_mode::is_system_dark_mode();
            if dark != self.os_dark_mode {
                self.os_dark_mode = dark;
                self.dialog_colors = if dark {
                    theme::DialogColors::dark()
                } else {
                    theme::DialogColors::light()
                };
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
            let s = self.current_settings.lock_safe();
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
            self.window_layer = s.window_layer.clone();
            let was_floating = self.floating_mode;
            self.floating_mode = s.floating_mode;
            self.floating_mode_arc
                .store(self.floating_mode, Ordering::Relaxed);
            self.floating_panels_locked = s.floating_panels_locked;
            self.floating_panel_scale = s.floating_panel_scale.clamp(0.4, 1.0) as f32;
            let was_fullscreen = self.fullscreen_mode;
            self.fullscreen_mode = s.fullscreen_mode;
            self.fullscreen_align = s.fullscreen_align.clone();
            self.dashboard_pinned = s.dashboard_pinned;
            let profile = s.dashboard_profile.clone();
            drop(s);
            // Toggling fullscreen changes how the window is sized; clear the
            // fit-to-content guard so the next fixed frame re-snaps correctly, and
            // drop the cached centering height so it is re-measured.
            if was_fullscreen != self.fullscreen_mode {
                self.last_fitted_height = None;
                self.fullscreen_content_h = None;
            }
            let (pos, [w, h]) = self.fixed_window_geometry(&profile);
            // Pinned, but this profile has no saved position yet (e.g. the user
            // switched profiles while pinned): persist the auto-targeted position
            // now so the profile is properly pinned and stays put across restarts
            // instead of lingering in a locked-but-unsaved state.
            if !self.floating_mode && self.dashboard_pinned {
                let mut s = self.current_settings.lock_safe();
                if !s.pinned_positions.contains_key(&profile) {
                    s.pinned_positions.insert(
                        profile.clone(),
                        [pos[0].round() as i32, pos[1].round() as i32],
                    );
                    self.persist_settings_logged(&s);
                }
            }
            // Only resize the main window when in fixed mode AND the size actually changed.
            // Sending InnerSize every settings-reload (e.g. opacity slider drag) causes a
            // visible jump even when the dimensions are identical.
            if !self.floating_mode {
                let new_size = [w, h];
                if self.last_applied_window_size != Some(new_size) {
                    self.last_applied_window_size = Some(new_size);
                    // Resize and snap to the monitor matching the profile orientation.
                    // (A size change implies a profile/fullscreen/panel change, not an
                    // opacity drag, so repositioning here does not cause jitter.)
                    let [px, py] = pos;
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                            px, py,
                        )));
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
                    self.last_fitted_height = None;
                }
            }
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    Self::window_level_from_layer(&self.window_layer),
                ));
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
                    // Restore to the correct portrait monitor position (and full
                    // monitor size if fullscreen is enabled).
                    let [px, py] = pos;
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                            px, py,
                        )));
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
                    // Force the fit-to-content path to re-dispatch InnerSize next
                    // fixed frame: compute_window_height is only an estimate and may
                    // understate the real content height, which would otherwise clip
                    // the bottom panel. Clearing the guard makes the next frame snap
                    // the window to the true min_rect height.
                    self.last_fitted_height = None;
                }
            }
        }

        // Handle tray commands forwarded by the background polling thread.
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCmd::OpenSettings => {
                    // Re-initialise draft from current settings each time the window opens.
                    let s = self.current_settings.lock_safe().clone();
                    *self.settings_win.lock_safe() =
                        windows::settings::SettingsWindow::from_settings(&s);
                    self.settings_open.store(true, Ordering::Relaxed);
                    self.settings_focus.store(true, Ordering::Relaxed);
                }
                TrayCmd::OpenAbout => {
                    self.about_open.store(true, Ordering::Relaxed);
                    self.about_focus.store(true, Ordering::Relaxed);
                }
                TrayCmd::OpenStatus => {
                    self.status_open.store(true, Ordering::Relaxed);
                    self.status_focus.store(true, Ordering::Relaxed);
                    windows::status::spawn_load(
                        self.status_win.clone(),
                        self.status_refreshing.clone(),
                        self.dir.as_ref().clone(),
                        self.latest.lhm_connected,
                        ui.ctx().clone(),
                    );
                }
                TrayCmd::OpenUpdater => {
                    self.updater_open.store(true, Ordering::Relaxed);
                    self.updater_focus.store(true, Ordering::Relaxed);
                }
                TrayCmd::ToggleFloating => {
                    let new_mode = {
                        let mut s = self.current_settings.lock_safe();
                        s.floating_mode = !s.floating_mode;
                        self.persist_settings_logged(&s);
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
                            let profile =
                                self.current_settings.lock_safe().dashboard_profile.clone();
                            let ([px, py], [w, h]) = self.fixed_window_geometry(&profile);
                            // Apply level + opacity BEFORE moving on-screen so they are
                            // already in effect when the window becomes visible.
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                    Self::window_level_from_layer(&self.window_layer),
                                ));
                            #[cfg(windows)]
                            win_opacity::set_opacity(self.hwnd, self.opacity);
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::Pos2::new(px, py),
                                ));
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                egui::Vec2::new(w, h),
                            ));
                            // See note in the settings-reload transition: clear the
                            // fit-to-content guard so the next fixed frame snaps the
                            // window to the true content height instead of the
                            // compute_window_height estimate (which can clip the
                            // bottom panel).
                            self.last_fitted_height = None;
                            // Re-apply for the next few frames as winit may reset the
                            // window level when it processes the move event.
                            self.reapply_window_props_frames = 4;
                        }
                    }
                }
                TrayCmd::ToggleRecording => {
                    let (new_enabled, retention_days) = {
                        let mut s = self.current_settings.lock_safe();
                        s.logging_enabled = !s.logging_enabled;
                        self.persist_settings_logged(&s);
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
            let mut s = self.current_settings.lock_safe();
            s.floating_panels_locked = arc_locked;
            self.persist_settings_logged(&s);
        }

        // ── Secondary windows ─────────────────────────────────────────────────

        let main_ctx = ui.ctx().clone();
        let dc = self.dialog_colors;

        // When the last dialog closes, restore the main-window dark visuals.
        // We only call set_visuals on the transition frame (not every frame) to
        // avoid the repaint loop that causes jerky window dragging in light mode.
        let any_dialog_open = self.settings_open.load(Ordering::Relaxed)
            || self.about_open.load(Ordering::Relaxed)
            || self.status_open.load(Ordering::Relaxed)
            || self.updater_open.load(Ordering::Relaxed);
        if !any_dialog_open && self.any_dialog_open_prev {
            let mut vis = egui::Visuals::dark();
            vis.panel_fill = egui::Color32::TRANSPARENT;
            vis.window_fill = egui::Color32::from_gray(28);
            vis.override_text_color = Some(theme::C_TEXT);
            ui.ctx().set_visuals(vis);
        }
        self.any_dialog_open_prev = any_dialog_open;

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
                        &dc,
                    );
                },
            );
            #[cfg(windows)]
            win32_dark_mode::apply_titlebar_theme(found_hwnd, self.os_dark_mode);
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
                    windows::about::show(child_ui.ctx(), &mctx, &open, &focus, &dir, &dc);
                },
            );
            #[cfg(windows)]
            win32_dark_mode::apply_titlebar_theme(found_hwnd, self.os_dark_mode);
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
            let refreshing = self.status_refreshing.clone();
            let collecting = self.status_collecting.clone();
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
                        &refreshing,
                        &collecting,
                        &dir,
                        lhm_connected,
                        &dc,
                    );
                },
            );
            #[cfg(windows)]
            win32_dark_mode::apply_titlebar_theme(found_hwnd, self.os_dark_mode);
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
                    windows::updater::show(child_ui.ctx(), &mctx, &open, &focus, &state, &dc);
                },
            );
            #[cfg(windows)]
            win32_dark_mode::apply_titlebar_theme(found_hwnd, self.os_dark_mode);
            #[cfg(windows)]
            if wants_focus {
                if found_hwnd != 0 {
                    win_opacity::bring_to_foreground(found_hwnd);
                } else {
                    focus.store(true, Ordering::Relaxed);
                }
            }

            // When the window sets status to Checking (manual button click),
            // kick off check+download on a plain OS thread — update_check is
            // synchronous and tokio::spawn is not safe to call from the egui
            // UI thread (which is not inside a tokio async context).
            let is_checking = matches!(
                self.updater_win.lock_safe().status,
                windows::updater::UpdateStatus::Checking
            );
            if is_checking && !self.updater_busy.swap(true, Ordering::Relaxed) {
                let win = self.updater_win.clone();
                let busy = self.updater_busy.clone();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let thread_result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let result = update_check::check();
                            match result {
                                Ok(update_check::CheckResult::UpdateAvailable(info)) => {
                                    let version = info.version.clone();
                                    let url = info.url.clone();
                                    let dest = update_check::installer_temp_path(&version);
                                    {
                                        let mut s = win.lock_safe();
                                        s.status = windows::updater::UpdateStatus::Downloading {
                                            downloaded: 0,
                                            total: 0,
                                        };
                                    }
                                    ctx.request_repaint();
                                    let win2 = win.clone();
                                    let ctx2 = ctx.clone();
                                    let dl_result =
                                        update_check::download(&url, &dest, |downloaded, total| {
                                            let mut s = win2.lock_safe();
                                            s.status =
                                                windows::updater::UpdateStatus::Downloading {
                                                    downloaded,
                                                    total,
                                                };
                                            ctx2.request_repaint();
                                        });
                                    let mut s = win.lock_safe();
                                    s.status = match dl_result {
                                        Ok(()) => windows::updater::UpdateStatus::Ready {
                                            info,
                                            installer_path: dest,
                                        },
                                        Err(e) => windows::updater::UpdateStatus::Error(e),
                                    };
                                }
                                Ok(update_check::CheckResult::UpToDate) => {
                                    win.lock_safe().status =
                                        windows::updater::UpdateStatus::UpToDate;
                                }
                                Err(e) => {
                                    win.lock_safe().status =
                                        windows::updater::UpdateStatus::Error(e);
                                }
                            }
                        }));
                    if let Err(_panic) = thread_result {
                        if let Ok(mut s) = win.lock() {
                            s.status = windows::updater::UpdateStatus::Error(
                                "Update check failed unexpectedly".to_string(),
                            );
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

        if self.floating_mode {
            // ── Floating mode — each panel in its own borderless viewport ─────
            self.render_floating_panels(ui);

            // Persist positions when any panel was dragged.
            if self.positions_dirty.swap(false, Ordering::Relaxed) {
                self.persist_floating_positions();
            }

            // Apply GPU preference change made from the floating GPU panel.
            if let Some(new_pref) = self.float_new_pref_gpu.lock_safe().take() {
                *self.preferred_gpu.lock_safe() = Some(new_pref.clone());
                let mut s = self.current_settings.lock_safe();
                s.preferred_gpu = Some(new_pref);
                self.persist_settings_logged(&s);
            }
        } else {
            // ── Fixed mode — all panels in one portrait/landscape window ──────

            // Track the window's current outer position so the padlock can pin
            // the exact spot it is at when clicked.
            if let Some(outer) = ui.ctx().input(|i| i.viewport().outer_rect) {
                self.last_fixed_pos = Some([outer.left().round(), outer.top().round()]);
            }

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

            // Padlock at the right of the drag strip: pins the whole dashboard.
            let pinned = self.dashboard_pinned;
            let padlock_center = egui::pos2(drag_rect.right() - 12.0, drag_rect.center().y);
            let padlock_hit = egui::Rect::from_center_size(
                padlock_center,
                egui::Vec2::new(24.0, drag_rect.height().max(14.0)),
            );
            let hover_pos = ui.ctx().input(|i| i.pointer.hover_pos());
            let over_padlock = hover_pos.map(|p| padlock_hit.contains(p)).unwrap_or(false);
            let padlock_resp = ui.interact(
                padlock_hit,
                ui.id().with("fixed_padlock"),
                egui::Sense::click(),
            );

            // Drag only when not pinned and not starting on the padlock.
            if drag_resp.dragged() && !pinned && !over_padlock {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            // After drag ends in "behind" mode, the window was activated for SC_MOVE
            // and is now in front. Push it back behind on the next few frames.
            if drag_resp.drag_stopped() && self.window_layer == "behind" {
                self.reapply_window_props_frames = 4;
            }

            if padlock_resp.clicked() {
                self.dashboard_pinned = !self.dashboard_pinned;
                let profile = self.current_settings.lock_safe().dashboard_profile.clone();
                let mut s = self.current_settings.lock_safe();
                s.dashboard_pinned = self.dashboard_pinned;
                if self.dashboard_pinned {
                    // Pin: remember the current window position for this profile.
                    if let Some([x, y]) = self.last_fixed_pos {
                        s.pinned_positions
                            .insert(profile, [x.round() as i32, y.round() as i32]);
                    }
                }
                self.persist_settings_logged(&s);
            }
            if padlock_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            // Drag dots — only when movable (unpinned) and hovering the strip.
            if drag_resp.hovered() && !pinned && !over_padlock {
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
            // Padlock icon — shown while pinned (so it can be released) or when
            // hovering the strip (so it is discoverable).
            if pinned || drag_resp.hovered() || padlock_resp.hovered() {
                let color = if pinned {
                    self.app_theme.accent
                } else if padlock_resp.hovered() {
                    egui::Color32::from_gray(210)
                } else {
                    egui::Color32::from_gray(150)
                };
                draw_padlock(ui.painter(), padlock_center, pinned, color);
            }

            // Panels in the order defined by visible_panels (respects user reordering).
            let panels_to_draw = self.visible_panels.clone();
            let profile = self.current_settings.lock_safe().dashboard_profile.clone();
            // Extract update version once per frame (cheap lock read).
            let update_ver: Option<String> = {
                use windows::updater::UpdateStatus;
                let st = self.updater_win.lock_safe();
                if let UpdateStatus::Ready { info, .. } = &st.status {
                    Some(info.version.clone())
                } else {
                    None
                }
            };
            let mut new_preferred_gpu: Option<String> = None;

            if profile_is_landscape(&profile) {
                // ── Landscape grid — panels packed into an even, adaptive grid ──
                // The window is fixed to the profile size, so there is no per-frame
                // content-fit; the grid fills the available area exactly.
                new_preferred_gpu =
                    self.render_landscape_grid(ui, &panels_to_draw, update_ver.as_deref());
            } else {
                // ── Portrait vertical stack ─────────────────────────────────────
                let sc = profile_scale(&profile);
                // Fullscreen + centered: pad the top so the panel stack is vertically
                // centered in the filled window. Panel proportions are untouched —
                // only the surrounding background grows. Use the measured content
                // height from the previous frame (cached) for an exact center; fall
                // back to the compute_window_height estimate on the first frame.
                let center_fullscreen = self.fullscreen_mode && self.fullscreen_align == "center";
                let center_pad = if center_fullscreen {
                    // Pure panel-stack height (excludes drag handle and pad).
                    let content = self.fullscreen_content_h.unwrap_or_else(|| {
                        compute_window_height(&panels_to_draw, sc) - theme::DRAG_HANDLE_H
                    });
                    // Center the visible content with equal background above and below.
                    // The drag handle occupies the first DRAG_HANDLE_H px (invisible),
                    // so discount it from the top so the content itself is centered.
                    let pad =
                        ((ui.available_height() - content - theme::DRAG_HANDLE_H) * 0.5).max(0.0);
                    ui.add_space(pad);
                    pad
                } else {
                    0.0
                };
                for panel in &panels_to_draw {
                    if let Some(p) = self.draw_one_panel(ui, panel, sc, update_ver.as_deref()) {
                        new_preferred_gpu = Some(p);
                    }
                    ui.add_space((6.0 * sc).round());
                }
                // Cache the true panel-stack content height for exact centering next
                // frame: min_rect = drag handle + center pad + content, so content =
                // min_rect − DRAG_HANDLE_H − pad. Re-center on the next frame if the
                // measurement drifts from what we used this frame.
                if center_fullscreen {
                    let content = ui.min_rect().height() - theme::DRAG_HANDLE_H - center_pad;
                    if content > 10.0 {
                        let changed = self
                            .fullscreen_content_h
                            .map(|h| (h - content).abs() > 0.5)
                            .unwrap_or(true);
                        self.fullscreen_content_h = Some(content);
                        if changed {
                            ui.ctx().request_repaint();
                        }
                    }
                }
                // Fit window height to actual rendered content every frame so no black gap
                // appears regardless of panel set, spacing, or egui version.
                // Skipped in fullscreen mode — there the window stays at the full monitor
                // size and must NOT shrink to content.
                if !self.fullscreen_mode {
                    let used_h = ui.min_rect().height();
                    // During the first frames after launch the window may not be fully
                    // realized, so early InnerSize commands can be dropped — which would
                    // leave the bottom panel clipped until the user toggles floating mode.
                    // Force a re-fit (and a fast repaint) for a handful of frames so the
                    // true content height always sticks at startup.
                    if self.startup_fit_frames > 0 {
                        self.startup_fit_frames -= 1;
                        self.last_fitted_height = None;
                        ui.ctx().request_repaint();
                    }
                    let changed = self
                        .last_fitted_height
                        .map(|h| (h - used_h).abs() > 0.5)
                        .unwrap_or(true);
                    if used_h > 10.0 && changed {
                        self.last_fitted_height = Some(used_h);
                        let [w, _] = profile_to_size(&profile);
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
                            egui::Vec2::new(w - 2.0, used_h),
                        ));
                    }
                }
            }

            if let Some(new_pref) = new_preferred_gpu {
                *self.preferred_gpu.lock_safe() = Some(new_pref.clone());
                let mut s = self.current_settings.lock_safe();
                s.preferred_gpu = Some(new_pref);
                self.persist_settings_logged(&s);
            }
        }

        // Idle repaint rate: 1 fps regardless of whether a dialog is open.
        // Interaction responsiveness (hover, click, drag) is driven by OS input events
        // which wake eframe immediately — request_repaint_after only matters when idle.
        // All dialogs call main_ctx.request_repaint_of(ROOT) on close, so the
        // show_viewport_immediate cleanup on the next frame is already fast.
        ui.ctx().request_repaint_after(Duration::from_secs(1));
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
    fn window_level_from_layer(layer: &str) -> egui::WindowLevel {
        match layer {
            "on_top" => egui::WindowLevel::AlwaysOnTop,
            "behind" => egui::WindowLevel::AlwaysOnBottom,
            _ => egui::WindowLevel::Normal,
        }
    }

    /// Draw a single panel by key into `ui` at content scale `sc`. Returns a new
    /// preferred-GPU key if the GPU panel's device selector was clicked. Shared by
    /// the portrait vertical stack and the landscape grid.
    fn draw_one_panel(
        &self,
        ui: &mut egui::Ui,
        panel: &str,
        sc: f32,
        update_ver: Option<&str>,
    ) -> Option<String> {
        let mut new_pref = None;
        match panel {
            // Panels always render at full opacity; window-level transparency is
            // applied by SetLayeredWindowAttributes (win_opacity module).
            "header" => {
                let _ = panels::header::draw(
                    ui,
                    &self.latest,
                    &self.textures,
                    1.0,
                    &self.app_theme,
                    sc,
                );
            }
            "clock" => {
                let _ = panels::clock::draw(
                    ui,
                    self.latest.uptime_secs,
                    1.0,
                    &self.app_theme,
                    update_ver,
                    sc,
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
                    sc,
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
                    self.thresholds.gpu_hotspot.0,
                    self.thresholds.gpu_hotspot.1,
                    sc,
                )
                .0
                {
                    new_pref = Some(p);
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
                    sc,
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
                    sc,
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
                    sc,
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
                    sc,
                );
            }
            "process" => {
                let _ = panels::process::draw(ui, &self.latest, 1.0, &self.app_theme, sc);
            }
            "battery" => {
                let _ = panels::battery::draw(
                    ui,
                    &self.latest,
                    1.0,
                    &self.app_theme,
                    sc,
                    self.thresholds.battery.0,
                    self.thresholds.battery.1,
                    self.thresholds.battery_power.0,
                    self.thresholds.battery_power.1,
                );
            }
            _ => {}
        }
        new_pref
    }

    /// Render the visible panels as an even, adaptive grid for landscape profiles.
    ///
    /// The column count is chosen so each cell is close to the portrait card width
    /// (~450 px), then the rows follow from the panel count. Every cell is the same
    /// size and the per-cell content scale `sc` is derived from the cell dimensions,
    /// so panels shrink/grow to fit any landscape resolution. Returns a new
    /// preferred-GPU key if the GPU device selector was clicked.
    fn render_landscape_grid(
        &self,
        ui: &mut egui::Ui,
        panels: &[String],
        update_ver: Option<&str>,
    ) -> Option<String> {
        let n = panels.len();
        if n == 0 {
            return None;
        }
        let gap = 6.0_f32;
        let avail_w = ui.available_width().max(40.0);
        let avail_h = ui.available_height().max(40.0);

        // Choose the column count that maximises the per-cell content scale, so
        // panels are as large as possible for the given screen shape. Ties are
        // broken toward fewer rows (wider cells suit short landscape screens).
        let mut n_cols = 1usize;
        let mut best_s = f32::MIN;
        let mut best_rows = usize::MAX;
        for c in 1..=n {
            let rows = n.div_ceil(c);
            let cw = ((avail_w - gap * (c as f32 - 1.0)) / c as f32).max(40.0);
            let ch = ((avail_h - gap * (rows as f32 - 1.0)) / rows as f32).max(40.0);
            let s = (cw / 450.0).min(ch / 224.0).clamp(0.4, 1.6);
            if s > best_s + 0.02 || ((s - best_s).abs() <= 0.02 && rows < best_rows) {
                n_cols = c;
                best_s = s;
                best_rows = rows;
            }
        }
        let n_rows = n.div_ceil(n_cols);

        let cell_w = ((avail_w - gap * (n_cols as f32 - 1.0)) / n_cols as f32).max(40.0);
        let cell_h = ((avail_h - gap * (n_rows as f32 - 1.0)) / n_rows as f32).max(40.0);

        // Per-cell content scale. Reference card is 450 px wide × ~224 px tall
        // (PANEL_DATA_H + frame margins + a little slack). Use the smaller of the
        // width/height ratios so content never overflows the cell.
        let sc_w = cell_w / 450.0;
        let sc_h = cell_h / 224.0;
        let sc = sc_w.min(sc_h).clamp(0.4, 1.6);

        let origin = ui.cursor().min;
        let mut new_pref = None;
        for row in 0..n_rows {
            for col in 0..n_cols {
                let idx = row * n_cols + col;
                if idx >= n {
                    break;
                }
                let x = origin.x + col as f32 * (cell_w + gap);
                let y = origin.y + row as f32 * (cell_h + gap);
                let cell_rect =
                    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w, cell_h));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(cell_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                if let Some(p) = self.draw_one_panel(&mut child, &panels[idx], sc, update_ver) {
                    new_pref = Some(p);
                }
            }
        }
        // Advance the parent cursor past the whole grid.
        let total_h = n_rows as f32 * cell_h + (n_rows as f32 - 1.0) * gap;
        ui.allocate_space(egui::vec2(avail_w, total_h));
        new_pref
    }

    /// Fixed-mode window geometry: returns `(top_left, [w, h])`.
    /// Width is always the profile width so panel proportions never stretch.
    /// In fullscreen mode the height fills the portrait monitor (intended for a
    /// monitor whose resolution matches the profile); otherwise it is the
    /// content-fit estimate (the per-frame fit then refines it). Falls back to the
    /// content-fit size if no monitor is found.
    fn fixed_window_geometry(&self, profile: &str) -> ([f32; 2], [f32; 2]) {
        let [w, h] = profile_to_size(profile);
        // Landscape: the grid fills a fixed window the size of the profile,
        // pinned to the top-left of the best landscape monitor.
        let (pos, size) = if profile_is_landscape(profile) {
            let [x, y, _mw, _mh] = pick_window_rect_for_profile(profile);
            ([x, y], [w, h])
        } else if self.fullscreen_mode {
            let [x, y, _mw, mh] = pick_window_rect();
            if mh > 0.0 {
                ([x, y], [w, mh])
            } else {
                let h = compute_window_height(&self.visible_panels, profile_scale(profile));
                (pick_window_position(), [w, h])
            }
        } else {
            let h = compute_window_height(&self.visible_panels, profile_scale(profile));
            (pick_window_position(), [w, h])
        };
        // Pinned override: keep the computed size but restore the saved position
        // for this profile instead of auto-targeting a monitor.
        if let Some(pp) = self.pinned_position(profile) {
            return (pp, size);
        }
        (pos, size)
    }

    /// Returns the saved pinned position for `profile` when the dashboard is
    /// pinned and the stored position is still on a connected monitor; otherwise
    /// `None` so the caller auto-targets the matching monitor.
    fn pinned_position(&self, profile: &str) -> Option<[f32; 2]> {
        if !self.dashboard_pinned {
            return None;
        }
        let saved = {
            let s = self.current_settings.lock_safe();
            s.pinned_positions.get(profile).copied()
        };
        #[cfg(windows)]
        let monitors = win_monitor::list();
        #[cfg(not(windows))]
        let monitors: Vec<(i32, i32, i32, i32)> = Vec::new();
        resolve_pinned_position(self.dashboard_pinned, saved, &monitors)
    }

    /// Render every visible panel as its own borderless OS window using
    /// `show_viewport_immediate`.  Called each frame when `floating_mode` is true.
    ///
    /// `show_viewport_immediate` renders each child synchronously as part of the
    /// parent frame — no deferred callbacks, no separate event loops.  The parent
    /// ticks at ~1 fps (via `request_repaint_after(1 s)` in `update()`), so all
    /// panels naturally update at ~1 fps without any Win32 tricks.
    fn render_floating_panels(&mut self, ui: &mut egui::Ui) {
        let s = self.current_settings.lock_safe();
        let window_level = Self::window_level_from_layer(&s.window_layer);
        let scale = self.floating_panel_scale;
        drop(s);

        let opacity = self.opacity;

        for idx in 0..self.visible_panels.len() {
            let key = self.visible_panels[idx].clone();

            let init_pos: [f32; 2] = {
                let default_pos = [100.0 + idx as f32 * 20.0, 80.0 + idx as f32 * 30.0];
                let positions = self.floating_positions.lock_safe();
                let saved = positions.get(&key).copied().unwrap_or(default_pos);
                guard_panel_position(saved, default_pos)
            };

            let panel_w = 450.0 * scale;
            let initial_h = panel_initial_h(&key) * scale;

            // Only set window position on first creation.  After that the OS
            // owns the position (via drag); re-sending with_position every frame
            // causes egui to diff-and-dispatch SetOuterPosition continuously,
            // which fights the OS and produces sub-pixel blur.
            let needs_position = !self.panels_positioned.contains(&key);
            if needs_position {
                self.panels_positioned.insert(key.clone());
            }

            // Title shared between ViewportBuilder and Win32 FindWindowW lookup.
            let win_title = format!("RigStats \u{2014} {}", panel_label(&key));
            let mut vp_builder = egui::ViewportBuilder::default()
                .with_title(win_title.clone())
                .with_inner_size([panel_w, initial_h])
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_window_level(window_level);

            let is_behind = window_level == egui::WindowLevel::AlwaysOnBottom;

            if needs_position {
                vp_builder = vp_builder.with_position(init_pos);
            }

            // `show_viewport_immediate` is FnMut with no Send/'static bound —
            // we can borrow self fields directly instead of going through Arc.
            let positions_arc = &self.floating_positions;
            let dirty = &self.positions_dirty;
            let behind_enforce = &self.behind_enforce;
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
                let st = self.updater_win.lock_safe();
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
                            let mut pos = positions_arc.lock_safe();
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
                    // apply_behind() re-arms once the drag finishes.
                    //
                    // Crucially we DON'T re-push every frame: send_viewport_cmd +
                    // SetWindowPos each generate a fresh repaint, so re-asserting
                    // unconditionally creates a spin loop that runs at the full
                    // refresh rate (the cause of the floating-mode CPU spike).
                    // Instead we enforce on creation, in a short burst after a
                    // drag, and otherwise ~1/s as an idle safety net.
                    let primary_down = ctx.input(|i| i.pointer.primary_down());
                    if is_behind {
                        let now = Instant::now();
                        let mut map = behind_enforce.borrow_mut();
                        let st = map.entry(key.clone()).or_insert(BehindEnforce {
                            last_enforce: now.checked_sub(Duration::from_secs(10)).unwrap_or(now),
                            prev_primary_down: false,
                            force_until: now,
                        });
                        let released = st.prev_primary_down && !primary_down;
                        st.prev_primary_down = primary_down;
                        if released {
                            // Window was activated for the drag; snap it back for
                            // a short burst so it settles behind reliably.
                            st.force_until = now + Duration::from_millis(400);
                        }
                        let should_enforce = !primary_down
                            && (needs_position
                                || now < st.force_until
                                || now.duration_since(st.last_enforce)
                                    >= Duration::from_millis(750));
                        if should_enforce {
                            st.last_enforce = now;
                            drop(map);
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                egui::WindowLevel::AlwaysOnBottom,
                            ));
                            #[cfg(windows)]
                            win32_behind::apply_behind(&win_title);
                        }
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
                                        self.thresholds.gpu_hotspot.0,
                                        self.thresholds.gpu_hotspot.1,
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
                                "battery" => panels::battery::draw(
                                    ui,
                                    stats,
                                    1.0,
                                    &app_theme,
                                    scale,
                                    self.thresholds.battery.0,
                                    self.thresholds.battery.1,
                                    self.thresholds.battery_power.0,
                                    self.thresholds.battery_power.1,
                                ),
                                _ => egui::Rect::NOTHING,
                            };

                            if let Some(p) = new_pref {
                                *new_pref_arc.lock_safe() = Some(p);
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

    /// Persist settings to disk, logging any failure instead of swallowing it
    /// silently — a failed write means the user loses settings/layout changes,
    /// which should never pass unnoticed.
    fn persist_settings_logged(&self, s: &settings::Settings) {
        if let Err(e) = settings::persist_settings(&self.dir, s) {
            debug::log_error(&self.dir, &format!("settings: persist failed — {e}"));
        }
    }

    /// Flush `floating_positions` → `panel_layouts` in settings and persist to disk.
    fn persist_floating_positions(&self) {
        let positions = self.floating_positions.lock_safe();
        let mut s = self.current_settings.lock_safe();
        for (key, &[x, y]) in positions.iter() {
            s.panel_layouts.insert(
                key.clone(),
                settings::PanelLayout {
                    x: x as i32,
                    y: y as i32,
                },
            );
        }
        drop(positions);
        self.persist_settings_logged(&s);
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
                    model: d.model.clone(),
                    kind: d.kind,
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
    debug::log_debug(&dir, &format!("hardware: ram_spec={ram_spec}"));
    let ram_details = tokio::task::spawn_blocking(hardware::detect_ram_details)
        .await
        .unwrap_or_default();
    debug::log_debug(&dir, &format!("hardware: ram_details={ram_details}"));
    let disk_model_map: HashMap<String, String> =
        tokio::task::spawn_blocking(hardware::detect_disk_model_map)
            .await
            .unwrap_or_default();
    debug::log_debug(
        &dir,
        &format!("hardware: disk_model_map entries={}", disk_model_map.len()),
    );
    let disk_type_map: HashMap<String, rigstats_backend::stats::DiskKind> =
        tokio::task::spawn_blocking(hardware::detect_disk_type_map)
            .await
            .unwrap_or_default();
    debug::log_debug(
        &dir,
        &format!("hardware: disk_type_map entries={}", disk_type_map.len()),
    );
    let ping_target = hardware::detect_ping_target();
    debug::log_debug(&dir, &format!("hardware: ping_target={ping_target}"));
    let mb_board: Option<String> = tokio::task::spawn_blocking(hardware::detect_motherboard_name)
        .await
        .ok()
        .flatten();
    debug::log_debug(&dir, &format!("hardware: mb_board={mb_board:?}"));
    let model_name: String = tokio::task::spawn_blocking(hardware::detect_model_name)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    debug::log_debug(&dir, &format!("hardware: model_name={model_name}"));
    let system_brand: String = tokio::task::spawn_blocking(hardware::detect_system_brand)
        .await
        .unwrap_or_default();
    debug::log_debug(&dir, &format!("hardware: system_brand={system_brand}"));
    let gpu_name: String = tokio::task::spawn_blocking(hardware::detect_gpu_name)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    debug::log_debug(&dir, &format!("hardware: gpu_name={gpu_name}"));

    let mut sys = System::new();
    sys.refresh_cpu();
    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_default();
    let hostname = System::host_name().unwrap_or_else(|| "—".to_string());
    debug::log_debug(&dir, &format!("hardware: cpu_model={cpu_model}"));
    debug::log_debug(&dir, &format!("hardware: hostname={hostname}"));

    let mut disks = Disks::new_with_refreshed_list();
    let mut networks = Networks::new_with_refreshed_list();
    let pipe = tokio::sync::Mutex::new(None::<lhm::LhmPipeReader>);

    let mut last_net_instant = Instant::now();
    let mut last_ping: Option<(Instant, Option<f64>)> = None;
    let mut last_ping_wan: Option<(Instant, Option<f64>)> = None;
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
                    model: String::new(),
                    kind: rigstats_backend::stats::DiskKind::Unknown,
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
        let ping_wan_ms = {
            let stale = last_ping_wan
                .as_ref()
                .map(|(t, _)| t.elapsed().as_secs_f64() >= 5.0)
                .unwrap_or(true);
            if stale {
                let measured = tokio::task::spawn_blocking(|| hardware::sample_ping_ms("1.1.1.1"))
                    .await
                    .unwrap_or(None);
                last_ping_wan = Some((Instant::now(), measured));
                measured
            } else {
                last_ping_wan.as_ref().and_then(|(_, v)| *v)
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
                        debug::log_warn(
                            &dir,
                            &format!("battery: sample_battery_wmi join error: {err}"),
                        );
                        None
                    }
                };
                if result.is_none() {
                    match tokio::task::spawn_blocking(hardware::probe_wmi_status).await {
                        Ok(Err(err)) => debug::log_warn(
                            &dir,
                            &format!("battery: WMI probe failed after battery read miss: {err}"),
                        ),
                        Err(err) => debug::log_warn(
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

        let pref = preferred_gpu.lock_safe().clone();
        let lhm_data = lhm::fetch_lhm_pipe(&pipe, pref.as_deref(), &dir).await;
        let lhm_connected = lhm_data.is_some();
        lhm_process::track_lhm_connection_state(&dir, lhm_connected);

        for drive in disk_drives.iter_mut() {
            let key = drive.fs.trim_end_matches(['\\', '/']).to_string();
            if let Some(model) = disk_model_map.get(&key) {
                drive.model = model.clone();
                drive.kind = disk_type_map
                    .get(model.as_str())
                    .copied()
                    .unwrap_or_default();
            }
        }

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

        let display_model_name = {
            let s = settings_arc.lock_safe();
            if s.model_name.is_empty() {
                model_name.clone()
            } else {
                s.model_name.clone()
            }
        };

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
            ram_details: ram_details.clone(),
            net_up_mbps: best_up,
            net_down_mbps: best_down,
            net_iface: best_iface,
            net_ping_ms: ping_ms,
            net_ping_wan_ms: ping_wan_ms,
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
            model_name: display_model_name,
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
            let s = settings_arc.lock_safe();
            (s.logging_enabled, s.log_retention_days)
        };
        if log_enabled {
            let payload = poll_stats_to_log_payload(&stats);
            if let Err(e) = logging::append_stats_row(&payload, &dir) {
                debug::log_error(&dir, &format!("logging: csv write error — {e}"));
            }
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
    debug::append_debug_log(&dir, "rigstats starting");
    debug::append_debug_log(&dir, &format!("settings dir: {}", dir.display()));

    #[cfg(windows)]
    {
        let dark = win32_dark_mode::is_system_dark_mode();
        debug::log_debug(&dir, &format!("os_dark_mode: {dark}"));
    }

    let s = settings::load_settings(&dir);
    let visible_panels = s.visible_panels.clone();
    let opacity = s.opacity.clamp(0.1, 1.0) as f32;
    let always_on_top = s.window_layer == "on_top";
    let [win_w, win_h] = profile_to_size(&s.dashboard_profile);
    let landscape = profile_is_landscape(&s.dashboard_profile);
    // Width is always the profile width (panels never stretch). In fullscreen the
    // height fills the monitor and the window pins to the monitor top-left; the
    // 2 px width trim used in normal mode is dropped so a matching screen fills.
    let fullscreen = s.fullscreen_mode && !s.floating_mode;
    let (inner_w, inner_h, mut pos_x, mut pos_y) = if landscape {
        // Landscape: the grid fills a fixed window the size of the profile, pinned
        // to the top-left of the monitor that best matches the profile resolution.
        let [mx, my, _mw, _mh] = pick_window_rect_for_profile(&s.dashboard_profile);
        (win_w, win_h, mx, my)
    } else {
        let [bx, by] = pick_window_position();
        if fullscreen {
            let [mx, my, _mw, mh] = pick_window_rect();
            if mh > 0.0 {
                (win_w, mh, mx, my)
            } else {
                let h = compute_window_height(&visible_panels, profile_scale(&s.dashboard_profile));
                (win_w - 2.0, h, bx, by)
            }
        } else {
            let h = compute_window_height(&visible_panels, profile_scale(&s.dashboard_profile));
            (win_w - 2.0, h, bx, by)
        }
    };
    // Pinned dashboard: restore the saved position for this profile (keeping the
    // computed size) instead of auto-targeting a monitor, when still on-screen.
    if !s.floating_mode && s.dashboard_pinned {
        let saved = s.pinned_positions.get(&s.dashboard_profile).copied();
        #[cfg(windows)]
        let monitors = win_monitor::list();
        #[cfg(not(windows))]
        let monitors: Vec<(i32, i32, i32, i32)> = Vec::new();
        if let Some([x, y]) = resolve_pinned_position(true, saved, &monitors) {
            pos_x = x;
            pos_y = y;
        }
    }
    debug::log_debug(
        &dir,
        &format!(
            "settings: profile={} panels={} opacity={opacity:.2} floating_mode={}",
            s.dashboard_profile,
            visible_panels.join(","),
            s.floating_mode
        ),
    );
    debug::log_debug(
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
        .with_icon(load_app_icon())
        .with_inner_size([inner_w, inner_h])
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
            // Suppress egui's built-in panel backgrounds; clear_color provides the base fill.
            visuals.panel_fill = egui::Color32::TRANSPARENT;
            // Popups (ComboBox, tooltips) use window_fill — keep it solid so they're readable.
            visuals.window_fill = egui::Color32::from_gray(28);
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
            let settings_id = tray.settings_id.clone();
            let about_id = tray.about_id.clone();
            let status_id = tray.status_id.clone();
            let updater_id = tray.updater_id.clone();
            let floating_id = tray.floating_id.clone();
            let recording_id = tray.recording_id.clone();
            let dir_tray = dir.clone();
            std::thread::spawn(move || loop {
                // Guard each iteration: a panic in a tray/Win32 call must not
                // silently kill this thread, or tray clicks would stop working
                // for the rest of the session. Recover and log instead.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
                            debug::append_debug_log(&dir_tray, "shutdown: clean (tray quit)");
                            std::process::exit(0);
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
                    // Drain tray-icon click events so they don't accumulate. We no
                    // longer act on left-click — the dashboard is always visible.
                    let _ = tray_icon::TrayIconEvent::receiver().try_recv();
                    if repaint {
                        ctx.request_repaint();
                    }
                }));
                if outcome.is_err() {
                    debug::log_warn(
                        &dir_tray,
                        "tray: event handler panicked — thread recovered, continuing",
                    );
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
            let fm_arc_hb = Arc::new(AtomicBool::new(current_settings.lock_safe().floating_mode));
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

            // Check for --just-updated=VERSION argument passed by the NSIS /autoupdate installer.
            if let Some(version) = std::env::args()
                .find(|a| a.starts_with("--just-updated="))
                .and_then(|a| a.split_once('=').map(|x| x.1.to_owned()))
            {
                if !version.is_empty() {
                    updater_win_bg.lock_safe().status =
                        windows::updater::UpdateStatus::JustUpdated { version };
                    updater_open_bg.store(true, Ordering::Relaxed);
                    updater_focus_bg.store(true, Ordering::Relaxed);
                }
            }

            {
                let win = updater_win_bg.clone();
                let open = updater_open_bg.clone();
                let focus = updater_focus_bg.clone();
                let ctx = cc.egui_ctx.clone();
                let dir_upd = dir.clone();
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
                                    let mut s = win.lock_safe();
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
                                        let mut s = win2.lock_safe();
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
                                        let mut s = win.lock_safe();
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
                                        win.lock_safe().status =
                                            windows::updater::UpdateStatus::Error(e);
                                    }
                                }
                            }
                            Ok(update_check::CheckResult::UpToDate) => {}
                            Err(e) => {
                                debug::log_warn(
                                    &dir_upd,
                                    &format!("update-check: background check failed — {e}"),
                                );
                            }
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

    debug::append_debug_log(&dir, "shutdown: clean");
    runtime.shutdown_background();
}

#[cfg(test)]
mod tests {
    use super::{
        position_on_any_monitor, profile_is_landscape, profile_to_size, resolve_pinned_position,
        select_profile_monitor,
    };

    #[test]
    fn landscape_profiles_detected_by_prefix() {
        assert!(profile_is_landscape("landscape-xl"));
        assert!(profile_is_landscape("landscape-4k-top"));
        assert!(!profile_is_landscape("portrait-xl"));
        assert!(!profile_is_landscape("portrait-4k-side"));
    }

    #[test]
    fn landscape_dims_are_wide() {
        for p in [
            "landscape-xl",
            "landscape-slim",
            "landscape-hd",
            "landscape-wxga",
            "landscape-fhd",
            "landscape-wuxga",
            "landscape-qhd",
            "landscape-hdplus",
            "landscape-1600x900",
            "landscape-1680x1050",
            "landscape-2560x1600",
            "landscape-4k",
            "landscape-fhd-top",
            "landscape-qhd-top",
            "landscape-4k-top",
        ] {
            let [w, h] = profile_to_size(p);
            assert!(w >= h, "{p} should be landscape (w >= h), got {w}x{h}");
        }
    }

    #[test]
    fn landscape_is_transpose_of_portrait() {
        // Each landscape profile is the matching portrait profile with axes swapped.
        let pairs = [
            ("portrait-xl", "landscape-xl"),
            ("portrait-fhd", "landscape-fhd"),
            ("portrait-qhd", "landscape-qhd"),
            ("portrait-4k", "landscape-4k"),
            ("portrait-fhd-side", "landscape-fhd-top"),
            ("portrait-qhd-side", "landscape-qhd-top"),
            ("portrait-4k-side", "landscape-4k-top"),
        ];
        for (portrait, landscape) in pairs {
            let [pw, ph] = profile_to_size(portrait);
            let [lw, lh] = profile_to_size(landscape);
            assert_eq!(
                [pw, ph],
                [lh, lw],
                "{landscape} should transpose {portrait}"
            );
        }
    }

    #[test]
    fn profile_monitor_prefers_matching_strip() {
        // Primary 2560x1440 at origin + a 1920x450 strip at (2560,0).
        let monitors = [(0, 0, 2560, 1440), (2560, 0, 4480, 450)];
        // landscape-xl is 1920x450 → must pick the strip (index 1).
        let [w, h] = profile_to_size("landscape-xl");
        assert_eq!(select_profile_monitor(&monitors, w, h), Some(1));
    }

    #[test]
    fn profile_monitor_falls_back_to_primary_when_no_match() {
        // Same layout; landscape-fhd-top (1080x253) matches neither screen, so it
        // must land on the primary monitor at the origin (index 0), NOT the strip.
        let monitors = [(0, 0, 2560, 1440), (2560, 0, 4480, 450)];
        let [w, h] = profile_to_size("landscape-fhd-top");
        assert_eq!(select_profile_monitor(&monitors, w, h), Some(0));
    }

    #[test]
    fn profile_monitor_empty_is_none() {
        let [w, h] = profile_to_size("landscape-xl");
        assert_eq!(select_profile_monitor(&[], w, h), None);
    }

    #[test]
    fn position_on_monitor_respects_overhang_margin() {
        let monitors = [(0, 0, 2560, 1440)];
        // Inside the monitor.
        assert!(position_on_any_monitor([100.0, 100.0], &monitors));
        // 40 px past the right edge — within the 60 px overhang margin.
        assert!(position_on_any_monitor([2600.0, 100.0], &monitors));
        // 200 px past the right edge — off-screen.
        assert!(!position_on_any_monitor([2760.0, 100.0], &monitors));
        // No monitors at all → never on-screen.
        assert!(!position_on_any_monitor([0.0, 0.0], &[]));
    }

    #[test]
    fn pinned_position_none_when_unpinned() {
        let monitors = [(0, 0, 2560, 1440)];
        // Even with a valid saved position, an unpinned dashboard auto-targets.
        assert_eq!(
            resolve_pinned_position(false, Some([100, 100]), &monitors),
            None
        );
    }

    #[test]
    fn pinned_position_none_when_no_saved() {
        let monitors = [(0, 0, 2560, 1440)];
        // Pinned but nothing saved for this profile → caller auto-targets.
        assert_eq!(resolve_pinned_position(true, None, &monitors), None);
    }

    #[test]
    fn pinned_position_restores_saved_when_on_screen() {
        let monitors = [(0, 0, 2560, 1440), (2560, 0, 4480, 450)];
        // Saved position sits on the strip → restored verbatim.
        assert_eq!(
            resolve_pinned_position(true, Some([2600, 10]), &monitors),
            Some([2600.0, 10.0])
        );
    }

    #[test]
    fn pinned_position_dropped_when_monitor_gone() {
        // The strip the position was saved on is now disconnected.
        let monitors = [(0, 0, 2560, 1440)];
        // Saved at (3000, 10) — well off the remaining monitor → auto-target.
        assert_eq!(
            resolve_pinned_position(true, Some([3000, 10]), &monitors),
            None
        );
    }
}
