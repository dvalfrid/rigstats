use egui::{RichText, Sense, Stroke, Ui, Vec2};

use crate::brand::Textures;
use crate::ring;
use crate::spark::Sparkline;
use crate::tempcolor::{color_unknown, temp_color};
use crate::theme;
use crate::PollStats;

const SPARK_H: f32 = 36.0;

const LEFT_LBL_W: f32 = 32.0; // "VRAM" / "3D"
const PCT_W: f32 = 36.0; // "100%"
const SIZE_W: f32 = 80.0; // "10.0/16.0G"
const ROW_H: f32 = 16.0;

fn fmt_opt(v: Option<f64>, unit: &str, decimals: usize) -> String {
    v.map_or(format!("--{unit}"), |x| format!("{x:.decimals$}{unit}"))
}

fn vram_frac(stats: &PollStats) -> f32 {
    match (stats.gpu_vram_used_mb, stats.gpu_vram_total_mb) {
        (Some(used), Some(total)) if total > 0.0 => (used / total) as f32,
        _ => 0.0,
    }
}

/// Returns `Some(gpu_name)` if the user clicked a GPU selector dot.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut Ui,
    stats: &PollStats,
    spark: &Sparkline,
    tex: &Textures,
    opacity: f32,
    th: &theme::AppTheme,
    warn: u8,
    crit: u8,
    sc: f32,
) -> (Option<String>, egui::Rect) {
    let mut new_gpu: Option<String> = None;

    let rect = theme::panel_frame(ui, opacity, th, sc, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H * sc);
        let load = stats.gpu_load.unwrap_or(0.0);
        let load_frac = (load / 100.0) as f32;
        let tc = temp_color(stats.gpu_temp, warn, crit);
        let ring_color = if stats.lhm_connected {
            theme::C_AMD
        } else {
            color_unknown()
        };
        let c = if stats.lhm_connected {
            theme::C_AMD
        } else {
            color_unknown()
        };

        // Header: title (+ optional selector dots) and model name stacked on left; logo sibling.
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("GPU LOAD")
                            .strong()
                            .color(theme::C_PANEL_TITLE)
                            .size(theme::FONT_PANEL_TITLE * sc),
                    );
                    if stats.gpu_devices.len() > 1 {
                        ui.add_space(6.0 * sc);
                        ui.spacing_mut().item_spacing.x = 3.0 * sc;
                        for (name, _vram) in &stats.gpu_devices {
                            let selected = name == &stats.gpu_name;
                            let (resp, painter) =
                                ui.allocate_painter(Vec2::splat(10.0 * sc), Sense::click());
                            let center = resp.rect.center();
                            if selected {
                                painter.circle_filled(center, 4.0 * sc, theme::C_AMD);
                            } else {
                                painter.circle_stroke(
                                    center,
                                    3.5 * sc,
                                    Stroke::new(1.5, th.text_muted),
                                );
                            }
                            let resp = resp.on_hover_text(name.as_str());
                            if resp.clicked() {
                                new_gpu = Some(name.clone());
                            }
                        }
                    }
                });
                if !stats.gpu_name.is_empty() {
                    let name = &stats.gpu_name;
                    let name = if name.len() > 44 { &name[..44] } else { name };
                    ui.label(RichText::new(name).size(11.0 * sc).color(th.text_muted));
                }
            });
            if let Some(logo) = tex.gpu_logo(&stats.gpu_name) {
                let [lw, lh] = logo.size();
                let target_h = 38.0 * sc;
                let w = lw as f32 * (target_h / lh as f32);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0 * sc);
                    let sized =
                        egui::load::SizedTexture::new(logo.id(), egui::Vec2::new(w, target_h));
                    ui.add(egui::Image::new(sized));
                });
            }
        });

        // Ring LEFT + 3×2 metadata grid RIGHT.
        let ring_label = if stats.lhm_connected {
            format!("{load:.0}%")
        } else {
            "--%".to_string()
        };

        ui.horizontal(|ui| {
            ring::show(
                ui,
                theme::RING_SIZE * sc,
                load_frac,
                ring_color,
                &ring_label,
            );
            ui.add_space(12.0 * sc);

            let temp_s = stats
                .gpu_temp
                .map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
            let freq_s = fmt_opt(stats.gpu_freq_mhz.map(|v| v / 1000.0), " GHz", 2);
            let pwr_s = fmt_opt(stats.gpu_power, " W", 0);
            let fan_s = fmt_opt(stats.gpu_fan, "%", 0);

            // 4-column grid: TEMP | CORE FREQ | POWER | FAN
            ui.vertical(|ui| {
                egui::Grid::new("gpu_meta")
                    .num_columns(4)
                    .min_col_width(38.0 * sc)
                    .show(ui, |ui| {
                        ui.label(RichText::new("TEMP").size(11.0 * sc).color(th.stat_label));
                        ui.label(RichText::new("FREQ").size(11.0 * sc).color(th.stat_label));
                        ui.label(RichText::new("POWER").size(11.0 * sc).color(th.stat_label));
                        ui.label(RichText::new("FAN").size(11.0 * sc).color(th.stat_label));
                        ui.end_row();
                        ui.label(RichText::new(&temp_s).size(14.0 * sc).color(tc));
                        ui.label(RichText::new(&freq_s).size(14.0 * sc).color(theme::C_TEXT));
                        ui.label(RichText::new(&pwr_s).size(14.0 * sc).color(theme::C_TEXT));
                        ui.label(RichText::new(&fan_s).size(14.0 * sc).color(theme::C_TEXT));
                        ui.end_row();
                    });
            });
        });

        ui.add_space(4.0 * sc);

        // ── Bar row: VRAM | 3D (same row, split 50/50 when 3D is active) ─────────
        let has_d3d = stats.gpu_d3d_3d.is_some();
        let sp = ui.spacing().item_spacing.x;

        if has_d3d {
            // Split: VRAM left half, 3D right half.
            // Total: 2×(LBL + bar + PCT) + extra_gap + 4×item_spacing
            let bar_half = theme::bar_avail(ui, (LEFT_LBL_W + PCT_W) * 2.0 * sc + 6.0 * sc, 5);
            let bar_half = bar_half / 2.0;

            ui.horizontal(|ui| {
                // VRAM
                let vfrac = vram_frac(stats);
                theme::fixed_label_r(
                    ui,
                    RichText::new("VRAM").size(11.0 * sc).color(th.stat_label),
                    LEFT_LBL_W * sc,
                    ROW_H * sc,
                );
                theme::thin_bar_scaled(ui, vfrac, bar_half, theme::C_AMD, sc);
                theme::fixed_label_r(
                    ui,
                    RichText::new(format!("{:.0}%", vfrac * 100.0))
                        .size(11.0 * sc)
                        .color(c),
                    PCT_W * sc,
                    ROW_H * sc,
                );

                ui.add_space(6.0 * sc);

                // 3D
                let d3d = stats.gpu_d3d_3d.unwrap_or(0.0);
                let d3d_frac = (d3d / 100.0) as f32;
                theme::fixed_label_r(
                    ui,
                    RichText::new("3D").size(11.0 * sc).color(th.stat_label),
                    LEFT_LBL_W * sc,
                    ROW_H * sc,
                );
                theme::thin_bar_scaled(ui, d3d_frac, bar_half, theme::C_AMD, sc);
                theme::fixed_label_r(
                    ui,
                    RichText::new(format!("{d3d:.0}%"))
                        .size(11.0 * sc)
                        .color(theme::C_TEXT),
                    PCT_W * sc,
                    ROW_H * sc,
                );
            });
        } else {
            // Full-width VRAM bar: LBL + bar + PCT + SIZE + 3×spacing
            let vram_bar_w = theme::bar_avail(ui, (LEFT_LBL_W + PCT_W + SIZE_W) * sc, 3);
            let vfrac = vram_frac(stats);
            ui.horizontal(|ui| {
                theme::fixed_label_r(
                    ui,
                    RichText::new("VRAM").size(11.0 * sc).color(th.stat_label),
                    LEFT_LBL_W * sc,
                    ROW_H * sc,
                );
                theme::thin_bar_scaled(ui, vfrac, vram_bar_w, theme::C_AMD, sc);
                theme::fixed_label_r(
                    ui,
                    RichText::new(format!("{:.0}%", vfrac * 100.0))
                        .size(11.0 * sc)
                        .color(c),
                    PCT_W * sc,
                    ROW_H * sc,
                );
                if let (Some(used), Some(total)) = (stats.gpu_vram_used_mb, stats.gpu_vram_total_mb)
                {
                    ui.label(
                        RichText::new(format!("{:.1}/{:.1}G", used / 1024.0, total / 1024.0))
                            .size(11.0 * sc)
                            .color(th.text_muted),
                    );
                }
            });
        }

        // Suppress unused-variable warning for sp (only needed for split path removed above)
        let _ = sp;

        if !stats.lhm_connected {
            ui.label(
                RichText::new("LibreHardwareMonitor not running.")
                    .size(11.0 * sc)
                    .color(theme::C_DIM),
            );
        }

        // Push sparkline to the bottom of the panel (same pattern as NET/RAM/DISK).
        let cursor_y = ui.cursor().top();
        let filler =
            (theme::PANEL_DATA_H * sc - (cursor_y - ui.min_rect().top()) - SPARK_H * sc).max(0.0);
        ui.add_space(filler);
        spark.draw(ui, SPARK_H * sc, theme::C_AMD);
    });

    (new_gpu, rect)
}
