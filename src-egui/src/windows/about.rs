use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── Colour tokens (dialog design system) ─────────────────────────────────────
const C_BG: egui::Color32 = egui::Color32::from_gray(38);
const C_CARD: egui::Color32 = egui::Color32::from_gray(27);
const C_CARD_BORDER: egui::Color32 = egui::Color32::from_gray(55);
const C_TEXT: egui::Color32 = egui::Color32::from_rgb(155, 180, 210);
const C_MUTED: egui::Color32 = egui::Color32::from_gray(115);
const C_LABEL: egui::Color32 = egui::Color32::from_gray(140);
const C_LINK: egui::Color32 = egui::Color32::from_rgb(80, 160, 220);
const C_TITLE: egui::Color32 = egui::Color32::from_rgb(210, 220, 235);

fn dialog_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(C_BG)
        .inner_margin(egui::Margin::same(0))
}

fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(C_CARD)
        .stroke(egui::Stroke::new(1.0, C_CARD_BORDER))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(14, 12))
}

fn link_row(ui: &mut egui::Ui, label: &str, display: &str, url: &str) {
    ui.horizontal(|ui| {
        ui.set_min_width(ui.available_width());
        ui.add_sized(
            [56.0, 16.0],
            egui::Label::new(egui::RichText::new(label).size(11.0).color(C_MUTED)),
        );
        ui.add(egui::Hyperlink::from_label_and_url(
            egui::RichText::new(display).size(12.0).color(C_LINK),
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
) {
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    // ── Top: hero ─────────────────────────────────────────────────────────────
    egui::TopBottomPanel::top("about_top")
        .frame(dialog_frame().inner_margin(egui::Margin {
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
                        .color(C_TITLE),
                );
                ui.add_space(3.0);
                ui.label(
                    egui::RichText::new(format!("v{VERSION}"))
                        .size(12.0)
                        .color(C_MUTED),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "Hardware stats dashboard for portrait secondary monitors",
                    )
                    .size(12.0)
                    .color(egui::Color32::from_gray(128)),
                );
            });
        });

    // ── Bottom: footer ────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("about_bottom")
        .frame(dialog_frame().inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 8,
            bottom: 12,
        }))
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::theme::dialog_btn_primary(ui, "Close").clicked() {
                    open.store(false, Ordering::Relaxed);
                    main_ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
            });
        });

    // ── Centre: cards ─────────────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(dialog_frame().inner_margin(egui::Margin::symmetric(14, 4)))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;

            // Links & License card
            card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    egui::RichText::new("Links & License")
                        .size(11.0)
                        .color(C_LABEL),
                );
                ui.add_space(8.0);

                link_row(
                    ui,
                    "GitHub",
                    "github.com/dvalfrid/rigstats",
                    "https://github.com/dvalfrid/rigstats",
                );
                ui.add(egui::Separator::default().spacing(6.0));
                link_row(
                    ui,
                    "Email",
                    "daniel@valfridsson.net",
                    "mailto:daniel@valfridsson.net",
                );
                ui.add(egui::Separator::default().spacing(6.0));
                link_row(
                    ui,
                    "License",
                    "MIT License",
                    "https://github.com/dvalfrid/rigstats/blob/main/LICENSE",
                );
            });

            // Built with card — wrapping text, no horizontal overflow
            card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    egui::RichText::new("Built with")
                        .size(11.0)
                        .color(C_LABEL),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Rust  ·  egui / eframe  ·  sysinfo  ·  WMI  ·  LibreHardwareMonitor",
                    )
                    .size(12.0)
                    .color(C_TEXT),
                );
            });
        });

    if ctx.input(|i| i.viewport().close_requested()) {
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}
