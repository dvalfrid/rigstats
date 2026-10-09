//! System tray icon, menu, and tray-command channel, plus small panel-label
//! helpers. Extracted from `main.rs`.

use crate::menu_icons;
use crate::theme;
use eframe::egui;
use rigstats_backend::control::Profile;
use rigstats_backend::lhm;
use std::sync::mpsc;
use tray_icon::{
    menu::{CheckMenuItem, IconMenuItem, Menu, MenuId, PredefinedMenuItem, Submenu},
    Icon, TrayIconBuilder,
};

/// Commands sent from the tray-polling thread to the UI thread.
#[derive(Debug, PartialEq, Eq)]
pub enum TrayCmd {
    OpenSettings,
    OpenAbout,
    OpenStatus,
    OpenUpdater,
    OpenDocs,
    OpenHistory,
    OpenControlCenter,
    ToggleFloating,
    ToggleRecording,
    ToggleOverlay,
    ToggleOverlayLock,
    /// Switch the light bar's desk lamp off or back on (#214).
    ToggleLamp,
    /// Set the displayed GPU; `None` = automatic (highest VRAM).
    SelectGpu(Option<String>),
    /// Apply a Control Center profile by id (#187).
    SelectProfile(String),
}

/// Menu-id prefix of the GPU submenu rows. Ids are derived from the choice
/// itself (not generated), so the tray-polling thread can decode a click with
/// [`gpu_choice_from_menu_id`] without sharing the (non-`Send`) menu items.
const GPU_ID_AUTO: &str = "gpu:auto";
const GPU_ID_PREFIX: &str = "gpu:name:";

fn gpu_menu_id(name: Option<&str>) -> MenuId {
    match name {
        None => MenuId::new(GPU_ID_AUTO),
        Some(n) => MenuId::new(format!("{GPU_ID_PREFIX}{n}")),
    }
}

/// Decodes a GPU submenu click: `Some(None)` = Automatic, `Some(Some(name))`
/// = that adapter, `None` = not a GPU submenu row.
pub fn gpu_choice_from_menu_id(id: &MenuId) -> Option<Option<String>> {
    let id = id.as_ref();
    if id == GPU_ID_AUTO {
        Some(None)
    } else {
        id.strip_prefix(GPU_ID_PREFIX)
            .map(|name| Some(name.to_string()))
    }
}

/// Index of the listed adapter matching `preferred` (`Settings.preferred_gpu`),
/// or `None` for Automatic — when unset or matching no listed adapter. Uses
/// the same WMI/LHM-tolerant matching as the poll loop's GPU selection
/// (exact normalized name first), so the tick agrees with what's displayed.
pub fn selected_gpu_index(names: &[String], preferred: Option<&str>) -> Option<usize> {
    let pref = preferred?;
    let pref_norm = lhm::normalize_gpu_name(pref);
    names
        .iter()
        .position(|n| lhm::normalize_gpu_name(n) == pref_norm)
        .or_else(|| names.iter().position(|n| lhm::gpu_names_match(n, pref)))
}

/// The tray "GPU" submenu: an "Automatic" row plus one check row per adapter.
/// Rows are check items so the current choice shows as a tick. Starts empty
/// (and disabled): adapter detection can take a while (WMI, or a PowerShell
/// fallback), so it runs on a background thread and [`GpuMenu::poll`] fills
/// the rows in once it reports back — startup never waits on it.
pub struct GpuMenu {
    submenu: Submenu,
    auto_item: CheckMenuItem,
    /// `(adapter name, row)` in menu order.
    items: Vec<(String, CheckMenuItem)>,
    names_rx: Option<mpsc::Receiver<Vec<String>>>,
}

impl GpuMenu {
    /// Adapter names listed in the submenu, in menu order (empty until
    /// detection has finished).
    pub fn names(&self) -> Vec<String> {
        self.items.iter().map(|(n, _)| n.clone()).collect()
    }

    /// Fills the rows in once background detection reports back. Returns the
    /// detected names on the call that populated the menu, `None` otherwise.
    pub fn poll(&mut self, preferred: Option<&str>) -> Option<Vec<String>> {
        let names = match self.names_rx.as_ref()?.try_recv() {
            Ok(names) => names,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Vec::new(),
        };
        self.names_rx = None;
        let _ = self.submenu.append(&PredefinedMenuItem::separator());
        for name in &names {
            let item = CheckMenuItem::with_id(gpu_menu_id(Some(name)), name, true, false, None);
            let _ = self.submenu.append(&item);
            self.items.push((name.clone(), item));
        }
        // A single adapter leaves nothing to choose between.
        self.submenu.set_enabled(names.len() > 1);
        self.set_selected(preferred);
        Some(names)
    }

    /// Ticks the row matching `preferred`, or "Automatic" (see
    /// [`selected_gpu_index`]).
    pub fn set_selected(&self, preferred: Option<&str>) {
        let selected = selected_gpu_index(&self.names(), preferred);
        self.auto_item.set_checked(selected.is_none());
        for (i, (_, item)) in self.items.iter().enumerate() {
            item.set_checked(Some(i) == selected);
        }
    }
}

/// Menu-id prefix of the Control Center profile submenu rows (#187) — same
/// choice-derived-id scheme as the GPU submenu above, for the same reason
/// (decode a click without sharing non-`Send` menu items across threads).
const PROFILE_ID_PREFIX: &str = "profile:id:";

fn profile_menu_id(id: &str) -> MenuId {
    MenuId::new(format!("{PROFILE_ID_PREFIX}{id}"))
}

/// Decodes a profile submenu click into the profile id, or `None` if `id`
/// isn't a profile submenu row.
pub fn profile_choice_from_menu_id(id: &MenuId) -> Option<String> {
    id.as_ref()
        .strip_prefix(PROFILE_ID_PREFIX)
        .map(str::to_string)
}

/// The tray "Profile" submenu (#187): a check row per Control Center
/// profile, ticked to match the active one. Unlike [`GpuMenu`] (filled once
/// by background hardware detection), profiles arrive over the control pipe
/// and already flow through the UI thread each frame via
/// `DashboardRuntime::drain_control` — so [`ProfileMenu::sync`] rebuilds the
/// rows whenever a profile was added, renamed or deleted (#257) and re-ticks
/// the active row on every call.
pub struct ProfileMenu {
    submenu: Submenu,
    /// `(profile id, profile name)` per row, in menu order — what the rows
    /// were built from, to tell when the list changed.
    rows: Vec<(String, String)>,
    items: Vec<CheckMenuItem>,
}

/// `(id, name)` per profile — the part of the list the submenu shows.
fn profile_rows(profiles: &[Profile]) -> Vec<(String, String)> {
    profiles
        .iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect()
}

impl ProfileMenu {
    pub fn sync(&mut self, profiles: &[Profile], active_id: Option<&str>) {
        let rows = profile_rows(profiles);
        if rows != self.rows {
            for item in self.items.drain(..) {
                let _ = self.submenu.remove(&item);
            }
            for (id, name) in &rows {
                let item = CheckMenuItem::with_id(profile_menu_id(id), name, true, false, None);
                let _ = self.submenu.append(&item);
                self.items.push(item);
            }
            self.submenu.set_enabled(!rows.is_empty());
            self.rows = rows;
        }
        for ((id, _), item) in self.rows.iter().zip(&self.items) {
            item.set_checked(Some(id.as_str()) == active_id);
        }
    }
}

pub struct Tray {
    icon: tray_icon::TrayIcon,
    pub settings_id: tray_icon::menu::MenuId,
    pub about_id: tray_icon::menu::MenuId,
    pub status_id: tray_icon::menu::MenuId,
    pub updater_id: tray_icon::menu::MenuId,
    pub docs_id: tray_icon::menu::MenuId,
    pub history_id: tray_icon::menu::MenuId,
    pub control_id: tray_icon::menu::MenuId,
    pub quit_id: tray_icon::menu::MenuId,
    pub floating_id: tray_icon::menu::MenuId,
    pub recording_id: tray_icon::menu::MenuId,
    pub overlay_id: tray_icon::menu::MenuId,
    pub overlay_lock_id: tray_icon::menu::MenuId,
    pub lamp_id: tray_icon::menu::MenuId,
    pub gpu_menu: GpuMenu,
    pub profile_menu: ProfileMenu,
    recording_item: IconMenuItem,
    overlay_lock_item: IconMenuItem,
    /// "Toggle Desk Lamp" — in the menu only while a lamp is connected
    /// ([`Tray::set_lamp_available`]).
    menu: Menu,
    lamp_item: IconMenuItem,
    lamp_shown: bool,
    // Tooltip inputs (recording state + wireless batteries, #290) and the
    // text last set, so the OS tooltip is only touched when it changes.
    recording: std::cell::Cell<bool>,
    peripherals: std::cell::RefCell<Vec<lhm::Peripheral>>,
    tooltip: std::cell::RefCell<String>,
}

/// What Windows shows of a tray tooltip: `szTip` holds 128 characters, but
/// an icon registered the legacy way (as `tray-icon` does) shows only the
/// first 63 — seen cutting "ROG Harpe Ace Aim Lab Edition" mid-line.
const TOOLTIP_MAX_CHARS: usize = 63;

/// Longest device name in the tooltip before it is cut with "…".
const TOOLTIP_NAME_CHARS: usize = 18;

/// A device name short enough for the tooltip: brand prefix dropped
/// ("ROG Azoth X" → "Azoth X"), then cut with "…".
fn tooltip_name(name: &str) -> String {
    let trimmed = ["ROG ", "ASUS ", "TUF Gaming "]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .unwrap_or(name);
    if trimmed.chars().count() <= TOOLTIP_NAME_CHARS {
        trimmed.to_string()
    } else {
        let cut: String = trimmed.chars().take(TOOLTIP_NAME_CHARS - 1).collect();
        format!("{}\u{2026}", cut.trim_end())
    }
}

/// The tray tooltip: "RIGStats" (+ " — Recording"), then one line per
/// wireless device with its battery, as many whole lines as Windows shows.
/// Plain text: the native tooltip has no colours or columns.
pub fn tray_tooltip(recording: bool, peripherals: &[lhm::Peripheral]) -> String {
    let mut text = String::from(if recording {
        "RIGStats \u{2014} Recording"
    } else {
        "RIGStats"
    });
    for device in peripherals {
        let line = format!(
            "\n{} {}%{}",
            tooltip_name(&device.name),
            device.battery,
            if device.charging { " CHG" } else { "" }
        );
        if text.chars().count() + line.chars().count() > TOOLTIP_MAX_CHARS {
            break;
        }
        text.push_str(&line);
    }
    text
}

/// Where "Toggle Desk Lamp" goes: right after "Control Center…".
const LAMP_ITEM_POSITION: usize = 8;

fn load_tray_icon() -> Icon {
    let bytes = include_bytes!("../../assets/tray.png");
    let img = image::load_from_memory(bytes).expect("tray.png").to_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).expect("tray icon rgba")
}

/// Load tray.png as egui IconData for dialog viewport windows.
pub fn load_app_icon() -> egui::IconData {
    let bytes = include_bytes!("../../assets/tray.png");
    let img = image::load_from_memory(bytes).expect("tray.png").to_rgba8();
    let (w, h) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    }
}

pub fn build_tray(
    logging_enabled: bool,
    overlay_locked: bool,
    gpu_names_rx: mpsc::Receiver<Vec<String>>,
) -> Tray {
    let floating_item = IconMenuItem::new(
        "Toggle Floating Mode",
        true,
        Some(menu_icons::floating()),
        None,
    );
    let recording_label = if logging_enabled {
        "Stop Recording"
    } else {
        "Start Recording"
    };
    let recording_icon = if logging_enabled {
        menu_icons::record_dot(255)
    } else {
        menu_icons::record_start()
    };
    let recording_item = IconMenuItem::new(recording_label, true, Some(recording_icon), None);
    let overlay_item = IconMenuItem::new(
        "Toggle Overlay Mode",
        true,
        Some(menu_icons::overlay()),
        None,
    );
    let overlay_lock_label = if overlay_locked {
        "Unlock Overlay"
    } else {
        "Lock Overlay"
    };
    let overlay_lock_item = IconMenuItem::new(
        overlay_lock_label,
        true,
        Some(menu_icons::lock(overlay_locked)),
        None,
    );
    let settings_item = IconMenuItem::new("Settings", true, Some(menu_icons::settings()), None);
    let about_item = IconMenuItem::new("About", true, Some(menu_icons::about()), None);
    let status_item = IconMenuItem::new("Status", true, Some(menu_icons::status()), None);
    let history_item =
        IconMenuItem::new("Session History", true, Some(menu_icons::history()), None);
    let control_item = IconMenuItem::new(
        "Control Center…",
        true,
        Some(menu_icons::control_center()),
        None,
    );
    let updater_item =
        IconMenuItem::new("Check for Updates", true, Some(menu_icons::updater()), None);
    let docs_item = IconMenuItem::new("Help / Docs", true, Some(menu_icons::docs()), None);
    let lamp_item = IconMenuItem::new("Toggle Desk Lamp", true, Some(menu_icons::lamp()), None);
    let quit_item = IconMenuItem::new("Quit", true, Some(menu_icons::quit()), None);

    // GPU submenu — the only way to pick the displayed GPU in modes where the
    // dashboard can't be clicked (Desktop Wallpaper). Adapter rows are added
    // later by `GpuMenu::poll`.
    let gpu_menu = GpuMenu {
        submenu: Submenu::new("GPU", false),
        auto_item: CheckMenuItem::with_id(
            gpu_menu_id(None),
            "Automatic (most VRAM)",
            true,
            true,
            None,
        ),
        items: Vec::new(),
        names_rx: Some(gpu_names_rx),
    };
    let _ = gpu_menu.submenu.append(&gpu_menu.auto_item);

    // Profile submenu (#187) — starts empty/disabled; `ProfileMenu::sync`
    // fills it in once the control pipe reports the profile list.
    let profile_menu = ProfileMenu {
        submenu: Submenu::new("Profile", false),
        rows: Vec::new(),
        items: Vec::new(),
    };

    let floating_id = floating_item.id().clone();
    let recording_id = recording_item.id().clone();
    let overlay_id = overlay_item.id().clone();
    let overlay_lock_id = overlay_lock_item.id().clone();
    let settings_id = settings_item.id().clone();
    let about_id = about_item.id().clone();
    let status_id = status_item.id().clone();
    let history_id = history_item.id().clone();
    let control_id = control_item.id().clone();
    let updater_id = updater_item.id().clone();
    let docs_id = docs_item.id().clone();
    let quit_id = quit_item.id().clone();
    let lamp_id = lamp_item.id().clone();

    let menu = Menu::new();
    let _ = menu.append(&floating_item);
    let _ = menu.append(&overlay_item);
    let _ = menu.append(&overlay_lock_item);
    let _ = menu.append(&recording_item);
    let _ = menu.append(&history_item);
    let _ = menu.append(&gpu_menu.submenu);
    let _ = menu.append(&profile_menu.submenu);
    let _ = menu.append(&control_item);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&settings_item);
    let _ = menu.append(&about_item);
    let _ = menu.append(&status_item);
    let _ = menu.append(&docs_item);
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
    let tooltip = tray_tooltip(logging_enabled, &[]);
    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu.clone()))
        .with_icon(icon)
        .with_tooltip(&tooltip)
        .build()
        .expect("tray icon");

    Tray {
        icon: tray_icon,
        settings_id,
        about_id,
        status_id,
        updater_id,
        docs_id,
        history_id,
        control_id,
        quit_id,
        floating_id,
        recording_id,
        overlay_id,
        overlay_lock_id,
        lamp_id,
        gpu_menu,
        profile_menu,
        recording_item,
        overlay_lock_item,
        menu,
        lamp_item,
        lamp_shown: false,
        recording: std::cell::Cell::new(logging_enabled),
        peripherals: std::cell::RefCell::new(Vec::new()),
        tooltip: std::cell::RefCell::new(tooltip),
    }
}

impl Tray {
    pub fn set_recording(&self, enabled: bool) {
        let label = if enabled {
            "Stop Recording"
        } else {
            "Start Recording"
        };
        self.recording_item.set_text(label);
        self.recording_item.set_icon(Some(if enabled {
            menu_icons::record_dot(255)
        } else {
            menu_icons::record_start()
        }));
        self.set_icon_variant(enabled);
        self.recording.set(enabled);
        self.refresh_tooltip();
    }

    /// The wireless devices' batteries for the tooltip; cheap to call every
    /// telemetry tick — the tooltip only changes when the list does.
    pub fn set_peripherals(&self, peripherals: &[lhm::Peripheral]) {
        if self.peripherals.borrow().as_slice() == peripherals {
            return;
        }
        *self.peripherals.borrow_mut() = peripherals.to_vec();
        self.refresh_tooltip();
    }

    fn refresh_tooltip(&self) {
        let text = tray_tooltip(self.recording.get(), &self.peripherals.borrow());
        if *self.tooltip.borrow() != text {
            let _ = self.icon.set_tooltip(Some(&text));
            *self.tooltip.borrow_mut() = text;
        }
    }

    /// Swaps both the tray icon glyph and the recording menu row's icon
    /// between bright/dim red — used to blink the recording indicator in
    /// sync while a session is active.
    pub fn set_recording_blink(&self, dot_visible: bool) {
        self.set_icon_variant(dot_visible);
        self.recording_item
            .set_icon(Some(menu_icons::record_dot(if dot_visible {
                255
            } else {
                90
            })));
    }

    /// Flips the "Lock/Unlock Overlay" row's label and padlock icon to match
    /// the current click-through state. Called from the tray toggle itself,
    /// the global hotkey, and the Settings dialog's Click-Through switch —
    /// whichever path changed `Settings.overlay_click_through`.
    pub fn set_overlay_lock(&self, locked: bool) {
        let label = if locked {
            "Unlock Overlay"
        } else {
            "Lock Overlay"
        };
        self.overlay_lock_item.set_text(label);
        self.overlay_lock_item
            .set_icon(Some(menu_icons::lock(locked)));
    }

    /// Shows "Toggle Desk Lamp" while a lighting device with a lamp is
    /// connected, and takes it out of the menu otherwise.
    pub fn set_lamp_available(&mut self, available: bool) {
        if available == self.lamp_shown {
            return;
        }
        let done = if available {
            self.menu.insert(&self.lamp_item, LAMP_ITEM_POSITION)
        } else {
            self.menu.remove(&self.lamp_item)
        };
        if done.is_ok() {
            self.lamp_shown = available;
        }
    }

    fn set_icon_variant(&self, dot: bool) {
        let bytes: &[u8] = if dot {
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
    }
}

// ── Panel helpers ─────────────────────────────────────────────────────────────

pub fn panel_label(key: &str) -> &'static str {
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
        "gpu_processes" => "GPU Apps",
        "power" => "System Power",
        "battery" => "Battery",
        "peripherals" => "Peripherals",
        _ => "Panel",
    }
}

/// Initial window height estimate for a panel (content + frame inner margin 16 px).
pub fn panel_initial_h(key: &str) -> f32 {
    match key {
        "header" | "clock" => theme::PANEL_HEADER_H + 16.0,
        _ => theme::PANEL_DATA_H + 16.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g14() -> Vec<String> {
        vec![
            "AMD Radeon(TM) 890M Graphics".to_string(),
            "NVIDIA GeForce RTX 5070 Ti Laptop GPU".to_string(),
        ]
    }

    fn peripheral(name: &str, battery: u8, charging: bool) -> lhm::Peripheral {
        lhm::Peripheral {
            id: name.to_lowercase(),
            name: name.into(),
            kind: "keyboard".into(),
            battery,
            charging,
        }
    }

    #[test]
    fn tooltip_lists_each_wireless_device_under_the_title() {
        let devices = [
            peripheral("ROG Azoth X", 82, false),
            peripheral("ROG Delta II", 23, true),
        ];
        assert_eq!(tray_tooltip(false, &[]), "RIGStats");
        assert_eq!(
            tray_tooltip(true, &devices),
            "RIGStats \u{2014} Recording\nAzoth X 82%\nDelta II 23% CHG"
        );
    }

    #[test]
    fn tooltip_names_drop_the_brand_and_cut_long_ones() {
        assert_eq!(tooltip_name("ROG Azoth X"), "Azoth X");
        assert_eq!(tooltip_name("ProArt KD300"), "ProArt KD300");
        assert_eq!(
            tooltip_name("ROG Harpe Ace Aim Lab Edition"),
            "Harpe Ace Aim Lab\u{2026}"
        );
    }

    #[test]
    fn tooltip_fits_the_owners_three_devices() {
        let devices = [
            peripheral("ROG Azoth X", 82, false),
            peripheral("ROG Delta II", 23, false),
            peripheral("ROG Harpe Ace Aim Lab Edition", 60, false),
        ];
        assert_eq!(
            tray_tooltip(false, &devices),
            "RIGStats\nAzoth X 82%\nDelta II 23%\nHarpe Ace Aim Lab\u{2026} 60%"
        );
    }

    #[test]
    fn tooltip_keeps_whole_lines_within_windows_limit() {
        let devices: Vec<_> = (0..10)
            .map(|i| peripheral(&format!("ROG Harpe Ace Aim Lab Edition {i}"), 60, false))
            .collect();
        let text = tray_tooltip(false, &devices);
        assert!(text.chars().count() <= TOOLTIP_MAX_CHARS);
        assert!(text.ends_with("60%"), "cut mid-line: {text:?}");
    }

    #[test]
    fn gpu_menu_ids_round_trip() {
        assert_eq!(gpu_choice_from_menu_id(&gpu_menu_id(None)), Some(None));
        let name = "NVIDIA GeForce RTX 5070 Ti Laptop GPU";
        assert_eq!(
            gpu_choice_from_menu_id(&gpu_menu_id(Some(name))),
            Some(Some(name.to_string()))
        );
    }

    #[test]
    fn gpu_menu_id_rejects_other_menu_rows() {
        assert_eq!(gpu_choice_from_menu_id(&MenuId::new("42")), None);
        assert_eq!(gpu_choice_from_menu_id(&MenuId::new("gpu:")), None);
    }

    #[test]
    fn profile_menu_ids_round_trip() {
        assert_eq!(
            profile_choice_from_menu_id(&profile_menu_id("gaming")),
            Some("gaming".to_string())
        );
        assert_eq!(
            profile_choice_from_menu_id(&profile_menu_id("silent")),
            Some("silent".to_string())
        );
    }

    #[test]
    fn profile_menu_id_rejects_other_menu_rows() {
        assert_eq!(profile_choice_from_menu_id(&MenuId::new("42")), None);
        assert_eq!(
            profile_choice_from_menu_id(&gpu_menu_id(Some("NVIDIA"))),
            None
        );
    }

    #[test]
    fn selected_gpu_index_none_preference_is_automatic() {
        assert_eq!(selected_gpu_index(&g14(), None), None);
    }

    #[test]
    fn selected_gpu_index_exact_name() {
        assert_eq!(
            selected_gpu_index(&g14(), Some("NVIDIA GeForce RTX 5070 Ti Laptop GPU")),
            Some(1)
        );
    }

    #[test]
    fn selected_gpu_index_tolerates_lhm_spelling() {
        // Saved from the GPU panel's click dots (LHM name, no "(TM)").
        assert_eq!(
            selected_gpu_index(&g14(), Some("AMD Radeon 890M Graphics")),
            Some(0)
        );
    }

    #[test]
    fn selected_gpu_index_unknown_gpu_is_automatic() {
        assert_eq!(selected_gpu_index(&g14(), Some("GTX 1080")), None);
        assert_eq!(selected_gpu_index(&[], Some("GTX 1080")), None);
    }
}
