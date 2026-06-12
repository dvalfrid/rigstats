use std::collections::VecDeque;

use egui::{Color32, RichText, Sense, Stroke, Ui, Vec2};

use crate::spark::Sparkline;
use crate::theme;
use crate::PollStats;

const SPARK_H: f32 = 40.0;

/// Split into (number_string, unit_string) so the caller can right-align
/// the number in a fixed-width box while the unit starts at a fixed position.
fn fmt_mbps_parts(v: f64) -> (String, &'static str) {
    if v >= 1000.0 {
        (format!("{:.2}", v / 1000.0), "Gbps")
    } else if v >= 1.0 {
        (format!("{v:.1}"), "Mbps")
    } else {
        (format!("{:.0}", v * 1000.0), "Kbps")
    }
}

/// Draw two sparklines on the same canvas (both with gradient fill).
fn draw_dual(
    ui: &mut Ui,
    height: f32,
    up: &Sparkline,
    dn: &Sparkline,
    up_color: Color32,
    dn_color: Color32,
) {
    let w = {
        let w = ui.available_width();
        if w.is_finite() && w > 0.0 {
            w
        } else {
            ui.ctx().content_rect().width().max(1.0)
        }
    };
    let (resp, painter) = ui.allocate_painter(Vec2::new(w, height), Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 0.0, Color32::from_gray(18));

    // Shared scale so both series are comparable.
    let up_max = up.values().iter().cloned().fold(0.0f32, f32::max);
    let dn_max = dn.values().iter().cloned().fold(0.0f32, f32::max);
    let shared_max = up_max.max(dn_max).max(1.0);

    let eff_h = (rect.height() - 4.0).max(0.0) * 0.88;

    let make_points = |vals: &VecDeque<f32>| -> Vec<egui::Pos2> {
        let n = vals.len();
        if n < 2 {
            return Vec::new();
        }
        vals.iter()
            .enumerate()
            .map(|(i, &v)| {
                let x = rect.left() + (i as f32 / (n - 1) as f32) * rect.width();
                let y = rect.bottom() - 4.0 - (v / shared_max).clamp(0.0, 1.0) * eff_h;
                egui::Pos2::new(x, y)
            })
            .collect()
    };

    let fill_series = |pts: &[egui::Pos2], color: Color32| {
        if pts.len() < 2 {
            return None;
        }
        let fill_y = rect.bottom();
        let mut mesh = egui::Mesh::default();
        for pair in pts.windows(2) {
            let (p0, p1) = (pair[0], pair[1]);
            let a0 = (((fill_y - p0.y) / rect.height()) * 0x55 as f32) as u8;
            let a1 = (((fill_y - p1.y) / rect.height()) * 0x55 as f32) as u8;
            let c0 = crate::spark::premul_color(color, a0);
            let c1 = crate::spark::premul_color(color, a1);
            let v = mesh.vertices.len() as u32;
            mesh.colored_vertex(p0, c0);
            mesh.colored_vertex(p1, c1);
            mesh.colored_vertex(egui::Pos2::new(p0.x, fill_y), Color32::TRANSPARENT);
            mesh.colored_vertex(egui::Pos2::new(p1.x, fill_y), Color32::TRANSPARENT);
            mesh.add_triangle(v, v + 2, v + 1);
            mesh.add_triangle(v + 1, v + 2, v + 3);
        }
        Some((
            mesh,
            egui::Shape::line(pts.to_vec(), Stroke::new(1.5, color)),
        ))
    };

    let dn_pts = make_points(dn.values());
    let up_pts = make_points(up.values());

    // Draw DOWN first (behind), then UP on top.
    if let Some((mesh, line)) = fill_series(&dn_pts, dn_color) {
        painter.add(egui::Shape::mesh(mesh));
        painter.add(line);
    }
    if let Some((mesh, line)) = fill_series(&up_pts, up_color) {
        painter.add(egui::Shape::mesh(mesh));
        painter.add(line);
    }
}

pub fn draw(
    ui: &mut Ui,
    stats: &PollStats,
    up_spark: &Sparkline,
    dn_spark: &Sparkline,
    opacity: f32,
    th: &theme::AppTheme,
    sc: f32,
) -> egui::Rect {
    theme::panel_frame(ui, opacity, th, sc, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H * sc);

        ui.label(
            RichText::new("NETWORK")
                .strong()
                .color(theme::C_PANEL_TITLE)
                .size(theme::FONT_PANEL_TITLE * sc),
        );
        ui.add_space(4.0 * sc);

        let number_w = 68.0 * sc;
        ui.columns(2, |cols| {
            let (val, unit) = fmt_mbps_parts(stats.net_up_mbps);
            // Arrow right-aligned in NUMBER_W box, then "UP" starts at fixed x.
            cols[0].horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(number_w, 14.0 * sc),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        theme::arrow_up(ui, 10.0 * sc, theme::C_GRN);
                    },
                );
                ui.label(RichText::new("UP").size(11.0 * sc).color(theme::C_GRN));
            });
            // Number right-aligned in same NUMBER_W box, unit starts at same x.
            cols[0].horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0 * sc;
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(number_w, 24.0 * sc),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.label(
                            RichText::new(&val)
                                .size(20.0 * sc)
                                .color(egui::Color32::WHITE),
                        );
                    },
                );
                ui.label(RichText::new(unit).size(11.0 * sc).color(th.text_muted));
            });

            let (val, unit) = fmt_mbps_parts(stats.net_down_mbps);
            cols[1].horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(number_w, 14.0 * sc),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        theme::arrow_down(ui, 10.0 * sc, theme::C_NET_DOWN);
                    },
                );
                ui.label(
                    RichText::new("DOWN")
                        .size(11.0 * sc)
                        .color(theme::C_NET_DOWN),
                );
            });
            cols[1].horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0 * sc;
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(number_w, 24.0 * sc),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.label(
                            RichText::new(&val)
                                .size(20.0 * sc)
                                .color(egui::Color32::WHITE),
                        );
                    },
                );
                ui.label(RichText::new(unit).size(11.0 * sc).color(th.text_muted));
            });
        });

        ui.add_space(4.0 * sc);

        // PING + IFACE — above the sparkline.
        ui.horizontal(|ui| {
            if let Some(p) = stats.net_ping_ms {
                ui.label(RichText::new("PING").size(11.0 * sc).color(th.stat_label));
                ui.label(
                    RichText::new(format!("{p:.0} ms"))
                        .size(11.0 * sc)
                        .color(theme::C_TEXT),
                );
                ui.add_space(12.0 * sc);
            }
            if !stats.net_iface.is_empty() {
                ui.label(RichText::new("IFACE").size(11.0 * sc).color(th.stat_label));
                ui.label(
                    RichText::new(&stats.net_iface)
                        .size(11.0 * sc)
                        .color(theme::C_TEXT),
                );
            }
        });

        // Push sparkline to bottom using cursor tracking.
        let cursor_y = ui.cursor().top();
        let filler =
            (theme::PANEL_DATA_H * sc - (cursor_y - ui.min_rect().top()) - SPARK_H * sc).max(0.0);
        ui.add_space(filler);

        draw_dual(
            ui,
            SPARK_H * sc,
            up_spark,
            dn_spark,
            theme::C_GRN,
            theme::C_NET_DOWN,
        );
    })
}

#[cfg(test)]
mod tests {
    use super::fmt_mbps_parts;

    #[test]
    fn below_one_mbps_formats_as_kbps() {
        let (v, u) = fmt_mbps_parts(0.5);
        assert_eq!(u, "Kbps");
        assert_eq!(v, "500");
    }

    #[test]
    fn one_mbps_formats_as_mbps() {
        let (v, u) = fmt_mbps_parts(1.0);
        assert_eq!(u, "Mbps");
        assert_eq!(v, "1.0");
    }

    #[test]
    fn below_1000_mbps_formats_as_mbps() {
        let (v, u) = fmt_mbps_parts(350.7);
        assert_eq!(u, "Mbps");
        assert_eq!(v, "350.7");
    }

    #[test]
    fn at_1000_mbps_formats_as_gbps() {
        let (v, u) = fmt_mbps_parts(1000.0);
        assert_eq!(u, "Gbps");
        assert_eq!(v, "1.00");
    }

    #[test]
    fn above_1000_mbps_formats_as_gbps() {
        let (v, u) = fmt_mbps_parts(2400.0);
        assert_eq!(u, "Gbps");
        assert_eq!(v, "2.40");
    }
}
