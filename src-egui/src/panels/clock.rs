use egui::{RichText, Ui};

use crate::theme;

pub fn format_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("UP {h:02}:{m:02}:{s:02}")
}

pub fn draw(
    ui: &mut Ui,
    uptime_secs: u64,
    opacity: f32,
    th: &theme::AppTheme,
    update_version: Option<&str>,
    sc: f32,
    // The compact layout (#604): TIME, DATE and UPTIME in one row of columns.
    compact: bool,
) -> egui::Rect {
    theme::panel_frame(ui, opacity, th, sc, |ui| {
        let now = chrono::Local::now();
        if compact {
            use super::CompactCell;
            let stat = |label: &'static str, value: String| CompactCell::Stat {
                label: label.into(),
                label_color: th.stat_label,
                value,
                value_color: theme::C_VALUE,
                note: None,
            };
            super::compact_row(
                ui,
                &[
                    stat("TIME", now.format("%H:%M:%S").to_string()),
                    stat("DATE", now.format("%a %-d %b").to_string()),
                    stat("UPTIME", format_uptime(uptime_secs).replace("UP ", "")),
                ],
                th,
                sc,
            );
            update_badge(ui, update_version, sc, true);
            return;
        }
        ui.set_min_height(theme::PANEL_HEADER_H * sc);

        // ── Main row: big time (left) + date/uptime stack (right) ────────────
        ui.horizontal(|ui| {
            // Big time
            ui.label(
                RichText::new(now.format("%H:%M:%S").to_string())
                    .size(40.0 * sc)
                    .color(egui::Color32::WHITE),
            );

            // Right column: date on top, uptime below
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.add_space(4.0 * sc);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.label(
                            RichText::new(now.format("%Y·%m·%d").to_string())
                                .size(11.0 * sc)
                                .color(th.text_muted),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.label(
                            RichText::new(format_uptime(uptime_secs))
                                .size(11.0 * sc)
                                .color(theme::C_GRN),
                        );
                    });
                });
            });
        });

        // ── Day name ─────────────────────────────────────────────────────────
        ui.label(
            RichText::new(now.format("%A").to_string().to_uppercase())
                .size(15.0 * sc)
                .color(theme::C_GRN),
        );

        update_badge(ui, update_version, sc, false);
    })
}

/// "UPDATE vX AVAILABLE" under the time while an update is ready; a click
/// sets the `"open_updater"` temp flag. Painted, arrow included: the `⬆`
/// character came from the emoji font as a blue tile. `centered` under the
/// compact layout's centred columns, else at the left edge like the rest of
/// the full panel.
fn update_badge(ui: &mut Ui, update_version: Option<&str>, sc: f32, centered: bool) {
    let Some(ver) = update_version else {
        return;
    };
    let green = egui::Color32::from_rgb(0x39, 0xff, 0x88);
    let galley = ui.painter().layout_no_wrap(
        format!("UPDATE v{ver} AVAILABLE"),
        egui::FontId::proportional(11.0 * sc),
        green,
    );
    let arrow = 8.0 * sc;
    let gap = 6.0 * sc;
    let pad = egui::vec2(8.0 * sc, 4.0 * sc);
    let size = egui::vec2(
        pad.x * 2.0 + arrow + gap + galley.size().x,
        pad.y * 2.0 + galley.size().y.max(arrow),
    );

    ui.add_space(if centered { 8.0 } else { 6.0 } * sc);
    let align = if centered {
        egui::Align::Center
    } else {
        egui::Align::Min
    };
    let resp = ui
        .with_layout(egui::Layout::top_down(align), |ui| {
            ui.allocate_exact_size(size, egui::Sense::click()).1
        })
        .inner;
    let rect = resp.rect;
    let hot = resp.hovered();
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(2),
        green.gamma_multiply(if hot { 0.18 } else { 0.10 }),
        egui::Stroke::new(1.0, green.gamma_multiply(if hot { 0.75 } else { 0.45 })),
        egui::StrokeKind::Inside,
    );
    // An up arrow: a head over a short stem, as tall as the capitals.
    let c = egui::pos2(rect.left() + pad.x + arrow / 2.0, rect.center().y);
    let half = arrow / 2.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x, c.y - half),
            egui::pos2(c.x + half, c.y),
            egui::pos2(c.x - half, c.y),
        ],
        green,
        egui::Stroke::NONE,
    ));
    painter.line_segment(
        [egui::pos2(c.x, c.y - 0.5), egui::pos2(c.x, c.y + half)],
        egui::Stroke::new(2.0 * sc, green),
    );
    painter.galley(
        egui::pos2(
            rect.left() + pad.x + arrow + gap,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        green,
    );

    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::unique("open_updater"), true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_seconds() {
        assert_eq!(format_uptime(0), "UP 00:00:00");
    }

    #[test]
    fn one_minute_one_second() {
        assert_eq!(format_uptime(61), "UP 00:01:01");
    }

    #[test]
    fn one_hour() {
        assert_eq!(format_uptime(3600), "UP 01:00:00");
    }

    #[test]
    fn hours_exceed_24() {
        assert_eq!(format_uptime(90000), "UP 25:00:00");
    }

    #[test]
    fn zero_padding() {
        assert_eq!(format_uptime(3661), "UP 01:01:01");
    }
}
