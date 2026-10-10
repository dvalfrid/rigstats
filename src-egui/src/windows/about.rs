//! About window: the app's name and version, then links and what it is
//! built with as grouped rows (`ui_kit`).

use crate::theme::{self, DialogColors};
use crate::windows::ui_kit;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Links: what, the text shown, where it goes.
const LINKS: &[(&str, &str, &str)] = &[
    ("Website", "rigstats.app", "https://rigstats.app"),
    (
        "Source code",
        "github.com/dvalfrid/rigstats",
        "https://github.com/dvalfrid/rigstats",
    ),
    (
        "Contact",
        "daniel@valfridsson.net",
        "mailto:daniel@valfridsson.net",
    ),
    (
        "License",
        "MIT License",
        "https://github.com/dvalfrid/rigstats/blob/main/LICENSE",
    ),
];

// The ctx-level panel API, as in every other dialog.
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

    // ── Name and version, centred ────────────────────────────────────────
    egui::TopBottomPanel::top("about_top")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 26,
            bottom: 18,
        }))
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("RIGStats")
                        .size(28.0)
                        .strong()
                        .color(dc.title),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("Version {VERSION}"))
                        .size(12.0)
                        .color(dc.muted),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Hardware monitor and control center for Windows")
                        .size(12.0)
                        .color(dc.text),
                );
            });
        });

    // ── Footer ───────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("about_bottom")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 10,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::dialog_btn_primary(ui, "Close").clicked() {
                    open.store(false, Ordering::Relaxed);
                    main_ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
            });
        });

    // ── Links and credits ────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin::symmetric(20, 4)))
        .show(ctx, |ui| {
            ui_kit::group(ui, dc, None, None, |g| {
                for &(title, text, url) in LINKS {
                    g.row(title, None, |ui| {
                        ui.add(egui::Hyperlink::from_label_and_url(
                            egui::RichText::new(text).size(12.0).color(dc.link),
                            url,
                        ));
                    });
                }
            });
            ui_kit::group(ui, dc, Some("Built with"), None, |g| {
                g.block(|ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Rust, egui and eframe, sysinfo, WMI and LibreHardwareMonitor.",
                            )
                            .size(12.0)
                            .color(dc.text),
                        )
                        .wrap(),
                    );
                });
            });
        });

    if ctx.input(|i| i.viewport().close_requested()) {
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}
