use egui::{RichText, Ui, Vec2};

use crate::brand::Textures;
use crate::ring;
use crate::spark::Sparkline;
use crate::tempcolor::temp_color;
use crate::theme;
use crate::PollStats;

const RING_SIZE: f32 = 80.0;
const SPARK_H: f32 = 36.0;

// Fixed column widths for per-core bar rows (at 12 px Small).
const CORE_LBL_W: f32 = 30.0; // "C16"
const CORE_VAL_W: f32 = 36.0; // "100%"
const CORE_ROW_H: f32 = 16.0;

pub fn draw(ui: &mut Ui, stats: &PollStats, spark: &Sparkline, tex: &Textures) {
  theme::panel_frame(ui, theme::C_ACCENT, |ui| {
    ui.set_min_height(theme::PANEL_DATA_H);
    let load_frac = stats.cpu_load as f32 / 100.0;
    let tc = temp_color(stats.cpu_temp, 80, 90);

    // Header: title + model name stacked vertically on the left; logo right-aligned.
    // The logo is in a sibling column so its height does not inflate the title→model gap.
    ui.horizontal(|ui| {
      ui.vertical(|ui| {
        ui.label(RichText::new("CPU LOAD").strong().color(theme::C_PANEL_TITLE).size(theme::FONT_PANEL_TITLE));
        if !stats.cpu_model.is_empty() {
          let model = &stats.cpu_model;
          let model = if model.len() > 44 { &model[..44] } else { model };
          ui.label(RichText::new(model).small().color(theme::C_TEXT_MUTED));
        }
      });
      if let Some(logo) = tex.cpu_logo(&stats.cpu_model) {
        let [lw, lh] = logo.size();
        let scale = 38.0 / lh as f32;
        let w = lw as f32 * scale;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
          ui.add_space(6.0);
          let sized = egui::load::SizedTexture::new(logo.id(), Vec2::new(w, 38.0));
          ui.add(egui::Image::new(sized));
        });
      }
    });

    // Ring gauge + metadata side by side.
    // ui.horizontal() uses left_to_right(Align::Center) internally — items are
    // vertically centred within the row height (= RING_SIZE), which is correct.
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

    // Per-core bars — two fixed-width columns so labels/values never shift.
    if !stats.cpu_cores.is_empty() {
      let cores = &stats.cpu_cores;
      let avail = ui.available_width();
      // 6 widgets, 5 auto-gaps (8 px each): 2*(LBL+VAL) + 5*8 = 132 + 40 = 172 fixed
      let bar_w = ((avail - 172.0) / 2.0).max(4.0);

      egui::ScrollArea::vertical()
        .id_salt("cpu_cores")
        .max_height(2.0 * CORE_ROW_H + 4.0)
        .show(ui, |ui| {
          ui.spacing_mut().item_spacing.y = 2.0;
          for (row, pair) in cores.chunks(2).enumerate() {
            ui.horizontal(|ui| {
              for (col, &load) in pair.iter().enumerate() {
                let idx = row * 2 + col;

                // Right-aligned label in fixed box
                ui.allocate_ui_with_layout(
                  Vec2::new(CORE_LBL_W, CORE_ROW_H),
                  egui::Layout::right_to_left(egui::Align::Center),
                  |ui| {
                    ui.label(RichText::new(format!("C{idx}")).small().color(theme::C_TEXT_MUTED));
                  },
                );

                theme::thin_bar(ui, load as f32 / 100.0, bar_w, theme::C_ACCENT);

                // Right-aligned value so all values end at the same x edge
                ui.allocate_ui_with_layout(
                  Vec2::new(CORE_VAL_W, CORE_ROW_H),
                  egui::Layout::right_to_left(egui::Align::Center),
                  |ui| {
                    ui.label(RichText::new(format!("{load}%")).small().color(theme::C_TEXT));
                  },
                );

                let _ = col; // suppress unused-variable warning for last col
              }
            });
          }
        });
    }

    ui.add_space(4.0);
    spark.draw(ui, SPARK_H, theme::C_ACCENT);
  });
}
