use egui::{RichText, Sense, Stroke, Ui, Vec2};

use crate::brand::Textures;
use crate::ring;
use crate::spark::Sparkline;
use crate::tempcolor::{color_unknown, temp_color};
use crate::theme;
use crate::PollStats;

const RING_SIZE: f32 = 80.0;
const SPARK_H: f32 = 36.0;

// Fixed column widths for bar rows — all four rows share the same grid.
const LEFT_LBL_W: f32 = 32.0; // "GPU" / "3D"
const RIGHT_LBL_W: f32 = 46.0; // "VRAM" / "VID"
const PCT_W: f32 = 36.0; // "100%"
const SIZE_W: f32 = 80.0; // "10.0/16.0G"
const ROW_H: f32 = 16.0;

fn fmt_opt(v: Option<f64>, unit: &str, decimals: usize) -> String {
    v.map_or(format!("--{unit}"), |x| format!("{x:.decimals$}{unit}"))
}

fn safe_bar_width(ui: &Ui) -> f32 {
    let w = ui.available_width();
    if w.is_finite() && w > 0.0 {
        w
    } else {
        ui.ctx().content_rect().width().max(1.0)
    }
}

/// Returns `Some(gpu_name)` if the user clicked a GPU selector dot.
pub fn draw(
    ui: &mut Ui,
    stats: &PollStats,
    spark: &Sparkline,
    tex: &Textures,
    opacity: f32,
) -> Option<String> {
    let mut new_gpu: Option<String> = None;

    theme::panel_frame(ui, theme::C_AMD, opacity, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H);
        let load = stats.gpu_load.unwrap_or(0.0);
        let load_frac = (load / 100.0) as f32;
        let tc = temp_color(stats.gpu_temp, 80, 90);
        let htc = temp_color(stats.gpu_hotspot, 90, 105);
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
                            .size(theme::FONT_PANEL_TITLE),
                    );
                    if stats.gpu_devices.len() > 1 {
                        ui.add_space(6.0);
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for (name, _vram) in &stats.gpu_devices {
                            let selected = name == &stats.gpu_name;
                            let (resp, painter) =
                                ui.allocate_painter(Vec2::splat(10.0), Sense::click());
                            let center = resp.rect.center();
                            if selected {
                                painter.circle_filled(center, 4.0, theme::C_AMD);
                            } else {
                                painter.circle_stroke(
                                    center,
                                    3.5,
                                    Stroke::new(1.5, theme::C_TEXT_MUTED),
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
                    ui.label(RichText::new(name).small().color(theme::C_TEXT_MUTED));
                }
            });
            if let Some(logo) = tex.gpu_logo(&stats.gpu_name) {
                let [lw, lh] = logo.size();
                let scale = 38.0 / lh as f32;
                let w = lw as f32 * scale;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0);
                    let sized = egui::load::SizedTexture::new(logo.id(), egui::Vec2::new(w, 38.0));
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
            ring::show(ui, RING_SIZE, load_frac, ring_color, &ring_label);
            ui.add_space(12.0);

            let temp_s = stats
                .gpu_temp
                .map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
            let hot_s = stats
                .gpu_hotspot
                .map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
            let freq_s = fmt_opt(stats.gpu_freq_mhz, " MHz", 0);
            let pwr_s = fmt_opt(stats.gpu_power, " W", 0);
            let mem_s = fmt_opt(stats.gpu_mem_freq_mhz, " MHz", 0);
            let fan_s = fmt_opt(stats.gpu_fan, "%", 0);

            ui.vertical(|ui| {
                egui::Grid::new("gpu_meta")
                    .num_columns(3)
                    .min_col_width(44.0)
                    .show(ui, |ui| {
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

        // ── Bar rows: GPU | VRAM and 3D | VID ────────────────────────────────────
        // All four rows share the same 6-column grid so bars always align:
        //   LEFT_LBL(32) · bar_L · PCT(36) · RIGHT_LBL(46) · bar_R · SIZE(80)
        //   + 5 × item_spacing(8) = 234 overhead
        let avail = safe_bar_width(ui);
        let bar_pool = (avail - (LEFT_LBL_W + PCT_W + RIGHT_LBL_W + SIZE_W + 5.0 * 8.0)).max(16.0);
        let bar_l = bar_pool * 0.5;
        let bar_r = bar_pool * 0.5;

        // Row: GPU load | VRAM
        // Labels (LEFT_LBL_W, RIGHT_LBL_W) and values (PCT_W) are right-aligned so
        // text is flush against the following bar regardless of string width.
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(LEFT_LBL_W, ROW_H),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.label(RichText::new("GPU").small().color(theme::C_STAT_LABEL));
                },
            );
            theme::thin_bar(ui, load_frac, bar_l, theme::C_AMD);
            ui.allocate_ui_with_layout(
                Vec2::new(PCT_W, ROW_H),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.label(RichText::new(format!("{load:.0}%")).small().color(c));
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(RIGHT_LBL_W, ROW_H),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.label(RichText::new("VRAM").small().color(theme::C_STAT_LABEL));
                },
            );
            if let (Some(used), Some(total)) = (stats.gpu_vram_used_mb, stats.gpu_vram_total_mb) {
                let vfrac = if total > 0.0 {
                    (used / total) as f32
                } else {
                    0.0
                };
                theme::thin_bar(ui, vfrac, bar_r, theme::C_AMD);
                ui.allocate_ui_with_layout(
                    Vec2::new(SIZE_W, ROW_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.label(
                            RichText::new(format!("{:.1}/{:.1}G", used / 1024.0, total / 1024.0))
                                .small()
                                .color(theme::C_TEXT_MUTED),
                        );
                    },
                );
            } else {
                theme::thin_bar(ui, 0.0, bar_r, theme::C_AMD);
                ui.allocate_ui_with_layout(
                    Vec2::new(SIZE_W, ROW_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.label(RichText::new("--").small().color(theme::C_DIM));
                    },
                );
            }
        });

        // Row: 3D | VID — identical column widths as GPU/VRAM row
        if stats.gpu_d3d_3d.is_some() || stats.gpu_d3d_vdec.is_some() {
            let d3d_frac = (stats.gpu_d3d_3d.unwrap_or(0.0) / 100.0) as f32;
            let vid_frac = (stats.gpu_d3d_vdec.unwrap_or(0.0) / 100.0) as f32;
            let d3d_s = stats
                .gpu_d3d_3d
                .map_or("--%".to_string(), |v| format!("{v:.0}%"));
            let vid_s = stats
                .gpu_d3d_vdec
                .map_or("--%".to_string(), |v| format!("{v:.0}%"));

            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(LEFT_LBL_W, ROW_H),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.label(RichText::new("3D").small().color(theme::C_STAT_LABEL));
                    },
                );
                theme::thin_bar(ui, d3d_frac, bar_l, theme::C_AMD);
                ui.allocate_ui_with_layout(
                    Vec2::new(PCT_W, ROW_H),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.label(RichText::new(&d3d_s).small().color(theme::C_TEXT));
                    },
                );
                ui.allocate_ui_with_layout(
                    Vec2::new(RIGHT_LBL_W, ROW_H),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.label(RichText::new("VID").small().color(theme::C_STAT_LABEL));
                    },
                );
                theme::thin_bar(ui, vid_frac, bar_r, theme::C_AMD);
                ui.allocate_ui_with_layout(
                    Vec2::new(SIZE_W, ROW_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.label(RichText::new(&vid_s).small().color(theme::C_TEXT));
                    },
                );
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
