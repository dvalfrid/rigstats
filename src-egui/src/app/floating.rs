//! Floating mode: every visible panel in its own borderless window.

use crate::{draw_padlock, take_fan_curve_request, BehindEnforce, RigStatsApp};
use eframe::egui;
use rigstats_backend::settings;
use rigstats_egui::geometry::guard_panel_position;
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::tray::{panel_initial_h, panel_label};
use rigstats_egui::{panels, theme, windows};
#[cfg(windows)]
use rigstats_egui::{win32_behind, win_opacity};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

impl RigStatsApp {
    /// Render every visible panel as its own borderless OS window using
    /// `show_viewport_immediate`.  Called each frame when `floating_mode` is true.
    ///
    /// `show_viewport_immediate` renders each child synchronously as part of the
    /// parent frame — no deferred callbacks, no separate event loops.  The parent
    /// ticks at ~1 fps (via `request_repaint_after(1 s)` in `update()`), so all
    /// panels naturally update at ~1 fps without any Win32 tricks.
    pub(crate) fn render_floating_panels(&mut self, ui: &mut egui::Ui) {
        let s = self.current_settings.lock_safe();
        let window_level = Self::window_level_from_layer(&s.window_layer);
        let scale = self.floating.panel_scale;
        drop(s);

        let opacity = self.opacity;
        // Per-pixel DComp transparency for floating panels (issue #169):
        // confirmed working by replaying the overlay's full fix, not just
        // the style bit — forced resize nudge + repeated reapply/
        // force_repaint burst + hidden until settled, both on panel
        // creation and on any later content-driven resize. A prior, lighter
        // attempt (style bit + relying on the natural one-shot creation
        // resize alone, no hide/burst) reproduced the same washed-out
        // white/gray result the original 2026-08-21 investigation on #169
        // found — the repeated reapply+force_repaint burst while hidden
        // turned out to be the missing piece, exactly as it was for the
        // overlay.
        let dcomp_available = self.dcomp_available;
        let content_opacity = if dcomp_available { opacity } else { 1.0 };
        let main_hwnd = self.hwnd;

        for idx in 0..self.runtime.visible_panels.len() {
            let key = self.runtime.visible_panels[idx].clone();

            let init_pos: [f32; 2] = {
                let default_pos = [100.0 + idx as f32 * 20.0, 80.0 + idx as f32 * 30.0];
                let positions = self.floating.positions.lock_safe();
                let saved = positions.get(&key).copied().unwrap_or(default_pos);
                guard_panel_position(saved, default_pos)
            };

            let panel_w = 450.0 * scale;
            let initial_h = panel_initial_h(&key) * scale;

            // Only set window position on first creation.  After that the OS
            // owns the position (via drag); re-sending with_position every frame
            // causes egui to diff-and-dispatch SetOuterPosition continuously,
            // which fights the OS and produces sub-pixel blur.
            let needs_position = !self.floating.panels_positioned.contains(&key);
            if needs_position {
                self.floating.panels_positioned.insert(key.clone());
            }

            // Pull this panel's DComp burst state out of the map for the
            // duration of this iteration (put back after show_viewport_immediate
            // returns) — same "extract, mutate as a local, write back" shape
            // the overlay uses for its own (single, not per-key) burst state.
            let mut dcomp = self
                .floating
                .dcomp
                .borrow_mut()
                .remove(&key)
                .unwrap_or_default();
            if needs_position {
                // Fresh panel: hide until the burst below settles, exactly
                // like the overlay's first show — the burst itself starts
                // once the closure below actually finds the HWND.
                dcomp.start(dcomp_available);
            }

            // Title shared between ViewportBuilder and Win32 FindWindowW lookup.
            let win_title = format!("RigStats \u{2014} {}", panel_label(&key));
            let mut vp_builder = egui::ViewportBuilder::default()
                .with_title(win_title.clone())
                .with_inner_size([panel_w, initial_h])
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_transparent(dcomp_available)
                .with_visible(!dcomp.pending_reveal())
                .with_window_level(window_level);

            let is_behind = window_level == egui::WindowLevel::AlwaysOnBottom;

            if needs_position {
                vp_builder = vp_builder.with_position(init_pos);
            }

            // `show_viewport_immediate` is FnMut with no Send/'static bound —
            // we can borrow self fields directly instead of going through Arc.
            let positions_arc = &self.floating.positions;
            let dirty = &self.floating.positions_dirty;
            let behind_enforce = &self.floating.behind_enforce;
            let new_pref_arc = &self.floating.new_pref_gpu;
            let lock_arc = &self.floating.lock_arc;
            let stats = &self.runtime.latest;
            let cspark = &self.runtime.cpu_spark;
            let gspark = &self.runtime.gpu_spark;
            let nuspark = &self.runtime.net_up_spark;
            let ndspark = &self.runtime.net_dn_spark;
            let tex = &self.runtime.textures;
            let app_theme = self.runtime.app_theme;
            let control = &self.runtime.control;
            let float_update_ver: Option<String> = {
                use windows::updater::UpdateStatus;
                let st = self.dialogs.updater_win.lock_safe();
                if let UpdateStatus::Ready { info, .. } = &st.status {
                    Some(info.version.clone())
                } else {
                    None
                }
            };
            let updater_open_arc = &self.dialogs.updater_open;
            let control_open_arc = &self.dialogs.control_open;
            let control_focus_arc = &self.dialogs.control_focus;
            // On the very first frame a viewport is shown, `outer_rect` reports the
            // egui-default position (before the OS has honoured `with_position`).
            // Saving that would overwrite the loaded position, so we skip tracking
            // on the first frame — `needs_position` was true iff this is that frame.
            let skip_pos_tracking = needs_position;

            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of(format!("float_{key}")),
                vp_builder,
                |child_ui, _class| {
                    let ctx = child_ui.ctx();

                    // ── Track window position for persistence ─────────────────
                    if !skip_pos_tracking {
                        if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                            let new_pos = [outer.left().round(), outer.top().round()];
                            let mut pos = positions_arc.lock_safe();
                            let stored = pos.entry(key.clone()).or_insert([f32::NAN, f32::NAN]);
                            if stored[0].is_nan()
                                || stored[1].is_nan()
                                || ((*stored)[0] - new_pos[0]).abs() > 0.5
                                || ((*stored)[1] - new_pos[1]).abs() > 0.5
                            {
                                *stored = new_pos;
                                dirty.store(true, Ordering::Relaxed);
                            }
                        }
                    }

                    // ── "Always Behind" enforcement ───────────────────────────
                    // SC_MOVE (StartDrag) requires the window to be active.
                    // WS_EX_NOACTIVATE prevents activation, so we only enforce
                    // "behind" when the primary button is NOT pressed on this
                    // panel.  prepare_for_drag() strips WS_EX_NOACTIVATE and
                    // activates the window just before sending StartDrag, then
                    // apply_behind() re-arms once the drag finishes.
                    //
                    // Crucially we DON'T re-push every frame: send_viewport_cmd +
                    // SetWindowPos each generate a fresh repaint, so re-asserting
                    // unconditionally creates a spin loop that runs at the full
                    // refresh rate (the cause of the floating-mode CPU spike).
                    // Instead we enforce on creation, in a short burst after a
                    // drag, and otherwise ~1/s as an idle safety net.
                    let primary_down = ctx.input(|i| i.pointer.primary_down());
                    if is_behind {
                        let now = Instant::now();
                        let mut map = behind_enforce.borrow_mut();
                        let st = map.entry(key.clone()).or_insert(BehindEnforce {
                            last_enforce: now.checked_sub(Duration::from_secs(10)).unwrap_or(now),
                            prev_primary_down: false,
                            force_until: now,
                        });
                        let released = st.prev_primary_down && !primary_down;
                        st.prev_primary_down = primary_down;
                        if released {
                            // Window was activated for the drag; snap it back for
                            // a short burst so it settles behind reliably.
                            st.force_until = now + Duration::from_millis(400);
                        }
                        let should_enforce = !primary_down
                            && (needs_position
                                || now < st.force_until
                                || now.duration_since(st.last_enforce)
                                    >= Duration::from_millis(750));
                        if should_enforce {
                            st.last_enforce = now;
                            drop(map);
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                egui::WindowLevel::AlwaysOnBottom,
                            ));
                            #[cfg(windows)]
                            win32_behind::apply_behind(&win_title);
                        }
                    }

                    // Transparent when DComp is driving real per-pixel alpha (each
                    // panel's own theme::panel_frame paints the visible background,
                    // premultiplied by content_opacity below) — opaque PANEL_FILL
                    // for the WS_EX_LAYERED fallback, which dims whatever's here
                    // uniformly and needs solid content behind it to look right.
                    let central_fill = if dcomp_available {
                        egui::Color32::TRANSPARENT
                    } else {
                        theme::PANEL_FILL
                    };
                    #[allow(deprecated)] // CentralPanel::show is correct in viewport callbacks
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none().fill(central_fill))
                        .show(ctx, |ui| {
                            // ── Drag & lock state ─────────────────────────────
                            let locked = lock_arc.load(Ordering::Relaxed);
                            let hover_pos = ctx.input(|i| i.pointer.hover_pos());
                            let just_pressed = ctx.input(|i| i.pointer.primary_pressed());

                            // ── Panel content ─────────────────────────────────
                            // Each draw() returns the panel's outer Rect so we can
                            // overlay the drag dots and padlock without extra height.
                            let mut new_pref: Option<String> = None;
                            let panel_rect = match key.as_str() {
                                "header" => {
                                    let r = panels::header::draw(
                                        ui,
                                        stats,
                                        tex,
                                        content_opacity,
                                        &app_theme,
                                        scale,
                                        control,
                                    );
                                    if ui
                                        .ctx()
                                        .data_mut(|d| {
                                            d.remove_temp::<bool>(egui::Id::new(
                                                "open_control_center",
                                            ))
                                        })
                                        .unwrap_or(false)
                                    {
                                        control_open_arc.store(true, Ordering::Relaxed);
                                        control_focus_arc.store(true, Ordering::Relaxed);
                                    }
                                    r
                                }
                                "clock" => {
                                    let r = panels::clock::draw(
                                        ui,
                                        stats.uptime_secs,
                                        content_opacity,
                                        &app_theme,
                                        float_update_ver.as_deref(),
                                        scale,
                                    );
                                    if ui
                                        .ctx()
                                        .data_mut(|d| {
                                            d.remove_temp::<bool>(egui::Id::new("open_updater"))
                                        })
                                        .unwrap_or(false)
                                    {
                                        updater_open_arc.store(true, Ordering::Relaxed);
                                    }
                                    r
                                }
                                "cpu" => panels::cpu::draw(
                                    ui,
                                    stats,
                                    cspark,
                                    tex,
                                    content_opacity,
                                    self.runtime.thresholds.cpu.0,
                                    self.runtime.thresholds.cpu.1,
                                    &app_theme,
                                    scale,
                                    self.runtime.control.active_ppt_limit(),
                                ),
                                "gpu" => {
                                    let r = panels::gpu::draw(
                                        ui,
                                        stats,
                                        gspark,
                                        tex,
                                        content_opacity,
                                        &app_theme,
                                        self.runtime.thresholds.gpu.0,
                                        self.runtime.thresholds.gpu.1,
                                        self.runtime.thresholds.gpu_hotspot,
                                        self.runtime.thresholds.gpu_mem,
                                        scale,
                                        self.runtime.control.active_gpu_limit(&stats.gpu_name),
                                    );
                                    new_pref = r.0;
                                    r.1
                                }
                                "ram" => panels::ram::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    self.runtime.thresholds.ram.0,
                                    self.runtime.thresholds.ram.1,
                                    &app_theme,
                                    scale,
                                ),
                                "net" => panels::net::draw(
                                    ui,
                                    stats,
                                    nuspark,
                                    ndspark,
                                    content_opacity,
                                    &app_theme,
                                    scale,
                                ),
                                "disk" => panels::disk::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    self.runtime.thresholds.disk.0,
                                    self.runtime.thresholds.disk.1,
                                    self.runtime.thresholds.disk_usage.0,
                                    self.runtime.thresholds.disk_usage.1,
                                    &app_theme,
                                    scale,
                                ),
                                "motherboard" => {
                                    let r = panels::motherboard::draw(
                                        ui,
                                        stats,
                                        content_opacity,
                                        self.runtime.thresholds.mb,
                                        &app_theme,
                                        scale,
                                        control.has_fan_control(),
                                    );
                                    if take_fan_curve_request(ui.ctx()) {
                                        control_open_arc.store(true, Ordering::Relaxed);
                                        control_focus_arc.store(true, Ordering::Relaxed);
                                    }
                                    r
                                }
                                "process" => panels::process::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    &app_theme,
                                    scale,
                                ),
                                "gpu_processes" => panels::gpu_processes::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    &app_theme,
                                    scale,
                                ),
                                "power" => panels::power::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    &app_theme,
                                    scale,
                                    self.runtime.psu_watts,
                                ),
                                "peripherals" => panels::peripherals::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    &app_theme,
                                    scale,
                                    self.runtime.thresholds.battery.0,
                                    self.runtime.thresholds.battery.1,
                                ),
                                "battery" => panels::battery::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    &app_theme,
                                    scale,
                                    self.runtime.thresholds.battery.0,
                                    self.runtime.thresholds.battery.1,
                                    self.runtime.thresholds.battery_power.0,
                                    self.runtime.thresholds.battery_power.1,
                                ),
                                _ => egui::Rect::NOTHING,
                            };

                            if let Some(p) = new_pref {
                                *new_pref_arc.lock_safe() = Some(p);
                            }

                            // ── Drag zone: top 24 px of the panel inner area ──
                            // inner_margin top = 8 px; title row is ~20 px tall,
                            // so top+24 covers the title row comfortably.
                            let drag_zone = egui::Rect::from_min_max(
                                panel_rect.min,
                                egui::pos2(panel_rect.right(), panel_rect.top() + 24.0),
                            );
                            // Padlock hit area: right 20 px of the drag zone.
                            let padlock_cx = drag_zone.right() - 14.0;
                            let padlock_cy = drag_zone.center().y;
                            let padlock_center = egui::pos2(padlock_cx, padlock_cy);
                            let padlock_hit = egui::Rect::from_center_size(
                                padlock_center,
                                egui::Vec2::new(22.0, drag_zone.height()),
                            );

                            let in_drag_zone =
                                hover_pos.map(|p| drag_zone.contains(p)).unwrap_or(false);
                            let in_padlock =
                                hover_pos.map(|p| padlock_hit.contains(p)).unwrap_or(false);

                            // Drag trigger (whole drag zone minus padlock area).
                            if !locked && just_pressed && in_drag_zone && !in_padlock {
                                #[cfg(windows)]
                                if is_behind {
                                    win32_behind::prepare_for_drag(&win_title);
                                }
                                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                            }

                            // Hover-only response so a tooltip can be attached without
                            // interfering with the raw just_pressed drag trigger above.
                            if !locked && !in_padlock {
                                let drag_zone_resp = ui.interact(
                                    drag_zone,
                                    ui.id().with("panel_drag_zone"),
                                    egui::Sense::hover(),
                                );
                                let _ = drag_zone_resp.on_hover_text("Drag to move");
                            }

                            // Padlock interaction.
                            let padlock_resp = ui
                                .interact(
                                    padlock_hit,
                                    ui.id().with("padlock"),
                                    egui::Sense::click(),
                                )
                                .on_hover_text(if locked {
                                    "Click to unlock"
                                } else {
                                    "Click to lock position here"
                                });
                            if padlock_resp.clicked() {
                                lock_arc.store(!locked, Ordering::Relaxed);
                            }
                            if padlock_resp.hovered() {
                                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                            }

                            // ── Overlay: dots + padlock painted on the panel ──
                            let painter = ui.painter();

                            // Three dots — shown when unlocked and hovering the drag zone.
                            if in_drag_zone && !locked {
                                let cy = drag_zone.center().y;
                                let cx = drag_zone.center().x;
                                for i in [-5.0f32, 0.0, 5.0] {
                                    painter.circle_filled(
                                        egui::pos2(cx + i, cy),
                                        1.5,
                                        egui::Color32::from_gray(160),
                                    );
                                }
                            }

                            // Padlock: full icon on hover; tiny dot when locked + not hovering.
                            if in_drag_zone {
                                let padlock_color = if locked {
                                    app_theme.accent
                                } else if padlock_resp.hovered() {
                                    egui::Color32::from_gray(130)
                                } else {
                                    egui::Color32::from_gray(100)
                                };
                                draw_padlock(painter, padlock_center, locked, padlock_color);
                            } else if locked {
                                // Subtle locked indicator when not hovering — just a small dot.
                                painter.circle_filled(
                                    padlock_center,
                                    2.5,
                                    egui::Color32::from_rgba_unmultiplied(
                                        app_theme.accent.r(),
                                        app_theme.accent.g(),
                                        app_theme.accent.b(),
                                        120,
                                    ),
                                );
                            }

                            // Auto-resize height to content, but only when it actually
                            // changes — sending InnerSize every frame causes sub-pixel
                            // jitter that makes the panel blurry after dragging.
                            let used_h = ui.min_rect().height().round();
                            if used_h > 10.0 {
                                let current_h = ctx
                                    .input(|i| i.viewport().inner_rect)
                                    .map_or(0.0, |r| r.height().round());
                                if (used_h - current_h).abs() > 0.5 {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                        egui::Vec2::new(panel_w, used_h),
                                    ));
                                }
                            }
                            // A later content-driven resize deliberately does NOT
                            // retrigger the hide/burst below (unlike the overlay,
                            // which only resizes on a deliberate user action like
                            // dragging Scale) — several of these panels' heights
                            // fluctuate with live data (e.g. GPU Apps'/Processes'
                            // row count), so retriggering on every such change
                            // caused near-constant hide/reveal flicker, worse than
                            // just leaving the resize non-hiding as before.

                            // Apply per-pixel DComp transparency — full burst
                            // treatment (forced resize nudge, repeated reapply +
                            // force_repaint, hidden until settled), same shape as
                            // the overlay's fix, not just the style bit — or fall
                            // back to whole-window WS_EX_LAYERED dimming.
                            #[cfg(windows)]
                            {
                                if dcomp_available {
                                    let tick = dcomp.tick(
                                        || win_opacity::find_hwnd(&win_title),
                                        dcomp_available,
                                    );
                                    if tick.just_started {
                                        // Force a genuine resize/reconfigure on
                                        // creation — same reasoning as the
                                        // overlay's own first-burst-frame nudge.
                                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                            egui::Vec2::new(panel_w + 2.0, initial_h),
                                        ));
                                    }
                                    if tick.reapply_style {
                                        win_opacity::set_no_redirection_bitmap(dcomp.hwnd());
                                    }
                                    if tick.force_repaint {
                                        win_opacity::force_repaint(dcomp.hwnd());
                                        win_opacity::force_repaint(main_hwnd);
                                        ctx.request_repaint();
                                    }
                                    if tick.reveal {
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                                    }
                                } else {
                                    let hwnd = win_opacity::find_hwnd(&win_title);
                                    win_opacity::set_opacity(hwnd, opacity);
                                }
                            }
                        });
                },
            );
            self.floating.dcomp.borrow_mut().insert(key.clone(), dcomp);
        }
    }

    /// Flush `floating_positions` → `panel_layouts` in settings and persist to disk.
    pub(crate) fn persist_floating_positions(&self) {
        let positions = self.floating.positions.lock_safe();
        let mut s = self.current_settings.lock_safe();
        for (key, &[x, y]) in positions.iter() {
            s.panel_layouts.insert(
                key.clone(),
                settings::PanelLayout {
                    x: x as i32,
                    y: y as i32,
                },
            );
        }
        drop(positions);
        self.persist_settings_logged(&s);
    }
}
