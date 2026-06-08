use egui::{RichText, Ui};

use crate::spark::Sparkline;
use crate::theme;
use crate::PollStats;

const SPARK_H: f32 = 32.0;
const GIB: f64 = 1_073_741_824.0;

pub fn draw(ui: &mut Ui, stats: &PollStats, spark: &Sparkline) {
  theme::panel_frame(ui, theme::C_RAM, |ui| {
    let used_gb = stats.ram_used as f64 / GIB;
    let total_gb = stats.ram_total as f64 / GIB;
    let frac =
      if stats.ram_total > 0 { (stats.ram_used as f32) / (stats.ram_total as f32) } else { 0.0 };
    let pct = (frac * 100.0) as u8;

    // Header
    ui.label(RichText::new("RAM USAGE").strong().color(theme::C_TEXT).size(13.0));

    // Large number
    ui.horizontal(|ui| {
      ui.label(RichText::new(format!("{used_gb:.1}")).size(48.0).color(theme::C_RAM));
      ui.vertical(|ui| {
        ui.add_space(20.0);
        ui.label(RichText::new(format!("/ {total_gb:.0} GB")).size(18.0).color(theme::C_TEXT_MUTED));
      });
    });

    // Spec line
    if !stats.ram_spec.is_empty() {
      ui.label(RichText::new(&stats.ram_spec).small().color(theme::C_TEXT_MUTED));
    }

    ui.add_space(4.0);

    // Bar — compute remaining width AFTER placing the "MEM" label.
    ui.horizontal(|ui| {
      ui.label(RichText::new("MEM").small().color(theme::C_STAT_LABEL));
      // Reserve ~32 px for the "XX%" label that follows.
      let bar_w = (ui.available_width() - 32.0).max(4.0);
      theme::thin_bar(ui, frac, bar_w, theme::C_RAM);
      ui.label(RichText::new(format!("{pct}%")).small().color(theme::C_RAM));
    });

    // Sparkline
    ui.add_space(2.0);
    spark.draw(ui, SPARK_H, theme::C_RAM);
  });
}
