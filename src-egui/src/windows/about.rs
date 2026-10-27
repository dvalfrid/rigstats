//! About window: the app's name and version, then links and what it is
//! built with as grouped rows (`ui_kit`).

use crate::theme::{self, DialogColors};
use crate::windows::ui_kit;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const VERSION: &str = crate::update_check::APP_VERSION;

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
    // The one place in the app that mentions support: a link here, no
    // reminder anywhere else (#631).
    (
        "Support",
        "Buy me a coffee",
        "https://www.buymeacoffee.com/dvalfrid",
    ),
];

/// A small cup in Buy Me a Coffee's yellow, painted so it stays sharp at any
/// DPI (#650): handle, then the body over it, then the lid.
fn coffee_cup(ui: &mut egui::Ui, dc: &DialogColors) {
    use egui::{pos2, vec2, Color32, Rect, Shape, Stroke};
    let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
    let p = ui.painter();
    let at = |x: f32, y: f32| pos2(r.left() + x, r.top() + y);
    let yellow = Color32::from_rgb(255, 221, 0);
    let edge = Stroke::new(1.0, dc.title);
    p.circle_stroke(at(10.5, 7.5), 2.5, Stroke::new(1.5, dc.title));
    p.add(Shape::convex_polygon(
        vec![at(2.0, 4.0), at(11.0, 4.0), at(9.5, 13.0), at(3.5, 13.0)],
        yellow,
        edge,
    ));
    p.rect(
        Rect::from_min_max(at(1.0, 2.0), at(12.0, 4.0)),
        1.0,
        yellow,
        edge,
        egui::StrokeKind::Middle,
    );
}

pub fn show(
    ui: &mut egui::Ui,
    main_ctx: &egui::Context,
    open: &Arc<AtomicBool>,
    needs_focus: &Arc<AtomicBool>,
    _dir: &Arc<PathBuf>,
    dc: &DialogColors,
) {
    let ctx = &ui.ctx().clone();
    dc.apply_to_ctx(ctx);
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    // ── Name and version, centred ────────────────────────────────────────
    egui::Panel::top("about_top")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 26,
            bottom: 18,
        }))
        .show_separator_line(false)
        .show(ui, |ui| {
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
    egui::Panel::bottom("about_bottom")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 10,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ui, |ui| {
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
        .show(ui, |ui| {
            ui_kit::group(ui, dc, None, None, |g| {
                for &(title, text, url) in LINKS {
                    g.row(title, None, |ui| {
                        ui.add(egui::Hyperlink::from_label_and_url(
                            egui::RichText::new(text).size(12.0).color(dc.link),
                            url,
                        ));
                        // Right-to-left row: this lands left of the link.
                        if url.contains("buymeacoffee") {
                            coffee_cup(ui, dc);
                        }
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
