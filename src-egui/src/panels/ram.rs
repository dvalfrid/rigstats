use egui::{Color32, ProgressBar, RichText, Ui};

use crate::spark::Sparkline;
use crate::tempcolor::color_accent;
use crate::PollStats;

const SPARK_H: f32 = 36.0;
const GIB: f64 = 1_073_741_824.0;

fn safe_bar_width(ui: &Ui) -> f32 {
  let w = ui.available_width();
  if w.is_finite() && w > 0.0 { w } else { ui.ctx().content_rect().width().max(1.0) }
}

pub fn draw(ui: &mut Ui, stats: &PollStats, spark: &mut Sparkline) {
  let used_gb = stats.ram_used as f64 / GIB;
  let total_gb = stats.ram_total as f64 / GIB;
  let frac = if stats.ram_total > 0 {
    (stats.ram_used as f32) / (stats.ram_total as f32)
  } else {
    0.0
  };
  let pct = (frac * 100.0) as u8;
  spark.push(frac * 100.0);

  // Header
  ui.horizontal(|ui| {
    ui.label(RichText::new("RAM").strong());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
      ui.label(RichText::new(format!("{pct}%")).color(color_accent()));
    });
  });

  // Sparkline
  spark.draw(ui, SPARK_H, color_accent());

  // Bar
  let bw = safe_bar_width(ui);
  ui.add(ProgressBar::new(frac).desired_width(bw));

  // Usage line
  ui.label(format!("{used_gb:.1} / {total_gb:.1} GB"));

  // Spec line
  if !stats.ram_spec.is_empty() {
    ui.label(RichText::new(&stats.ram_spec).small().color(Color32::from_gray(150)));
  }
}
