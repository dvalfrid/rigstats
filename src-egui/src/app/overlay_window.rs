//! The game overlay's own window (#183): rendering it each frame, and the
//! tray/hotkey toggles for showing it and for click-through.

use crate::RigStatsApp;
use eframe::egui;
use rigstats_backend::{debug, settings};
use rigstats_egui::geometry::{guard_panel_position, monitor_rect_at, overlay_anchor_position};
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::overlay::{content_inset, draw_overlay, estimate_window_size};
#[cfg(windows)]
use rigstats_egui::win_opacity;

impl RigStatsApp {
    /// Show/hide the overlay's own secondary viewport this frame, independent
    /// of `window_layer`/`floating_mode` — the whole point of the add-on
    /// design is that it coexists with whatever the main window is doing.
    /// Called unconditionally every frame; a no-op if `overlay_enabled` and
    /// `overlay_active` (last frame's shown-state) are both false.
    ///
    /// Mirrors `render_floating_panels`' per-viewport shape: size and anchor
    /// position are recomputed from current settings every shown frame (cheap
    /// pure functions) and simply passed into the `ViewportBuilder`, which
    /// egui diffs and only actually moves/resizes the OS window when the
    /// value changes — so a metric-list/layout/scale/anchor/margin edit is
    /// picked up live with no manual "did it change" bookkeeping needed.
    /// `"free"`-anchor position is the one exception: after the first shown
    /// frame of an activation it's OS-owned (via drag), matching floating
    /// panels' `panels_positioned` — re-sending it every frame would fight
    /// the OS and cause sub-pixel blur.
    pub(crate) fn render_overlay_viewport(&mut self, ctx: &egui::Context) {
        let want = self.overlay.enabled;
        if want && !self.overlay.active {
            self.overlay.active = true;
            self.overlay.positioned = false;
            // Fresh window each activation (the old one was destroyed on
            // hide) — force the resize-detection block below to treat this
            // activation's first size as "first ever" again, so it calls
            // `overlay_dcomp.start()` (which keeps the window hidden until
            // the reapply burst below settles — the user must never see the
            // transient DWM-redirection-bitmap white box, only the final,
            // correctly-composited frame) instead of being silently skipped
            // because a *previous* activation already recorded a size.
            self.overlay.last_size = None;
            // Same reasoning applies to the MousePassthrough guard below:
            // the new window starts out click-through-less (it's a brand
            // new HWND), but `overlay_click_through`'s logical value may be
            // unchanged from the last activation, so the "only send on
            // change" guard would otherwise skip resending it here — leaving
            // the new window stuck capturing mouse input.
            self.overlay.last_applied_click_through = None;
            debug::log_debug(&self.dir, "overlay: showing");
        } else if !want && self.overlay.active {
            self.overlay.active = false;
            debug::log_debug(&self.dir, "overlay: hiding");
        }
        if !want {
            return;
        }

        let (metrics, columns, anchor, margin, scale, opacity, background) = {
            let s = self.current_settings.lock_safe();
            (
                s.overlay_metrics.clone(),
                s.overlay_columns.min(6) as usize,
                s.overlay_anchor.clone(),
                s.overlay_margin as f32,
                s.overlay_scale.clamp(0.5, 2.0) as f32,
                s.overlay_opacity.clamp(0.05, 1.0) as f32,
                s.overlay_background,
            )
        };
        let measured = estimate_window_size(ctx, &metrics, columns, scale, background);
        let size = [measured.x, measured.y];
        if self.overlay.last_size != Some(size) {
            // Only the very first size (right after activation,
            // self.overlay.last_size still None) gets the full hide/burst
            // treatment. A later resize (e.g. dragging Scale) just resizes
            // silently, same as floating panels' later content-driven
            // resizes (#169) — confirmed by direct visual testing that the
            // DComp binding, once correctly established by the activation
            // burst, survives an ordinary resize without needing to be
            // re-applied every time. An earlier finding (from *before* this
            // session's hide/burst fix existed) had the white-box artifact
            // appearing right after changing Scale; that no longer holds now
            // that the underlying fix is actually correct. Scale is read
            // live here (no debounce) since a resize no longer re-triggers
            // any hide/reveal flicker for the debounce to guard against.
            let is_first_size = self.overlay.last_size.is_none();
            self.overlay.last_size = Some(size);
            if is_first_size {
                self.overlay.dcomp.start(self.dcomp_available);
            }
        }

        // Anchors to the primary monitor (origin at 0,0) — a game overlay's
        // whole purpose is sitting on the display the user is actually
        // looking at; `"free"` drag lets the user move it elsewhere.
        let monitor = monitor_rect_at([0.0, 0.0]).unwrap_or([0.0, 0.0, 1920.0, 1080.0]);
        let inset = content_inset(scale, background);
        let inset = [inset.x, inset.y];
        // A "free" position's content sits at window_origin + inset (top-left
        // aligned — see draw_overlay's h_align/content_layout for "free").
        // If the inset just changed (Background/Scale edited while already
        // dragged), shift the saved origin by the same delta and resend it
        // this frame so the content itself doesn't move — otherwise the
        // window would keep its old origin and the new padding would push
        // the content inward by the delta instead.
        let mut inset_compensated = false;
        if let (Some(prev), Some(pos)) = (self.overlay.last_content_inset, self.overlay.position) {
            if prev != inset {
                self.overlay.position =
                    Some([pos[0] - (inset[0] - prev[0]), pos[1] - (inset[1] - prev[1])]);
                inset_compensated = true;
            }
        }
        self.overlay.last_content_inset = Some(inset);
        let needs_position = !self.overlay.positioned;
        let pos: Option<[f32; 2]> = if anchor == "free" {
            if needs_position {
                let fallback = overlay_anchor_position("top-right", margin, size, monitor, inset);
                Some(
                    self.overlay
                        .position
                        .map(|p| guard_panel_position(p, fallback))
                        .unwrap_or(fallback),
                )
            } else if inset_compensated {
                self.overlay.position
            } else {
                None // OS/drag owns it now — don't fight it by resending.
            }
        } else {
            // Non-"free" anchors are never user-dragged (drag always claims
            // "free" immediately, see below), so it's always safe — and
            // necessary, since a resize changes where the anchored corner
            // sits — to keep recomputing and applying this every frame.
            Some(overlay_anchor_position(
                &anchor, margin, size, monitor, inset,
            ))
        };
        if needs_position {
            self.overlay.positioned = true;
        }

        // True per-pixel DComp transparency, without a CentralPanel wrapper.
        // Confirmed working (activation and live Scale-drag resize both stay
        // artifact-free) once paired with the hide/reapply-burst/reveal
        // sequence below — see `overlay_pending_reveal`'s doc for why that's
        // needed. If DComp is ever unavailable on a given system, the
        // `dcomp_available` branches throughout this function fall back to
        // whole-window opacity instead (see the `overlay-addon-wholewindow-fade`
        // tag for that fallback's history).
        let mut vp_builder = egui::ViewportBuilder::default()
            .with_title("RigStats \u{2014} Overlay")
            .with_inner_size(size)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_transparent(self.dcomp_available)
            // Held hidden until the reapply burst below settles, so the
            // very first frame the user ever sees is already correctly
            // per-pixel composited — see `DcompRevealBurst`'s doc.
            .with_visible(!self.overlay.dcomp.pending_reveal());
        if let Some([x, y]) = pos {
            vp_builder = vp_builder.with_position([x, y]);
        }

        let th = self.runtime.app_theme;
        let latest = &self.runtime.latest;
        let thresholds = &self.runtime.thresholds;
        let click_through = self.overlay.click_through;
        let dcomp_available = self.dcomp_available;
        // The overlay's own hwnd stays hidden (`with_visible(false)`) for
        // most of the reveal burst below — invalidating a hidden window
        // doesn't reliably queue a dispatched WM_PAINT the way it does for a
        // visible one, so `force_repaint(hwnd)` alone left the burst mostly
        // stalled on the app's ~1 fps idle heartbeat instead of running at
        // "as fast as possible" (observed: revealing the overlay took
        // 3-5 s instead of a fraction of a second). The main window is
        // always visible, so forcing its repaint too reliably drives the
        // next `ui()` call regardless of the overlay's own visibility state.
        let main_hwnd = self.hwnd;
        let mut applied_click_through = self.overlay.last_applied_click_through;
        let current_settings = self.current_settings.clone();
        let dir = self.dir.clone();
        let mut drag_pos: Option<[f32; 2]> = None;
        let dcomp = &mut self.overlay.dcomp;

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("overlay"),
            vp_builder,
            |child_ui, _class| {
                #[cfg(windows)]
                {
                    if dcomp_available {
                        let tick = dcomp.tick(
                            || win_opacity::find_hwnd("RigStats \u{2014} Overlay"),
                            dcomp_available,
                        );
                        if tick.just_started {
                            // Force a genuine size *change* (not just a style
                            // bit) — empirically, the swap chain only seems to
                            // pick up per-pixel-alpha support on an actual
                            // resize/reconfigure, not merely from
                            // WS_EX_NOREDIRECTIONBITMAP being set (which alone
                            // leaves a region of the surface stuck showing
                            // opaque white). Nudge to a different size now;
                            // the builder's real target `size` (sent every
                            // frame regardless) snaps it back next frame,
                            // triggering a second real reconfigure —
                            // mirroring exactly what manually dragging Scale
                            // down-then-up was observed to fix.
                            child_ui
                                .ctx()
                                .send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                    egui::Vec2::new(size[0] + 2.0, size[1]),
                                ));
                        }
                        if tick.reapply_style {
                            win_opacity::set_no_redirection_bitmap(dcomp.hwnd());
                        }
                        if tick.force_repaint {
                            // A style/frame change alone doesn't reliably
                            // force DWM to recomposite the DComp visual at its
                            // new size — nudge a real repaint too, and force
                            // the next frame to arrive quickly rather than
                            // waiting for the app's normal ~1 Hz tick, so the
                            // whole burst actually lands within a fraction of
                            // a second. `force_repaint` no-ops on a 0 hwnd, so
                            // this is safe even while still waiting to find it.
                            win_opacity::force_repaint(dcomp.hwnd());
                            win_opacity::force_repaint(main_hwnd);
                            child_ui.ctx().request_repaint();
                        }
                        if tick.reveal {
                            // Burst settled (or the safety timeout hit) —
                            // reveal now instead of waiting for the builder
                            // diff on the next frame.
                            child_ui
                                .ctx()
                                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        }
                    } else {
                        let hwnd = win_opacity::find_hwnd("RigStats \u{2014} Overlay");
                        win_opacity::set_opacity(hwnd, opacity);
                    }
                }

                // The one and only place MousePassthrough is sent for this
                // viewport: guarded so it's only dispatched on an actual change.
                if applied_click_through != Some(click_through) {
                    applied_click_through = Some(click_through);
                    child_ui
                        .ctx()
                        .send_viewport_cmd(egui::ViewportCommand::MousePassthrough(click_through));
                }

                let resp = draw_overlay(
                    child_ui, &th, latest, thresholds, &metrics, columns, &anchor, scale, opacity,
                    background,
                );

                // Always draggable when unlocked, regardless of the configured
                // anchor — matching every other draggable element in this app
                // (no separate "enable dragging" mode to find first). Starting
                // a drag claims free positioning immediately (persisted right
                // away) so the anchor logic above doesn't fight the drag by
                // recomputing the anchored position next frame.
                if !click_through {
                    if resp.response.drag_started() && anchor != "free" {
                        let mut s = current_settings.lock_safe();
                        s.overlay_anchor = "free".to_string();
                        if let Err(e) = settings::persist_settings(&dir, &s) {
                            debug::log_error(&dir, &format!("settings: persist failed — {e}"));
                        }
                    }
                    if resp.response.drag_started() {
                        child_ui
                            .ctx()
                            .send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    // Only track/persist position while an actual drag is in
                    // progress or just finished — reading the window's current
                    // position on every ordinary frame would otherwise overwrite
                    // a saved free-drag position whenever a non-"free" anchor
                    // moves the window on its own (a margin change, etc.).
                    if resp.response.dragged() || resp.response.drag_stopped() {
                        if let Some(outer) = child_ui.ctx().input(|i| i.viewport().outer_rect) {
                            drag_pos = Some([outer.left().round(), outer.top().round()]);
                        }
                    }
                }
            },
        );

        self.overlay.last_applied_click_through = applied_click_through;
        if let Some(pos) = drag_pos {
            if self.overlay.position != Some(pos) {
                self.overlay.position = Some(pos);
                self.overlay.position_dirty = true;
            }
        }
        if self.overlay.position_dirty {
            self.overlay.position_dirty = false;
            if let Some([x, y]) = self.overlay.position {
                let mut s = self.current_settings.lock_safe();
                s.overlay_position = Some([x.round() as i32, y.round() as i32]);
                self.persist_settings_logged(&s);
            }
        }
    }

    /// Flips `overlay_click_through`, persists it immediately, and updates
    /// the tray row's label/icon. Shared by the tray's "Lock/Unlock Overlay"
    /// row and the global hotkey (`hotkey.rs`) — both are just different
    /// triggers for the same action.
    pub(crate) fn toggle_overlay_lock(&mut self) {
        let mut s = self.current_settings.lock_safe();
        s.overlay_click_through = !s.overlay_click_through;
        self.overlay.click_through = s.overlay_click_through;
        self.persist_settings_logged(&s);
        drop(s);
        self.tray.set_overlay_lock(self.overlay.click_through);
    }

    /// Flips `overlay_enabled` and persists it — i.e. show/hide the overlay
    /// itself, the way a game-overlay hotkey is expected to work (distinct
    /// from `toggle_overlay_lock`, which only locks/unlocks click-through on
    /// an already-visible overlay). Shared by the tray's "Toggle Overlay
    /// Mode" row and the global hotkey. The overlay is an add-on window, so
    /// the main window's mode is left as it is.
    pub(crate) fn toggle_overlay_mode(&mut self) {
        let mut s = self.current_settings.lock_safe();
        s.overlay_enabled = !s.overlay_enabled;
        self.overlay.enabled = s.overlay_enabled;
        self.persist_settings_logged(&s);
        // `render_overlay_viewport` (called every frame) picks up the change
        // on the next frame and shows/hides the overlay's own viewport —
        // independent of window_layer/floating_mode, so nothing else here
        // needs to change.
    }
}
