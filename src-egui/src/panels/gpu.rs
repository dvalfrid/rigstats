use egui::{RichText, Ui};

use crate::brand::Textures;
use crate::ring;
use crate::spark::Sparkline;
use crate::tempcolor::{color_unknown, temp_color};
use crate::theme;
use crate::PollStats;

const RING_SIZE: f32 = 90.0;
const SPARK_H: f32 = 36.0;

fn fmt_opt(v: Option<f64>, unit: &str, decimals: usize) -> String {
  v.map_or(format!("--{unit}"), |x| format!("{x:.decimals$}{unit}"))
}

fn safe_bar_width(ui: &Ui) -> f32 {
  let w = ui.available_width();
  if w.is_finite() && w > 0.0 { w } else { ui.ctx().content_rect().width().max(1.0) }
}

pub fn draw(ui: &mut Ui, stats: &PollStats, spark: &Sparkline, tex: &Textures) {
  theme::panel_frame(ui, theme::C_AMD, |ui| {
    let load = stats.gpu_load.unwrap_or(0.0);
    let load_frac = (load / 100.0) as f32;
    let tc = temp_color(stats.gpu_temp, 80, 90);
    let htc = temp_color(stats.gpu_hotspot, 90, 105);

    // Header row: title + vendor logo
    ui.horizontal(|ui| {
      ui.label(RichText::new("GPU LOAD").strong().color(theme::C_TEXT).size(13.0));
      if let Some(logo) = tex.gpu_logo(&stats.gpu_name) {
        let [lw, lh] = logo.size();
        let scale = 28.0 / lh as f32;
        let w = lw as f32 * scale;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
          let sized = egui::load::SizedTexture::new(logo.id(), egui::Vec2::new(w, 28.0));
          ui.add(egui::Image::new(sized));
        });
      }
    });
    ui.add_space(4.0);

    // Ring gauge centred
    let ring_label = if stats.lhm_connected {
      format!("{load:.0}%")
    } else {
      "--%".to_string()
    };
    let ring_color = if stats.lhm_connected { theme::C_AMD } else { color_unknown() };
    ui.horizontal(|ui| {
      let avail = safe_bar_width(ui);
      let offset = ((avail - RING_SIZE) / 2.0).max(0.0);
      ui.add_space(offset);
      ring::show(ui, RING_SIZE, load_frac, ring_color, &ring_label);
    });

    ui.add_space(4.0);

    // Metadata grid: 3 columns × 2 rows
    let temp_s = stats.gpu_temp.map_or("--*C".to_string(), |t| format!("{t:.0}*C"));
    let hot_s = stats.gpu_hotspot.map_or("--*C".to_string(), |t| format!("{t:.0}*C"));
    let freq_s = fmt_opt(stats.gpu_freq_mhz, " MHz", 0);
    let pwr_s = fmt_opt(stats.gpu_power, " W", 0);
    let mem_s = fmt_opt(stats.gpu_mem_freq_mhz, " MHz", 0);
    let fan_s = fmt_opt(stats.gpu_fan, "%", 0);

    egui::Grid::new("gpu_meta").num_columns(3).min_col_width(50.0).show(ui, |ui| {
      ui.label(RichText::new("TEMP").small().color(theme::C_STAT_LABEL));
      ui.label(RichText::new("HOT SPOT").small().color(theme::C_STAT_LABEL));
      ui.label(RichText::new("CORE CLK").small().color(theme::C_STAT_LABEL));
      ui.end_row();
      ui.label(RichText::new(&temp_s).color(tc));
      ui.label(RichText::new(&hot_s).color(htc));
      ui.label(RichText::new(&freq_s).color(theme::C_TEXT));
      ui.end_row();

      ui.label(RichText::new("POWER").small().color(theme::C_STAT_LABEL));
      ui.label(RichText::new("MEM CLK").small().color(theme::C_STAT_LABEL));
      ui.label(RichText::new("FAN").small().color(theme::C_STAT_LABEL));
      ui.end_row();
      ui.label(RichText::new(&pwr_s).color(theme::C_TEXT));
      ui.label(RichText::new(&mem_s).color(theme::C_TEXT));
      ui.label(RichText::new(&fan_s).color(theme::C_TEXT));
      ui.end_row();
    });

    ui.add_space(6.0);

    // GPU load + VRAM bars
    if let (Some(used), Some(total)) = (stats.gpu_vram_used_mb, stats.gpu_vram_total_mb) {
      let vram_frac = if total > 0.0 { (used / total) as f32 } else { 0.0 };
      let used_gb = used / 1024.0;
      let total_gb = total / 1024.0;

      // GPU row
      ui.horizontal(|ui| {
        ui.label(RichText::new("GPU").small().color(theme::C_STAT_LABEL));
        let c = if stats.lhm_connected { theme::C_AMD } else { color_unknown() };
        let bar_w = (ui.available_width() - 36.0).max(4.0);
        theme::thin_bar(ui, load_frac, bar_w, theme::C_AMD);
        ui.label(RichText::new(format!("{load:.0}%")).small().color(c));
      });
      // VRAM row
      ui.horizontal(|ui| {
        ui.label(RichText::new("VRAM").small().color(theme::C_STAT_LABEL));
        let bar_w = (ui.available_width() - 76.0).max(4.0);
        theme::thin_bar(ui, vram_frac, bar_w, theme::C_AMD);
        ui.label(
          RichText::new(format!("{used_gb:.1}/{total_gb:.1} GB"))
            .small()
            .color(theme::C_TEXT_MUTED),
        );
      });
    } else {
      ui.horizontal(|ui| {
        ui.label(RichText::new("GPU").small().color(theme::C_STAT_LABEL));
        let c = if stats.lhm_connected { theme::C_AMD } else { color_unknown() };
        let bar_w = (ui.available_width() - 36.0).max(4.0);
        theme::thin_bar(ui, load_frac, bar_w, theme::C_AMD);
        ui.label(RichText::new(format!("{load:.0}%")).small().color(c));
      });
    }

    if !stats.lhm_connected {
      ui.label(
        RichText::new("LibreHardwareMonitor not running - GPU metrics unavailable.")
          .small()
          .color(theme::C_AMD),
      );
    }

    // Sparkline
    ui.add_space(4.0);
    spark.draw(ui, SPARK_H, theme::C_AMD);
  });
}
