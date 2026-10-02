use crate::lock_ext::LockSafe;
use crate::overlay::{OverlayMetric, ALL_OVERLAY_METRICS};
use crate::theme::{self, DialogColors};
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
    ("gpu_processes", "GPU Apps"),
    ("power", "System Power"),
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

// Semantic alert colours — independent of light/dark mode.
const C_WARN: egui::Color32 = egui::Color32::from_rgb(255, 180, 30);
const C_CRIT: egui::Color32 = egui::Color32::from_rgb(220, 60, 60);

// ── State ─────────────────────────────────────────────────────────────────────

pub struct SettingsWindow {
    pub draft: settings::Settings,
    pub original: settings::Settings,
    last_preview: settings::Settings,
    pub tab: usize,
    pub error: Option<String>,
    /// Whether the system has a battery. Detected once at startup on a
    /// background thread (see `RigStatsApp::new`) and read live here, so
    /// opening the dialog never blocks on WMI.
    battery_present: Arc<AtomicBool>,
    /// Detected GPU adapter names for the Display tab's GPU selector.
    gpu_names: Vec<String>,
}

impl SettingsWindow {
    pub fn from_settings(
        s: &settings::Settings,
        gpu_names: Vec<String>,
        battery_present: Arc<AtomicBool>,
    ) -> Self {
        let mut draft = s.clone();
        draft.autostart_enabled = autostart::is_run_key_present();
        Self {
            original: draft.clone(),
            last_preview: draft.clone(),
            draft,
            tab: 0,
            error: None,
            battery_present,
            gpu_names,
        }
    }

    /// Supplies the GPU list once background adapter detection finishes
    /// (it may still be running when the dialog is first created).
    pub fn set_gpu_names(&mut self, names: Vec<String>) {
        self.gpu_names = names;
    }
}

// ── Frames ────────────────────────────────────────────────────────────────────

fn dialog_frame(dc: &DialogColors) -> egui::Frame {
    egui::Frame::new()
        .fill(dc.bg)
        .inner_margin(egui::Margin::same(0))
}

fn card_frame(dc: &DialogColors) -> egui::Frame {
    egui::Frame::new()
        .fill(dc.card)
        .stroke(egui::Stroke::new(1.0_f32, dc.card_border))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(12, 8))
}

fn inner_row(dc: &DialogColors) -> egui::Frame {
    egui::Frame::new()
        .fill(dc.inner)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(10, 6))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Amber banner shown across all Settings tabs while the *applied* window layer is
/// Desktop Wallpaper: the visible dashboard is the host process rendering from disk,
/// so no change previews live — everything takes effect on Save.
fn wallpaper_save_banner(ui: &mut egui::Ui, dc: &DialogColors) {
    const AMBER: egui::Color32 = egui::Color32::from_rgb(0xE5, 0xC0, 0x7B);
    egui::Frame::new()
        .fill(dc.card)
        .stroke(egui::Stroke::new(1.0_f32, AMBER.gamma_multiply(0.5)))
        .corner_radius(egui::CornerRadius::same(5))
        .inner_margin(egui::Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(egui::RichText::new("⚠").size(12.0).color(AMBER));
                ui.label(
                    egui::RichText::new(
                        "Desktop Wallpaper mode — changes apply when you click Save (no live \
                         preview). Switch to another layer to change the Display Profile.",
                    )
                    .size(10.5)
                    .color(dc.text),
                );
            });
        });
}

fn section_label(ui: &mut egui::Ui, dc: &DialogColors, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(11.0)
            .strong()
            .color(dc.label),
    );
}

/// Cyan toggle switch.
fn toggle_switch(ui: &mut egui::Ui, dc: &DialogColors, on: &mut bool) -> egui::Response {
    let desired = egui::vec2(44.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(desired, egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        // Dim the switch when the surrounding UI is disabled so it reads as grayed out.
        let dim = if ui.is_enabled() { 1.0 } else { 0.4 };
        let bg = if *on { dc.toggle_on } else { dc.toggle_off }.gamma_multiply(dim);
        let r = (rect.height() / 2.0) as u8;
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(r), bg);
        let cx = if *on {
            rect.right() - rect.height() / 2.0
        } else {
            rect.left() + rect.height() / 2.0
        };
        ui.painter().circle_filled(
            egui::pos2(cx, rect.center().y),
            rect.height() / 2.0 - 3.0,
            egui::Color32::WHITE.gamma_multiply(dim),
        );
    }
    resp
}

/// Draw three horizontal lines as a drag-handle icon (font-independent).
fn drag_handle(ui: &mut egui::Ui, dc: &DialogColors) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 20.0), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let cx = rect.center().x;
        let stroke = egui::Stroke::new(1.5_f32, dc.muted);
        for dy in [-4.0_f32, 0.0, 4.0] {
            let y = rect.center().y + dy;
            ui.painter()
                .line_segment([egui::pos2(cx - 5.0, y), egui::pos2(cx + 5.0, y)], stroke);
        }
    }
}

/// Styled tab button. Returns true when clicked. `width` lets the caller size
/// all tabs equally to fill the available bar width (so 5 tabs always fit).
pub(crate) fn tab_btn(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    label: &str,
    active: bool,
    width: f32,
) -> bool {
    let mut clicked = false;
    ui.scope(|ui| {
        let cr = egui::CornerRadius::same(5);
        // egui 0.34 buttons paint `weak_bg_fill`; `bg_fill` alone never shows.
        let w = &mut ui.visuals_mut().widgets;
        if active {
            w.inactive.weak_bg_fill = dc.tab_active;
            w.hovered.weak_bg_fill = dc.tab_active;
        } else {
            w.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
            w.hovered.weak_bg_fill = dc.card;
        }
        if active {
            ui.visuals_mut().widgets.inactive.bg_fill = dc.tab_active;
            ui.visuals_mut().widgets.inactive.fg_stroke =
                egui::Stroke::new(1.0_f32, dc.tab_active_text);
            ui.visuals_mut().widgets.inactive.corner_radius = cr;
            ui.visuals_mut().widgets.hovered.bg_fill = dc.tab_active;
            ui.visuals_mut().widgets.hovered.fg_stroke =
                egui::Stroke::new(1.0_f32, dc.tab_active_text);
            ui.visuals_mut().widgets.hovered.corner_radius = cr;
            ui.visuals_mut().widgets.active.corner_radius = cr;
        } else {
            ui.visuals_mut().widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
            ui.visuals_mut().widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0_f32, dc.card_border);
            ui.visuals_mut().widgets.inactive.corner_radius = cr;
            ui.visuals_mut().widgets.hovered.bg_fill = dc.card;
            ui.visuals_mut().widgets.hovered.corner_radius = cr;
            ui.visuals_mut().widgets.active.corner_radius = cr;
        }
        let text_col = if active { dc.tab_active_text } else { dc.text };
        if ui
            .add_sized(
                [width, 28.0],
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
    dc: &DialogColors,
    dcomp_available: bool,
) {
    dc.apply_to_ctx(ctx);
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    let mut action_save = false;
    let mut action_cancel = false;

    // ── Hero ──────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::top("settings_hero")
        .frame(dialog_frame(dc).inner_margin(egui::Margin {
            left: 16,
            right: 16,
            top: 14,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("Settings")
                    .size(22.0)
                    .strong()
                    .color(dc.title),
            );
        });

    // ── Footer ────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("settings_footer")
        .frame(dialog_frame(dc).inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 8,
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

    // ── Central: tab bar + scrollable content ─────────────────────────────────
    egui::CentralPanel::default()
        .frame(dialog_frame(dc))
        .show(ctx, |ui| {
            let mut st = state.lock_safe();

            // Tab bar
            egui::Frame::new()
                .fill(dc.bg)
                .inner_margin(egui::Margin {
                    left: 14,
                    right: 14,
                    top: 8,
                    bottom: 8,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let spacing = 4.0;
                        ui.spacing_mut().item_spacing.x = spacing;
                        const TAB_LABELS: [&str; 6] = [
                            "Display",
                            "Panels",
                            "Alerts",
                            "Appearance",
                            "General",
                            "Overlay",
                        ];
                        // Size every tab equally to fill the bar so all of them
                        // always fit (5 fixed-width tabs would overflow the dialog).
                        let n = TAB_LABELS.len() as f32;
                        let tab_w = ((ui.available_width() - spacing * (n - 1.0)) / n).floor();
                        for (i, lbl) in TAB_LABELS.iter().enumerate() {
                            if tab_btn(ui, dc, lbl, st.tab == i, tab_w) {
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
                        .fill(dc.bg)
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 8.0;
                            let tab = st.tab;
                            let battery_present = st.battery_present.load(Ordering::Relaxed);
                            // The currently *applied* window layer. Live preview forces
                            // window_layer back to whatever is actually running (it's
                            // Save-only), so until Save the app keeps running its current
                            // layer even if the draft selects another. Read from the live
                            // `saved` settings, not `st.original` (a dialog-open-time
                            // snapshot) — the layer can also change from outside this
                            // dialog while it's open (tray "Toggle Overlay Mode", the
                            // global hotkey), and `original` would then be stale, causing
                            // the live-preview push below to silently revert that external
                            // change back to whatever the layer was when the dialog opened.
                            // The Display tab grays its wallpaper-incompatible controls
                            // based on this (reality), not the draft, so switching away
                            // from wallpaper doesn't falsely enable controls before the
                            // switch actually takes effect.
                            let applied_layer = saved.lock_safe().window_layer.clone();
                            // While the *applied* layer is Desktop Wallpaper the visible
                            // dashboard is the separate host process rendering from disk,
                            // so nothing in Settings previews live — every change (profile,
                            // theme, panels, thresholds, layer) only shows after Save. A
                            // single banner states this once for all tabs.
                            if applied_layer == "wallpaper" {
                                wallpaper_save_banner(ui, dc);
                            }
                            let gpu_names = st.gpu_names.clone();
                            let draft = &mut st.draft;
                            match tab {
                                0 => draw_display(ui, dc, draft, &applied_layer, &gpu_names),
                                1 => draw_panels(ui, dc, draft, battery_present),
                                2 => draw_alerts(ui, dc, draft),
                                3 => draw_appearance(ui, dc, draft),
                                4 => draw_general(ui, dc, draft, dir.as_ref()),
                                5 => draw_overlay(
                                    ui,
                                    dc,
                                    draft,
                                    saved,
                                    reload,
                                    dir.as_ref(),
                                    dcomp_available,
                                    battery_present,
                                ),
                                _ => {}
                            }
                        });
                });
        });

    // ── Apply deferred actions ────────────────────────────────────────────────

    // Live preview: push draft to main app on every change (before save/cancel).
    // window_layer is the only excluded field — it's applied on Save only since
    // it's a window-level hint that doesn't need instant feedback.
    // Everything else (opacity, theme, panels, profile, floating_mode, etc.) previews live.
    {
        let mut st = state.lock_safe();
        if st.draft != st.last_preview {
            st.last_preview = st.draft.clone();
            let mut preview = st.draft.clone();
            // The live value, not `st.original` — see the `applied_layer` comment
            // above for why a stale dialog-open-time snapshot is wrong here.
            preview.window_layer = saved.lock_safe().window_layer.clone();
            // overlay_click_through and overlay_enabled both bypass the
            // draft/Save/Cancel flow entirely (applied and persisted
            // immediately by their own switches, the tray, or the hotkey) —
            // pushing the draft's stale copy here would clobber a change made
            // through one of those paths while this dialog is open, so
            // always carry the current live values forward.
            preview.overlay_click_through = saved.lock_safe().overlay_click_through;
            preview.overlay_enabled = saved.lock_safe().overlay_enabled;
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
        to_persist.overlay_click_through = saved.lock_safe().overlay_click_through;
        to_persist.overlay_enabled = saved.lock_safe().overlay_enabled;
        let save_result = settings::persist_settings(dir.as_ref(), &to_persist);
        match (save_result, autostart_result) {
            (Ok(()), Ok(())) => {
                // Push the full draft (including window_layer, profile, etc.) to main app.
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
    if action_cancel {
        // Revert live preview back to original — except window_layer,
        // overlay_click_through, and overlay_enabled, none of which was ever
        // part of the draft being cancelled: window_layer only previews live
        // via the *live* value (see the comments above), so cancelling must
        // preserve that same live value rather than snapping back to a stale
        // dialog-open-time snapshot (which would undo an external tray/hotkey
        // change the user never asked this dialog to revert).
        let st = state.lock_safe();
        let mut reverted = st.original.clone();
        reverted.window_layer = saved.lock_safe().window_layer.clone();
        reverted.overlay_click_through = saved.lock_safe().overlay_click_through;
        reverted.overlay_enabled = saved.lock_safe().overlay_enabled;
        *saved.lock_safe() = reverted;
        reload.store(true, Ordering::Relaxed);
        drop(st);
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
    if ctx.input(|i| i.viewport().close_requested()) {
        // Treat window-close as cancel (revert preview).
        let st = state.lock_safe();
        let mut reverted = st.original.clone();
        reverted.window_layer = saved.lock_safe().window_layer.clone();
        reverted.overlay_click_through = saved.lock_safe().overlay_click_through;
        reverted.overlay_enabled = saved.lock_safe().overlay_enabled;
        *saved.lock_safe() = reverted;
        reload.store(true, Ordering::Relaxed);
        drop(st);
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

// ── Display tab ───────────────────────────────────────────────────────────────
fn draw_display(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    draft: &mut settings::Settings,
    applied_layer: &str,
    gpu_names: &[String],
) {
    // Desktop Wallpaper mode is display-only: floating and fill-screen have no
    // effect there (the host is sized to the profile, centred on its monitor), so
    // those controls are disabled while wallpaper is the *applied* layer. We gate
    // on the applied layer (not the draft) because window_layer is Save-only — the
    // app stays in wallpaper mode until Save even after the draft selects Normal,
    // so enabling the controls before Save would be misleading.
    let is_wallpaper = applied_layer == "wallpaper";
    // Display Profile
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Display Profile");
        ui.add_space(8.0);
        let w = ui.available_width();
        if draft.floating_mode {
            let current = ALL_PROFILES
                .iter()
                .find(|(k, _)| *k == draft.dashboard_profile.as_str())
                .map(|(_, l)| *l)
                .unwrap_or(draft.dashboard_profile.as_str());
            egui::Frame::new()
                .fill(dc.inner)
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_min_width(w - 16.0);
                    ui.label(egui::RichText::new(current).size(13.0).color(dc.muted));
                });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Not used in floating mode")
                    .size(11.0)
                    .color(dc.muted),
            );
        } else if is_wallpaper {
            // Profile is locked while pinned to the wallpaper — switch to a
            // non-wallpaper layer to change it, then pin again.
            let current = ALL_PROFILES
                .iter()
                .find(|(k, _)| *k == draft.dashboard_profile.as_str())
                .map(|(_, l)| *l)
                .unwrap_or(draft.dashboard_profile.as_str());
            egui::Frame::new()
                .fill(dc.inner)
                .corner_radius(egui::CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.set_min_width(w - 16.0);
                    ui.label(egui::RichText::new(current).size(13.0).color(dc.muted));
                });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Switch to a non-wallpaper layer to change the profile")
                    .size(11.0)
                    .color(dc.muted),
            );
        } else {
            let current = ALL_PROFILES
                .iter()
                .find(|(k, _)| *k == draft.dashboard_profile.as_str())
                .map(|(_, l)| l.to_string())
                .unwrap_or_else(|| draft.dashboard_profile.clone());
            egui::ComboBox::from_id_salt("profile_combo")
                .selected_text(current)
                .width(w)
                .show_ui(ui, |ui| {
                    let mut shown_landscape_heading = false;
                    ui.label(egui::RichText::new("Portrait").small().color(dc.muted));
                    for &(key, label) in ALL_PROFILES {
                        if key.starts_with("landscape-") && !shown_landscape_heading {
                            shown_landscape_heading = true;
                            ui.separator();
                            ui.label(egui::RichText::new("Landscape").small().color(dc.muted));
                        }
                        ui.selectable_value(&mut draft.dashboard_profile, key.to_string(), label);
                    }
                });
        }
    });

    // Window — layer (Normal / On Top / Behind / Desktop Wallpaper) + opacity.
    // Kept on the same tab as Layout/Fill so the wallpaper-driven gray-out of
    // those controls is visible right below the layer that causes it.
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Window");
        ui.add_space(8.0);

        // Window Layer
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Window Layer")
                    .size(12.0)
                    .color(dc.muted),
            );
            ui.add_space(8.0);
            let layer_label = match draft.window_layer.as_str() {
                "on_top" => "Always on Top",
                "behind" => "Always Behind",
                "wallpaper" => "Desktop Wallpaper",
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
                    ui.selectable_value(
                        &mut draft.window_layer,
                        "wallpaper".to_string(),
                        "Desktop Wallpaper",
                    );
                });
        });
        if draft.window_layer == "wallpaper" {
            // Wallpaper mode and floating mode are mutually exclusive; selecting
            // wallpaper turns floating off (the runtime also lets floating win if
            // both are somehow set).
            draft.floating_mode = false;
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(
                    "Lives in the desktop wallpaper layer (survives Win+D), at the spot the \
                     window had before switching — position it in Normal mode first, then \
                     switch. Display-only — disables floating mode; changes apply on Save \
                     (no live preview).",
                )
                .size(10.0)
                .color(dc.muted),
            );
        }
        // Window-layer changes are Save-only: the app stays in the *applied* layer
        // (and grays the controls accordingly) until Save. Make that explicit so
        // the user doesn't expect the switch — or the re-enabled controls — to take
        // effect just from picking a different layer.
        if draft.window_layer != applied_layer {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(
                    egui::RichText::new("⚠")
                        .size(10.0)
                        .color(egui::Color32::from_rgb(0xE5, 0xC0, 0x7B)),
                );
                ui.label(
                    egui::RichText::new("Window Layer changes take effect when you click Save.")
                        .size(10.0)
                        .color(dc.link),
                );
            });
        }

        ui.add_space(6.0);
        ui.add(egui::Separator::default().spacing(4.0));
        ui.add_space(4.0);

        // Opacity — in Desktop Wallpaper mode this is applied per-pixel via a
        // DirectComposition-backed swap chain (`win_opacity::set_no_redirection_bitmap`
        // + `wgpu_options` in `bin/wallpaper.rs`) rather than WS_EX_LAYERED, which a
        // WorkerW child window rejects. Supported in every window layer.
        let pct = (draft.opacity * 100.0).round() as u32;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Opacity").size(12.0).color(dc.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{pct}%"))
                        .size(12.0)
                        .color(dc.text),
                );
                let mut opacity = draft.opacity as f32;
                let slider = egui::Slider::new(&mut opacity, 0.1_f32..=1.0_f32)
                    .step_by(0.05)
                    .show_value(false)
                    .trailing_fill(true);
                if ui.add(slider).changed() {
                    draft.opacity = opacity as f64;
                }
            });
        });
    });

    // Layout
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Layout");
        ui.add_space(8.0);
        inner_row(dc).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add_enabled_ui(!is_wallpaper, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Floating Mode")
                                .size(13.0)
                                .color(dc.text),
                        );
                        let desc = if is_wallpaper {
                            "Not available in Desktop Wallpaper mode"
                        } else {
                            "Each panel opens as a separate frameless window"
                        };
                        ui.label(egui::RichText::new(desc).size(11.0).color(dc.muted));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        toggle_switch(ui, dc, &mut draft.floating_mode);
                    });
                });
            });
        });

        if draft.floating_mode {
            ui.add_space(8.0);
            inner_row(dc).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Panel Scale")
                            .size(12.0)
                            .color(dc.muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{:.0}%",
                                draft.floating_panel_scale * 100.0
                            ))
                            .size(12.0)
                            .color(dc.text),
                        );
                        let scale = &mut draft.floating_panel_scale;
                        let slider = egui::Slider::new(scale, 0.4..=1.0)
                            .show_value(false)
                            .trailing_fill(true);
                        ui.add(slider);
                    });
                });
            });
        }

        // Fill Screen — applies to both the portrait stack and the landscape grid
        // in non-floating mode. Off, each shrinks/grows to the panels actually
        // shown; on, the window fills the whole monitor (the landscape grid
        // stretches to use it, the portrait stack keeps its size and centers or
        // top-aligns within it). Wallpaper mode always fills the monitor already,
        // so the toggle has no effect there.
        let is_landscape = crate::geometry::profile_is_landscape(&draft.dashboard_profile);
        let fill_enabled = !draft.floating_mode && !is_wallpaper;
        ui.add_space(8.0);
        inner_row(dc).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.add_enabled_ui(fill_enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Fill Screen").size(13.0).color(dc.text));
                        let desc = if is_wallpaper {
                            "Not available in Desktop Wallpaper mode"
                        } else {
                            "Cover the whole monitor; the dashboard background fills the rest"
                        };
                        ui.label(egui::RichText::new(desc).size(11.0).color(dc.muted));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        toggle_switch(ui, dc, &mut draft.fullscreen_mode);
                    });
                });
            });
        });

        // Alignment only applies to the portrait stack — the landscape grid
        // always stretches to fill the whole window, so there is no leftover
        // space to align within.
        if draft.fullscreen_mode && fill_enabled && !is_landscape {
            ui.add_space(8.0);
            inner_row(dc).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Alignment").size(12.0).color(dc.muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let current = if draft.fullscreen_align == "top" {
                            "Top"
                        } else {
                            "Center"
                        };
                        egui::ComboBox::from_id_salt("fullscreen_align")
                            .selected_text(current)
                            .width(140.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut draft.fullscreen_align,
                                    "center".to_string(),
                                    "Center",
                                );
                                ui.selectable_value(
                                    &mut draft.fullscreen_align,
                                    "top".to_string(),
                                    "Top",
                                );
                            });
                    });
                });
            });
        }
    });

    // GPU — which adapter the GPU panel and overlay show on multi-GPU systems.
    // Same choice as the tray "GPU" submenu and the GPU panel's click dots.
    if gpu_names.len() > 1 {
        card_frame(dc).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            section_label(ui, dc, "GPU");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Displayed GPU")
                        .size(12.0)
                        .color(dc.muted),
                );
                ui.add_space(8.0);
                const AUTO: &str = "Automatic (most VRAM)";
                // Same WMI/LHM-tolerant matching as the tray tick, so a
                // preference saved under a slightly different spelling
                // (e.g. from the GPU panel's click dots) still shows as the
                // right adapter instead of an unlisted raw string.
                let selected =
                    crate::tray::selected_gpu_index(gpu_names, draft.preferred_gpu.as_deref());
                let current = selected.map_or(AUTO, |i| gpu_names[i].as_str());
                let w = ui.available_width();
                egui::ComboBox::from_id_salt("preferred_gpu")
                    .selected_text(current)
                    .width(w)
                    .show_ui(ui, |ui| {
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
        });
    }
}

// ── General tab ───────────────────────────────────────────────────────────────
fn draw_general(ui: &mut egui::Ui, dc: &DialogColors, draft: &mut settings::Settings, dir: &Path) {
    // Launch at Startup
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Startup");
        ui.add_space(8.0);
        inner_row(dc).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Launch at Startup")
                            .size(13.0)
                            .color(dc.text),
                    );
                    ui.label(
                        egui::RichText::new("Starts with Windows · LHM connects automatically")
                            .size(11.0)
                            .color(dc.muted),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    toggle_switch(ui, dc, &mut draft.autostart_enabled);
                });
            });
        });
    });

    // Session Recording
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Session Recording");
        ui.add_space(8.0);

        ui.label(
            egui::RichText::new(
                "Start/stop a recording from the tray menu, then browse past sessions from \
                 the tray's \u{201c}Session History\u{201d} window.",
            )
            .size(11.0)
            .color(dc.muted),
        );

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Retain for").size(12.0).color(dc.muted));
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::dialog_btn_secondary(ui, "Open Log Folder", dc).clicked() {
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
            });
        });
    });
}

/// Small clickable reorder button with a painted triangle arrow.
fn arrow_btn(ui: &mut egui::Ui, dc: &DialogColors, down: bool) -> egui::Response {
    let size = egui::vec2(20.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let color = if resp.hovered() { dc.text } else { dc.muted };
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

fn draw_panels(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    draft: &mut settings::Settings,
    battery_present: bool,
) {
    ui.label(
        egui::RichText::new("Use arrows to reorder · toggle to show/hide")
            .size(11.0)
            .color(dc.muted),
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

    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        for (i, &(key, label, visible)) in ordered.iter().enumerate() {
            if i > 0 {
                ui.add(egui::Separator::default().spacing(1.0));
            }
            let unavailable = key == "battery" && !battery_present && cfg!(not(debug_assertions));
            let label_color = if unavailable { dc.muted } else { dc.text };

            ui.horizontal(|ui| {
                ui.add_space(6.0);
                drag_handle(ui, dc);
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
                                dc.toggle_off,
                            );
                            ui.painter().circle_filled(
                                egui::pos2(rect.left() + rect.height() / 2.0, rect.center().y),
                                rect.height() / 2.0 - 3.0,
                                dc.muted,
                            );
                        }
                    } else {
                        let mut v = visible;
                        if toggle_switch(ui, dc, &mut v).changed() {
                            toggle = Some((key.to_string(), v));
                        }
                    }
                    if visible && !unavailable {
                        let idx = draft
                            .visible_panels
                            .iter()
                            .position(|p| p == key)
                            .unwrap_or(0);
                        ui.add_space(6.0);
                        if idx + 1 < vis_count {
                            if arrow_btn(ui, dc, true).clicked() {
                                reorder = Some((key.to_string(), 1));
                            }
                        } else {
                            ui.add_space(20.0);
                        }
                        if idx > 0 {
                            if arrow_btn(ui, dc, false).clicked() {
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

    // System Power — PSU capacity
    ui.add_space(8.0);
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "System Power");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("PSU capacity")
                    .size(13.0)
                    .color(dc.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut enabled = draft.psu_watts.is_some();
                if toggle_switch(ui, dc, &mut enabled).changed() {
                    draft.psu_watts = if enabled { Some(650) } else { None };
                }
                ui.add_space(6.0);
                if let Some(ref mut w) = draft.psu_watts {
                    let mut val = *w as i32;
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
                } else {
                    ui.label(egui::RichText::new("auto").size(12.0).color(dc.muted));
                }
            });
        });
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("Scale the power bar to your PSU's rated wattage")
                .size(10.0)
                .color(dc.muted),
        );
    });

    if let Some((key, add)) = toggle {
        if add {
            if !draft.visible_panels.contains(&key) {
                // Re-insert at the natural ALL_PANELS position rather than appending.
                // Find the first panel that comes AFTER `key` in ALL_PANELS and is
                // already visible — insert before it.
                let all_idx = ALL_PANELS
                    .iter()
                    .position(|(k, _)| *k == key)
                    .unwrap_or(usize::MAX);
                let insert_at = draft.visible_panels.iter().position(|p| {
                    let pi = ALL_PANELS
                        .iter()
                        .position(|(k, _)| k == p)
                        .unwrap_or(usize::MAX);
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

fn draw_alerts(ui: &mut egui::Ui, dc: &DialogColors, draft: &mut settings::Settings) {
    // Notifications card
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Notifications");
        ui.add_space(8.0);

        inner_row(dc).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Notify on Critical")
                        .size(13.0)
                        .color(dc.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    toggle_switch(ui, dc, &mut draft.notify_on_crit);
                });
            });
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Cooldown").size(12.0).color(dc.muted));
            ui.add_space(8.0);
            let mut secs = draft.alert_cooldown_secs as i32;
            if ui
                .add(
                    egui::DragValue::new(&mut secs)
                        .range(60..=3600)
                        .suffix(" s"),
                )
                .changed()
            {
                draft.alert_cooldown_secs = secs.max(60) as u64;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::dialog_btn_secondary(ui, "Test Notification", dc).clicked() {
                    send_test_notification();
                }
            });
        });
    });

    // Thresholds card — compact flat table, units/direction inline in row labels
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Thresholds (blank to disable)");
        ui.add_space(6.0);

        // Shared Warn / Crit header
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_sized(
                    [44.0, 16.0],
                    egui::Label::new(
                        egui::RichText::new("Crit")
                            .size(11.0)
                            .strong()
                            .color(C_CRIT),
                    ),
                );
                ui.add_space(4.0);
                ui.add_sized(
                    [44.0, 16.0],
                    egui::Label::new(
                        egui::RichText::new("Warn")
                            .size(11.0)
                            .strong()
                            .color(C_WARN),
                    ),
                );
            });
        });

        // Temperature/load rows — unit inline as muted suffix
        for &(key, label, unit) in &[
            ("cpu", "CPU", "°C"),
            ("gpu", "GPU", "°C"),
            ("cpu_load", "CPU Load", "%"),
            ("gpu_load", "GPU Load", "%"),
            ("ram", "RAM", "°C"),
            ("ram_load", "RAM Usage", "%"),
            ("disk", "Disk", "°C"),
            ("disk_usage", "Disk Usage", "%"),
        ] {
            let e = draft.thresholds.entry(key.to_string()).or_insert_with(|| {
                settings::default_thresholds()
                    .remove(key)
                    .unwrap_or_default()
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).size(13.0).color(dc.text));
                ui.label(egui::RichText::new(unit).size(11.0).color(dc.muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    threshold_field(ui, &mut e.crit);
                    ui.add_space(4.0);
                    threshold_field(ui, &mut e.warn);
                });
            });
        }

        // Thin divider + battery description
        ui.add_space(3.0);
        ui.add(egui::Separator::default().spacing(1.0));
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "Charge: alert when % drops below · Power: alert when W exceeds (discharge only)",
            )
            .size(10.0)
            .color(dc.muted),
        );
        ui.add_space(4.0);

        // Battery charge % — fires when charge drops BELOW threshold
        let bat = draft
            .thresholds
            .entry("battery".to_string())
            .or_insert_with(|| {
                settings::default_thresholds()
                    .remove("battery")
                    .unwrap_or_default()
            });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Charge %").size(13.0).color(dc.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                threshold_field(ui, &mut bat.crit);
                ui.add_space(4.0);
                threshold_field(ui, &mut bat.warn);
            });
        });

        // Battery power W — fires when draw exceeds threshold (discharge only)
        let bat_pwr = draft
            .thresholds
            .entry("battery_power".to_string())
            .or_insert_with(|| {
                settings::default_thresholds()
                    .remove("battery_power")
                    .unwrap_or_default()
            });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Power W").size(13.0).color(dc.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                threshold_field(ui, &mut bat_pwr.crit);
                ui.add_space(4.0);
                threshold_field(ui, &mut bat_pwr.warn);
            });
        });
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

// ── Appearance tab ────────────────────────────────────────────────────────────

fn draw_appearance(ui: &mut egui::Ui, dc: &DialogColors, draft: &mut settings::Settings) {
    // Override Model Name card
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Override Model Name");
        ui.add_space(8.0);
        let w = ui.available_width();
        ui.add_sized(
            [w, 24.0],
            egui::TextEdit::singleline(&mut draft.model_name).text_color(dc.text),
        );
    });

    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Theme");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Theme").color(dc.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let current = draft.theme.clone();
                egui::ComboBox::from_id_salt("theme_select")
                    .selected_text(current.as_str())
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for key in theme::THEME_KEYS {
                            ui.selectable_value(&mut draft.theme, key.to_string(), *key);
                        }
                    });
            });
        });
    });
}

// ── Overlay tab ───────────────────────────────────────────────────────────────

/// `saved`/`reload`/`dir` are needed for the Enable and Click-Through
/// switches, which (unlike every other field here) bypass the draft/Save/
/// Cancel flow entirely — see the top and bottom cards of this function. The
/// overlay is an independent add-on window (not a `window_layer` value), so
/// it can be toggled from the tray or the global hotkey at any time,
/// including while this dialog happens to be open — bypassing the draft
/// keeps that external toggle from being silently reverted the next time any
/// other field on this tab changes.
#[allow(clippy::too_many_arguments)]
fn draw_overlay(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    draft: &mut settings::Settings,
    saved: &Arc<Mutex<settings::Settings>>,
    reload: &Arc<AtomicBool>,
    dir: &Path,
    dcomp_available: bool,
    battery_present: bool,
) {
    // Enable — bypasses the draft/Save/Cancel flow entirely: applies and
    // persists immediately, same as the tray "Toggle Overlay Mode" row and
    // the global hotkey.
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Overlay");
        ui.add_space(8.0);
        let mut enabled = saved.lock_safe().overlay_enabled;
        inner_row(dc).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Show Overlay")
                            .size(13.0)
                            .color(dc.text),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Compact metric strip shown over everything else, independent \
                             of Window Layer/Floating Mode. Applies immediately — not \
                             affected by Cancel.",
                        )
                        .size(11.0)
                        .color(dc.muted),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if toggle_switch(ui, dc, &mut enabled).changed() {
                        let mut s = saved.lock_safe();
                        s.overlay_enabled = enabled;
                        let s_clone = s.clone();
                        drop(s);
                        if let Err(e) = settings::persist_settings(dir, &s_clone) {
                            debug::append_debug_log(
                                dir,
                                &format!("settings: overlay enabled persist failed — {e}"),
                            );
                        }
                        reload.store(true, Ordering::Relaxed);
                    }
                });
            });
        });
    });

    // Metrics — enabled ones first (in display order, reorderable via
    // arrows), disabled ones after in registry order. Mirrors draw_panels'
    // pattern for the main dashboard's panel list — `draw_overlay` renders
    // metrics in `overlay_metrics`'s order, so this list *is* the sort order.
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Metrics");
        ui.label(
            egui::RichText::new("Use arrows to reorder · toggle to show/hide")
                .size(11.0)
                .color(dc.muted),
        );
        ui.add_space(8.0);

        let mut ordered: Vec<&OverlayMetric> = Vec::new();
        for key in &draft.overlay_metrics {
            if let Some(m) = ALL_OVERLAY_METRICS.iter().find(|m| m.key == key.as_str()) {
                ordered.push(m);
            }
        }
        for m in ALL_OVERLAY_METRICS {
            if !draft.overlay_metrics.iter().any(|k| k.as_str() == m.key) {
                ordered.push(m);
            }
        }

        let enabled_count = draft.overlay_metrics.len();
        let mut toggle: Option<(&'static str, bool)> = None;
        let mut reorder: Option<(&'static str, i32)> = None;

        for m in &ordered {
            let on = draft.overlay_metrics.iter().any(|k| k.as_str() == m.key);
            // Mirrors draw_panels' battery row: no point offering a metric
            // this rig can't produce. Disabled in debug builds so it can
            // still be exercised without a physical battery.
            let unavailable =
                m.key == "battery_pct" && !battery_present && cfg!(not(debug_assertions));
            let label_color = if unavailable { dc.muted } else { dc.text };
            inner_row(dc).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} ({})", m.label, m.unit.trim()))
                            .size(13.0)
                            .color(label_color),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if unavailable {
                            // Static grayed-out toggle — no interaction.
                            let desired = egui::vec2(44.0, 24.0);
                            let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
                            if ui.is_rect_visible(rect) {
                                let r = (rect.height() / 2.0) as u8;
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(r),
                                    dc.toggle_off,
                                );
                                ui.painter().circle_filled(
                                    egui::pos2(rect.left() + rect.height() / 2.0, rect.center().y),
                                    rect.height() / 2.0 - 3.0,
                                    dc.muted,
                                );
                            }
                        } else {
                            let mut v = on;
                            if toggle_switch(ui, dc, &mut v).changed() {
                                toggle = Some((m.key, v));
                            }
                        }
                        if on && !unavailable {
                            let idx = draft
                                .overlay_metrics
                                .iter()
                                .position(|k| k.as_str() == m.key)
                                .unwrap_or(0);
                            ui.add_space(6.0);
                            if idx + 1 < enabled_count {
                                if arrow_btn(ui, dc, true).clicked() {
                                    reorder = Some((m.key, 1));
                                }
                            } else {
                                ui.add_space(20.0);
                            }
                            if idx > 0 {
                                if arrow_btn(ui, dc, false).clicked() {
                                    reorder = Some((m.key, -1));
                                }
                            } else {
                                ui.add_space(20.0);
                            }
                        } else if on {
                            ui.add_space(46.0);
                        }
                    });
                });
            });
            ui.add_space(4.0);
        }

        if let Some((key, add)) = toggle {
            if add {
                if !draft.overlay_metrics.iter().any(|k| k.as_str() == key) {
                    draft.overlay_metrics.push(key.to_string());
                }
            } else {
                draft.overlay_metrics.retain(|k| k.as_str() != key);
            }
        }
        if let Some((key, d)) = reorder {
            if let Some(idx) = draft.overlay_metrics.iter().position(|k| k.as_str() == key) {
                let new_idx = if d < 0 {
                    idx.saturating_sub(1)
                } else {
                    (idx + 1).min(draft.overlay_metrics.len().saturating_sub(1))
                };
                draft.overlay_metrics.swap(idx, new_idx);
            }
        }
    });

    // Layout & anchor
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Layout");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Arrangement")
                    .size(12.0)
                    .color(dc.muted),
            );
            ui.add_space(8.0);
            let label = match draft.overlay_layout.as_str() {
                "vertical" => "Vertical",
                "grid" => "Grid",
                _ => "Horizontal",
            };
            let w = ui.available_width();
            egui::ComboBox::from_id_salt("overlay_layout")
                .selected_text(label)
                .width(w)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut draft.overlay_layout,
                        "horizontal".to_string(),
                        "Horizontal",
                    );
                    ui.selectable_value(
                        &mut draft.overlay_layout,
                        "vertical".to_string(),
                        "Vertical",
                    );
                    ui.selectable_value(&mut draft.overlay_layout, "grid".to_string(), "Grid");
                });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Anchor").size(12.0).color(dc.muted));
            ui.add_space(8.0);
            let label = match draft.overlay_anchor.as_str() {
                "top-left" => "Top-Left",
                "bottom-left" => "Bottom-Left",
                "bottom-right" => "Bottom-Right",
                "free" => "Free (drag to position)",
                _ => "Top-Right",
            };
            let w = ui.available_width();
            egui::ComboBox::from_id_salt("overlay_anchor")
                .selected_text(label)
                .width(w)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut draft.overlay_anchor,
                        "top-left".to_string(),
                        "Top-Left",
                    );
                    ui.selectable_value(
                        &mut draft.overlay_anchor,
                        "top-right".to_string(),
                        "Top-Right",
                    );
                    ui.selectable_value(
                        &mut draft.overlay_anchor,
                        "bottom-left".to_string(),
                        "Bottom-Left",
                    );
                    ui.selectable_value(
                        &mut draft.overlay_anchor,
                        "bottom-right".to_string(),
                        "Bottom-Right",
                    );
                    ui.selectable_value(
                        &mut draft.overlay_anchor,
                        "free".to_string(),
                        "Free (drag to position)",
                    );
                });
        });
        if draft.overlay_anchor != "free" {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Margin").size(12.0).color(dc.muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} px", draft.overlay_margin))
                            .size(12.0)
                            .color(dc.text),
                    );
                    let mut margin = draft.overlay_margin as f32;
                    let slider = egui::Slider::new(&mut margin, 0.0_f32..=64.0_f32)
                        .show_value(false)
                        .trailing_fill(true);
                    if ui.add(slider).changed() {
                        draft.overlay_margin = margin.round() as i32;
                    }
                });
            });
        }
    });

    // Appearance
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Appearance");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Scale").size(12.0).color(dc.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{:.0}%", draft.overlay_scale * 100.0))
                        .size(12.0)
                        .color(dc.text),
                );
                let mut scale = draft.overlay_scale as f32;
                let slider = egui::Slider::new(&mut scale, 0.5_f32..=2.0_f32)
                    .step_by(0.05)
                    .show_value(false)
                    .trailing_fill(true);
                if ui.add(slider).changed() {
                    draft.overlay_scale = scale as f64;
                }
            });
        });
        ui.add_space(6.0);
        let pct = (draft.overlay_opacity * 100.0).round() as u32;
        // Opacity only has a visible effect without Background on the
        // whole-window-fade fallback (no DComp support) — with DComp, no
        // Background means the window is already per-pixel transparent
        // outside the text via draw_overlay's Frame::NONE path, and Opacity
        // is never read in that branch at all.
        let opacity_matters = draft.overlay_background || !dcomp_available;
        ui.add_enabled_ui(opacity_matters, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Opacity").size(12.0).color(dc.muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{pct}%"))
                            .size(12.0)
                            .color(dc.text),
                    );
                    let mut opacity = draft.overlay_opacity as f32;
                    let slider = egui::Slider::new(&mut opacity, 0.05_f32..=1.0_f32)
                        .step_by(0.05)
                        .show_value(false)
                        .trailing_fill(true);
                    if ui.add(slider).changed() {
                        draft.overlay_opacity = opacity as f64;
                    }
                });
            });
        });
        ui.add_space(8.0);
        inner_row(dc).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Background").size(13.0).color(dc.text));
                    ui.label(
                        egui::RichText::new("Frosted card behind the text")
                            .size(11.0)
                            .color(dc.muted),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    toggle_switch(ui, dc, &mut draft.overlay_background);
                });
            });
        });
    });

    // Click-through — bypasses the draft/Save/Cancel flow entirely: applies
    // and persists immediately, same as the tray "Lock/Unlock Overlay" row
    // and the global hotkey, so it can never be left stuck locked mid-edit.
    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Click-Through");
        ui.add_space(8.0);
        let mut locked = saved.lock_safe().overlay_click_through;
        inner_row(dc).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("Lock (click-through)")
                            .size(13.0)
                            .color(dc.text),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Mouse clicks pass through to the game underneath. Applies \
                             immediately — not affected by Cancel.",
                        )
                        .size(11.0)
                        .color(dc.muted),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if toggle_switch(ui, dc, &mut locked).changed() {
                        let mut s = saved.lock_safe();
                        s.overlay_click_through = locked;
                        let s_clone = s.clone();
                        drop(s);
                        if let Err(e) = settings::persist_settings(dir, &s_clone) {
                            debug::append_debug_log(
                                dir,
                                &format!("settings: overlay click-through persist failed — {e}"),
                            );
                        }
                        reload.store(true, Ordering::Relaxed);
                    }
                });
            });
        });
    });
}
