//! The tray menu's commands: [`RigStatsApp::handle_tray_commands`] runs
//! the ones the tray thread forwarded, once per frame from `ui()`.

use crate::RigStatsApp;
use eframe::egui;
use rigstats_backend::{control, debug, logging};
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::tray::TrayCmd;
#[cfg(windows)]
use rigstats_egui::win_opacity;
use rigstats_egui::windows;
use std::sync::atomic::Ordering;
use std::time::Instant;

impl RigStatsApp {
    /// Runs the tray commands forwarded by the background polling thread.
    pub(crate) fn handle_tray_commands(&mut self, ctx: &egui::Context) {
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCmd::OpenSettings => self.tray_open_settings(),
                TrayCmd::OpenAbout => self.tray_open_about(),
                TrayCmd::OpenStatus => self.tray_open_status(ctx),
                TrayCmd::OpenHistory => self.tray_open_history(ctx),
                TrayCmd::OpenControlCenter => {
                    self.dialogs.control_open.store(true, Ordering::Relaxed);
                    self.dialogs.control_focus.store(true, Ordering::Relaxed);
                }
                TrayCmd::OpenUpdater => self.tray_open_updater(),
                TrayCmd::OpenDocs => self.tray_open_docs(ctx),
                TrayCmd::ToggleFloating => self.tray_toggle_floating(ctx),
                TrayCmd::ToggleOverlay => self.toggle_overlay_mode(),
                TrayCmd::ToggleOverlayLock => self.toggle_overlay_lock(),
                TrayCmd::ToggleRecording => self.tray_toggle_recording(ctx),
                TrayCmd::ToggleLamp => {
                    let _ = self
                        .control_cmd_tx
                        .try_send(control::ControlCmd::ToggleLamp);
                }
                TrayCmd::SelectGpu(pref) => self.select_gpu(pref),
                TrayCmd::SelectProfile(id) => self.apply_profile(id),
            }
        }
    }

    fn tray_open_settings(&mut self) {
        // Re-initialise draft from current settings each time the window opens.
        let s = self.current_settings.lock_safe().clone();
        *self.dialogs.settings_win.lock_safe() = windows::settings::SettingsWindow::from_settings(
            &s,
            self.tray.gpu_menu.names(),
            self.battery_present.clone(),
        );
        self.dialogs.settings_open.store(true, Ordering::Relaxed);
        self.dialogs.settings_focus.store(true, Ordering::Relaxed);
    }

    fn tray_open_about(&mut self) {
        self.dialogs.about_open.store(true, Ordering::Relaxed);
        self.dialogs.about_focus.store(true, Ordering::Relaxed);
    }

    fn tray_open_status(&mut self, ctx: &egui::Context) {
        self.dialogs.status_open.store(true, Ordering::Relaxed);
        self.dialogs.status_focus.store(true, Ordering::Relaxed);
        windows::status::spawn_load(
            self.dialogs.status_win.clone(),
            self.dialogs.status_refreshing.clone(),
            self.dir.as_ref().clone(),
            self.runtime.latest.lhm_connected,
            self.wallpaper.is_active(),
            ctx.clone(),
        );
    }

    fn tray_open_history(&mut self, ctx: &egui::Context) {
        self.dialogs.history_open.store(true, Ordering::Relaxed);
        self.dialogs.history_focus.store(true, Ordering::Relaxed);
        windows::history::spawn_load_sessions(
            self.dialogs.history_win.clone(),
            self.dialogs.history_refreshing.clone(),
            self.dir.as_ref().clone(),
            ctx.clone(),
        );
    }

    fn tray_open_updater(&mut self) {
        self.dialogs.updater_open.store(true, Ordering::Relaxed);
        self.dialogs.updater_focus.store(true, Ordering::Relaxed);
    }

    fn tray_open_docs(&mut self, ctx: &egui::Context) {
        ctx.open_url(egui::OpenUrl::new_tab("https://rigstats.app"));
    }

    fn tray_toggle_floating(&mut self, ctx: &egui::Context) {
        let new_mode = {
            let mut s = self.current_settings.lock_safe();
            s.floating_mode = !s.floating_mode;
            self.persist_settings_logged(&s);
            s.floating_mode
        };
        let was_floating = self.floating.mode;
        self.floating.mode = new_mode;
        self.floating.mode_arc.store(new_mode, Ordering::Relaxed);
        if was_floating != new_mode {
            if new_mode {
                self.floating.panels_positioned.clear();
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                    -32000.0, -32000.0,
                )));
            } else {
                let profile = self.current_settings.lock_safe().dashboard_profile.clone();
                let ([px, py], [w, h]) = self.fixed_window_geometry(&profile);
                // Apply level BEFORE moving on-screen so it is already
                // in effect when the window becomes visible.
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    Self::window_level_from_layer(&self.window_layer),
                ));
                #[cfg(windows)]
                if !self.dcomp_available {
                    win_opacity::set_opacity(self.hwnd, self.opacity);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                    px, py,
                )));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
                // See note in the settings-reload transition: clear the
                // fit-to-content guard so the next fixed frame snaps the
                // window to the true content height instead of the
                // compute_window_height estimate (which can clip the
                // bottom panel).
                self.window.last_fitted_height = None;
                // Re-apply for the next few frames as winit may reset the
                // window level when it processes the move event.
                self.window.reapply_window_props_frames = 4;
            }
        }
    }

    fn tray_toggle_recording(&mut self, ctx: &egui::Context) {
        // The active session lives in the on-disk index, not in-process
        // state — the same source both this app's and the wallpaper
        // host's poll loops check each tick (see `poll_loop`).
        let active = logging::load_sessions(&self.dir)
            .into_iter()
            .find(logging::SessionMeta::is_active);
        if let Some(session) = active {
            let retention_days = self.current_settings.lock_safe().log_retention_days;
            logging::end_session(&self.dir, &session.id, logging::unix_now_secs());
            logging::prune_old_sessions(&self.dir, retention_days);
            self.tray.set_recording(false);
            self.recording.active = false;
            self.recording.active_shared.store(false, Ordering::Relaxed);
        } else {
            match logging::start_session(&self.dir) {
                Ok(_) => {
                    self.tray.set_recording(true);
                    self.recording.active = true;
                    self.recording.active_shared.store(true, Ordering::Relaxed);
                    self.recording.blink_on = true;
                    self.recording.blink_at = Instant::now();
                }
                Err(e) => {
                    debug::log_error(
                        &self.dir,
                        &format!("logging: failed to start session — {e}"),
                    );
                }
            }
        }
        // The Session History window (if open) shows its own snapshot of
        // the session list — without this it keeps showing the session
        // that was just started/stopped as still recording until the
        // user manually hits Refresh.
        if self.dialogs.history_open.load(Ordering::Relaxed) {
            windows::history::spawn_load_sessions(
                self.dialogs.history_win.clone(),
                self.dialogs.history_refreshing.clone(),
                self.dir.as_ref().clone(),
                ctx.clone(),
            );
        }
    }
}
