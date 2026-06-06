use egui::{Color32, ProgressBar, RichText, Ui};

use crate::spark::Sparkline;
use crate::tempcolor::{color_accent, temp_color};
use crate::PollStats;

const SPARK_H: f32 = 36.0;
const LABEL_COLOR: Color32 = Color32::from_gray(130);

fn safe_bar_width(ui: &Ui) -> f32 {
  let w = ui.available_width();
  if w.is_finite() && w > 0.0 { w } else { ui.ctx().content_rect().width().max(1.0) }
}

pub fn draw(ui: &mut Ui, stats: &PollStats, spark: &mut Sparkline) {
  spark.push(stats.cpu_load as f32);

  let load_frac = stats.cpu_load as f32 / 100.0;
  let tc = temp_color(stats.cpu_temp, 80, 90);

  // Header: label + load %
  ui.horizontal(|ui| {
    ui.label(RichText::new("CPU").strong());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
      ui.label(RichText::new(format!("{}%", stats.cpu_load)).color(color_accent()));
    });
  });

  // Sparkline
  spark.draw(ui, SPARK_H, color_accent());

  // Load bar
  let bw = safe_bar_width(ui);
  ui.add(ProgressBar::new(load_frac).desired_width(bw));

  // Metadata grid: TEMP / FREQ / POWER (labels row + values row)
  let freq_str = if stats.cpu_freq_mhz > 0.0 {
    format!("{:.2} GHz", stats.cpu_freq_mhz / 1000.0)
  } else {
    "--".to_string()
  };
  let temp_str = stats.cpu_temp.map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
  let power_str = stats.cpu_power.map_or("-- W".to_string(), |p| format!("{p:.0} W"));

  egui::Grid::new("cpu_meta").num_columns(3).min_col_width(60.0).show(ui, |ui| {
    ui.label(RichText::new("TEMP").small().color(LABEL_COLOR));
    ui.label(RichText::new("FREQ").small().color(LABEL_COLOR));
    ui.label(RichText::new("POWER").small().color(LABEL_COLOR));
    ui.end_row();
    ui.label(RichText::new(temp_str).color(tc));
    ui.label(freq_str);
    ui.label(power_str);
    ui.end_row();
  });

  // Per-core bars (up to 16 cores, displayed in pairs)
  if !stats.cpu_cores.is_empty() {
    let cores = stats.cpu_cores.iter().take(16).copied().collect::<Vec<_>>();
    for pair in cores.chunks(2) {
      ui.horizontal(|ui| {
        for (i, &load) in pair.iter().enumerate() {
          ui.label(RichText::new(format!("C{i}")).small().color(Color32::from_gray(110)));
          ui.add(ProgressBar::new(load as f32 / 100.0).desired_width(60.0));
          ui.label(RichText::new(format!("{load}%")).small());
          ui.add_space(4.0);
        }
      });
    }
  }
}
