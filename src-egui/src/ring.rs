use egui::{Color32, FontId, Painter, Pos2, Sense, Stroke, Ui, Vec2};
use std::f32::consts::TAU;

const STROKE_W: f32 = 7.0;
// 4% white — from_rgba_unmultiplied is not const, so we use from_rgba_premultiplied
// with pre-divided RGB: 255 * (10/255) ≈ 10, already negligible vs 0
const TRACK_COLOR: Color32 = Color32::from_rgba_premultiplied(10, 10, 10, 10);

/// Allocate `size × size` and draw a ring gauge with a centered label.
/// Returns the allocated Rect.
pub fn show(ui: &mut Ui, size: f32, frac: f32, color: Color32, label: &str) -> egui::Rect {
  let (resp, painter) = ui.allocate_painter(Vec2::splat(size), Sense::hover());
  let center = resp.rect.center();
  let radius = size / 2.0 - STROKE_W / 2.0 - 2.0;
  draw(&painter, center, radius, frac, color, label);
  resp.rect
}

/// Draw a ring gauge (no allocation — caller provides center + radius).
pub fn draw(painter: &Painter, center: Pos2, radius: f32, frac: f32, color: Color32, label: &str) {
  // Background ring
  painter.circle_stroke(center, radius, Stroke::new(STROKE_W, TRACK_COLOR));

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
    painter.add(egui::Shape::line(points, Stroke::new(STROKE_W, color)));

    // Round caps: small filled circle at arc start and end
    let cap_r = STROKE_W / 2.0;
    painter.circle_filled(
      Pos2::new(center.x + radius * start.cos(), center.y + radius * start.sin()),
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
    FontId::proportional(16.0),
    Color32::WHITE,
  );
}
