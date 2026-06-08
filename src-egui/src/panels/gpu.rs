use egui::{RichText, Sense, Stroke, Ui, Vec2};

use crate::brand::Textures;
use crate::ring;
use crate::spark::Sparkline;
use crate::tempcolor::{color_unknown, temp_color};
use crate::theme;
use crate::PollStats;

const RING_SIZE: f32 = 80.0;
const SPARK_H: f32 = 36.0;

fn fmt_opt(v: Option<f64>, unit: &str, decimals: usize) -> String {
  v.map_or(format!("--{unit}"), |x| format!("{x:.decimals$}{unit}"))
}

fn safe_bar_width(ui: &Ui) -> f32 {
  let w = ui.available_width();
  if w.is_finite() && w > 0.0 { w } else { ui.ctx().content_rect().width().max(1.0) }
}

/// Returns `Some(gpu_name)` if the user clicked a GPU selector dot.
pub fn draw(
  ui: &mut Ui,
  stats: &PollStats,
  spark: &Sparkline,
  tex: &Textures,
) -> Option<String> {
  let mut new_gpu: Option<String> = None;

  theme::panel_frame(ui, theme::C_AMD, |ui| {
    ui.set_min_height(theme::PANEL_DATA_H);
    let load = stats.gpu_load.unwrap_or(0.0);
    let load_frac = (load / 100.0) as f32;
    let tc = temp_color(stats.gpu_temp, 80, 90);
    let htc = temp_color(stats.gpu_hotspot, 90, 105);
    let ring_color = if stats.lhm_connected { theme::C_AMD } else { color_unknown() };
    let c = if stats.lhm_connected { theme::C_AMD } else { color_unknown() };

    // Header row: title | selector dots (inline) | logo (right-aligned)
    ui.horizontal(|ui| {
      ui.label(RichText::new("GPU LOAD").strong().color(theme::C_TEXT).size(13.0));

      if stats.gpu_devices.len() > 1 {
        ui.add_space(6.0);
        for (name, _vram) in &stats.gpu_devices {
          let selected = name == &stats.gpu_name;
          let (resp, painter) = ui.allocate_painter(Vec2::splat(14.0), Sense::click());
          let center = resp.rect.center();
          if selected {
            painter.circle_filled(center, 4.5, theme::C_AMD);
          } else {
            painter.circle_stroke(center, 4.0, Stroke::new(1.5, theme::C_TEXT_MUTED));
          }
          let resp = resp.on_hover_text(name.as_str());
          if resp.clicked() {
            new_gpu = Some(name.clone());
          }
          ui.add_space(2.0);
        }
      }

      if let Some(logo) = tex.gpu_logo(&stats.gpu_name) {
        let [lw, lh] = logo.size();
        let scale = 40.0 / lh as f32;
        let w = lw as f32 * scale;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
          ui.add_space(6.0);
          let sized = egui::load::SizedTexture::new(logo.id(), egui::Vec2::new(w, 40.0));
          ui.add(egui::Image::new(sized));
        });
      }
    });

    // GPU model name
    if !stats.gpu_name.is_empty() {
      let name = &stats.gpu_name;
      let name = if name.len() > 44 { &name[..44] } else { name };
      ui.label(RichText::new(name).small().color(theme::C_TEXT_MUTED));
    }

    ui.add_space(2.0);

    // Ring LEFT + 3×2 metadata grid RIGHT
    let ring_label = if stats.lhm_connected { format!("{load:.0}%") } else { "--%".to_string() };

    ui.horizontal(|ui| {
      ring::show(ui, RING_SIZE, load_frac, ring_color, &ring_label);
      ui.add_space(12.0);

      let temp_s = stats.gpu_temp.map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
      let hot_s = stats.gpu_hotspot.map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
      let freq_s = fmt_opt(stats.gpu_freq_mhz, " MHz", 0);
      let pwr_s = fmt_opt(stats.gpu_power, " W", 0);
      let mem_s = fmt_opt(stats.gpu_mem_freq_mhz, " MHz", 0);
      let fan_s = fmt_opt(stats.gpu_fan, "%", 0);

      ui.vertical(|ui| {
        egui::Grid::new("gpu_meta").num_columns(3).min_col_width(44.0).show(ui, |ui| {
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
      });
    });

    ui.add_space(4.0);

    // ── Row 1: GPU load | VRAM ────────────────────────────────────────────────
    let avail = safe_bar_width(ui);
    // Fixed overhead: "GPU"~24 + "pct"~26 + gap~8 + "VRAM"~32 + "x.x/x.xGB"~60 = ~150
    let bar_pool = (avail - 150.0).max(16.0);
    let gpu_bar_w = bar_pool * 0.5;
    let vram_bar_w = bar_pool * 0.5;

    ui.horizontal(|ui| {
      ui.label(RichText::new("GPU").small().color(theme::C_STAT_LABEL));
      theme::thin_bar(ui, load_frac, gpu_bar_w, theme::C_AMD);
      ui.label(RichText::new(format!("{load:.0}%")).small().color(c));

      ui.add_space(8.0);
      ui.label(RichText::new("VRAM").small().color(theme::C_STAT_LABEL));
      if let (Some(used), Some(total)) = (stats.gpu_vram_used_mb, stats.gpu_vram_total_mb) {
        let vfrac = if total > 0.0 { (used / total) as f32 } else { 0.0 };
        theme::thin_bar(ui, vfrac, vram_bar_w, theme::C_AMD);
        ui.label(
          RichText::new(format!("{:.1}/{:.1}G", used / 1024.0, total / 1024.0))
            .small()
            .color(theme::C_TEXT_MUTED),
        );
      } else {
        theme::thin_bar(ui, 0.0, vram_bar_w, theme::C_AMD);
        ui.label(RichText::new("--").small().color(theme::C_DIM));
      }
    });

    // ── Row 2: 3D | VID — only when at least one field is non-null ────────────
    if stats.gpu_d3d_3d.is_some() || stats.gpu_d3d_vdec.is_some() {
      let d3d_frac = (stats.gpu_d3d_3d.unwrap_or(0.0) / 100.0) as f32;
      let vid_frac = (stats.gpu_d3d_vdec.unwrap_or(0.0) / 100.0) as f32;
      let d3d_s = stats.gpu_d3d_3d.map_or("--%".to_string(), |v| format!("{v:.0}%"));
      let vid_s = stats.gpu_d3d_vdec.map_or("--%".to_string(), |v| format!("{v:.0}%"));

      // Fixed overhead: "3D"~18 + "pct"~26 + gap~8 + "VID"~20 + "pct"~26 = ~98
      let d3d_bar_w = (bar_pool * 0.5).max(4.0);
      let vid_bar_w = (bar_pool * 0.5).max(4.0);

      ui.horizontal(|ui| {
        ui.label(RichText::new("3D").small().color(theme::C_STAT_LABEL));
        theme::thin_bar(ui, d3d_frac, d3d_bar_w, theme::C_AMD);
        ui.label(RichText::new(&d3d_s).small().color(theme::C_TEXT));

        ui.add_space(8.0);
        ui.label(RichText::new("VID").small().color(theme::C_STAT_LABEL));
        theme::thin_bar(ui, vid_frac, vid_bar_w, theme::C_AMD);
        ui.label(RichText::new(&vid_s).small().color(theme::C_TEXT));
      });
    }

    if !stats.lhm_connected {
      ui.label(
        RichText::new("LibreHardwareMonitor not running.")
          .small()
          .color(theme::C_DIM),
      );
    }

    ui.add_space(4.0);
    spark.draw(ui, SPARK_H, theme::C_AMD);
  });

  new_gpu
}
