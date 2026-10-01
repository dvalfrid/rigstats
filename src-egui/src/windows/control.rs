//! Control Center window (#187, phase 0) — profiles on the left, capability-
//! driven tabs on the right (only "Power" exists in phase 0, since
//! `PowerPlanProvider` is the only registered provider; later phases add
//! Fans/CPU/GPU/Lighting the same way, gated on `control.capabilities`
//! rather than a fixed tab list). Follows the dialog contract in
//! `src-egui/src/windows/CLAUDE.md`: three panels sharing `dialog_frame`,
//! `DialogColors`, `theme::dialog_btn_*`. Read-only over `ControlState` —
//! actions are fire-and-forget `ControlCmd`s sent to `control_task`; results
//! come back later as `ControlEvent`s the caller has already folded into
//! `control` by the time this renders (no local draft/mutex state needed,
//! unlike `settings.rs`).

use crate::theme::{self, DialogColors};
use rigstats_backend::control::{ControlCmd, ControlState};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
        .inner_margin(egui::Margin::symmetric(14, 12))
}

fn section_label(ui: &mut egui::Ui, dc: &DialogColors, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(11.0)
            .strong()
            .color(dc.label),
    );
}

#[allow(deprecated)]
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    main_ctx: &egui::Context,
    open: &Arc<AtomicBool>,
    needs_focus: &Arc<AtomicBool>,
    control: &ControlState,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    dc: &DialogColors,
) {
    dc.apply_to_ctx(ctx);
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    // ── Hero ─────────────────────────────────────────────────────────────
    egui::TopBottomPanel::top("control_hero")
        .frame(dialog_frame(dc).inner_margin(egui::Margin {
            left: 16,
            right: 16,
            top: 14,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Control Center")
                        .size(18.0)
                        .strong()
                        .color(dc.title),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (dot, text) = if control.protocol_mismatch.is_some() {
                        (
                            egui::Color32::from_rgb(0xff, 0x55, 0x55),
                            "Version mismatch".to_string(),
                        )
                    } else if control.connected {
                        (
                            egui::Color32::from_rgb(0x39, 0xff, 0x88),
                            "Connected".to_string(),
                        )
                    } else {
                        (dc.muted, "Connecting…".to_string())
                    };
                    ui.label(egui::RichText::new(text).size(11.0).color(dc.muted));
                    // Painted dot rather than a Unicode bullet glyph, which
                    // the embedded font doesn't have (renders as a tofu box)
                    // — same fix as the "Recording" indicator in
                    // windows/history.rs.
                    let (dot_rect, _) =
                        ui.allocate_exact_size(egui::vec2(8.0, 11.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 2.5, dot);
                });
            });
        });

    // ── Footer ───────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("control_footer")
        .frame(dialog_frame(dc).inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 8,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            if let Some(result) = &control.last_apply_result {
                if !result.ok {
                    ui.label(
                        egui::RichText::new(result.message.as_deref().unwrap_or("Apply failed."))
                            .size(11.0)
                            .color(egui::Color32::from_rgb(0xff, 0x77, 0x77)),
                    );
                    ui.add_space(6.0);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::dialog_btn_primary(ui, "Close").clicked() {
                    open.store(false, Ordering::Relaxed);
                    main_ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
            });
        });

    // ── Profiles (left) ──────────────────────────────────────────────────
    egui::SidePanel::left("control_profiles")
        .resizable(false)
        .exact_width(180.0)
        .frame(dialog_frame(dc).inner_margin(egui::Margin::same(10)))
        .show(ctx, |ui| {
            section_label(ui, dc, "Profiles");
            ui.add_space(6.0);
            if control.profiles.is_empty() {
                ui.label(
                    egui::RichText::new("No profiles yet.")
                        .size(11.0)
                        .color(dc.muted),
                );
            }
            for profile in &control.profiles {
                let is_active = control.active_profile.as_deref() == Some(profile.id.as_str());
                let resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new(&profile.name)
                            .size(13.0)
                            .color(if is_active { dc.title } else { dc.text }),
                    )
                    .fill(if is_active {
                        dc.card
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .min_size(egui::vec2(ui.available_width(), 28.0)),
                );
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() && !is_active {
                    let _ = cmd_tx.try_send(ControlCmd::ApplyProfile(profile.id.clone()));
                }
                ui.add_space(2.0);
            }
        });

    // ── Power tab (central) ──────────────────────────────────────────────
    // Only tab in phase 0 — gated on the "power_plan" capability actually
    // being reported, same as later phases will gate Fans/CPU/GPU/Lighting
    // on their own domains. A single always-shown tab doesn't need a tab
    // bar widget yet; add one here once a second domain exists (#188+).
    egui::CentralPanel::default().frame(dialog_frame(dc).inner_margin(egui::Margin::same(14))).show(ctx, |ui| {
        let power = control.capabilities.iter().find(|c| c.domain == "power_plan");
        match power {
            None if !control.connected => {
                ui.label(egui::RichText::new("Waiting for the RIGStats service…").size(12.0).color(dc.muted));
            }
            None => {
                ui.label(egui::RichText::new("No control capabilities reported.").size(12.0).color(dc.muted));
            }
            Some(cap) if !cap.supported => {
                ui.label(
                    egui::RichText::new(cap.reason.as_deref().unwrap_or("Power plan control unavailable."))
                        .size(12.0)
                        .color(dc.muted),
                );
            }
            Some(_) => {
                card_frame(dc).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    section_label(ui, dc, "Power");
                    ui.add_space(8.0);
                    let active_name = control
                        .active_profile
                        .as_deref()
                        .and_then(|id| control.profiles.iter().find(|p| p.id == id))
                        .map_or("—", |p| p.name.as_str());
                    ui.label(
                        egui::RichText::new(format!("Active power plan: {active_name}"))
                            .size(12.0)
                            .color(dc.text),
                    );
                    if control.dry_run {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new("Dry-run mode — the service is logging changes instead of applying them.")
                                .size(11.0)
                                .color(dc.muted),
                        );
                    }
                });
            }
        }
    });

    if ctx.input(|i| i.viewport().close_requested()) {
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}
