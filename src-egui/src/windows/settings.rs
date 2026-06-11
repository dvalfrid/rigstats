use crate::theme;
use rigstats_backend::{autostart, debug, settings};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// ── Panel / profile data ───────────────────────────────────────────────────────

const ALL_PANELS: &[(&str, &str)] = &[
    ("header", "Header"),
    ("clock", "Clock"),
    ("cpu", "CPU"),
    ("gpu", "GPU"),
    ("ram", "RAM"),
    ("net", "Network"),
    ("disk", "Storage"),
    ("motherboard", "Motherboard"),
    ("process", "Processes"),
    ("battery", "Battery"),
];

const ALL_PROFILES: &[(&str, &str)] = &[
    ("portrait-xl", "Portrait XL (450×1920)"),
    ("portrait-slim", "Portrait Slim (480×1920)"),
    ("portrait-hd", "Portrait HD (720×1280)"),
    ("portrait-wxga", "Portrait WXGA (800×1280)"),
    ("portrait-fhd", "Portrait FHD (1080×1920)"),
    ("portrait-wuxga", "Portrait WUXGA (1200×1920)"),
    ("portrait-qhd", "Portrait QHD (1440×2560)"),
    ("portrait-hdplus", "Portrait HD+ (768×1366)"),
    ("portrait-900x1600", "Portrait 900×1600"),
    ("portrait-1050x1680", "Portrait 1050×1680"),
    ("portrait-1600x2560", "Portrait 1600×2560"),
    ("portrait-4k", "Portrait 4K (2160×3840)"),
    ("portrait-fhd-side", "FHD Side (253×1080)"),
    ("portrait-qhd-side", "QHD Side (338×1440)"),
    ("portrait-4k-side", "4K Side (506×2160)"),
];

const LOG_RETENTION_OPTIONS: &[(u32, &str)] = &[
    (1, "1 day"),
    (3, "3 days"),
    (7, "7 days"),
    (14, "14 days"),
    (30, "30 days"),
    (60, "60 days"),
    (90, "90 days"),
];

// ── Colour tokens ─────────────────────────────────────────────────────────────

const C_BG: egui::Color32 = egui::Color32::from_gray(38);
const C_CARD: egui::Color32 = egui::Color32::from_gray(27);
const C_CARD_BORDER: egui::Color32 = egui::Color32::from_gray(55);
const C_TEXT: egui::Color32 = egui::Color32::from_rgb(155, 180, 210);
const C_MUTED: egui::Color32 = egui::Color32::from_gray(115);
const C_LABEL: egui::Color32 = egui::Color32::from_gray(140);
const C_TITLE: egui::Color32 = egui::Color32::from_rgb(210, 220, 235);
const C_INNER: egui::Color32 = egui::Color32::from_gray(33);
const C_TOGGLE_ON: egui::Color32 = egui::Color32::from_rgb(0, 188, 212);
const C_TOGGLE_OFF: egui::Color32 = egui::Color32::from_gray(55);
const C_TAB_ACTIVE: egui::Color32 = egui::Color32::from_rgb(0, 188, 212);
const C_TAB_ACTIVE_TEXT: egui::Color32 = egui::Color32::from_gray(15);
const C_WARN: egui::Color32 = egui::Color32::from_rgb(255, 180, 30);
const C_CRIT: egui::Color32 = egui::Color32::from_rgb(220, 60, 60);

// ── State ─────────────────────────────────────────────────────────────────────

pub struct SettingsWindow {
    pub draft: settings::Settings,
    pub original: settings::Settings,
    last_preview: settings::Settings,
    pub tab: usize,
    pub error: Option<String>,
    pub battery_present: bool,
}

impl SettingsWindow {
    pub fn from_settings(s: &settings::Settings) -> Self {
        let mut draft = s.clone();
        draft.autostart_enabled = autostart::is_run_key_present();
        Self {
            original: draft.clone(),
            last_preview: draft.clone(),
            draft,
            tab: 0,
            error: None,
            battery_present: detect_battery_present(),
        }
    }
}

fn detect_battery_present() -> bool {
    debug::run_hidden_command(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "if (Get-CimInstance Win32_Battery -EA SilentlyContinue) { 'yes' } else { 'no' }",
        ],
    )
    .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "yes")
    .unwrap_or(true)
}

// ── Frames ────────────────────────────────────────────────────────────────────

fn dialog_frame() -> egui::Frame {
    egui::Frame::new().fill(C_BG).inner_margin(egui::Margin::same(0))
}

fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(C_CARD)
        .stroke(egui::Stroke::new(1.0, C_CARD_BORDER))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(12, 8))
}

fn inner_row() -> egui::Frame {
    egui::Frame::new()
        .fill(C_INNER)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(10, 6))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).strong().color(C_LABEL));
}

/// Cyan toggle switch.
fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let desired = egui::vec2(44.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(desired, egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let bg = if *on { C_TOGGLE_ON } else { C_TOGGLE_OFF };
        let r = (rect.height() / 2.0) as u8;
        ui.painter().rect_filled(rect, egui::CornerRadius::same(r), bg);
        let cx = if *on {
            rect.right() - rect.height() / 2.0
        } else {
            rect.left() + rect.height() / 2.0
        };
        ui.painter().circle_filled(
            egui::pos2(cx, rect.center().y),
            rect.height() / 2.0 - 3.0,
            egui::Color32::WHITE,
        );
    }
    resp
}

/// Draw three horizontal lines as a drag-handle icon (font-independent).
fn drag_handle(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 20.0), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let cx = rect.center().x;
        let stroke = egui::Stroke::new(1.5, C_MUTED);
        for dy in [-4.0_f32, 0.0, 4.0] {
            let y = rect.center().y + dy;
            ui.painter().line_segment(
                [egui::pos2(cx - 5.0, y), egui::pos2(cx + 5.0, y)],
                stroke,
            );
        }
    }
}

/// Styled tab button. Returns true when clicked.
fn tab_btn(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    let mut clicked = false;
    ui.scope(|ui| {
        let cr = egui::CornerRadius::same(5);
        if active {
            ui.visuals_mut().widgets.inactive.bg_fill = C_TAB_ACTIVE;
            ui.visuals_mut().widgets.inactive.fg_stroke =
                egui::Stroke::new(1.0, C_TAB_ACTIVE_TEXT);
            ui.visuals_mut().widgets.inactive.corner_radius = cr;
            ui.visuals_mut().widgets.hovered.bg_fill = C_TAB_ACTIVE;
            ui.visuals_mut().widgets.hovered.fg_stroke =
                egui::Stroke::new(1.0, C_TAB_ACTIVE_TEXT);
            ui.visuals_mut().widgets.hovered.corner_radius = cr;
            ui.visuals_mut().widgets.active.corner_radius = cr;
        } else {
            ui.visuals_mut().widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
            ui.visuals_mut().widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0, egui::Color32::from_gray(60));
            ui.visuals_mut().widgets.inactive.corner_radius = cr;
            ui.visuals_mut().widgets.hovered.bg_fill =
                egui::Color32::from_gray(48);
            ui.visuals_mut().widgets.hovered.corner_radius = cr;
            ui.visuals_mut().widgets.active.corner_radius = cr;
        }
        let text_col = if active { C_TAB_ACTIVE_TEXT } else { C_TEXT };
        if ui
            .add_sized(
                [128.0, 28.0],
                egui::Button::new(egui::RichText::new(label).size(12.0).color(text_col)),
            )
            .clicked()
        {
            clicked = true;
        }
    });
    clicked
}

/// Text field for an optional threshold value; hint "—" when None.
fn threshold_field(ui: &mut egui::Ui, value: &mut Option<u8>) {
    let mut text = value.map(|v| v.to_string()).unwrap_or_default();
    let resp = ui.add_sized(
        [44.0, 22.0],
        egui::TextEdit::singleline(&mut text)
            .hint_text("—")
            .font(egui::TextStyle::Monospace),
    );
    if resp.changed() {
        *value = text.trim().parse::<u8>().ok();
    }
}

// ── show() ────────────────────────────────────────────────────────────────────

#[allow(deprecated)]
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    main_ctx: &egui::Context,
    open: &Arc<AtomicBool>,
    needs_focus: &Arc<AtomicBool>,
    state: &Arc<Mutex<SettingsWindow>>,
    dir: &Arc<PathBuf>,
    saved: &Arc<Mutex<settings::Settings>>,
    reload: &Arc<AtomicBool>,
) {
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    let mut action_save = false;
    let mut action_cancel = false;

    // ── Hero ──────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::top("settings_hero")
        .frame(dialog_frame().inner_margin(egui::Margin { left: 16, right: 16, top: 14, bottom: 12 }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("Settings")
                    .size(22.0)
                    .strong()
                    .color(C_TITLE),
            );
        });

    // ── Footer ────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("settings_footer")
        .frame(dialog_frame().inner_margin(egui::Margin { left: 14, right: 14, top: 8, bottom: 12 }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            let err = state.lock().unwrap().error.clone();
            if let Some(ref e) = err {
                ui.label(
                    egui::RichText::new(e.as_str())
                        .size(11.0)
                        .color(egui::Color32::from_rgb(220, 80, 80)),
                );
                ui.add_space(4.0);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::dialog_btn_secondary(ui, "Cancel").clicked() {
                    action_cancel = true;
                }
                ui.add_space(6.0);
                if theme::dialog_btn_primary(ui, "Save").clicked() {
                    action_save = true;
                }
            });
        });

    // ── Central: tab bar + scrollable content ─────────────────────────────────
    egui::CentralPanel::default()
        .frame(dialog_frame())
        .show(ctx, |ui| {
            let mut st = state.lock().unwrap();

            // Tab bar
            egui::Frame::new()
                .fill(C_BG)
                .inner_margin(egui::Margin { left: 14, right: 14, top: 8, bottom: 8 })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (i, lbl) in ["Dashboard", "Panels", "Alerts", "Appearance"].iter().enumerate() {
                            if tab_btn(ui, lbl, st.tab == i) {
                                st.tab = i;
                            }
                        }
                    });
                });

            ui.add(egui::Separator::default().spacing(0.0));

            egui::ScrollArea::vertical()
                .id_salt("settings_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::new()
                        .fill(C_BG)
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 8.0;
                            let tab = st.tab;
                            let battery_present = st.battery_present;
                            let draft = &mut st.draft;
                            match tab {
                                0 => draw_dashboard(ui, draft, dir.as_ref()),
                                1 => draw_panels(ui, draft, battery_present),
                                2 => draw_alerts(ui, draft),
                                3 => draw_appearance(ui, draft),
                                _ => {}
                            }
                        });
                });
        });

    // ── Apply deferred actions ────────────────────────────────────────────────

    // Live preview: push draft to main app on every change (before save/cancel).
    // window_layer is the only excluded field — it's applied on Save only since
    // it's a window-level hint that doesn't need instant feedback.
    // Everything else (opacity, panels, profile, floating_mode, etc.) previews live.
    {
        let mut st = state.lock().unwrap();
        if st.draft != st.last_preview {
            st.last_preview = st.draft.clone();
            let mut preview = st.draft.clone();
            preview.window_layer = st.original.window_layer.clone();
            *saved.lock().unwrap() = preview;
            reload.store(true, Ordering::Relaxed);
            main_ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }

    if action_save {
        let mut st = state.lock().unwrap();
        let autostart_result = if st.draft.autostart_enabled {
            autostart::register_autostart()
        } else {
            autostart::unregister_autostart()
        };
        let save_result = settings::persist_settings(dir.as_ref(), &st.draft);
        match (save_result, autostart_result) {
            (Ok(()), Ok(())) => {
                // Push the full draft (including window_layer, profile, etc.) to main app.
                *saved.lock().unwrap() = st.draft.clone();
                reload.store(true, Ordering::Relaxed);
                st.error = None;
                open.store(false, Ordering::Relaxed);
                main_ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
            (Err(e), _) | (Ok(()), Err(e)) => {
                st.error = Some(format!("Save failed: {e}"));
            }
        }
    }
    if action_cancel {
        // Revert live preview back to original.
        let st = state.lock().unwrap();
        *saved.lock().unwrap() = st.original.clone();
        reload.store(true, Ordering::Relaxed);
        drop(st);
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
    if ctx.input(|i| i.viewport().close_requested()) {
        // Treat window-close as cancel (revert preview).
        let st = state.lock().unwrap();
        *saved.lock().unwrap() = st.original.clone();
        reload.store(true, Ordering::Relaxed);
        drop(st);
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

// ── Dashboard tab ─────────────────────────────────────────────────────────────

fn draw_dashboard(ui: &mut egui::Ui, draft: &mut settings::Settings, dir: &Path) {
    // Display Profile
    card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, "Display Profile");
        ui.add_space(8.0);
        let w = ui.available_width();
        let current = ALL_PROFILES
            .iter()
            .find(|(k, _)| *k == draft.dashboard_profile.as_str())
            .map(|(_, l)| *l)
            .unwrap_or(draft.dashboard_profile.as_str());
        egui::ComboBox::from_id_salt("profile_combo")
            .selected_text(current)
            .width(w)
            .show_ui(ui, |ui| {
                for &(key, label) in ALL_PROFILES {
                    ui.selectable_value(&mut draft.dashboard_profile, key.to_string(), label);
                }
            });
    });

    // Layout
    card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, "Layout");
        ui.add_space(8.0);
        inner_row().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Floating Mode").size(13.0).color(C_TEXT));
                    ui.label(
                        egui::RichText::new("Each panel opens as a separate frameless window")
                            .size(11.0)
                            .color(C_MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    toggle_switch(ui, &mut draft.floating_mode);
                });
            });
        });
    });

    // Stats Logging
    card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, "Stats Logging");
        ui.add_space(8.0);

        inner_row().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Enable Logging").size(13.0).color(C_TEXT));
                    ui.label(
                        egui::RichText::new("Saves one CSV row per second to the app data folder")
                            .size(11.0)
                            .color(C_MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    toggle_switch(ui, &mut draft.logging_enabled);
                });
            });
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Retain for").size(12.0).color(C_MUTED));
            ui.add_space(8.0);
            let current = LOG_RETENTION_OPTIONS
                .iter()
                .find(|(d, _)| *d == draft.log_retention_days)
                .map(|(_, l)| *l)
                .unwrap_or("7 days");
            egui::ComboBox::from_id_salt("log_retention")
                .selected_text(current)
                .width(140.0)
                .show_ui(ui, |ui| {
                    for &(days, label) in LOG_RETENTION_OPTIONS {
                        ui.selectable_value(&mut draft.log_retention_days, days, label);
                    }
                });
        });

        ui.add_space(8.0);
        if theme::dialog_btn_secondary(ui, "Open Log Folder").clicked() {
            let _ = std::process::Command::new("explorer").arg(dir).spawn();
        }
    });
}

/// Small clickable reorder button with a painted triangle arrow.
fn arrow_btn(ui: &mut egui::Ui, down: bool) -> egui::Response {
    let size = egui::vec2(20.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let color = if resp.hovered() {
            C_TEXT
        } else {
            C_MUTED
        };
        let cx = rect.center().x;
        let tri = if down {
            vec![
                egui::pos2(rect.left() + 3.0, rect.center().y - 3.0),
                egui::pos2(rect.right() - 3.0, rect.center().y - 3.0),
                egui::pos2(cx, rect.center().y + 4.0),
            ]
        } else {
            vec![
                egui::pos2(cx, rect.center().y - 4.0),
                egui::pos2(rect.right() - 3.0, rect.center().y + 3.0),
                egui::pos2(rect.left() + 3.0, rect.center().y + 3.0),
            ]
        };
        ui.painter()
            .add(egui::Shape::convex_polygon(tri, color, egui::Stroke::NONE));
    }
    resp
}

// ── Panels tab ────────────────────────────────────────────────────────────────

fn draw_panels(ui: &mut egui::Ui, draft: &mut settings::Settings, battery_present: bool) {
    ui.label(
        egui::RichText::new("Use arrows to reorder · toggle to show/hide")
            .size(11.0)
            .color(C_MUTED),
    );
    ui.add_space(4.0);

    // Build ordered list: visible first (in order), hidden at bottom.
    let mut ordered: Vec<(&str, &str, bool)> = Vec::new();
    for key in &draft.visible_panels {
        if let Some(&(_, label)) = ALL_PANELS.iter().find(|(k, _)| k == key) {
            ordered.push((key, label, true));
        }
    }
    for &(key, label) in ALL_PANELS {
        if !draft.visible_panels.iter().any(|p| p == key) {
            ordered.push((key, label, false));
        }
    }

    let vis_count = draft.visible_panels.len();
    let mut toggle: Option<(String, bool)> = None;
    let mut reorder: Option<(String, i32)> = None;

    card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        for (i, &(key, label, visible)) in ordered.iter().enumerate() {
            if i > 0 {
                ui.add(egui::Separator::default().spacing(1.0));
            }
            let unavailable = key == "battery" && !battery_present;
            let label_color = if unavailable { C_MUTED } else { C_TEXT };

            ui.horizontal(|ui| {
                ui.add_space(6.0);
                drag_handle(ui);
                ui.add_space(6.0);
                ui.label(egui::RichText::new(label).size(13.0).color(label_color));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if unavailable {
                        // Static grayed-out toggle — no interaction
                        let desired = egui::vec2(44.0, 24.0);
                        let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
                        if ui.is_rect_visible(rect) {
                            let r = (rect.height() / 2.0) as u8;
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(r),
                                egui::Color32::from_gray(45),
                            );
                            ui.painter().circle_filled(
                                egui::pos2(rect.left() + rect.height() / 2.0, rect.center().y),
                                rect.height() / 2.0 - 3.0,
                                egui::Color32::from_gray(75),
                            );
                        }
                    } else {
                        let mut v = visible;
                        if toggle_switch(ui, &mut v).changed() {
                            toggle = Some((key.to_string(), v));
                        }
                    }
                    if visible && !unavailable {
                        let idx =
                            draft.visible_panels.iter().position(|p| p == key).unwrap_or(0);
                        ui.add_space(6.0);
                        if idx + 1 < vis_count {
                            if arrow_btn(ui, true).clicked() {
                                reorder = Some((key.to_string(), 1));
                            }
                        } else {
                            ui.add_space(20.0);
                        }
                        if idx > 0 {
                            if arrow_btn(ui, false).clicked() {
                                reorder = Some((key.to_string(), -1));
                            }
                        } else {
                            ui.add_space(20.0);
                        }
                    } else if visible {
                        ui.add_space(46.0);
                    }
                });
            });
        }
    });

    if let Some((key, add)) = toggle {
        if add {
            if !draft.visible_panels.contains(&key) {
                // Re-insert at the natural ALL_PANELS position rather than appending.
                // Find the first panel that comes AFTER `key` in ALL_PANELS and is
                // already visible — insert before it.
                let all_idx = ALL_PANELS.iter().position(|(k, _)| *k == key).unwrap_or(usize::MAX);
                let insert_at = draft.visible_panels.iter().position(|p| {
                    let pi = ALL_PANELS.iter().position(|(k, _)| k == p).unwrap_or(usize::MAX);
                    pi > all_idx
                });
                match insert_at {
                    Some(i) => draft.visible_panels.insert(i, key),
                    None => draft.visible_panels.push(key),
                }
            }
        } else {
            draft.visible_panels.retain(|p| p != &key);
        }
    }
    if let Some((key, d)) = reorder {
        if let Some(idx) = draft.visible_panels.iter().position(|p| p == &key) {
            let new_idx = if d < 0 {
                idx.saturating_sub(1)
            } else {
                (idx + 1).min(draft.visible_panels.len().saturating_sub(1))
            };
            draft.visible_panels.swap(idx, new_idx);
        }
    }
}

// ── Alerts tab ────────────────────────────────────────────────────────────────

fn draw_alerts(ui: &mut egui::Ui, draft: &mut settings::Settings) {
    // Notifications card
    card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, "Notifications");
        ui.add_space(8.0);

        inner_row().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Notify on Critical").size(13.0).color(C_TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    toggle_switch(ui, &mut draft.notify_on_crit);
                });
            });
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Cooldown").size(12.0).color(C_MUTED));
            ui.add_space(8.0);
            let mut secs = draft.alert_cooldown_secs as i32;
            if ui
                .add(egui::DragValue::new(&mut secs).range(60..=3600).suffix(" s"))
                .changed()
            {
                draft.alert_cooldown_secs = secs.max(60) as u64;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::dialog_btn_secondary(ui, "Test Notification").clicked() {
                    send_test_notification();
                }
            });
        });
    });

    // Thresholds card
    card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, "Thresholds (blank to disable)");
        ui.add_space(8.0);

        // Temperature section
        ui.label(egui::RichText::new("Temperature °C").size(11.0).color(C_MUTED));
        ui.add_space(4.0);

        // Column headers
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_sized(
                    [44.0, 16.0],
                    egui::Label::new(egui::RichText::new("Crit").size(11.0).strong().color(C_CRIT)),
                );
                ui.add_space(4.0);
                ui.add_sized(
                    [44.0, 16.0],
                    egui::Label::new(egui::RichText::new("Warn").size(11.0).strong().color(C_WARN)),
                );
            });
        });

        for &(key, label) in &[("cpu", "CPU"), ("gpu", "GPU"), ("ram", "RAM"), ("disk", "Disk")] {
            let entry = draft
                .thresholds
                .entry(key.to_string())
                .or_insert_with(|| settings::default_thresholds().remove(key).unwrap_or_default());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).size(13.0).color(C_TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    threshold_field(ui, &mut entry.crit);
                    ui.add_space(4.0);
                    threshold_field(ui, &mut entry.warn);
                });
            });
        }

        ui.add_space(8.0);
        ui.label(egui::RichText::new("Battery % (below)").size(11.0).color(C_MUTED));
        ui.add_space(4.0);

        let bat = draft
            .thresholds
            .entry("battery".to_string())
            .or_insert_with(|| {
                settings::default_thresholds()
                    .remove("battery")
                    .unwrap_or_default()
            });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Battery").size(13.0).color(C_TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                threshold_field(ui, &mut bat.crit);
                ui.add_space(4.0);
                threshold_field(ui, &mut bat.warn);
            });
        });
    });
}

fn send_test_notification() {
    let script = concat!(
        "Add-Type -AssemblyName System.Windows.Forms; ",
        "$n = New-Object System.Windows.Forms.NotifyIcon; ",
        "$n.Icon = [System.Drawing.SystemIcons]::Information; ",
        "$n.Visible = $true; ",
        "$n.ShowBalloonTip(3000,'RigStats','Test notification',[System.Windows.Forms.ToolTipIcon]::Info); ",
        "Start-Sleep 4; $n.Dispose()"
    );
    let _ = debug::run_hidden_command(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    );
}

// ── Appearance tab ────────────────────────────────────────────────────────────

fn draw_appearance(ui: &mut egui::Ui, draft: &mut settings::Settings) {
    // Rig Name card
    card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, "Rig Name");
        ui.add_space(8.0);
        let w = ui.available_width();
        ui.add_sized(
            [w, 24.0],
            egui::TextEdit::singleline(&mut draft.model_name).text_color(C_TEXT),
        );
    });

    // Window card
    card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, "Window");
        ui.add_space(8.0);

        // Opacity
        let pct = (draft.opacity * 100.0).round() as u32;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Opacity").size(12.0).color(C_MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(format!("{pct}%")).size(12.0).color(C_TEXT));
            });
        });
        let mut opacity = draft.opacity as f32;
        if ui
            .add(
                egui::Slider::new(&mut opacity, 0.1_f32..=1.0_f32)
                    .step_by(0.05)
                    .show_value(false),
            )
            .changed()
        {
            draft.opacity = opacity as f64;
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Transparent").size(10.0).color(C_MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new("Solid").size(10.0).color(C_MUTED));
            });
        });

        ui.add_space(6.0);
        ui.add(egui::Separator::default().spacing(4.0));
        ui.add_space(4.0);

        // Window Layer
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Window Layer").size(12.0).color(C_MUTED));
            ui.add_space(8.0);
            let layer_label = match draft.window_layer.as_str() {
                "on_top" => "Always on Top",
                "behind" => "Always Behind",
                _ => "Normal",
            };
            let w = ui.available_width();
            egui::ComboBox::from_id_salt("window_layer")
                .selected_text(layer_label)
                .width(w)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut draft.window_layer, "normal".to_string(), "Normal");
                    ui.selectable_value(
                        &mut draft.window_layer,
                        "on_top".to_string(),
                        "Always on Top",
                    );
                    ui.selectable_value(
                        &mut draft.window_layer,
                        "behind".to_string(),
                        "Always Behind",
                    );
                });
        });

        ui.add_space(4.0);
        ui.add(egui::Separator::default().spacing(4.0));
        ui.add_space(4.0);

        // Launch at Startup
        inner_row().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Launch at Startup").size(13.0).color(C_TEXT),
                    );
                    ui.label(
                        egui::RichText::new("Starts with Windows · LHM connects automatically")
                            .size(11.0)
                            .color(C_MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    toggle_switch(ui, &mut draft.autostart_enabled);
                });
            });
        });
    });

    // Theme card (placeholder — theme system not yet implemented)
    card_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, "Theme");
        ui.add_space(8.0);
        let w = ui.available_width();
        egui::ComboBox::from_id_salt("theme_combo")
            .selected_text("Dark Cyan (default)")
            .width(w)
            .show_ui(ui, |ui| {
                let _ = ui.selectable_label(true, "Dark Cyan (default)");
            });
    });
}

