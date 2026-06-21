//! System tray icon, menu, and tray-command channel, plus small panel-label
//! helpers. Extracted from `main.rs`.

use crate::theme;
use eframe::egui;
use tray_icon::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    Icon, TrayIconBuilder,
};

/// Commands sent from the tray-polling thread to the UI thread.
pub(crate) enum TrayCmd {
    OpenSettings,
    OpenAbout,
    OpenStatus,
    OpenUpdater,
    ToggleFloating,
    ToggleRecording,
}

pub(crate) struct Tray {
    icon: tray_icon::TrayIcon,
    pub(crate) settings_id: tray_icon::menu::MenuId,
    pub(crate) about_id: tray_icon::menu::MenuId,
    pub(crate) status_id: tray_icon::menu::MenuId,
    pub(crate) updater_id: tray_icon::menu::MenuId,
    pub(crate) quit_id: tray_icon::menu::MenuId,
    pub(crate) floating_id: tray_icon::menu::MenuId,
    pub(crate) recording_id: tray_icon::menu::MenuId,
    recording_item: tray_icon::menu::MenuItem,
}

fn load_tray_icon() -> Icon {
    let bytes = include_bytes!("../../assets/tray.png");
    let img = image::load_from_memory(bytes).expect("tray.png").to_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).expect("tray icon rgba")
}

/// Load tray.png as egui IconData for dialog viewport windows.
pub(crate) fn load_app_icon() -> egui::IconData {
    let bytes = include_bytes!("../../assets/tray.png");
    let img = image::load_from_memory(bytes).expect("tray.png").to_rgba8();
    let (w, h) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    }
}

pub(crate) fn build_tray(logging_enabled: bool) -> Tray {
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
    pub(crate) fn set_recording(&self, enabled: bool) {
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

pub(crate) fn panel_label(key: &str) -> &'static str {
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
pub(crate) fn panel_initial_h(key: &str) -> f32 {
    match key {
        "header" | "clock" => theme::PANEL_HEADER_H + 16.0,
        _ => theme::PANEL_DATA_H + 16.0,
    }
}
