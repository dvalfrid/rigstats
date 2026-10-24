//! Settings window: what applies to the whole app — start-up, the
//! dashboard window, the overlay's placement, notifications and the global
//! shortcuts. What a
//! Control Center profile holds (accent colour, panels, overlay content,
//! alert thresholds) is edited in the Control Center (`profile_look.rs`);
//! this window points there. Built from `ui_kit`: a sidebar of pages, each
//! a column of grouped rows.

use crate::lock_ext::LockSafe;
use crate::theme::{self, DialogColors};
use crate::windows::control;
use crate::windows::ui_kit::{self, Icon};
use crate::windows::{OpenRequest, OpenRequests};
use rigstats_backend::control::Severity;
use rigstats_backend::settings::{self, ProfileLook};
use rigstats_backend::shortcut::{self, Action, Mods, Rejected, Shortcut, Shortcuts};
use rigstats_backend::{autostart, debug, microphone};
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
    Shortcuts,
}

const PAGES: &[(Page, Icon, &str)] = &[
    (Page::General, Icon::General, "General"),
    (Page::Display, Icon::Display, "Display"),
    (Page::Overlay, Icon::Overlay, "Overlay"),
    (Page::Notifications, Icon::Alerts, "Notifications"),
    (Page::Shortcuts, Icon::Keyboard, "Shortcuts"),
];

pub struct SettingsWindow {
    pub draft: settings::Settings,
    pub original: settings::Settings,
    last_preview: settings::Settings,
    pub page: Page,
    pub error: Option<String>,
    /// When click-through (applied at once) last changed — the footer
    /// confirms it for a moment.
    saved_at: Option<std::time::Instant>,
    /// Detected GPU adapter names for the Display page's GPU selector.
    gpu_names: Vec<String>,
    /// Windows' microphones for the General page's selector (#425), read
    /// when the dialog opens.
    microphones: Vec<microphone::Device>,
    /// The chosen microphone's level and live meter (#427).
    mic: MicPanel,
    /// The shortcut waiting for its new keys (#569); the app's own
    /// shortcuts are held off meanwhile (`hotkey::set_paused`).
    capture: Option<Action>,
    /// Why the last keys or choice for a shortcut weren't taken.
    shortcut_note: Option<(Action, String)>,
}

/// The General page's microphone level and meter (#427), for one device —
/// rebuilt when the device choice changes. The level is Windows' own input
/// level, so it applies at once rather than with Save; the meter opens its
/// stream only while drawn (`microphone::Meter`).
struct MicPanel {
    device: Option<String>,
    level: Option<f64>,
    meter: microphone::Meter,
}

impl MicPanel {
    fn for_device(device: Option<String>) -> Self {
        Self {
            level: microphone::level(device.as_deref()).map(f64::from),
            meter: microphone::Meter::new(device.clone()),
            device,
        }
    }
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
            saved_at: None,
            gpu_names,
            microphones: microphone::list(),
            mic: MicPanel::for_device(s.microphone_device.clone()),
            capture: None,
            shortcut_note: None,
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
    dst.app_rules = live.app_rules.clone();
    dst.desktop_background_before = live.desktop_background_before.clone();
    dst.wallpaper_engine_before = live.wallpaper_engine_before.clone();
}

/// Whether the desktop wallpaper is what runs: its layer, and floating off —
/// floating turned on over it (the tray) wins, as in `update_wallpaper_mode`.
fn wallpaper_running(live: &settings::Settings) -> bool {
    live.window_layer == "wallpaper" && !live.floating_mode
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

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
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
    let ctx = &ui.ctx().clone();
    dc.apply_to_ctx(ctx);
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    let mut action_save = false;
    let mut action_cancel = false;

    ui_kit::hero(ui, dc, "settings", "Settings", |_| {});

    // ── Footer ────────────────────────────────────────────────────────────────
    egui::Panel::bottom("settings_footer")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 10,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ui, |ui| {
            let err = state.lock_safe().error.clone();
            if let Some(ref e) = err {
                ui_kit::row_problem(ui, dc, rigstats_backend::control::Severity::Error, e);
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
                let recent = state
                    .lock_safe()
                    .saved_at
                    .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(3));
                if recent {
                    ui.add_space(10.0);
                    ui_kit::status(ui, ui_kit::C_GOOD, "Saved");
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(500));
                }
            });
        });

    // ── Sidebar ───────────────────────────────────────────────────────────────
    egui::Panel::left("settings_nav")
        .resizable(false)
        .exact_size(ui_kit::SIDEBAR_W)
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin::same(10)))
        .show(ui, |ui| {
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
        .show(ui, |ui| {
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
                            let (applied_layer, wallpaper_running) = {
                                let live = saved.lock_safe();
                                (live.window_layer.clone(), wallpaper_running(&live))
                            };
                            let page = st.page;
                            // In wallpaper mode the visible dashboard is the
                            // host process, which reads the settings file:
                            // nothing previews until Save.
                            if wallpaper_running {
                                wallpaper_notice(ui, dc);
                            }
                            let gpu_names = st.gpu_names.clone();
                            let microphones = st.microphones.clone();
                            let SettingsWindow {
                                draft,
                                saved_at,
                                mic,
                                capture,
                                shortcut_note,
                                ..
                            } = &mut *st;
                            match page {
                                Page::General => {
                                    general_page(ui, dc, draft, &microphones, mic, saved_at)
                                }
                                Page::Display => display_page(
                                    ui,
                                    dc,
                                    draft,
                                    &applied_layer,
                                    wallpaper_running,
                                    &gpu_names,
                                ),
                                Page::Overlay => overlay_page(
                                    ui,
                                    dc,
                                    draft,
                                    saved,
                                    reload,
                                    dir.as_ref(),
                                    dcomp_available,
                                    requests,
                                    saved_at,
                                ),
                                Page::Notifications => notifications_page(ui, dc, draft, requests),
                                Page::Shortcuts => shortcuts_page(
                                    ui,
                                    dc,
                                    &mut draft.shortcuts,
                                    capture,
                                    shortcut_note,
                                ),
                            }
                            // Waiting for keys only while that page shows.
                            if page != Page::Shortcuts {
                                *capture = None;
                            }
                        });
                });
        });

    // ── Apply deferred actions ────────────────────────────────────────────────

    // A chord RIGStats holds would fire instead of reaching the capture.
    #[cfg(windows)]
    crate::hotkey::set_paused(state.lock_safe().capture.is_some());

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
        st.capture = None;
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
        let mut st = state.lock_safe();
        st.capture = None;
        let reverted = live_preview(&st.original, &saved.lock_safe());
        *saved.lock_safe() = reverted;
        reload.store(true, Ordering::Relaxed);
        drop(st);
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// On every page while the dashboard is in the desktop wallpaper: changes
/// show when Save writes them to disk.
fn wallpaper_notice(ui: &mut egui::Ui, dc: &DialogColors) {
    ui_kit::notice(ui, dc, &crate::problems::wallpaper_saves_only());
    ui.add_space(4.0);
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
        if theme::dialog_btn_secondary(ui, "Open Control Center", dc).clicked() {
            *requests.lock_safe() = Some(OpenRequest::Control(page));
        }
    });
}

// ── General ───────────────────────────────────────────────────────────────────

fn general_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    draft: &mut settings::Settings,
    microphones: &[microphone::Device],
    mic: &mut MicPanel,
    saved_at: &mut Option<std::time::Instant>,
) {
    ui_kit::page_header(
        ui,
        dc,
        "General",
        "Start-up, updates, the header, the microphone and recordings.",
    );
    ui_kit::group(ui, dc, None, None, |g| {
        g.row(
            "Start with Windows",
            Some("RIGStats opens when you sign in"),
            |ui| {
                ui_kit::toggle(ui, dc, &mut draft.autostart_enabled);
            },
        );
        g.row(
            "Updates",
            Some("Release candidates come before a release, for testing it — they can have bugs"),
            |ui| {
                use settings::UpdateChannel;
                let label = |c: UpdateChannel| match c {
                    UpdateChannel::Stable => "Releases",
                    UpdateChannel::ReleaseCandidates => "Release candidates too",
                };
                ui_kit::dropdown(ui, "update_channel", label(draft.update_channel), |ui| {
                    for c in [UpdateChannel::Stable, UpdateChannel::ReleaseCandidates] {
                        if ui
                            .selectable_label(draft.update_channel == c, label(c))
                            .clicked()
                        {
                            draft.update_channel = c;
                        }
                    }
                });
            },
        );
        g.row(
            "Model name",
            Some("Shown in the header panel; empty uses the detected model"),
            |ui| {
                ui.add_sized(
                    [ui_kit::CONTROL_W, ui_kit::CONTROL_H],
                    egui::TextEdit::singleline(&mut draft.model_name)
                        .align(egui::Align2::LEFT_CENTER)
                        .hint_text("Detected automatically")
                        .text_color(dc.text),
                );
            },
        );
    });
    ui_kit::group(
        ui,
        dc,
        Some("Microphone"),
        Some(&microphone_footnote(draft.shortcuts.microphone)),
        |g| {
            g.row(
                "Microphone",
                Some("The one Windows uses for calls, or a specific one — if it isn't connected, the one for calls"),
                |ui| {
                    const DEFAULT: &str = "The one Windows uses for calls";
                    let current = draft
                        .microphone_device
                        .as_deref()
                        .and_then(|id| microphones.iter().find(|m| m.id == id))
                        .map_or(DEFAULT, |m| m.name.as_str());
                    ui_kit::dropdown(ui, "microphone_device", current, |ui| {
                        if ui.selectable_label(draft.microphone_device.is_none(), DEFAULT).clicked() {
                            draft.microphone_device = None;
                        }
                        for m in microphones {
                            let selected = draft.microphone_device.as_deref() == Some(m.id.as_str());
                            if ui.selectable_label(selected, &m.name).clicked() {
                                draft.microphone_device = Some(m.id.clone());
                            }
                        }
                    });
                },
            );
            // The level and meter follow the device shown above, even before Save.
            if mic.device != draft.microphone_device {
                *mic = MicPanel::for_device(draft.microphone_device.clone());
            }
            g.row(
                "Level",
                Some("How loud the microphone records"),
                |ui| match mic.level.as_mut() {
                    Some(level) => {
                        if ui_kit::slider_pct(ui, dc, level, 0.0..=1.0) {
                            match microphone::set_level(mic.device.as_deref(), *level as f32) {
                                Ok(read) => {
                                    *level = f64::from(read);
                                    *saved_at = Some(std::time::Instant::now());
                                }
                                Err(_) => {
                                    mic.level =
                                        microphone::level(mic.device.as_deref()).map(f64::from)
                                }
                            }
                        }
                    }
                    None => {
                        ui.label(
                            egui::RichText::new("No microphone")
                                .size(12.0)
                                .color(dc.muted),
                        );
                    }
                },
            );
            if mic.level.is_some() {
                g.row("Hearing now", Some("Speak to check the level"), |ui| {
                    ui_kit::meter(
                        ui,
                        dc,
                        "settings_mic",
                        mic.meter.level(),
                        ui_kit::SLIDER_PCT_W,
                    );
                });
            }
        },
    );
    ui_kit::group(
        ui,
        dc,
        Some("Session recording"),
        Some("Start and stop a recording from the tray menu; browse past ones — and their files — in Session History."),
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
    wallpaper_running: bool,
    gpu_names: &[String],
) {
    ui_kit::page_header(
        ui,
        dc,
        "Display",
        "Where and how the dashboard window appears, for every profile.",
    );
    // Desktop Wallpaper mode is display-only: the display profile and
    // fill-screen have no effect there, so those controls are off while the
    // wallpaper is what runs — the layer itself only changes on Save.
    let is_wallpaper = wallpaper_running;
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
                        let picked = ui
                            .selectable_value(
                                &mut draft.window_layer,
                                layer.to_string(),
                                layer_label(layer),
                            )
                            .clicked();
                        // Choosing the wallpaper means the docked dashboard
                        // in it, so floating goes off. Only on that choice:
                        // floating turned on over the wallpaper (the tray)
                        // is a state the app runs, and opening this page
                        // must not undo it.
                        if picked && layer == "wallpaper" {
                            draft.floating_mode = false;
                        }
                    }
                });
            });
        },
    );

    let is_landscape = crate::geometry::profile_is_landscape(&draft.dashboard_profile);
    let fill_enabled = !draft.floating_mode && !is_wallpaper;
    ui_kit::group(ui, dc, Some("Layout"), None, |g| {
        g.row(
            "Floating panels",
            Some("Each panel in its own window, placed anywhere"),
            // Floating wins over the wallpaper layer (`update_wallpaper_mode`),
            // so it can be switched in any layer, as from the tray.
            |ui| {
                ui_kit::toggle(ui, dc, &mut draft.floating_mode);
            },
        );
        if draft.floating_mode {
            g.row("Panel size", None, |ui| {
                ui_kit::slider_pct(ui, dc, &mut draft.floating_panel_scale, 0.4..=2.0);
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
    saved_at: &mut Option<std::time::Instant>,
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
                    *saved_at = Some(std::time::Instant::now());
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
    // Turned off in Windows: say so here, where someone looks first (#449).
    #[cfg(windows)]
    if let Some(why) = crate::notify::off() {
        let problem = crate::problems::notifications_off(why);
        if let Some(action) = ui_kit::notice(ui, dc, &problem) {
            crate::windows::run_problem_action(ui.ctx(), &action, requests);
        }
        ui.add_space(4.0);
    }
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
        g.row(
            "Microphone muted or unmuted",
            Some("After its shortcut or the tray menu"),
            |ui| {
                ui_kit::toggle(ui, dc, &mut draft.microphone_notify);
            },
        );
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
/// The General page's microphone footnote, naming its shortcut (#569).
fn microphone_footnote(chord: Option<Shortcut>) -> String {
    let rest = "The level is Windows' own input level and applies at once. Whether muting \
                shows a notification is under Notifications.";
    match chord {
        Some(chord) => format!(
            "{chord} (under Shortcuts), the tray menu and the microphone in the dashboard \
             header mute or unmute it. {rest}"
        ),
        None => format!(
            "The tray menu and the microphone in the dashboard header mute or unmute it; its \
             shortcut is off under Shortcuts. {rest}"
        ),
    }
}

// ── Shortcuts ─────────────────────────────────────────────────────────────────

/// One row on the Shortcuts page: the action, its title and what it does.
type ShortcutRow = (Action, &'static str, &'static str);

/// The Shortcuts page, grouped by what the shortcuts do (#594).
const SHORTCUT_GROUPS: &[(&str, &[ShortcutRow])] = &[
    (
        "Overlay and dashboard",
        &[
            (
                Action::Overlay,
                "Show or hide the overlay",
                "Without leaving the game",
            ),
            (
                Action::OverlayLock,
                "Lock or unlock the overlay",
                "Unlocked, it can be dragged; locked, clicks pass through to the game",
            ),
            (
                Action::Floating,
                "Floating panels on or off",
                "Each panel as its own window, or the dashboard",
            ),
        ],
    ),
    (
        "Profiles",
        &[
            (
                Action::NextProfile,
                "Next profile",
                "Switches to the next Control Center profile",
            ),
            (
                Action::Profiles,
                "Profile 1–9",
                "Straight to the profile at that place in the profile bar — bind a macro key to it",
            ),
        ],
    ),
    (
        "Devices",
        &[
            (
                Action::Microphone,
                "Mute the microphone",
                "Mutes or unmutes it; which one is under General",
            ),
            (
                Action::DeskLamp,
                "Desk lamp on or off",
                "The ROG light bar's lamp; does nothing without one",
            ),
        ],
    ),
    (
        "Windows",
        &[
            (
                Action::ControlCenter,
                "Open the Control Center",
                "Profiles, fans, lighting and the rest",
            ),
            (Action::Settings, "Open Settings", "This window"),
            (
                Action::Status,
                "Open Status",
                "What's wrong, the logs and diagnostics",
            ),
        ],
    ),
    (
        "Recording",
        &[(
            Action::Recording,
            "Start or stop recording",
            "A session of every reading, kept in Session History",
        )],
    ),
];

/// The chord a key press makes: egui's key for the layout, or where it sits
/// when the layout's key has no name (AltGr turned it into a symbol).
fn captured_chord(
    key: egui::Key,
    physical_key: Option<egui::Key>,
    modifiers: egui::Modifiers,
) -> Option<Shortcut> {
    let vk = crate::hotkey::vk_for(key).or_else(|| physical_key.and_then(crate::hotkey::vk_for))?;
    Some(Shortcut::new(
        Mods {
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
        },
        vk,
    ))
}

/// Ctrl, Shift, Alt or the Windows key on its own: part of a chord, never
/// its key.
fn is_modifier_key(key: egui::Key) -> bool {
    use egui::Key as K;
    matches!(
        key,
        K::ShiftLeft
            | K::ShiftRight
            | K::ControlLeft
            | K::ControlRight
            | K::AltLeft
            | K::AltRight
            | K::SuperLeft
            | K::SuperRight
    )
}

/// Why `chord` can't be `action`'s, in words; `None` when it can.
/// `alt_gr_char` is `hotkey::alt_gr_char` (Windows asks the keyboard
/// layouts), a stand-in in the tests.
fn shortcut_rejection(
    shortcuts: &Shortcuts,
    action: Action,
    chord: Shortcut,
    alt_gr_char: impl Fn(Shortcut) -> Option<char>,
) -> Option<String> {
    match shortcuts.check(action, chord) {
        Some(Rejected::TooFewModifiers) => {
            return Some(
                "Hold two of Ctrl, Alt and Shift with the key, so the shortcut doesn't take a \
                 key other programs use."
                    .to_owned(),
            )
        }
        Some(Rejected::InUse(other)) => return Some(in_use(chord, other)),
        None => {}
    }
    let typed = alt_gr_char(chord)?;
    let key = shortcut::key_name(chord.vk).unwrap_or_default();
    Some(format!(
        "On your keyboard AltGr+{key} types “{typed}”, and {chord} would stop that. Pick another \
         key — an F-key never types one."
    ))
}

fn in_use(chord: Shortcut, other: Action) -> String {
    format!("{chord} already {}.", other.purpose())
}

/// Reads the keys for `action` while Settings waits for them: Esc cancels,
/// a chord that passes the rules is taken, one that doesn't says why and
/// keeps waiting.
fn capture_keys(
    ui: &egui::Ui,
    shortcuts: &mut Shortcuts,
    action: Action,
    capture: &mut Option<Action>,
    note: &mut Option<(Action, String)>,
) {
    // Keys pressed in another window never come here: stop waiting.
    if ui.input(|i| i.viewport().focused) == Some(false) {
        *capture = None;
        return;
    }
    let presses: Vec<_> = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *physical_key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    for (key, physical_key, modifiers) in presses {
        // egui (0.37) reports the modifiers' own presses too: holding Ctrl
        // before the key is no wrong key.
        if is_modifier_key(key) {
            continue;
        }
        if key == egui::Key::Escape && modifiers.is_none() {
            *capture = None;
            *note = None;
            return;
        }
        let Some(chord) = captured_chord(key, physical_key, modifiers) else {
            *note = Some((
                action,
                "Use a letter, a digit, an F-key, Space, an arrow, Insert, Delete, Home, End, \
                 Page Up or Page Down."
                    .to_owned(),
            ));
            continue;
        };
        #[cfg(windows)]
        let alt_gr = crate::hotkey::alt_gr_char;
        #[cfg(not(windows))]
        let alt_gr = |_| None;
        match shortcut_rejection(shortcuts, action, chord, alt_gr) {
            Some(why) => *note = Some((action, why)),
            None => {
                shortcuts.set(action, Some(chord));
                *capture = None;
                *note = None;
                return;
            }
        }
    }
}

/// The drop-down of a single shortcut: a new one (pressed), its default, or
/// off.
fn shortcut_dropdown(
    ui: &mut egui::Ui,
    shortcuts: &mut Shortcuts,
    action: Action,
    capture: &mut Option<Action>,
    note: &mut Option<(Action, String)>,
) {
    let current = shortcuts.get(action);
    let default = Shortcuts::default().get(action);
    let shown = if *capture == Some(action) {
        "Press the keys…".to_owned()
    } else {
        current.map_or_else(|| "Off".to_owned(), |c| c.to_string())
    };
    ui_kit::dropdown(ui, ("shortcut", action as u8), shown, |ui| {
        if ui.selectable_label(false, "New shortcut…").clicked() {
            *capture = Some(action);
            *note = None;
        }
        if let Some(default) = default {
            let label = format!("Default ({default})");
            if ui
                .selectable_label(current == Some(default), label)
                .clicked()
            {
                *capture = None;
                // Another action may use the default now.
                *note = match shortcuts.check(action, default) {
                    Some(Rejected::InUse(other)) => Some((action, in_use(default, other))),
                    _ => None,
                };
                if note.is_none() {
                    shortcuts.set(action, Some(default));
                }
            }
        }
        if ui.selectable_label(current.is_none(), "Off").clicked() {
            *capture = None;
            *note = None;
            shortcuts.set(action, None);
        }
    });
}

/// The profile shortcuts' drop-down: F1…F9 with one of a few modifier sets,
/// or off.
fn profiles_dropdown(
    ui: &mut egui::Ui,
    shortcuts: &mut Shortcuts,
    capture: &mut Option<Action>,
    note: &mut Option<(Action, String)>,
) {
    let shown = shortcuts
        .profiles
        .map_or_else(|| "Off".to_owned(), Shortcuts::profiles_label);
    ui_kit::dropdown(ui, "shortcut_profiles", shown, |ui| {
        for mods in shortcut::PROFILE_MODS {
            let label = Shortcuts::profiles_label(mods);
            if ui
                .selectable_label(shortcuts.profiles == Some(mods), label.as_str())
                .clicked()
            {
                *capture = None;
                *note = match shortcuts.check_profiles(mods) {
                    Some(Rejected::InUse(other)) => Some((
                        Action::Profiles,
                        format!(
                            "{label} would take a shortcut that already {}.",
                            other.purpose()
                        ),
                    )),
                    _ => None,
                };
                if note.is_none() {
                    shortcuts.profiles = Some(mods);
                }
            }
        }
        if ui
            .selectable_label(shortcuts.profiles.is_none(), "Off")
            .clicked()
        {
            *capture = None;
            *note = None;
            shortcuts.profiles = None;
        }
    });
}

/// The chords a row registers: one, or F1…F9 for the profiles.
fn row_chords(shortcuts: &Shortcuts, action: Action) -> Vec<Shortcut> {
    match action {
        Action::Profiles => (0..shortcut::PROFILE_SHORTCUTS)
            .filter_map(|i| shortcuts.profile(i))
            .collect(),
        _ => shortcuts.get(action).into_iter().collect(),
    }
}

fn shortcuts_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    shortcuts: &mut Shortcuts,
    capture: &mut Option<Action>,
    note: &mut Option<(Action, String)>,
) {
    ui_kit::page_header(
        ui,
        dc,
        "Shortcuts",
        "Keys that work in every program, games too, whichever profile is on. The overlay, the profiles and the microphone have keys from the start; the rest are off until you choose some.",
    );
    if let Some(action) = *capture {
        capture_keys(ui, shortcuts, action, capture, note);
    }
    #[cfg(windows)]
    let taken = crate::hotkey::taken();
    #[cfg(not(windows))]
    let taken: Vec<(String, String)> = Vec::new();
    for &(heading, rows) in SHORTCUT_GROUPS {
        ui_kit::group(ui, dc, Some(heading), None, |g| {
            shortcut_rows(g, dc, rows, shortcuts, capture, note, &taken);
        });
    }
    ui_kit::footnote(
        ui,
        dc,
        "Choose New shortcut…, then press the keys: two of Ctrl, Alt and Shift with a letter, digit or F-key. Esc cancels. A shortcut another program already uses never reaches RIGStats.",
    );
}

/// A row per action: its dropdown, then what's wrong with it, if anything.
fn shortcut_rows(
    g: &mut ui_kit::Group<'_>,
    dc: &DialogColors,
    rows: &[(Action, &str, &str)],
    shortcuts: &mut Shortcuts,
    capture: &mut Option<Action>,
    note: &mut Option<(Action, String)>,
    taken: &[(String, String)],
) {
    for &(action, title, subtitle) in rows {
        g.row(title, Some(subtitle), |ui| match action {
            Action::Profiles => profiles_dropdown(ui, shortcuts, capture, note),
            _ => shortcut_dropdown(ui, shortcuts, action, capture, note),
        });
        // This row's chords another program holds.
        let held: Vec<String> = row_chords(shortcuts, action)
            .iter()
            .map(Shortcut::to_string)
            .filter(|name| taken.iter().any(|(t, _)| t == name))
            .collect();
        if *capture == Some(action) && note.is_none() {
            g.block(|ui| {
                ui_kit::footnote(
                    ui,
                    dc,
                    "Press the new shortcut now — Esc cancels. If nothing happens, another program already uses those keys.",
                );
            });
        } else if let Some((_, why)) = note.as_ref().filter(|(a, _)| *a == action) {
            g.block(|ui| {
                ui_kit::row_problem(ui, dc, Severity::Attention, why);
            });
        } else if !held.is_empty() && *capture != Some(action) {
            g.block(|ui| {
                ui_kit::row_problem(
                    ui,
                    dc,
                    Severity::Attention,
                    &crate::problems::hotkey_taken_in_row(&held),
                );
            });
        }
    }
}

fn send_test_notification() {
    let _ = crate::notify::send(
        "Test notification",
        "Notifications from RIGStats show like this.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_over_the_wallpaper_layer_is_not_the_wallpaper_running() {
        // Floating turned on from the tray while the layer is the wallpaper:
        // floating runs, and the Display page must treat it so.
        let mut s = settings::Settings::default();
        s.window_layer = "wallpaper".into();
        assert!(wallpaper_running(&s));
        s.floating_mode = true;
        assert!(!wallpaper_running(&s));
        s.window_layer = "normal".into();
        s.floating_mode = false;
        assert!(!wallpaper_running(&s));
    }

    #[test]
    fn writing_the_draft_back_keeps_what_this_window_does_not_edit() {
        // The live settings: wallpaper layer, click-through on, a look the
        // Control Center set, and a profile's stored look.
        let mut live = settings::Settings::default();
        live.window_layer = "wallpaper".into();
        live.overlay_click_through = true;
        live.theme = "red".into();
        live.overlay_enabled = true;
        settings::switch_profile_look(&mut live, "gaming");
        // Edited in the Control Center while this window was open.
        live.app_rules = vec![rigstats_backend::profile_rules::AppRule {
            exe: "cs2.exe".into(),
            profile_id: "gaming".into(),
            foreground_only: false,
            path: None,
            name: None,
        }];
        let own = rigstats_backend::desktop_background::DesktopBackground {
            path: r"C:own.jpg".into(),
            fit: Default::default(),
            screens: Vec::new(),
        };
        live.desktop_background = Some(own.clone());
        live.desktop_background_before = Some(own);
        live.wallpaper_engine_before = Some(vec![rigstats_backend::wallpaper_engine::WeSlot {
            monitor: 0,
            file: r"D:\we\project.json".into(),
        }]);
        // A draft from when the dialog opened, with one change of its own.
        let mut draft = settings::Settings::default();
        draft.model_name = "My rig".into();

        let written = live_preview(&draft, &live);
        assert_eq!(written.model_name, "My rig", "the dialog's own change");
        assert_eq!(written.window_layer, "wallpaper", "layer waits for Save");
        assert!(written.overlay_click_through);
        assert_eq!(written.theme, "red", "the look belongs to the profile");
        assert!(written.overlay_enabled);
        assert_eq!(written.look_profile.as_deref(), Some("gaming"));
        assert!(written.profile_looks.contains_key("gaming"));
        assert_eq!(
            written.app_rules, live.app_rules,
            "rules aren't edited here"
        );
        assert_eq!(written.desktop_background, live.desktop_background);
        assert_eq!(
            written.desktop_background_before, live.desktop_background_before,
            "Windows' own background, kept by the app"
        );
        assert_eq!(
            written.wallpaper_engine_before,
            live.wallpaper_engine_before
        );
    }

    /// Every action has exactly one row on the Shortcuts page (#594): one
    /// added to `Shortcuts` but not to the page could never be chosen.
    #[test]
    fn every_shortcut_has_one_row() {
        use rigstats_backend::shortcut::SINGLE_ACTIONS;
        let rows: Vec<Action> = SHORTCUT_GROUPS
            .iter()
            .flat_map(|(_, rows)| rows.iter())
            .map(|&(a, _, _)| a)
            .collect();
        for action in SINGLE_ACTIONS.into_iter().chain([Action::Profiles]) {
            assert_eq!(
                rows.iter().filter(|&&a| a == action).count(),
                1,
                "{action:?}"
            );
        }
        assert_eq!(rows.len(), SINGLE_ACTIONS.len() + 1);
    }

    /// Holding Ctrl or Shift before the key sends those presses first
    /// (egui 0.37); they must not read as a wrong key (seen live, #569).
    #[test]
    fn a_modifier_on_its_own_is_not_a_key() {
        assert!(is_modifier_key(egui::Key::ControlLeft));
        assert!(is_modifier_key(egui::Key::ShiftLeft));
        assert!(is_modifier_key(egui::Key::AltRight));
        assert!(!is_modifier_key(egui::Key::M));
        assert!(!is_modifier_key(egui::Key::F10));
    }

    #[test]
    fn a_pressed_chord_is_taken_only_when_it_passes_the_rules() {
        let shortcuts = Shortcuts::default();
        let no_alt_gr = |_: Shortcut| None;
        let chord = |t| Shortcut::parse(t).unwrap();
        assert_eq!(
            shortcut_rejection(
                &shortcuts,
                Action::Microphone,
                chord("Ctrl+Shift+M"),
                no_alt_gr
            ),
            None
        );
        assert_eq!(
            shortcut_rejection(
                &shortcuts,
                Action::Microphone,
                chord("Ctrl+Alt+O"),
                no_alt_gr
            )
            .as_deref(),
            Some("Ctrl+Alt+O already shows or hides the overlay.")
        );
        assert!(
            shortcut_rejection(&shortcuts, Action::Overlay, chord("Alt+O"), no_alt_gr)
                .unwrap()
                .starts_with("Hold two of Ctrl, Alt and Shift")
        );
        // A layout where AltGr+E types the euro sign (Swedish, German).
        let euro = |c: Shortcut| (c.mods.is_alt_gr() && c.vk == 0x45).then_some('€');
        assert_eq!(
            shortcut_rejection(&shortcuts, Action::Microphone, chord("Ctrl+Alt+E"), euro)
                .as_deref(),
            Some(
                "On your keyboard AltGr+E types “€”, and Ctrl+Alt+E would stop that. Pick \
                 another key — an F-key never types one."
            )
        );
    }

    #[test]
    fn a_key_alt_gr_turned_into_a_symbol_is_read_where_it_sits() {
        let ctrl_alt = egui::Modifiers {
            ctrl: true,
            alt: true,
            command: true,
            ..Default::default()
        };
        // AltGr+7 is { on a Swedish layout: egui names the symbol, which no
        // chord can end in, so the key is read where it sits.
        let chord = captured_chord(egui::Key::OpenCurlyBracket, Some(egui::Key::Num7), ctrl_alt);
        assert_eq!(chord.map(|c| c.to_string()).as_deref(), Some("Ctrl+Alt+7"));
        let chord = captured_chord(egui::Key::M, Some(egui::Key::M), ctrl_alt);
        assert_eq!(chord.map(|c| c.to_string()).as_deref(), Some("Ctrl+Alt+M"));
        let chord = captured_chord(egui::Key::F7, Some(egui::Key::F7), ctrl_alt);
        assert_eq!(chord.map(|c| c.to_string()).as_deref(), Some("Ctrl+Alt+F7"));
        assert_eq!(captured_chord(egui::Key::Tab, None, ctrl_alt), None);
    }

    #[test]
    fn the_microphone_footnote_names_its_shortcut_or_says_it_is_off() {
        assert!(microphone_footnote(Shortcut::parse("Ctrl+Shift+M"))
            .starts_with("Ctrl+Shift+M (under Shortcuts), the tray menu"));
        assert!(microphone_footnote(None).contains("its shortcut is off under Shortcuts"));
    }
}
