use egui::{RichText, Ui};

use crate::brand::Textures;
use crate::ring;
use crate::spark::Sparkline;
use crate::tempcolor::temp_color;
use crate::theme;
use crate::PollStats;

const RING_SIZE: f32 = 94.0;
const SPARK_H: f32 = 36.0;

pub fn draw(ui: &mut Ui, stats: &PollStats, spark: &Sparkline, tex: &Textures) {
  theme::panel_frame(ui, theme::C_ACCENT, |ui| {
    let load_frac = stats.cpu_load as f32 / 100.0;
    let tc = temp_color(stats.cpu_temp, 80, 90);

    // Header row: title + vendor logo
    ui.horizontal(|ui| {
      ui.label(RichText::new("CPU LOAD").strong().color(theme::C_TEXT).size(13.0));
      if let Some(logo) = tex.cpu_logo(&stats.cpu_model) {
        let [lw, lh] = logo.size();
        let scale = 28.0 / lh as f32;
        let w = lw as f32 * scale;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
          let sized = egui::load::SizedTexture::new(logo.id(), egui::Vec2::new(w, 28.0));
          ui.add(egui::Image::new(sized));
        });
      }
    });

    if !stats.cpu_model.is_empty() {
      let model = &stats.cpu_model;
      let model = if model.len() > 44 { &model[..44] } else { model };
      ui.label(RichText::new(model).small().color(theme::C_TEXT_MUTED));
    }

    ui.add_space(4.0);

    // Ring gauge + metadata side by side
    ui.horizontal(|ui| {
      ring::show(ui, RING_SIZE, load_frac, theme::C_ACCENT, &format!("{}%", stats.cpu_load));
      ui.add_space(16.0);

      let freq_str = if stats.cpu_freq_mhz > 0.0 {
        format!("{:.2} GHz", stats.cpu_freq_mhz / 1000.0)
      } else {
        "--".to_string()
      };
      let temp_str = stats.cpu_temp.map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
      let power_str = stats.cpu_power.map_or("-- W".to_string(), |p| format!("{p:.0} W"));

      ui.vertical(|ui| {
        egui::Grid::new("cpu_meta").num_columns(3).min_col_width(50.0).show(ui, |ui| {
          ui.label(RichText::new("TEMP").small().color(theme::C_STAT_LABEL));
          ui.label(RichText::new("FREQ").small().color(theme::C_STAT_LABEL));
          ui.label(RichText::new("POWER").small().color(theme::C_STAT_LABEL));
          ui.end_row();
          ui.label(RichText::new(temp_str).color(tc));
          ui.label(RichText::new(freq_str).color(theme::C_TEXT));
          ui.label(RichText::new(power_str).color(theme::C_TEXT));
          ui.end_row();
        });
      });
    });

    ui.add_space(6.0);

    // Per-core bars — scroll area shows 4 cores (2 rows) at a time.
    if !stats.cpu_cores.is_empty() {
      let cores = &stats.cpu_cores;
      let row_h = 18.0;
      egui::ScrollArea::vertical()
        .id_salt("cpu_cores")
        .max_height(2.0 * row_h + 4.0)
        .show(ui, |ui| {
          ui.spacing_mut().item_spacing.y = 2.0;
          for (row, pair) in cores.chunks(2).enumerate() {
            ui.horizontal(|ui| {
              for (col, &load) in pair.iter().enumerate() {
                let idx = row * 2 + col;
                ui.label(
                  RichText::new(format!("C{idx}")).small().color(theme::C_TEXT_MUTED),
                );
                theme::thin_bar(ui, load as f32 / 100.0, 60.0, theme::C_ACCENT);
                ui.label(
                  RichText::new(format!("{load}%")).small().color(theme::C_TEXT),
                );
                ui.add_space(8.0);
              }
            });
          }
        });
    }

    ui.add_space(4.0);
    spark.draw(ui, SPARK_H, theme::C_ACCENT);
  });
}
