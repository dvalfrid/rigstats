//! The dialog windows (Settings, About, Status, Session History, Updates,
//! Control Center): showing each one's viewport every frame it is open,
//! and the reveal/teardown handling they share (see `dialog_reveal`).

use crate::RigStatsApp;
use eframe::egui;
use rigstats_egui::geometry::dialog_center;
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::tray::load_app_icon;
use rigstats_egui::{theme, update_flow, windows};
#[cfg(windows)]
use rigstats_egui::{win32_dark_mode, win_opacity};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Settings and the Control Center: a sidebar of pages beside grouped rows.
const SETTINGS_SIZE: [f32; 2] = [760.0, 620.0];
const CONTROL_SIZE: [f32; 2] = [940.0, 700.0];

impl RigStatsApp {
    /// Shared tail of every dialog's per-frame render (Settings, About,
    /// Status, History, Updater), after its `show_viewport_immediate`:
    /// dark title bar, hidden-until-rendered reveal (see `dialog_reveal`),
    /// and bringing a newly opened dialog to the foreground.
    ///
    /// `visible` is what this frame's `ViewportBuilder` asked for. While
    /// still hidden, frames are forced so the reveal isn't left waiting on
    /// the ~1 fps idle heartbeat — `request_repaint` plus a real WM_PAINT to
    /// the main window, the same combination `render_overlay_viewport` uses
    /// for its hidden reveal burst. Focus waits until the dialog is visible
    /// (a hidden window can't take the foreground), so the flag is kept set
    /// until then.
    fn finish_dialog_frame(
        &mut self,
        ctx: &egui::Context,
        id: &'static str,
        found_hwnd: isize,
        wants_focus: bool,
        focus: &Arc<AtomicBool>,
        visible: bool,
    ) {
        self.dialogs.dialog_reveal.rendered(id);
        #[cfg(windows)]
        {
            win32_dark_mode::apply_titlebar_theme(found_hwnd, self.os_dark_mode);
            // Set while the dialog is open so it's in place before it closes.
            win_opacity::disable_dwm_transitions(found_hwnd);
        }
        if !visible {
            // Glyphs first drawn in a hidden pass can lose their atlas upload
            // (egui 0.34, see `refresh_font_atlas_after_minimize`): rebuild
            // the atlas once the dialog shows instead of up to 30 s later.
            self.font_atlas.stale = true;
            ctx.request_repaint();
            #[cfg(windows)]
            win_opacity::force_repaint(self.hwnd);
        }
        if wants_focus {
            // If found_hwnd == 0 the window wasn't ready yet; keep the flag
            // so we retry on the next frame.
            #[cfg(windows)]
            if found_hwnd != 0 && visible {
                win_opacity::bring_to_foreground(found_hwnd);
            } else {
                focus.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Shows every open dialog's viewport for this frame, hides the ones
    /// that just closed, and starts a manual update check when the Updates
    /// dialog asks for one.
    pub(crate) fn render_dialogs(&mut self, ui: &mut egui::Ui) {
        let main_ctx = ui.ctx().clone();
        let dc = self.dialog_colors;

        // When the last dialog closes, restore the main-window dark visuals.
        // We only call set_visuals on the transition frame (not every frame) to
        // avoid the repaint loop that causes jerky window dragging in light mode.
        let any_dialog_open = self.dialogs.settings_open.load(Ordering::Relaxed)
            || self.dialogs.about_open.load(Ordering::Relaxed)
            || self.dialogs.status_open.load(Ordering::Relaxed)
            || self.dialogs.updater_open.load(Ordering::Relaxed)
            || self.dialogs.history_open.load(Ordering::Relaxed)
            || self.dialogs.control_open.load(Ordering::Relaxed);
        if !any_dialog_open && self.dialogs.any_dialog_open_prev {
            let mut vis = egui::Visuals::dark();
            vis.panel_fill = egui::Color32::TRANSPARENT;
            vis.window_fill = egui::Color32::from_gray(28);
            vis.override_text_color = Some(theme::C_TEXT);
            ui.ctx().set_visuals(vis);
        }
        self.dialogs.any_dialog_open_prev = any_dialog_open;

        for (id, open) in [
            ("settings", &self.dialogs.settings_open),
            ("about", &self.dialogs.about_open),
            ("status", &self.dialogs.status_open),
            ("history", &self.dialogs.history_open),
            ("updater", &self.dialogs.updater_open),
            ("control", &self.dialogs.control_open),
        ] {
            self.dialogs
                .dialog_reveal
                .track(id, open.load(Ordering::Relaxed));
        }
        // A dialog that closed this frame gets one last, content-less frame
        // that only hides its window, so eframe drops its GPU surface from an
        // already-hidden window next frame (no white flash on close — see
        // `DialogReveal::take_closing`). Only `visible` is set on the
        // builder: egui only applies fields that are set, so size/position
        // are left untouched.
        let closing = self.dialogs.dialog_reveal.take_closing();
        if !closing.is_empty() {
            for id in closing {
                ui.ctx().show_viewport_immediate(
                    egui::ViewportId::from_hash_of(id),
                    egui::ViewportBuilder::default().with_visible(false),
                    |_, _| {},
                );
            }
            // Make the teardown frame come promptly.
            ui.ctx().request_repaint();
            #[cfg(windows)]
            win_opacity::force_repaint(self.hwnd);
        }

        if self.dialogs.settings_open.load(Ordering::Relaxed) {
            let open = self.dialogs.settings_open.clone();
            let focus = self.dialogs.settings_focus.clone();
            let state = self.dialogs.settings_win.clone();
            let dir = self.dir.clone();
            let saved = self.current_settings.clone();
            let reload = self.settings_reload.clone();
            let requests = self.dialogs.open_requests.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(SETTINGS_SIZE[0], SETTINGS_SIZE[1]);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let dcomp_available = self.dcomp_available;
            let visible = self.dialogs.dialog_reveal.visible("settings");
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("settings"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Settings")
                    .with_visible(visible)
                    .with_inner_size(SETTINGS_SIZE)
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
                        dcomp_available,
                        &requests,
                    );
                },
            );
            self.finish_dialog_frame(
                ui.ctx(),
                "settings",
                found_hwnd,
                wants_focus,
                &focus,
                visible,
            );
        }

        if self.dialogs.about_open.load(Ordering::Relaxed) {
            let open = self.dialogs.about_open.clone();
            let focus = self.dialogs.about_focus.clone();
            let dir = self.dir.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(360.0, 280.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let visible = self.dialogs.dialog_reveal.visible("about");
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("about"),
                egui::ViewportBuilder::default()
                    .with_title("About RigStats")
                    .with_visible(visible)
                    .with_inner_size([380.0, 460.0])
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
            self.finish_dialog_frame(ui.ctx(), "about", found_hwnd, wants_focus, &focus, visible);
        }

        if self.dialogs.control_open.load(Ordering::Relaxed) {
            let open = self.dialogs.control_open.clone();
            let focus = self.dialogs.control_focus.clone();
            let mctx = main_ctx.clone();
            let cmd_tx = self.control_cmd_tx.clone();
            let [px, py] = dialog_center(CONTROL_SIZE[0], CONTROL_SIZE[1]);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let visible = self.dialogs.dialog_reveal.visible("control");
            let control_state = self.runtime.control.clone();
            let peripherals = self.runtime.latest.peripherals.clone();
            let control_ui = &mut self.dialogs.control_ui;
            let look_link = windows::profile_look::LookLink {
                settings: &self.current_settings,
                dir: &self.dir,
                reload: &self.settings_reload,
                open: &self.dialogs.open_requests,
                battery_present: &self.battery_present,
            };
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("control"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Control Center")
                    .with_visible(visible)
                    .with_inner_size(CONTROL_SIZE)
                    .with_position([px, py])
                    .with_resizable(false)
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("RigStats — Control Center");
                    }
                    windows::control::show(
                        child_ui.ctx(),
                        &mctx,
                        &open,
                        &focus,
                        &control_state,
                        &cmd_tx,
                        &dc,
                        control_ui,
                        &look_link,
                        &peripherals,
                    );
                },
            );
            self.finish_dialog_frame(
                ui.ctx(),
                "control",
                found_hwnd,
                wants_focus,
                &focus,
                visible,
            );
        }

        if self.dialogs.status_open.load(Ordering::Relaxed) {
            let open = self.dialogs.status_open.clone();
            let focus = self.dialogs.status_focus.clone();
            let state = self.dialogs.status_win.clone();
            let refreshing = self.dialogs.status_refreshing.clone();
            let collecting = self.dialogs.status_collecting.clone();
            let dir = self.dir.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(680.0, 820.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let lhm_connected = self.runtime.latest.lhm_connected;
            let wallpaper_active = self.wallpaper.is_active();
            let control_state = self.runtime.control.clone();
            let visible = self.dialogs.dialog_reveal.visible("status");
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("status"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Status")
                    .with_visible(visible)
                    .with_inner_size([680.0, 820.0])
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
                        wallpaper_active,
                        &control_state,
                        &dc,
                    );
                },
            );
            self.finish_dialog_frame(ui.ctx(), "status", found_hwnd, wants_focus, &focus, visible);
        }

        if self.dialogs.history_open.load(Ordering::Relaxed) {
            let open = self.dialogs.history_open.clone();
            let focus = self.dialogs.history_focus.clone();
            let state = self.dialogs.history_win.clone();
            let refreshing = self.dialogs.history_refreshing.clone();
            let loading_rows = self.dialogs.history_loading_rows.clone();
            let dir = self.dir.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(820.0, 720.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let visible = self.dialogs.dialog_reveal.visible("history");
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("history"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Session History")
                    .with_visible(visible)
                    .with_inner_size([820.0, 720.0])
                    .with_position([px, py])
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("RigStats \u{2014} Session History");
                    }
                    windows::history::show(
                        child_ui.ctx(),
                        &mctx,
                        &open,
                        &focus,
                        &state,
                        &refreshing,
                        &loading_rows,
                        &dir,
                        &dc,
                    );
                },
            );
            self.finish_dialog_frame(
                ui.ctx(),
                "history",
                found_hwnd,
                wants_focus,
                &focus,
                visible,
            );
        }

        if self.dialogs.updater_open.load(Ordering::Relaxed) {
            let open = self.dialogs.updater_open.clone();
            let focus = self.dialogs.updater_focus.clone();
            let state = self.dialogs.updater_win.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(600.0, 620.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let visible = self.dialogs.dialog_reveal.visible("updater");
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("updater"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Updates")
                    .with_visible(visible)
                    .with_inner_size([600.0, 620.0])
                    .with_position([px, py])
                    .with_resizable(false)
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("RigStats — Updates");
                    }
                    windows::updater::show(child_ui.ctx(), &mctx, &open, &focus, &state, &dc);
                },
            );
            self.finish_dialog_frame(
                ui.ctx(),
                "updater",
                found_hwnd,
                wants_focus,
                &focus,
                visible,
            );

            // When the window sets status to Checking (manual button click),
            // run check+download on a plain OS thread — it blocks, and
            // tokio::spawn is not safe to call from the egui UI thread
            // (which is not inside a tokio async context).
            let start_check = {
                let s = self.dialogs.updater_win.lock_safe();
                matches!(s.status, windows::updater::UpdateStatus::Checking) && !s.busy
            };
            if start_check {
                let win = self.dialogs.updater_win.clone();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    update_flow::run_check_and_download(&win, &ctx, update_flow::Trigger::Manual);
                });
            }
        }
    }
}
