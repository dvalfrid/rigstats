use egui::{RichText, Ui};

use crate::theme;

pub fn draw(
    ui: &mut Ui,
    uptime_secs: u64,
    opacity: f32,
    th: &theme::AppTheme,
    update_version: Option<&str>,
) -> egui::Rect {
    theme::panel_frame(ui, opacity, th, |ui| {
        ui.set_min_height(theme::PANEL_HEADER_H);
        let now = chrono::Local::now();

        // ── Main row: big time (left) + date/uptime stack (right) ────────────
        ui.horizontal(|ui| {
            // Big time
            ui.label(
                RichText::new(now.format("%H:%M:%S").to_string())
                    .size(40.0)
                    .color(egui::Color32::WHITE),
            );

            // Right column: date on top, uptime below
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.label(
                            RichText::new(now.format("%Y·%m·%d").to_string())
                                .small()
                                .color(th.text_muted),
                        );
                    });
                    let h = uptime_secs / 3600;
                    let m = (uptime_secs % 3600) / 60;
                    let s = uptime_secs % 60;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.label(
                            RichText::new(format!("UP {h:02}:{m:02}:{s:02}"))
                                .small()
                                .color(theme::C_GRN),
                        );
                    });
                });
            });
        });

        // ── Day name ─────────────────────────────────────────────────────────
        ui.label(
            RichText::new(now.format("%A").to_string().to_uppercase())
                .size(15.0)
                .color(theme::C_GRN),
        );

        // ── Update badge ─────────────────────────────────────────────────────
        if let Some(ver) = update_version {
            let label = format!("⬆  UPDATE v{ver} AVAILABLE");
            let badge_color = egui::Color32::from_rgb(0x39, 0xff, 0x88);
            let border_color = egui::Color32::from_rgba_unmultiplied(0x39, 0xff, 0x88, 115);
            let bg_color = egui::Color32::from_rgba_unmultiplied(0x39, 0xff, 0x88, 26);

            ui.add_space(4.0);
            let resp = ui.add(
                egui::Button::new(RichText::new(&label).small().color(badge_color))
                    .fill(bg_color)
                    .stroke(egui::Stroke::new(1.0, border_color))
                    .corner_radius(egui::CornerRadius::same(2)),
            );
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(egui::Id::new("open_updater"), true));
            }
        }
    })
}
