use egui::{RichText, Ui, Vec2};

use crate::brand::Textures;
use crate::ring;
use crate::spark::Sparkline;
use crate::tempcolor::temp_color;
use crate::theme;
use crate::PollStats;

const SPARK_H: f32 = 36.0;

// Fixed column widths for per-core bar rows (at 12 px Small).
const CORE_LBL_W: f32 = 30.0; // "C16"
const CORE_VAL_W: f32 = 36.0; // "100%"
const CORE_ROW_H: f32 = 16.0;

#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut Ui,
    stats: &PollStats,
    spark: &Sparkline,
    tex: &Textures,
    opacity: f32,
    warn: u8,
    crit: u8,
    th: &theme::AppTheme,
    sc: f32,
) -> egui::Rect {
    theme::panel_frame(ui, opacity, th, sc, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H * sc);
        let load_frac = stats.cpu_load as f32 / 100.0;
        let tc = temp_color(stats.cpu_temp, warn, crit);

        // Header: title + model name stacked vertically on the left; logo right-aligned.
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("CPU LOAD")
                        .strong()
                        .color(theme::C_PANEL_TITLE)
                        .size(theme::FONT_PANEL_TITLE * sc),
                );
                if !stats.cpu_model.is_empty() {
                    let model = &stats.cpu_model;
                    let model = if model.len() > 44 {
                        &model[..44]
                    } else {
                        model
                    };
                    ui.label(RichText::new(model).size(11.0 * sc).color(th.text_muted));
                }
            });
            if let Some(logo) = tex.cpu_logo(&stats.cpu_model) {
                let [lw, lh] = logo.size();
                let target_h = 38.0 * sc;
                let w = lw as f32 * (target_h / lh as f32);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0 * sc);
                    let sized = egui::load::SizedTexture::new(logo.id(), Vec2::new(w, target_h));
                    ui.add(egui::Image::new(sized));
                });
            }
        });

        // Ring gauge + metadata side by side.
        ui.horizontal(|ui| {
            ring::show(
                ui,
                theme::RING_SIZE * sc,
                load_frac,
                theme::C_ACCENT,
                &format!("{}%", stats.cpu_load),
            );
            ui.add_space(16.0 * sc);

            let freq_str = if stats.cpu_freq_mhz > 0.0 {
                format!("{:.2} GHz", stats.cpu_freq_mhz / 1000.0)
            } else {
                "--".to_string()
            };
            let temp_str = stats
                .cpu_temp
                .map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
            let power_str = stats
                .cpu_power
                .map_or("-- W".to_string(), |p| format!("{p:.0} W"));

            ui.vertical(|ui| {
                egui::Grid::new("cpu_meta")
                    .num_columns(3)
                    .min_col_width(50.0 * sc)
                    .show(ui, |ui| {
                        ui.label(RichText::new("TEMP").size(11.0 * sc).color(th.stat_label));
                        ui.label(RichText::new("FREQ").size(11.0 * sc).color(th.stat_label));
                        ui.label(RichText::new("POWER").size(11.0 * sc).color(th.stat_label));
                        ui.end_row();
                        ui.label(RichText::new(temp_str).size(14.0 * sc).color(tc));
                        ui.label(RichText::new(freq_str).size(14.0 * sc).color(theme::C_TEXT));
                        ui.label(
                            RichText::new(power_str)
                                .size(14.0 * sc)
                                .color(theme::C_TEXT),
                        );
                        ui.end_row();
                    });
            });
        });

        ui.add_space(6.0 * sc);

        // Per-core bars — two fixed-width columns, 2 rows visible, scrollable.
        if !stats.cpu_cores.is_empty() {
            let cores = &stats.cpu_cores;
            let avail = ui.available_width();
            // 6 widgets, 5 auto-gaps (~8 px each): 2*(LBL+VAL) + 5*8 = 132 + 40 = 172 fixed
            let bar_w = ((avail - 172.0 * sc) / 2.0).max(4.0 * sc);

            // Hard-allocate exactly 2 rows of vertical space in the parent layout,
            // then render the ScrollArea inside that fixed region. This avoids
            // relying solely on max_height, which can be overridden by set_min_height.
            let row_gap = 2.0_f32 * sc;
            let scroll_h = CORE_ROW_H * 2.0 * sc + row_gap;
            let (outer_rect, _) =
                ui.allocate_exact_size(Vec2::new(avail, scroll_h), egui::Sense::hover());

            let mut inner_ui = ui.new_child(egui::UiBuilder::new().max_rect(outer_rect));

            egui::ScrollArea::vertical()
                .id_salt("cpu_cores")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                .show(&mut inner_ui, |ui| {
                    ui.style_mut().spacing.item_spacing.y = row_gap;
                    for (row, pair) in cores.chunks(2).enumerate() {
                        ui.horizontal(|ui| {
                            for (col, &load) in pair.iter().enumerate() {
                                let idx = row * 2 + col;

                                ui.allocate_ui_with_layout(
                                    Vec2::new(CORE_LBL_W * sc, CORE_ROW_H * sc),
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(format!("C{idx}"))
                                                .size(11.0 * sc)
                                                .color(th.text_muted),
                                        );
                                    },
                                );

                                theme::thin_bar_scaled(
                                    ui,
                                    load as f32 / 100.0,
                                    bar_w,
                                    theme::C_ACCENT,
                                    sc,
                                );

                                ui.allocate_ui_with_layout(
                                    Vec2::new(CORE_VAL_W * sc, CORE_ROW_H * sc),
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(format!("{load}%"))
                                                .size(11.0 * sc)
                                                .color(theme::C_TEXT),
                                        );
                                    },
                                );
                            }
                        });
                    }
                });
        }

        // Push sparkline to the bottom of the panel (same pattern as NET/RAM/DISK).
        let cursor_y = ui.cursor().top();
        let filler =
            (theme::PANEL_DATA_H * sc - (cursor_y - ui.min_rect().top()) - SPARK_H * sc).max(0.0);
        ui.add_space(filler);
        spark.draw(ui, SPARK_H * sc, theme::C_ACCENT);
    })
}
