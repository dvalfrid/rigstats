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
    hotspot_warn: u8,
    hotspot_crit: u8,
    sc: f32,
) -> (Option<String>, egui::Rect) {
    let mut new_gpu: Option<String> = None;

    let rect = theme::panel_frame(ui, opacity, th, sc, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H * sc);
        let load = stats.gpu_load.unwrap_or(0.0);
        let load_frac = (load / 100.0) as f32;
        let tc = stats.gpu_temp.map_or(theme::C_UNAVAILABLE, |_| {
            temp_color(stats.gpu_temp, warn, crit)
        });
        let ring_color = if stats.lhm_connected {
            theme::C_AMD
        } else {
            color_unknown()
        };
        let pct_color = if stats.lhm_connected {
            theme::C_TEXT
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

        // Ring LEFT + stats grid RIGHT.
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
            let hotspot_s = stats
                .gpu_hotspot
                .map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
            let hotspot_c = stats.gpu_hotspot.map_or(theme::C_UNAVAILABLE, |_| {
                temp_color(stats.gpu_hotspot, hotspot_warn, hotspot_crit)
            });
            let freq_s = fmt_opt(stats.gpu_freq_mhz.map(|v| v / 1000.0), " GHz", 2);
            let pwr_s = fmt_opt(stats.gpu_power, " W", 0);
            let freq_c = theme::avail_color(&stats.gpu_freq_mhz, theme::C_TEXT);
            let pwr_c = theme::avail_color(&stats.gpu_power, theme::C_TEXT);

            // 4-column grid: TEMP | HOT | FREQ | POWER
            ui.vertical(|ui| {
                egui::Grid::new("gpu_meta")
                    .num_columns(4)
                    .min_col_width(38.0 * sc)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("TEMP")
                                .size(11.0 * sc)
                                .color(theme::avail_color(&stats.gpu_temp, th.stat_label)),
                        );
                        ui.label(
                            RichText::new("HOT")
                                .size(11.0 * sc)
                                .color(theme::avail_color(&stats.gpu_hotspot, th.stat_label)),
                        );
                        ui.label(
                            RichText::new("FREQ")
                                .size(11.0 * sc)
                                .color(theme::avail_color(&stats.gpu_freq_mhz, th.stat_label)),
                        );
                        ui.label(
                            RichText::new("POWER")
                                .size(11.0 * sc)
                                .color(theme::avail_color(&stats.gpu_power, th.stat_label)),
                        );
                        ui.end_row();
                        ui.label(RichText::new(&temp_s).size(14.0 * sc).color(tc));
                        ui.label(RichText::new(&hotspot_s).size(14.0 * sc).color(hotspot_c));
                        ui.label(RichText::new(&freq_s).size(14.0 * sc).color(freq_c));
                        ui.label(RichText::new(&pwr_s).size(14.0 * sc).color(pwr_c));
                        ui.end_row();
                    });
            });
        });

        ui.add_space(4.0 * sc);

        // ── Bar section: 2 rows (VRAM+3D, FAN+VDEC) ─────────────────────────────
        // Claim only what's available before the sparkline so the panel keeps its
        // proportions at small scale. new_child clips bars that don't fit.
        {
            let has_d3d = stats.gpu_d3d_3d.is_some();
            let max_bar_h = ROW_H * 2.0 * sc + ui.spacing().item_spacing.y;
            let avail_for_bars =
                (ui.available_height() - SPARK_H * sc - ui.spacing().item_spacing.y).max(0.0);
            let container_h = avail_for_bars.min(max_bar_h);
            let avail_w = ui.available_width();
            let (outer_rect, _) =
                ui.allocate_exact_size(Vec2::new(avail_w, container_h), Sense::hover());
            let mut inner_ui = ui.new_child(egui::UiBuilder::new().max_rect(outer_rect));

            let draw_bar_col = |col_ui: &mut egui::Ui,
                                label: &str,
                                frac: f32,
                                pct_s: &str,
                                label_color: egui::Color32,
                                text_color: egui::Color32| {
                col_ui.horizontal(|ui| {
                    let sp = ui.spacing().item_spacing.x;
                    let bar_w =
                        (ui.available_width() - (LEFT_LBL_W + PCT_W) * sc - 2.0 * sp).max(4.0 * sc);
                    theme::fixed_label_r(
                        ui,
                        RichText::new(label).size(11.0 * sc).color(label_color),
                        LEFT_LBL_W * sc,
                        ROW_H * sc,
                    );
                    theme::thin_bar_scaled(ui, frac, bar_w, theme::C_AMD, sc);
                    theme::fixed_label_r(
                        ui,
                        RichText::new(pct_s).size(11.0 * sc).color(text_color),
                        PCT_W * sc,
                        ROW_H * sc,
                    );
                });
            };

            if has_d3d {
                let d3d_frac = (stats.gpu_d3d_3d.unwrap_or(0.0) / 100.0) as f32;
                let vdec_frac = (stats.gpu_d3d_vdec.unwrap_or(0.0) / 100.0) as f32;
                let d3d_s = format!("{:.0}%", stats.gpu_d3d_3d.unwrap_or(0.0));
                let vdec_s = stats
                    .gpu_d3d_vdec
                    .map_or("0%".to_string(), |v| format!("{v:.0}%"));
                let fan_s = fmt_opt(stats.gpu_fan, "%", 0);
                let fan_frac = (stats.gpu_fan.unwrap_or(0.0) / 100.0) as f32;
                let vfrac = vram_frac(stats);
                let vram_s = format!("{:.0}%", vfrac * 100.0);

                inner_ui.columns(2, |cols| {
                    draw_bar_col(
                        &mut cols[0],
                        "VRAM",
                        vfrac,
                        &vram_s,
                        th.stat_label,
                        pct_color,
                    );
                    draw_bar_col(
                        &mut cols[1],
                        "3D",
                        d3d_frac,
                        &d3d_s,
                        th.stat_label,
                        theme::C_TEXT,
                    );
                });
                inner_ui.columns(2, |cols| {
                    draw_bar_col(
                        &mut cols[0],
                        "FAN",
                        fan_frac,
                        &fan_s,
                        theme::avail_color(&stats.gpu_fan, th.stat_label),
                        theme::avail_color(&stats.gpu_fan, pct_color),
                    );
                    draw_bar_col(
                        &mut cols[1],
                        "VDEC",
                        vdec_frac,
                        &vdec_s,
                        theme::avail_color(&stats.gpu_d3d_vdec, th.stat_label),
                        theme::avail_color(&stats.gpu_d3d_vdec, theme::C_TEXT),
                    );
                });
            } else {
                let vfrac = vram_frac(stats);
                let fan_frac = (stats.gpu_fan.unwrap_or(0.0) / 100.0) as f32;
                let fan_s = fmt_opt(stats.gpu_fan, "%", 0);

                let vram_bar_w = theme::bar_avail(&inner_ui, (LEFT_LBL_W + PCT_W + SIZE_W) * sc, 3);
                inner_ui.horizontal(|ui| {
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
                            .color(pct_color),
                        PCT_W * sc,
                        ROW_H * sc,
                    );
                    if let (Some(used), Some(total)) =
                        (stats.gpu_vram_used_mb, stats.gpu_vram_total_mb)
                    {
                        ui.label(
                            RichText::new(format!("{:.1}/{:.1}G", used / 1024.0, total / 1024.0))
                                .size(11.0 * sc)
                                .color(th.text_muted),
                        );
                    }
                });

                let fan_bar_w = theme::bar_avail(&inner_ui, (LEFT_LBL_W + PCT_W) * sc, 2);
                inner_ui.horizontal(|ui| {
                    theme::fixed_label_r(
                        ui,
                        RichText::new("FAN")
                            .size(11.0 * sc)
                            .color(theme::avail_color(&stats.gpu_fan, th.stat_label)),
                        LEFT_LBL_W * sc,
                        ROW_H * sc,
                    );
                    theme::thin_bar_scaled(ui, fan_frac, fan_bar_w, theme::C_AMD, sc);
                    theme::fixed_label_r(
                        ui,
                        RichText::new(&fan_s)
                            .size(11.0 * sc)
                            .color(theme::avail_color(&stats.gpu_fan, pct_color)),
                        PCT_W * sc,
                        ROW_H * sc,
                    );
                });
            }
        }

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

#[cfg(test)]
mod tests {
    use super::fmt_opt;

    #[test]
    fn fmt_opt_some_formats_with_unit_and_decimals() {
        assert_eq!(fmt_opt(Some(72.0), "°C", 0), "72°C");
        assert_eq!(fmt_opt(Some(1234.0), "MHz", 0), "1234MHz");
        assert_eq!(fmt_opt(Some(8.5), "W", 1), "8.5W");
    }

    #[test]
    fn fmt_opt_none_returns_dash_with_unit() {
        assert_eq!(fmt_opt(None, "°C", 0), "--°C");
        assert_eq!(fmt_opt(None, "W", 1), "--W");
    }
}
