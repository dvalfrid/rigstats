//! System tray icon, menu, and tray-command channel, plus small panel-label
//! helpers. Extracted from `main.rs`.

use crate::menu_icons;
use crate::theme;
use eframe::egui;
use tray_icon::{
    menu::{IconMenuItem, Menu, PredefinedMenuItem},
    Icon, TrayIconBuilder,
};

/// Commands sent from the tray-polling thread to the UI thread.
pub enum TrayCmd {
    OpenSettings,
    OpenAbout,
    OpenStatus,
    OpenUpdater,
    OpenDocs,
    OpenHistory,
    ToggleFloating,
    ToggleRecording,
}

pub struct Tray {
    icon: tray_icon::TrayIcon,
    pub settings_id: tray_icon::menu::MenuId,
    pub about_id: tray_icon::menu::MenuId,
    pub status_id: tray_icon::menu::MenuId,
    pub updater_id: tray_icon::menu::MenuId,
    pub docs_id: tray_icon::menu::MenuId,
    pub history_id: tray_icon::menu::MenuId,
    pub quit_id: tray_icon::menu::MenuId,
    pub floating_id: tray_icon::menu::MenuId,
    pub recording_id: tray_icon::menu::MenuId,
    recording_item: IconMenuItem,
}

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

pub fn build_tray(logging_enabled: bool) -> Tray {
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
    let settings_item = IconMenuItem::new("Settings", true, Some(menu_icons::settings()), None);
    let about_item = IconMenuItem::new("About", true, Some(menu_icons::about()), None);
    let status_item = IconMenuItem::new("Status", true, Some(menu_icons::status()), None);
    let history_item =
        IconMenuItem::new("Session History", true, Some(menu_icons::history()), None);
    let updater_item =
        IconMenuItem::new("Check for Updates", true, Some(menu_icons::updater()), None);
    let docs_item = IconMenuItem::new("Help / Docs", true, Some(menu_icons::docs()), None);
    let quit_item = IconMenuItem::new("Quit", true, Some(menu_icons::quit()), None);

    let floating_id = floating_item.id().clone();
    let recording_id = recording_item.id().clone();
    let settings_id = settings_item.id().clone();
    let about_id = about_item.id().clone();
    let status_id = status_item.id().clone();
    let history_id = history_item.id().clone();
    let updater_id = updater_item.id().clone();
    let docs_id = docs_item.id().clone();
    let quit_id = quit_item.id().clone();

    let menu = Menu::new();
    let _ = menu.append(&floating_item);
    let _ = menu.append(&recording_item);
    let _ = menu.append(&history_item);
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
        docs_id,
        history_id,
        quit_id,
        floating_id,
        recording_id,
        recording_item,
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
        let tooltip = if enabled {
            "RIGStats \u{2014} Recording"
        } else {
            "RIGStats"
        };
        let _ = self.icon.set_tooltip(Some(tooltip));
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
        "power" => "System Power",
        "battery" => "Battery",
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
