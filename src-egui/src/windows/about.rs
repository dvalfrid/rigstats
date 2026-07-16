use crate::theme::{self, DialogColors};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const VERSION: &str = env!("CARGO_PKG_VERSION");

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

fn link_row(ui: &mut egui::Ui, dc: &DialogColors, label: &str, display: &str, url: &str) {
    ui.horizontal(|ui| {
        ui.set_min_width(ui.available_width());
        ui.add_sized(
            [56.0, 16.0],
            egui::Label::new(egui::RichText::new(label).size(11.0).color(dc.muted)),
        );
        ui.add(egui::Hyperlink::from_label_and_url(
            egui::RichText::new(display).size(12.0).color(dc.link),
            url,
        ));
    });
}

#[allow(deprecated)]
pub fn show(
    ctx: &egui::Context,
    main_ctx: &egui::Context,
    open: &Arc<AtomicBool>,
    needs_focus: &Arc<AtomicBool>,
    _dir: &Arc<PathBuf>,
    dc: &DialogColors,
) {
    dc.apply_to_ctx(ctx);
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    // ── Top: hero ─────────────────────────────────────────────────────────────
    egui::TopBottomPanel::top("about_top")
        .frame(dialog_frame(dc).inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 24,
            bottom: 16,
        }))
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("RigStats")
                        .size(30.0)
                        .strong()
                        .color(dc.title),
                );
                ui.add_space(3.0);
                ui.label(
                    egui::RichText::new(format!("v{VERSION}"))
                        .size(12.0)
                        .color(dc.muted),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Hardware stats dashboard for portrait secondary monitors")
                        .size(12.0)
                        .color(dc.muted),
                );
            });
        });

    // ── Bottom: footer ────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("about_bottom")
        .frame(dialog_frame(dc).inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 8,
            bottom: 12,
        }))
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::dialog_btn_primary(ui, "Close").clicked() {
                    open.store(false, Ordering::Relaxed);
                    main_ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
            });
        });

    // ── Centre: cards ─────────────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(dialog_frame(dc).inner_margin(egui::Margin::symmetric(14, 4)))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;

            // Links & License card
            card_frame(dc).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    egui::RichText::new("Links & License")
                        .size(11.0)
                        .color(dc.label),
                );
                ui.add_space(8.0);

                link_row(ui, dc, "Website", "rigstats.app", "https://rigstats.app");
                ui.add(egui::Separator::default().spacing(6.0));
                link_row(
                    ui,
                    dc,
                    "GitHub",
                    "github.com/dvalfrid/rigstats",
                    "https://github.com/dvalfrid/rigstats",
                );
                ui.add(egui::Separator::default().spacing(6.0));
                link_row(
                    ui,
                    dc,
                    "Email",
                    "daniel@valfridsson.net",
                    "mailto:daniel@valfridsson.net",
                );
                ui.add(egui::Separator::default().spacing(6.0));
                link_row(
                    ui,
                    dc,
                    "License",
                    "MIT License",
                    "https://github.com/dvalfrid/rigstats/blob/main/LICENSE",
                );
            });

            // Built with card — wrapping text, no horizontal overflow
            card_frame(dc).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(egui::RichText::new("Built with").size(11.0).color(dc.label));
                ui.add_space(6.0);
                ui.vertical_centered(|ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Rust\u{a0}\u{a0}·  egui\u{a0}/\u{a0}eframe\u{a0}\u{a0}·  sysinfo\u{a0}\u{a0}·  WMI\u{a0}\u{a0}·  LibreHardwareMonitor",
                            )
                            .size(12.0)
                            .color(dc.text),
                        )
                        .halign(egui::Align::Center),
                    );
                });
            });
        });

    if ctx.input(|i| i.viewport().close_requested()) {
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}
