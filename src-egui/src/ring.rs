use egui::{Color32, FontId, Painter, Pos2, Sense, Stroke, Ui, Vec2};
use std::f32::consts::TAU;

const TRACK_COLOR: Color32 = Color32::from_rgba_premultiplied(10, 10, 10, 10);

/// Allocate `size × size` and draw a ring gauge with a centered label.
/// `font_size` scales the center label.
/// Returns the allocated Rect.
pub fn show(ui: &mut Ui, size: f32, frac: f32, color: Color32, label: &str) -> egui::Rect {
    let (resp, painter) = ui.allocate_painter(Vec2::splat(size), Sense::hover());
    let center = resp.rect.center();
    let stroke_w = (size * 0.11).max(3.0);
    let radius = size / 2.0 - stroke_w / 2.0 - 2.0;
    let font_size = (size * 0.26).max(8.0);
    draw(
        &painter, center, radius, frac, color, label, font_size, stroke_w,
    );
    resp.rect
}

/// Draw a ring gauge (no allocation — caller provides center + radius).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    frac: f32,
    color: Color32,
    label: &str,
    font_size: f32,
    stroke_w: f32,
) {
    // Background ring
    painter.circle_stroke(center, radius, Stroke::new(stroke_w, TRACK_COLOR));

    // Foreground arc
    let clamped = frac.clamp(0.0, 1.0);
    if clamped > 0.001 {
        let n = 80usize;
        let start = -TAU / 4.0; // 12 o'clock
        let sweep = clamped * TAU;
        let points: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = start + (i as f32 / n as f32) * sweep;
                Pos2::new(center.x + radius * a.cos(), center.y + radius * a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(stroke_w, color)));

        // Round caps: small filled circle at arc start and end
        let cap_r = stroke_w / 2.0;
        painter.circle_filled(
            Pos2::new(
                center.x + radius * start.cos(),
                center.y + radius * start.sin(),
            ),
            cap_r,
            color,
        );
        let end = start + sweep;
        painter.circle_filled(
            Pos2::new(center.x + radius * end.cos(), center.y + radius * end.sin()),
            cap_r,
            color,
        );
    }

    // Center label
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(font_size),
        Color32::WHITE,
    );
}
