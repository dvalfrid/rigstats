use std::collections::VecDeque;

use egui::{Color32, Sense, Stroke, Ui, Vec2};

pub struct Sparkline {
  values: VecDeque<f32>,
  capacity: usize,
  max_value: f32,
}

impl Sparkline {
  pub fn new(capacity: usize, max_value: f32) -> Self {
    Self { values: VecDeque::with_capacity(capacity), capacity, max_value }
  }

  pub fn push(&mut self, value: f32) {
    if self.values.len() >= self.capacity {
      self.values.pop_front();
    }
    self.values.push_back(value);
  }

  /// Draw the sparkline spanning the full available width at the given height.
  /// Uses `ctx.content_rect().width()` as fallback when the Ui width is NaN
  /// (which happens in eframe 0.34's root `App::ui` on the first frame).
  pub fn draw(&self, ui: &mut Ui, height: f32, color: Color32) {
    let w = {
      let w = ui.available_width();
      if w.is_finite() && w > 0.0 { w } else { ui.ctx().content_rect().width().max(1.0) }
    };
    let (response, painter) = ui.allocate_painter(Vec2::new(w, height), Sense::hover());
    let rect = response.rect;

    painter.rect_filled(rect, 0.0, Color32::from_gray(18));

    let n = self.values.len();
    if n < 2 {
      return;
    }

    let points: Vec<egui::Pos2> = self
      .values
      .iter()
      .enumerate()
      .map(|(i, &v)| {
        let x = rect.left() + (i as f32 / (n - 1) as f32) * rect.width();
        let y = rect.bottom() - (v / self.max_value).clamp(0.0, 1.0) * rect.height();
        egui::Pos2::new(x, y)
      })
      .collect();

    painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));
  }
}
