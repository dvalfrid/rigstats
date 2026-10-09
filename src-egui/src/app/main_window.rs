//! The main window's content each frame: the floating panels in floating
//! mode, otherwise the fixed dashboard (portrait stack or landscape grid)
//! fitted to its content.

use crate::{draw_padlock, RigStatsApp};
use eframe::egui;
use rigstats_egui::geometry::{
    compute_window_height, profile_is_landscape, profile_scale, profile_to_size,
};
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::{theme, windows};
use std::sync::atomic::Ordering;

impl RigStatsApp {
    /// Renders the main window for this frame — or, in floating mode, each
    /// panel in its own window — and applies any GPU picked in a panel.
    pub(crate) fn render_main_window(&mut self, ui: &mut egui::Ui) {
        // On the first frame: move main window off-screen when floating mode is active.
        // We NEVER use Visible(false) in floating mode because a hidden window is not
        // ticked by eframe — show_viewport_immediate would stop being called and all
        // floating panels would freeze.
        if !self.floating.initial_applied {
            self.floating.initial_applied = true;
            if self.floating.mode {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                        -32000.0, -32000.0,
                    )));
            }
        }

        if self.floating.mode {
            // ── Floating mode — each panel in its own borderless viewport ─────
            self.render_floating_panels(ui);

            // Persist positions when any panel was dragged.
            if self.floating.positions_dirty.swap(false, Ordering::Relaxed) {
                self.persist_floating_positions();
            }

            // Apply GPU preference change made from the floating GPU panel.
            let float_pref = self.floating.new_pref_gpu.lock_safe().take();
            if let Some(new_pref) = float_pref {
                self.select_gpu(Some(new_pref));
            }
        } else {
            // ── Fixed mode — all panels in one portrait/landscape window ──────

            // Track the window's current outer position so the padlock can pin
            // the exact spot it is at when clicked, and so a profile change can
            // carry the window over to its current spot. Skip while the window is
            // parked off-screen for wallpaper mode, which would otherwise overwrite
            // the real position with the parking coordinates.
            if !self.wallpaper.is_active() {
                if let Some(outer) = ui.ctx().input(|i| i.viewport().outer_rect) {
                    self.window.last_fixed_pos = Some([outer.left().round(), outer.top().round()]);
                }
            }

            // Drag handle — thin invisible strip at top for moving the borderless window.
            let drag_w = {
                let w = ui.available_width();
                if w.is_finite() && w > 0.0 {
                    w
                } else {
                    ui.ctx().content_rect().width().max(1.0)
                }
            };
            let (drag_rect, drag_resp) = ui.allocate_exact_size(
                egui::Vec2::new(drag_w, theme::DRAG_HANDLE_H),
                egui::Sense::drag(),
            );

            // Padlock at the right of the drag strip: pins the whole dashboard.
            let pinned = self.window.dashboard_pinned;
            let padlock_center = egui::pos2(drag_rect.right() - 12.0, drag_rect.center().y);
            let padlock_hit = egui::Rect::from_center_size(
                padlock_center,
                egui::Vec2::new(24.0, drag_rect.height().max(14.0)),
            );
            let hover_pos = ui.ctx().input(|i| i.pointer.hover_pos());
            let over_padlock = hover_pos.map(|p| padlock_hit.contains(p)).unwrap_or(false);
            let drag_resp = if over_padlock {
                drag_resp
            } else {
                drag_resp.on_hover_text("Drag to move")
            };
            let padlock_resp = ui
                .interact(
                    padlock_hit,
                    ui.id().with("fixed_padlock"),
                    egui::Sense::click(),
                )
                .on_hover_text(if pinned {
                    "Click to unlock"
                } else {
                    "Click to lock position here"
                });

            // Drag only when not pinned and not starting on the padlock.
            if drag_resp.dragged() && !pinned && !over_padlock {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            // After drag ends in "behind" mode, the window was activated for SC_MOVE
            // and is now in front. Push it back behind on the next few frames.
            if drag_resp.drag_stopped() && self.window_layer == "behind" {
                self.window.reapply_window_props_frames = 4;
            }

            if padlock_resp.clicked() {
                self.window.dashboard_pinned = !self.window.dashboard_pinned;
                let profile = self.current_settings.lock_safe().dashboard_profile.clone();
                let mut s = self.current_settings.lock_safe();
                s.dashboard_pinned = self.window.dashboard_pinned;
                if self.window.dashboard_pinned {
                    // Pin: remember the current window position for this profile.
                    if let Some([x, y]) = self.window.last_fixed_pos {
                        s.pinned_positions
                            .insert(profile, [x.round() as i32, y.round() as i32]);
                    }
                }
                self.persist_settings_logged(&s);
            }
            if padlock_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            // Drag dots — only when movable (unpinned) and hovering the strip.
            if drag_resp.hovered() && !pinned && !over_padlock {
                let painter = ui.painter();
                let cy = drag_rect.center().y;
                let cx = drag_rect.center().x;
                for i in [-5.0f32, 0.0, 5.0] {
                    painter.circle_filled(
                        egui::Pos2::new(cx + i, cy),
                        1.5,
                        egui::Color32::from_gray(160),
                    );
                }
            }
            // Padlock icon — shown while pinned (so it can be released) or when
            // hovering the strip (so it is discoverable).
            if pinned || drag_resp.hovered() || padlock_resp.hovered() {
                let color = if pinned {
                    self.runtime.app_theme.accent
                } else if padlock_resp.hovered() {
                    egui::Color32::from_gray(210)
                } else {
                    egui::Color32::from_gray(150)
                };
                draw_padlock(ui.painter(), padlock_center, pinned, color);
            }

            // Panels in the order defined by visible_panels (respects user reordering).
            let panels_to_draw = self.runtime.visible_panels.clone();
            let profile = self.current_settings.lock_safe().dashboard_profile.clone();
            // Extract update version once per frame (cheap lock read).
            let update_ver: Option<String> = {
                use windows::updater::UpdateStatus;
                let st = self.dialogs.updater_win.lock_safe();
                if let UpdateStatus::Ready { info, .. } = &st.status {
                    Some(info.version.clone())
                } else {
                    None
                }
            };
            let mut new_preferred_gpu: Option<String> = None;

            let is_landscape = profile_is_landscape(&profile);
            if is_landscape {
                // ── Landscape grid — panels packed into an even, adaptive grid ──
                // Fullscreen: the window is fixed to the profile/monitor size and
                // the grid stretches to fill it exactly. Otherwise the grid uses
                // natural cell sizes and the fit-to-content block below shrinks
                // the window to the rows actually used.
                new_preferred_gpu = self.render_landscape_grid(
                    ui,
                    &panels_to_draw,
                    update_ver.as_deref(),
                    &profile,
                );
            } else {
                // ── Portrait vertical stack ─────────────────────────────────────
                let sc = profile_scale(&profile);
                // Fullscreen + centered: pad the top so the panel stack is vertically
                // centered in the filled window. Panel proportions are untouched —
                // only the surrounding background grows. Use the measured content
                // height from the previous frame (cached) for an exact center; fall
                // back to the compute_window_height estimate on the first frame.
                let center_fullscreen =
                    self.window.fullscreen_mode && self.window.fullscreen_align == "center";
                let center_pad = if center_fullscreen {
                    // Pure panel-stack height (excludes drag handle and pad).
                    let content = self.window.fullscreen_content_h.unwrap_or_else(|| {
                        compute_window_height(&panels_to_draw, sc) - theme::DRAG_HANDLE_H
                    });
                    // Center the visible content with equal background above and below.
                    // The drag handle occupies the first DRAG_HANDLE_H px (invisible),
                    // so discount it from the top so the content itself is centered.
                    let pad =
                        ((ui.available_height() - content - theme::DRAG_HANDLE_H) * 0.5).max(0.0);
                    ui.add_space(pad);
                    pad
                } else {
                    0.0
                };
                for panel in &panels_to_draw {
                    if let Some(p) = self.draw_one_panel(ui, panel, sc, update_ver.as_deref()) {
                        new_preferred_gpu = Some(p);
                    }
                    ui.add_space((6.0 * sc).round());
                }
                // Cache the true panel-stack content height for exact centering next
                // frame: min_rect = drag handle + center pad + content, so content =
                // min_rect − DRAG_HANDLE_H − pad. Re-center on the next frame if the
                // measurement drifts from what we used this frame.
                if center_fullscreen {
                    let content = ui.min_rect().height() - theme::DRAG_HANDLE_H - center_pad;
                    if content > 10.0 {
                        let changed = self
                            .window
                            .fullscreen_content_h
                            .map(|h| (h - content).abs() > 0.5)
                            .unwrap_or(true);
                        self.window.fullscreen_content_h = Some(content);
                        if changed {
                            ui.ctx().request_repaint();
                        }
                    }
                }
            }

            // Fit window height to actual rendered content every frame so no black gap
            // appears regardless of panel set, spacing, or egui version. Applies to both
            // orientations — the landscape grid uses natural cell sizes outside fullscreen
            // so it shrinks/grows with the panel count just like the portrait stack.
            // Skipped in fullscreen mode — there the window stays at the full monitor
            // size and must NOT shrink to content.
            if !self.window.fullscreen_mode {
                let used_h = ui.min_rect().height();
                // During the first frames after launch the window may not be fully
                // realized, so early InnerSize commands can be dropped — which would
                // leave the bottom panel clipped until the user toggles floating mode.
                // Force a re-fit (and a fast repaint) for a handful of frames so the
                // true content height always sticks at startup.
                if self.window.startup_fit_frames > 0 {
                    self.window.startup_fit_frames -= 1;
                    self.window.last_fitted_height = None;
                    ui.ctx().request_repaint();
                }
                let changed = self
                    .window
                    .last_fitted_height
                    .map(|h| (h - used_h).abs() > 0.5)
                    .unwrap_or(true);
                if used_h > 10.0 && changed {
                    self.window.last_fitted_height = Some(used_h);
                    let [w, _] = profile_to_size(&profile);
                    // Landscape keeps the full profile width (no trim); the portrait
                    // stack trims 2 px to avoid a sub-pixel edge artifact.
                    let target_w = if is_landscape { w } else { w - 2.0 };
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                            target_w, used_h,
                        )));
                }
            }

            if let Some(new_pref) = new_preferred_gpu {
                self.select_gpu(Some(new_pref));
            }
        }
    }
}
