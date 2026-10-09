//! Applying settings saved from the Settings window to the running app.

use crate::RigStatsApp;
use eframe::egui;
use rigstats_egui::lock_ext::LockSafe;
#[cfg(windows)]
use rigstats_egui::win_opacity;
use std::sync::atomic::Ordering;

impl RigStatsApp {
    /// Applies the settings the Settings window saved (flagged through
    /// `settings_reload`): theme, thresholds, window layer and mode,
    /// profile, opacity and the other cached values, with the window
    /// changes they need.
    pub(crate) fn apply_reloaded_settings(&mut self, ctx: &egui::Context) {
        if self.settings_reload.swap(false, Ordering::Relaxed) {
            let s = self.current_settings.lock_safe();
            let prev_visible = self.runtime.visible_panels.clone();
            let panels_changed = self.runtime.apply_settings(&s);
            // Any panel that was visible before but is now hidden must be removed
            // from panels_positioned so its saved position is re-applied when it
            // reappears (e.g. after cancel reverts a live preview toggle).
            if panels_changed {
                for key in &prev_visible {
                    if !self.runtime.visible_panels.contains(key) {
                        self.floating.panels_positioned.remove(key);
                    }
                }
            }
            self.opacity = s.opacity.clamp(0.1, 1.0) as f32;
            self.window_layer = s.window_layer.clone();
            let was_floating = self.floating.mode;
            self.floating.mode = s.floating_mode;
            self.floating
                .mode_arc
                .store(self.floating.mode, Ordering::Relaxed);
            self.floating.panels_locked = s.floating_panels_locked;
            self.floating.panel_scale = s.floating_panel_scale.clamp(0.4, 1.0) as f32;
            self.overlay.click_through = s.overlay_click_through;
            self.overlay.enabled = s.overlay_enabled;
            let preferred_gpu = s.preferred_gpu.clone();
            let was_fullscreen = self.window.fullscreen_mode;
            self.window.fullscreen_mode = s.fullscreen_mode;
            self.window.fullscreen_align = s.fullscreen_align.clone();
            self.window.dashboard_pinned = s.dashboard_pinned;
            let profile = s.dashboard_profile.clone();
            drop(s);
            // Settings → Display → GPU (live preview, Save, or Cancel revert).
            self.apply_preferred_gpu(preferred_gpu);
            // Toggling fullscreen changes how the window is sized; clear the
            // fit-to-content guard so the next fixed frame re-snaps correctly, and
            // drop the cached centering height so it is re-measured.
            if was_fullscreen != self.window.fullscreen_mode {
                self.window.last_fitted_height = None;
                self.window.fullscreen_content_h = None;
            }
            let (pos, [w, h]) = self.fixed_window_geometry(&profile);
            // Pinned, but this profile has no saved position yet (e.g. the user
            // switched profiles while pinned): persist the auto-targeted position
            // now so the profile is properly pinned and stays put across restarts
            // instead of lingering in a locked-but-unsaved state.
            if !self.floating.mode && self.window.dashboard_pinned {
                let mut s = self.current_settings.lock_safe();
                if !s.pinned_positions.contains_key(&profile) {
                    s.pinned_positions.insert(
                        profile.clone(),
                        [pos[0].round() as i32, pos[1].round() as i32],
                    );
                    self.persist_settings_logged(&s);
                }
            }
            // Only resize the main window when in fixed mode AND the size actually changed.
            // Sending InnerSize every settings-reload (e.g. opacity slider drag) causes a
            // visible jump even when the dimensions are identical.
            //
            // In wallpaper mode our window is parked off-screen and the host owns the
            // visible dashboard — repositioning here would yank the (frozen, polling-
            // paused) main window back on-screen over the wallpaper, so skip it.
            if !self.floating.mode && !self.wallpaper.is_active() {
                let new_size = [w, h];
                if self.window.last_applied_window_size != Some(new_size) {
                    self.window.last_applied_window_size = Some(new_size);
                    // Resize and snap to the monitor matching the profile orientation.
                    // (A size change implies a profile/fullscreen/panel change, not an
                    // opacity drag, so repositioning here does not cause jitter.)
                    let [px, py] = pos;
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                        px, py,
                    )));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
                    self.window.last_fitted_height = None;
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                Self::window_level_from_layer(&self.window_layer),
            ));
            #[cfg(windows)]
            if !self.dcomp_available {
                win_opacity::set_opacity(self.hwnd, self.opacity);
            }
            // Toggle main window position when floating mode changes.
            // We move it off-screen instead of hiding it — a hidden window is not
            // ticked by eframe, so the floating panels would not update.
            if was_floating != self.floating.mode {
                if self.floating.mode {
                    // Clear positioned-set so restored positions are re-applied
                    // from the saved layout the first time each panel is shown.
                    self.floating.panels_positioned.clear();
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                        -32000.0, -32000.0,
                    )));
                } else {
                    // Restore to the correct portrait monitor position (and full
                    // monitor size if fullscreen is enabled).
                    let [px, py] = pos;
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                        px, py,
                    )));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
                    // Force the fit-to-content path to re-dispatch InnerSize next
                    // fixed frame: compute_window_height is only an estimate and may
                    // understate the real content height, which would otherwise clip
                    // the bottom panel. Clearing the guard makes the next frame snap
                    // the window to the true min_rect height.
                    self.window.last_fitted_height = None;
                }
            }
        }
    }
}
