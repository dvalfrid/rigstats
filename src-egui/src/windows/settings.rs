//! Settings window: what applies to the whole app — start-up, the
//! dashboard window, the overlay's placement and notifications. What a
//! Control Center profile holds (accent colour, panels, overlay content,
//! alert thresholds) is edited in the Control Center (`profile_look.rs`);
//! this window points there. Built from `ui_kit`: a sidebar of pages, each
//! a column of grouped rows.

use crate::lock_ext::LockSafe;
use crate::theme::{self, DialogColors};
use crate::windows::control;
use crate::windows::ui_kit::{self, Icon};
use crate::windows::{OpenRequest, OpenRequests};
use rigstats_backend::settings::{self, ProfileLook};
use rigstats_backend::{autostart, debug};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// ── Panel / profile data ───────────────────────────────────────────────────────

pub(crate) const ALL_PANELS: &[(&str, &str)] = &[
    ("header", "Header"),
    ("clock", "Clock"),
    ("cpu", "CPU"),
    ("gpu", "GPU"),
    ("ram", "RAM"),
    ("net", "Network"),
    ("disk", "Storage"),
    ("motherboard", "Motherboard"),
    ("process", "Processes"),
    ("gpu_processes", "GPU Apps"),
    ("power", "System Power"),
    ("battery", "Battery"),
    ("peripherals", "Peripherals"),
];

pub(crate) const ALL_PROFILES: &[(&str, &str)] = &[
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
    ("landscape-xl", "Landscape XL (1920×450)"),
    ("landscape-slim", "Landscape Slim (1920×480)"),
    ("landscape-hd", "Landscape HD (1280×720)"),
    ("landscape-wxga", "Landscape WXGA (1280×800)"),
    ("landscape-fhd", "Landscape FHD (1920×1080)"),
    ("landscape-wuxga", "Landscape WUXGA (1920×1200)"),
    ("landscape-qhd", "Landscape QHD (2560×1440)"),
    ("landscape-hdplus", "Landscape HD+ (1366×768)"),
    ("landscape-1600x900", "Landscape 1600×900"),
    ("landscape-1680x1050", "Landscape 1680×1050"),
    ("landscape-2560x1600", "Landscape 2560×1600"),
    ("landscape-4k", "Landscape 4K (3840×2160)"),
    ("landscape-fhd-top", "FHD Top (1080×253)"),
    ("landscape-qhd-top", "QHD Top (1440×338)"),
    ("landscape-4k-top", "4K Top (2160×506)"),
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

// ── State ─────────────────────────────────────────────────────────────────────

/// The pages of the Settings window — what applies to the whole app. What a
/// profile holds (accent colour, panels, overlay content, alert thresholds)
/// is in the Control Center.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    General,
    Display,
    Overlay,
    Notifications,
}

const PAGES: &[(Page, Icon, &str)] = &[
    (Page::General, Icon::General, "General"),
    (Page::Display, Icon::Display, "Display"),
    (Page::Overlay, Icon::Overlay, "Overlay"),
    (Page::Notifications, Icon::Alerts, "Notifications"),
];

pub struct SettingsWindow {
    pub draft: settings::Settings,
    pub original: settings::Settings,
    last_preview: settings::Settings,
    pub page: Page,
    pub error: Option<String>,
    /// Detected GPU adapter names for the Display page's GPU selector.
    gpu_names: Vec<String>,
}

impl SettingsWindow {
    pub fn from_settings(s: &settings::Settings, gpu_names: Vec<String>) -> Self {
        let mut draft = s.clone();
        draft.autostart_enabled = autostart::is_run_key_present();
        Self {
            original: draft.clone(),
            last_preview: draft.clone(),
            draft,
            page: Page::default(),
            error: None,
            gpu_names,
        }
    }

    /// Supplies the GPU list once background adapter detection finishes
    /// (it may still be running when the dialog is first created).
    pub fn set_gpu_names(&mut self, names: Vec<String>) {
        self.gpu_names = names;
    }
}

/// What this dialog doesn't edit, taken from the live settings whenever it
/// writes its draft back: the profile's look (edited in the Control Center,
/// switched with the profile, toggled from the tray or the hotkey) and
/// click-through (applied at once by its own switch, the tray or the
/// hotkey). A draft from when the dialog opened must not undo those.
fn carry_live(dst: &mut settings::Settings, live: &settings::Settings) {
    dst.overlay_click_through = live.overlay_click_through;
    ProfileLook::capture(live).apply_to(dst);
    dst.profile_looks = live.profile_looks.clone();
    dst.look_profile = live.look_profile.clone();
}

/// The draft as the app should run it now: the window layer stays what is
/// running until Save (it starts or stops the wallpaper host).
fn live_preview(draft: &settings::Settings, live: &settings::Settings) -> settings::Settings {
    let mut s = draft.clone();
    s.window_layer = live.window_layer.clone();
    carry_live(&mut s, live);
    s
}

// ── show() ────────────────────────────────────────────────────────────────────

// The ctx-level panel API, as in every other dialog.
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
    dc: &DialogColors,
    dcomp_available: bool,
    requests: &OpenRequests,
) {
    dc.apply_to_ctx(ctx);
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    let mut action_save = false;
    let mut action_cancel = false;

    ui_kit::hero(ctx, dc, "settings", "Settings", |_| {});

    // ── Footer ────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("settings_footer")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 10,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            let err = state.lock_safe().error.clone();
            if let Some(ref e) = err {
                ui.label(
                    egui::RichText::new(e.as_str())
                        .size(11.0)
                        .color(egui::Color32::from_rgb(220, 80, 80)),
                );
                ui.add_space(4.0);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::dialog_btn_secondary(ui, "Cancel", dc).clicked() {
                    action_cancel = true;
                }
                ui.add_space(6.0);
                if theme::dialog_btn_primary(ui, "Save").clicked() {
                    action_save = true;
                }
            });
        });

    // ── Sidebar ───────────────────────────────────────────────────────────────
    egui::SidePanel::left("settings_nav")
        .resizable(false)
        .exact_width(ui_kit::SIDEBAR_W)
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin::same(10)))
        .show(ctx, |ui| {
            let mut st = state.lock_safe();
            ui.add_space(4.0);
            for &(page, icon, label) in PAGES {
                if ui_kit::nav_item(ui, dc, icon, label, st.page == page).clicked() {
                    st.page = page;
                }
            }
            // Where the rest went: the profile's own settings.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.add_space(4.0);
                if theme::dialog_btn_secondary(ui, "Open Control Center", dc).clicked() {
                    *requests.lock_safe() = Some(OpenRequest::Control(control::Page::Dashboard));
                }
                ui.add_space(6.0);
                ui_kit::footnote(
                    ui,
                    dc,
                    "Accent colour, panels, overlay metrics and alerts belong to your \
                     profile — set them in the Control Center.",
                );
            });
        });

    // ── Page ──────────────────────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(ui_kit::dialog_frame(dc))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("settings_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: 24,
                            right: 24,
                            top: 18,
                            bottom: 8,
                        })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let mut st = state.lock_safe();
                            // The window layer is Save-only, so what is
                            // running is the live value, not the draft (and
                            // not a dialog-open-time snapshot — the tray can
                            // change it while this dialog is open).
                            let applied_layer = saved.lock_safe().window_layer.clone();
                            let page = st.page;
                            // In wallpaper mode the visible dashboard is the
                            // host process, which reads the settings file:
                            // nothing previews until Save.
                            if applied_layer == "wallpaper" {
                                wallpaper_notice(ui, dc);
                            }
                            let gpu_names = st.gpu_names.clone();
                            let draft = &mut st.draft;
                            match page {
                                Page::General => general_page(ui, dc, draft, dir.as_ref()),
                                Page::Display => {
                                    display_page(ui, dc, draft, &applied_layer, &gpu_names)
                                }
                                Page::Overlay => overlay_page(
                                    ui,
                                    dc,
                                    draft,
                                    saved,
                                    reload,
                                    dir.as_ref(),
                                    dcomp_available,
                                    requests,
                                ),
                                Page::Notifications => notifications_page(ui, dc, draft, requests),
                            }
                        });
                });
        });

    // ── Apply deferred actions ────────────────────────────────────────────────

    // Live preview: push the draft to the app on every change. The window
    // layer waits for Save; see `live_preview`.
    {
        let mut st = state.lock_safe();
        if st.draft != st.last_preview {
            st.last_preview = st.draft.clone();
            let preview = live_preview(&st.draft, &saved.lock_safe());
            *saved.lock_safe() = preview;
            reload.store(true, Ordering::Relaxed);
            main_ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }

    if action_save {
        let mut st = state.lock_safe();
        let autostart_result = if st.draft.autostart_enabled {
            autostart::register_autostart()
        } else {
            autostart::unregister_autostart()
        };
        let mut to_persist = st.draft.clone();
        carry_live(&mut to_persist, &saved.lock_safe());
        let save_result = settings::persist_settings(dir.as_ref(), &to_persist);
        match (save_result, autostart_result) {
            (Ok(()), Ok(())) => {
                // Push the full draft (including the window layer) to the app.
                *saved.lock_safe() = to_persist;
                reload.store(true, Ordering::Relaxed);
                st.error = None;
                open.store(false, Ordering::Relaxed);
                main_ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
            (Err(e), _) | (Ok(()), Err(e)) => {
                let msg = format!("Save failed: {e}");
                debug::append_debug_log(dir.as_ref(), &format!("settings: {msg}"));
                st.error = Some(msg);
            }
        }
    }
    // Cancel, or closing the window: put back what was there when the
    // dialog opened — except what it never edited (`live_preview`).
    if action_cancel || ctx.input(|i| i.viewport().close_requested()) {
        let st = state.lock_safe();
        let reverted = live_preview(&st.original, &saved.lock_safe());
        *saved.lock_safe() = reverted;
        reload.store(true, Ordering::Relaxed);
        drop(st);
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// Amber notice on every page while the dashboard is in the desktop
/// wallpaper: changes show when Save writes them to disk.
fn wallpaper_notice(ui: &mut egui::Ui, dc: &DialogColors) {
    const AMBER: egui::Color32 = egui::Color32::from_rgb(0xE5, 0xC0, 0x7B);
    ui_kit::card_frame(dc)
        .stroke(egui::Stroke::new(1.0_f32, AMBER.gamma_multiply(0.6)))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "The dashboard is in the desktop wallpaper, so changes show when you click Save.",
                    )
                    .size(12.0)
                    .color(AMBER),
                )
                .wrap(),
            );
        });
    ui.add_space(14.0);
}

/// A "where it is now" row that opens the Control Center on `page`.
fn control_center_row(
    g: &mut ui_kit::Group<'_>,
    dc: &DialogColors,
    requests: &OpenRequests,
    title: &str,
    subtitle: &str,
    page: control::Page,
) {
    g.row(title, Some(subtitle), |ui| {
        if theme::dialog_btn_secondary(ui, "Control Center", dc).clicked() {
            *requests.lock_safe() = Some(OpenRequest::Control(page));
        }
    });
}

// ── General ───────────────────────────────────────────────────────────────────

fn general_page(ui: &mut egui::Ui, dc: &DialogColors, draft: &mut settings::Settings, dir: &Path) {
    ui_kit::page_header(ui, dc, "General", "Start-up, the header and recordings.");
    ui_kit::group(ui, dc, None, None, |g| {
        g.row(
            "Start with Windows",
            Some("RIGStats opens when you sign in"),
            |ui| {
                ui_kit::toggle(ui, dc, &mut draft.autostart_enabled);
            },
        );
        g.row(
            "Model name",
            Some("Shown in the header panel; empty uses the detected model"),
            |ui| {
                ui.add_sized(
                    [ui_kit::CONTROL_W, 24.0],
                    egui::TextEdit::singleline(&mut draft.model_name)
                        .hint_text("Detected automatically")
                        .text_color(dc.text),
                );
            },
        );
    });
    ui_kit::group(
        ui,
        dc,
        Some("Session recording"),
        Some("Start and stop a recording from the tray menu; browse past ones in Session History."),
        |g| {
            g.row("Keep recordings for", None, |ui| {
                let current = LOG_RETENTION_OPTIONS
                    .iter()
                    .find(|(d, _)| *d == draft.log_retention_days)
                    .map_or("7 days", |(_, l)| *l);
                ui_kit::dropdown(ui, "log_retention", current, |ui| {
                    for &(days, label) in LOG_RETENTION_OPTIONS {
                        ui.selectable_value(&mut draft.log_retention_days, days, label);
                    }
                });
            });
            g.row("Recordings and logs", None, |ui| {
                if theme::dialog_btn_secondary(ui, "Open Folder", dc).clicked() {
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
            });
        },
    );
}

// ── Display ───────────────────────────────────────────────────────────────────

fn layer_label(layer: &str) -> &'static str {
    match layer {
        "on_top" => "Always on top",
        "behind" => "Always behind",
        "wallpaper" => "Desktop wallpaper",
        _ => "Normal",
    }
}

fn display_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    draft: &mut settings::Settings,
    applied_layer: &str,
    gpu_names: &[String],
) {
    ui_kit::page_header(
        ui,
        dc,
        "Display",
        "Where and how the dashboard window appears, for every profile.",
    );
    // Desktop Wallpaper mode is display-only: floating and fill-screen have
    // no effect there, so those controls are off while wallpaper is the
    // *applied* layer — the layer itself only changes on Save.
    let is_wallpaper = applied_layer == "wallpaper";
    ui_kit::group(
        ui,
        dc,
        Some("Window"),
        Some(if draft.window_layer != applied_layer {
            "The new window layer takes effect when you click Save."
        } else if is_wallpaper {
            "In the desktop wallpaper the dashboard stays behind every window and survives \
             Win+D. Place it in Normal mode first, then switch."
        } else {
            "Desktop wallpaper puts the dashboard behind every window, where you placed it."
        }),
        |g| {
            let profile_note = if draft.floating_mode {
                Some("Not used for floating panels")
            } else if is_wallpaper {
                Some("Switch away from desktop wallpaper to change it")
            } else {
                Some("The size of the monitor it is shown on")
            };
            g.row("Display profile", profile_note, |ui| {
                ui.add_enabled_ui(!draft.floating_mode && !is_wallpaper, |ui| {
                    let current = ALL_PROFILES
                        .iter()
                        .find(|(k, _)| *k == draft.dashboard_profile.as_str())
                        .map_or(draft.dashboard_profile.clone(), |(_, l)| l.to_string());
                    ui_kit::dropdown(ui, "profile_combo", current, |ui| {
                        ui.label(egui::RichText::new("Portrait").small().color(dc.muted));
                        let mut landscape_heading = false;
                        for &(key, label) in ALL_PROFILES {
                            if key.starts_with("landscape-") && !landscape_heading {
                                landscape_heading = true;
                                ui.separator();
                                ui.label(egui::RichText::new("Landscape").small().color(dc.muted));
                            }
                            ui.selectable_value(
                                &mut draft.dashboard_profile,
                                key.to_string(),
                                label,
                            );
                        }
                    });
                });
            });
            g.row("Window layer", None, |ui| {
                ui_kit::dropdown(ui, "window_layer", layer_label(&draft.window_layer), |ui| {
                    for layer in ["normal", "on_top", "behind", "wallpaper"] {
                        ui.selectable_value(
                            &mut draft.window_layer,
                            layer.to_string(),
                            layer_label(layer),
                        );
                    }
                });
            });
        },
    );
    // Wallpaper and floating mode exclude each other.
    if draft.window_layer == "wallpaper" {
        draft.floating_mode = false;
    }

    let is_landscape = crate::geometry::profile_is_landscape(&draft.dashboard_profile);
    let fill_enabled = !draft.floating_mode && !is_wallpaper;
    ui_kit::group(ui, dc, Some("Layout"), None, |g| {
        g.row(
            "Floating panels",
            Some(if is_wallpaper {
                "Not available in desktop wallpaper mode"
            } else {
                "Each panel in its own window, placed anywhere"
            }),
            |ui| {
                ui.add_enabled_ui(!is_wallpaper, |ui| {
                    ui_kit::toggle(ui, dc, &mut draft.floating_mode);
                });
            },
        );
        if draft.floating_mode {
            g.row("Panel size", None, |ui| {
                ui_kit::slider_pct(ui, dc, &mut draft.floating_panel_scale, 0.4..=1.0);
            });
        }
        g.row(
            "Fill screen",
            Some(if fill_enabled {
                "Cover the whole monitor; the background fills the rest"
            } else {
                "Only for the docked dashboard, not in wallpaper mode"
            }),
            |ui| {
                ui.add_enabled_ui(fill_enabled, |ui| {
                    ui_kit::toggle(ui, dc, &mut draft.fullscreen_mode);
                });
            },
        );
        // Only the portrait stack has room left to align within.
        if draft.fullscreen_mode && fill_enabled && !is_landscape {
            g.row("Panel position", None, |ui| {
                ui_kit::segmented(
                    ui,
                    dc,
                    &mut draft.fullscreen_align,
                    &[("top".to_string(), "Top"), ("center".to_string(), "Centre")],
                );
            });
        }
    });

    ui_kit::group(ui, dc, Some("Panels"), None, |g| {
        // Which adapter the GPU panel and overlay show on multi-GPU
        // systems — the same choice as the tray's GPU submenu.
        if gpu_names.len() > 1 {
            g.row("Graphics card shown", None, |ui| {
                const AUTO: &str = "Automatic (most VRAM)";
                let selected =
                    crate::tray::selected_gpu_index(gpu_names, draft.preferred_gpu.as_deref());
                let current = selected.map_or(AUTO, |i| gpu_names[i].as_str());
                ui_kit::dropdown(ui, "preferred_gpu", current, |ui| {
                    if ui.selectable_label(selected.is_none(), AUTO).clicked() {
                        draft.preferred_gpu = None;
                    }
                    for (i, name) in gpu_names.iter().enumerate() {
                        if ui
                            .selectable_label(selected == Some(i), name.as_str())
                            .clicked()
                        {
                            draft.preferred_gpu = Some(name.clone());
                        }
                    }
                });
            });
        }
        g.row(
            "Power supply",
            Some("Scales the System Power bar; off uses 500 W (laptop: 120 W)"),
            |ui| {
                let mut set = draft.psu_watts.is_some();
                if ui_kit::toggle(ui, dc, &mut set).changed() {
                    draft.psu_watts = set.then_some(650);
                }
                if let Some(w) = &mut draft.psu_watts {
                    ui.add_space(4.0);
                    let mut val = i32::from(*w);
                    if ui
                        .add(
                            egui::DragValue::new(&mut val)
                                .range(100..=5000)
                                .suffix(" W"),
                        )
                        .changed()
                    {
                        *w = val as u16;
                    }
                }
            },
        );
    });
}

// ── Overlay ───────────────────────────────────────────────────────────────────

fn anchor_label(anchor: &str) -> &'static str {
    match anchor {
        "top-left" => "Top left",
        "bottom-left" => "Bottom left",
        "bottom-right" => "Bottom right",
        "free" => "Where I drag it",
        _ => "Top right",
    }
}

/// Click-through bypasses the draft: it applies and is saved at once, like
/// the tray's "Lock/Unlock Overlay" and the hotkey, so it can never be left
/// stuck mid-edit (and Cancel doesn't touch it).
#[allow(clippy::too_many_arguments)]
fn overlay_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    draft: &mut settings::Settings,
    saved: &Arc<Mutex<settings::Settings>>,
    reload: &Arc<AtomicBool>,
    dir: &Path,
    dcomp_available: bool,
    requests: &OpenRequests,
) {
    ui_kit::page_header(
        ui,
        dc,
        "Overlay",
        "Where the game overlay sits and how it looks, for every profile.",
    );
    ui_kit::group(ui, dc, Some("Position"), None, |g| {
        g.row("Corner", None, |ui| {
            ui_kit::dropdown(
                ui,
                "overlay_anchor",
                anchor_label(&draft.overlay_anchor),
                |ui| {
                    for a in [
                        "top-left",
                        "top-right",
                        "bottom-left",
                        "bottom-right",
                        "free",
                    ] {
                        ui.selectable_value(
                            &mut draft.overlay_anchor,
                            a.to_string(),
                            anchor_label(a),
                        );
                    }
                },
            );
        });
        if draft.overlay_anchor != "free" {
            g.row("Distance from the edge", None, |ui| {
                ui.add_sized(
                    [40.0, 20.0],
                    egui::Label::new(
                        egui::RichText::new(format!("{} px", draft.overlay_margin))
                            .size(12.0)
                            .color(dc.text),
                    ),
                );
                ui.spacing_mut().slider_width = 150.0;
                let mut margin = draft.overlay_margin as f32;
                let slider = egui::Slider::new(&mut margin, 0.0_f32..=64.0_f32)
                    .show_value(false)
                    .trailing_fill(true);
                if ui.add(slider).changed() {
                    draft.overlay_margin = margin.round() as i32;
                }
            });
        }
    });
    // Opacity only shows with a background, or on the whole-window fade
    // used without DirectComposition.
    let opacity_matters = draft.overlay_background || !dcomp_available;
    ui_kit::group(ui, dc, Some("Appearance"), None, |g| {
        g.row("Size", None, |ui| {
            ui_kit::slider_pct(ui, dc, &mut draft.overlay_scale, 0.5..=2.0);
        });
        g.row("Background", Some("A frosted card behind the text"), |ui| {
            ui_kit::toggle(ui, dc, &mut draft.overlay_background);
        });
        g.row(
            "Opacity",
            (!opacity_matters).then_some("Only with a background"),
            |ui| {
                ui.add_enabled_ui(opacity_matters, |ui| {
                    ui_kit::slider_pct(ui, dc, &mut draft.overlay_opacity, 0.05..=1.0);
                });
            },
        );
    });
    ui_kit::group(ui, dc, None, None, |g| {
        let mut locked = saved.lock_safe().overlay_click_through;
        g.row(
            "Click-through",
            Some("Clicks go to the game underneath. Applies at once."),
            |ui| {
                if ui_kit::toggle(ui, dc, &mut locked).changed() {
                    let mut s = saved.lock_safe();
                    s.overlay_click_through = locked;
                    if let Err(e) = settings::persist_settings(dir, &s) {
                        debug::append_debug_log(
                            dir,
                            &format!("settings: overlay click-through persist failed — {e}"),
                        );
                    }
                    reload.store(true, Ordering::Relaxed);
                }
            },
        );
    });
    ui_kit::group(ui, dc, None, None, |g| {
        control_center_row(
            g,
            dc,
            requests,
            "What the overlay shows",
            "Showing it, its metrics and layout belong to your profile",
            control::Page::Overlay,
        );
    });
}

// ── Notifications ─────────────────────────────────────────────────────────────

const COOLDOWNS: &[(u64, &str)] = &[
    (60, "1 minute"),
    (300, "5 minutes"),
    (900, "15 minutes"),
    (1800, "30 minutes"),
    (3600, "1 hour"),
];

fn notifications_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    draft: &mut settings::Settings,
    requests: &OpenRequests,
) {
    ui_kit::page_header(
        ui,
        dc,
        "Notifications",
        "Whether alerts notify you, for every profile.",
    );
    ui_kit::group(ui, dc, None, None, |g| {
        g.row(
            "Critical alerts",
            Some("When a value passes its critical level"),
            |ui| {
                ui_kit::toggle(ui, dc, &mut draft.notify_on_crit);
            },
        );
        g.row(
            "Warnings",
            Some("When a value passes its warning level"),
            |ui| {
                ui_kit::toggle(ui, dc, &mut draft.notify_on_warn);
            },
        );
        g.row("Repeat at most every", Some("For the same alert"), |ui| {
            let current = COOLDOWNS
                .iter()
                .find(|(s, _)| *s == draft.alert_cooldown_secs)
                .map_or_else(
                    || format!("{} s", draft.alert_cooldown_secs),
                    |(_, l)| l.to_string(),
                );
            ui_kit::dropdown(ui, "alert_cooldown", current, |ui| {
                for &(secs, label) in COOLDOWNS {
                    ui.selectable_value(&mut draft.alert_cooldown_secs, secs, label);
                }
            });
        });
        g.row("Test", Some("Shows a sample notification"), |ui| {
            if theme::dialog_btn_secondary(ui, "Send Test", dc).clicked() {
                send_test_notification();
            }
        });
    });
    ui_kit::group(ui, dc, None, None, |g| {
        control_center_row(
            g,
            dc,
            requests,
            "When alerts fire",
            "The warning and critical levels belong to your profile",
            control::Page::Alerts,
        );
    });
}
/// Severity of a [`send_notification`] balloon tip — controls both the tray
/// glyph shown briefly while the balloon is up and the small icon inside it.
#[derive(Clone, Copy)]
pub enum NotifyIcon {
    Info,
    Warning,
    Error,
}

/// Shows a real Windows balloon-tip notification via a hidden PowerShell
/// `System.Windows.Forms.NotifyIcon` — the only notification mechanism this
/// app uses; there is no toast/WinRT dependency. Blocks the calling thread
/// for the duration of the balloon (script does `Start-Sleep` before
/// disposing the icon), so callers outside a deliberate button click (which
/// can tolerate a brief pause) should call this from a background thread.
pub fn send_notification(title: &str, message: &str, icon: NotifyIcon) {
    let icon_name = match icon {
        NotifyIcon::Info => "Information",
        NotifyIcon::Warning => "Warning",
        NotifyIcon::Error => "Error",
    };
    let tooltip_icon = match icon {
        NotifyIcon::Info => "Info",
        NotifyIcon::Warning => "Warning",
        NotifyIcon::Error => "Error",
    };
    // PowerShell single-quoted string escape: a literal `'` is written as `''`.
    let esc = |s: &str| s.replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; \
         $n = New-Object System.Windows.Forms.NotifyIcon; \
         $n.Icon = [System.Drawing.SystemIcons]::{icon_name}; \
         $n.Visible = $true; \
         $n.ShowBalloonTip(5000,'{}','{}',[System.Windows.Forms.ToolTipIcon]::{tooltip_icon}); \
         Start-Sleep 6; $n.Dispose()",
        esc(title),
        esc(message),
    );
    let _ = debug::run_hidden_command(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    );
}

fn send_test_notification() {
    send_notification("RigStats", "Test notification", NotifyIcon::Info);
}
