use egui::{Color32, ProgressBar, RichText, Ui};

use crate::spark::Sparkline;
use crate::tempcolor::{color_accent, color_unknown, temp_color};
use crate::PollStats;

const SPARK_H: f32 = 36.0;
const LABEL_COLOR: Color32 = Color32::from_gray(130);

fn fmt_opt(v: Option<f64>, unit: &str, decimals: usize) -> String {
  v.map_or(format!("--{unit}"), |x| format!("{x:.decimals$}{unit}"))
}

fn safe_bar_width(ui: &Ui) -> f32 {
  let w = ui.available_width();
  if w.is_finite() && w > 0.0 { w } else { ui.ctx().content_rect().width().max(1.0) }
}

pub fn draw(ui: &mut Ui, stats: &PollStats, spark: &mut Sparkline) {
  let load = stats.gpu_load.unwrap_or(0.0);
  spark.push(load as f32);

  let load_frac = (load / 100.0) as f32;
  let tc = temp_color(stats.gpu_temp, 80, 90);
  let htc = temp_color(stats.gpu_hotspot, 90, 105);

  // Header: label + load %
  ui.horizontal(|ui| {
    ui.label(RichText::new("GPU").strong());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
      let color = if stats.lhm_connected { color_accent() } else { color_unknown() };
      ui.label(RichText::new(format!("{load:.0}%")).color(color));
    });
  });

  // Sparkline
  spark.draw(ui, SPARK_H, color_accent());

  // Load bar
  let bw = safe_bar_width(ui);
  ui.add(ProgressBar::new(load_frac).desired_width(bw));

  // Metadata 6-column grid: TEMP / HOTSPOT / FREQ / MEM / POWER / FAN
  let temp_s = stats.gpu_temp.map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
  let hot_s = stats.gpu_hotspot.map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
  let freq_s = fmt_opt(stats.gpu_freq_mhz, " MHz", 0);
  let mem_s = fmt_opt(stats.gpu_mem_freq_mhz, " MHz", 0);
  let pwr_s = fmt_opt(stats.gpu_power, " W", 0);
  let fan_s = fmt_opt(stats.gpu_fan, "%", 0);

  egui::Grid::new("gpu_meta").num_columns(6).min_col_width(40.0).show(ui, |ui| {
    for lbl in ["TEMP", "HOTSPOT", "FREQ", "MEM", "POWER", "FAN"] {
      ui.label(RichText::new(lbl).small().color(LABEL_COLOR));
    }
    ui.end_row();
    ui.label(RichText::new(&temp_s).color(tc));
    ui.label(RichText::new(&hot_s).color(htc));
    ui.label(&freq_s);
    ui.label(&mem_s);
    ui.label(&pwr_s);
    ui.label(&fan_s);
    ui.end_row();
  });

  // VRAM bar
  if let (Some(used), Some(total)) = (stats.gpu_vram_used_mb, stats.gpu_vram_total_mb) {
    let frac = if total > 0.0 { (used / total) as f32 } else { 0.0 };
    let used_gb = used / 1024.0;
    let total_gb = total / 1024.0;
    ui.horizontal(|ui| {
      ui.label(RichText::new("VRAM").small().color(LABEL_COLOR));
      ui.add(ProgressBar::new(frac).desired_width(120.0));
      ui.label(format!("{used_gb:.1} / {total_gb:.1} GB"));
    });
  }
}
