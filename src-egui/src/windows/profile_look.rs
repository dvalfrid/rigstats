//! The Control Center's Dashboard, Overlay and Alerts pages (#305): the part
//! of a profile that is the app's own look — accent colour, opacity, panels
//! and their order, the overlay's content and the alert thresholds
//! (`settings::ProfileLook`). Everything app-wide (window layer, display
//! profile, overlay placement, notifications, …) lives in Settings.
//!
//! The current settings are the active profile's look, so these pages edit
//! them directly: a change shows at once (and is written to the settings
//! file, so the wallpaper host follows), Save keeps it for the profile,
//! Revert or Close puts back what was there before.

use crate::lock_ext::LockSafe;
use crate::overlay::ALL_OVERLAY_METRICS;
use crate::theme::{self, AppTheme, DialogColors};
use crate::windows::settings::{self as settings_win, ALL_PANELS};
use crate::windows::ui_kit::{self, Group, C_CRIT, C_WARN};
use crate::windows::{OpenRequest, OpenRequests};
use rigstats_backend::settings::{self, ProfileLook, Settings};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// What the pages need to read and save the settings.
pub struct LookLink<'a> {
    pub settings: &'a Arc<Mutex<Settings>>,
    pub dir: &'a Path,
    pub reload: &'a Arc<AtomicBool>,
    pub open: &'a OpenRequests,
    pub battery_present: &'a Arc<AtomicBool>,
}

/// Theme keys with their colour and a readable name.
pub fn theme_options() -> Vec<(&'static str, egui::Color32, &'static str)> {
    theme::THEME_KEYS
        .iter()
        .map(|k| (*k, AppTheme::from_key(k).accent, theme_name(k)))
        .collect()
}

pub fn theme_name(key: &str) -> &'static str {
    match key {
        "amber" => "Amber",
        "green" => "Green",
        "purple" => "Purple",
        "slate" => "Slate",
        "red" => "Red",
        "blue" => "Blue",
        _ => "Cyan",
    }
}

/// Threshold rows: key, label, unit, what the value means.
const THRESHOLDS: &[(&str, &str, &str, Option<&str>)] = &[
    ("cpu", "CPU temperature", "°C", None),
    ("gpu", "GPU temperature", "°C", None),
    ("cpu_load", "CPU load", "%", None),
    ("gpu_load", "GPU load", "%", None),
    ("ram", "RAM temperature", "°C", None),
    ("ram_load", "RAM usage", "%", None),
    ("disk", "Disk temperature", "°C", None),
    ("disk_usage", "Disk usage", "%", None),
];
const BATTERY_THRESHOLDS: &[(&str, &str, &str, Option<&str>)] = &[
    (
        "battery",
        "Battery charge",
        "%",
        Some("Alerts when it drops below"),
    ),
    (
        "battery_power",
        "Battery power draw",
        "W",
        Some("While running on battery"),
    ),
];
/// Width of the unit after each row's fields.
const UNIT_W: f32 = 26.0;

/// The look being edited. Outside an edit it follows the settings (so a
/// change from the tray or the hotkey shows); during one it holds the
/// edit, and `before` what to put back.
#[derive(Debug, Default)]
pub struct LookEditor {
    profile_id: Option<String>,
    look: Option<ProfileLook>,
    before: Option<ProfileLook>,
}

fn persist(link: &LookLink, s: &Settings) {
    if let Err(e) = settings::persist_settings(link.dir, s) {
        rigstats_backend::debug::log_error(link.dir, &format!("settings: persist failed — {e}"));
    }
    link.reload.store(true, Ordering::Relaxed);
}

impl LookEditor {
    /// Unsaved changes to the look.
    pub fn dirty(&self) -> bool {
        self.before.is_some() && self.before != self.look
    }

    /// Runs every frame, whichever page is shown. Outside an edit the look
    /// follows the settings. A profile switch ends an edit: the app has
    /// already stored the edited look in the outgoing profile and loaded
    /// the new one's, so the outgoing profile gets its look from before the
    /// edit back.
    pub fn sync(&mut self, active_id: Option<&str>, link: &LookLink) {
        if self.profile_id.as_deref() != active_id {
            if let (Some(before), Some(old)) = (self.before.take(), self.profile_id.take()) {
                let mut s = link.settings.lock_safe();
                s.profile_looks.insert(old, before);
                persist(link, &s);
            }
            self.profile_id = active_id.map(str::to_owned);
            self.look = None;
        }
        if self.before.is_none() {
            self.look = Some(ProfileLook::capture(&link.settings.lock_safe()));
        }
    }

    /// Shows the edit: writes it to the app when it differs from the app.
    fn preview(&mut self, link: &LookLink) {
        let Some(look) = &self.look else {
            return;
        };
        let mut s = link.settings.lock_safe();
        let current = ProfileLook::capture(&s);
        if current == *look {
            return;
        }
        self.before.get_or_insert(current);
        look.apply_to(&mut s);
        persist(link, &s);
    }

    /// Keeps the edit for the active profile.
    pub fn save(&mut self, link: &LookLink) {
        if self.before.take().is_some() {
            let mut s = link.settings.lock_safe();
            settings::store_active_look(&mut s);
            persist(link, &s);
        }
    }

    /// Ends the edit without keeping it (Revert, Close).
    pub fn discard(&mut self, link: &LookLink) {
        if let Some(before) = self.before.take() {
            let mut s = link.settings.lock_safe();
            before.apply_to(&mut s);
            persist(link, &s);
        }
        self.look = None;
    }

    /// The look as the overview shows it.
    pub fn current(&self) -> Option<&ProfileLook> {
        self.look.as_ref()
    }
}

fn open_settings_row(
    g: &mut Group<'_>,
    dc: &DialogColors,
    link: &LookLink,
    title: &str,
    subtitle: &str,
    page: settings_win::Page,
) {
    g.row(title, Some(subtitle), |ui| {
        if theme::dialog_btn_secondary(ui, "Open Settings", dc).clicked() {
            *link.open.lock_safe() = Some(OpenRequest::Settings(page));
        }
    });
}

pub fn dashboard_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    profile: &str,
    editor: &mut LookEditor,
    link: &LookLink,
) {
    ui_kit::page_header(
        ui,
        dc,
        "Dashboard",
        &format!("How the dashboard looks in {profile}."),
    );
    let Some(look) = editor.look.as_mut() else {
        return;
    };
    ui_kit::group(ui, dc, Some("Appearance"), None, |g| {
        g.row("Accent colour", Some(theme_name(&look.theme)), |ui| {
            ui_kit::swatches(ui, dc, &mut look.theme, &theme_options());
        });
        g.row("Opacity", Some("How see-through the panels are"), |ui| {
            ui_kit::slider_pct(ui, dc, &mut look.opacity, 0.1..=1.0);
        });
    });
    let battery = link.battery_present.load(Ordering::Relaxed);
    let items: Vec<(&str, String)> = ALL_PANELS
        .iter()
        .map(|(k, l)| (*k, l.to_string()))
        .collect();
    ui_kit::group(
        ui,
        dc,
        Some("Panels"),
        Some("Shown panels appear top to bottom in this order. Use the arrows to move them."),
        |g| {
            ui_kit::ordered_rows(g, &mut look.visible_panels, &items, |k| {
                k == "battery" && !battery && cfg!(not(debug_assertions))
            });
        },
    );
    ui_kit::group(ui, dc, None, None, |g| {
        open_settings_row(
            g,
            dc,
            link,
            "Window and screen",
            "Display profile, window layer and full screen apply to every profile",
            settings_win::Page::Display,
        );
    });
    editor.preview(link);
}

/// What an overlay metric is, in words — the overlay itself shows short
/// labels ("CPU", "HOTSPOT") with a unit.
fn metric_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "cpu_load" => "CPU load",
        "cpu_temp" => "CPU temperature",
        "cpu_freq" => "CPU clock speed",
        "cpu_power" => "CPU power draw",
        "gpu_load" => "GPU load",
        "gpu_temp" => "GPU temperature",
        "gpu_hotspot" => "GPU hotspot temperature",
        "gpu_mem_temp" => "Video memory temperature",
        "gpu_core_clock" => "GPU clock speed",
        "gpu_mem_clock" => "Video memory clock speed",
        "gpu_vram_used" => "Video memory used",
        "gpu_vram_pct" => "Video memory used (%)",
        "gpu_power" => "GPU power draw",
        "gpu_fan" => "GPU fan speed",
        "ram_used" => "RAM used",
        "ram_pct" => "RAM used (%)",
        "net_up" => "Upload speed",
        "net_down" => "Download speed",
        "net_ping" => "Ping",
        "disk_read" => "Disk read speed",
        "disk_write" => "Disk write speed",
        "battery_pct" => "Battery charge",
        _ => return None,
    })
}

fn columns_label(n: u8) -> String {
    match n {
        0 => "All on one row".to_owned(),
        1 => "One column".to_owned(),
        n => format!("{n} columns"),
    }
}

pub fn overlay_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    profile: &str,
    editor: &mut LookEditor,
    link: &LookLink,
) {
    ui_kit::page_header(
        ui,
        dc,
        "Overlay",
        &format!("The metric strip shown over games in {profile}."),
    );
    let Some(look) = editor.look.as_mut() else {
        return;
    };
    ui_kit::group(ui, dc, None, None, |g| {
        g.row(
            "Show overlay",
            Some("Ctrl+Alt+O shows or hides it at any time"),
            |ui| {
                ui_kit::toggle(ui, dc, &mut look.overlay_enabled);
            },
        );
        g.row("Layout", None, |ui| {
            ui_kit::dropdown(
                ui,
                "look_overlay_columns",
                columns_label(look.overlay_columns),
                |ui| {
                    for n in 0..=6u8 {
                        ui.selectable_value(&mut look.overlay_columns, n, columns_label(n));
                    }
                },
            );
        });
    });
    let battery = link.battery_present.load(Ordering::Relaxed);
    let items: Vec<(&str, String)> = ALL_OVERLAY_METRICS
        .iter()
        .map(|m| {
            let name = metric_name(m.key)
                .map_or_else(|| format!("{} ({})", m.label, m.unit.trim()), str::to_owned);
            (m.key, name)
        })
        .collect();
    ui_kit::group(
        ui,
        dc,
        Some("Metrics"),
        Some("Shown metrics appear in this order."),
        |g| {
            ui_kit::ordered_rows(g, &mut look.overlay_metrics, &items, |k| {
                k == "battery_pct" && !battery && cfg!(not(debug_assertions))
            });
        },
    );
    ui_kit::group(ui, dc, None, None, |g| {
        open_settings_row(
            g,
            dc,
            link,
            "Position and size",
            "Where the overlay sits, its size and click-through apply to every profile",
            settings_win::Page::Overlay,
        );
    });
    editor.preview(link);
}

pub fn alerts_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    profile: &str,
    editor: &mut LookEditor,
    link: &LookLink,
) {
    ui_kit::page_header(
        ui,
        dc,
        "Alerts",
        &format!("When RIGStats notifies you in {profile}."),
    );
    let Some(look) = editor.look.as_mut() else {
        return;
    };
    let defaults = settings::default_thresholds();
    let header = |g: &mut Group<'_>, title: &str| {
        g.row(title, None, |ui| {
            ui.add_space(UNIT_W);
            ui.add_sized(
                [48.0, 20.0],
                egui::Label::new(
                    egui::RichText::new("Critical")
                        .size(11.0)
                        .strong()
                        .color(C_CRIT),
                ),
            );
            ui.add_sized(
                [48.0, 20.0],
                egui::Label::new(
                    egui::RichText::new("Warning")
                        .size(11.0)
                        .strong()
                        .color(C_WARN),
                ),
            );
        });
    };
    let mut rows = |g: &mut Group<'_>, list: &[(&str, &str, &str, Option<&str>)]| {
        for &(key, label, unit, note) in list {
            let e = look
                .thresholds
                .entry(key.to_string())
                .or_insert_with(|| defaults.get(key).cloned().unwrap_or_default());
            g.row(label, note, |ui| {
                ui.add_sized(
                    [UNIT_W, 20.0],
                    egui::Label::new(egui::RichText::new(unit).size(12.0).color(dc.muted)),
                );
                ui_kit::threshold_field(ui, &mut e.crit);
                ui_kit::threshold_field(ui, &mut e.warn);
            });
        }
    };
    ui_kit::group(
        ui,
        dc,
        Some("Thresholds"),
        Some("Leave a field empty for no alert at that level."),
        |g| {
            header(g, "Alert when above");
            rows(g, THRESHOLDS);
        },
    );
    ui_kit::group(
        ui,
        dc,
        Some("Battery"),
        Some("Laptop battery and wireless devices."),
        |g| {
            header(g, "Alert at");
            rows(g, BATTERY_THRESHOLDS);
        },
    );
    ui_kit::group(ui, dc, None, None, |g| {
        open_settings_row(
            g,
            dc,
            link,
            "Notifications",
            "Turning alerts on or off and how often they repeat apply to every profile",
            settings_win::Page::Notifications,
        );
    });
    editor.preview(link);
}
