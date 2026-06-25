use egui::{Color32, RichText, Ui};

use crate::{theme, PollStats};

/// Estimated platform overhead: fans, SSD, NIC, chipset, USB draw.
const DESKTOP_OVERHEAD_W: f64 = 25.0;
const LAPTOP_OVERHEAD_W: f64 = 10.0;

/// Scale ceiling for the bottom total-power gauge (not a threshold).
const DESKTOP_REFERENCE_W: f64 = 500.0;
const LAPTOP_REFERENCE_W: f64 = 120.0;

const LBL_W: f32 = 40.0;
const VAL_W: f32 = 50.0;

/// Compute estimated total system power from available component readings.
/// Returns `None` when both sensors are absent.
/// Tuple: (total, cpu_w, gpu_w, overhead, complete)
fn estimate_total(
    cpu: Option<f64>,
    gpu: Option<f64>,
    on_battery_platform: bool,
) -> Option<(f64, f64, f64, f64, bool)> {
    if cpu.is_none() && gpu.is_none() {
        return None;
    }
    let overhead = if on_battery_platform {
        LAPTOP_OVERHEAD_W
    } else {
        DESKTOP_OVERHEAD_W
    };
    let cpu_w = cpu.unwrap_or(0.0);
    let gpu_w = gpu.unwrap_or(0.0);
    Some((
        cpu_w + gpu_w + overhead,
        cpu_w,
        gpu_w,
        overhead,
        cpu.is_some() && gpu.is_some(),
    ))
}

pub fn draw(
    ui: &mut Ui,
    stats: &PollStats,
    opacity: f32,
    th: &theme::AppTheme,
    sc: f32,
    psu_watts: Option<u16>,
) -> egui::Rect {
    theme::panel_frame(ui, opacity, th, sc, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H * sc);
        ui.label(
            RichText::new("SYSTEM POWER")
                .strong()
                .color(theme::C_PANEL_TITLE)
                .size(theme::FONT_PANEL_TITLE * sc),
        );

        let is_laptop = stats.battery_present;
        let reference_w = psu_watts.map(|w| w as f64).unwrap_or(if is_laptop {
            LAPTOP_REFERENCE_W
        } else {
            DESKTOP_REFERENCE_W
        });
        let est = estimate_total(stats.cpu_power, stats.total_gpu_power, is_laptop);
        let has_data = est.is_some();
        let (total_w, cpu_w, gpu_w, other_w, complete) = est.unwrap_or((0.0, 0.0, 0.0, 0.0, false));

        // Hero: ~245 W  est.
        let (hero_str, hero_color, total_frac) = if has_data {
            (
                format!("{total_w:.0}"),
                theme::C_TEXT,
                (total_w / reference_w).clamp(0.0, 1.0) as f32,
            )
        } else {
            ("--".to_string(), theme::C_UNAVAILABLE, 0.0f32)
        };

        ui.horizontal(|ui| {
            ui.label(RichText::new("~").size(36.0 * sc).color(th.text_muted));
            ui.label(RichText::new(&hero_str).size(44.0 * sc).color(hero_color));
            ui.vertical(|ui| {
                ui.add_space(18.0 * sc);
                ui.label(RichText::new("W").size(18.0 * sc).color(th.text_muted));
                ui.label(RichText::new("est.").size(10.0 * sc).color(theme::C_DIM));
            });
        });

        ui.add_space(2.0 * sc);

        // Breakdown bars: label | ████ | value
        let draw_bar = |row_ui: &mut Ui,
                        label: &str,
                        frac: f32,
                        val: &str,
                        bar_color: Color32,
                        lbl_color: Color32,
                        val_color: Color32| {
            row_ui.horizontal(|ui| {
                let sp = ui.spacing().item_spacing.x;
                let bar_w = (ui.available_width() - (LBL_W + VAL_W) * sc - 2.0 * sp).max(4.0 * sc);
                theme::fixed_label_r(
                    ui,
                    RichText::new(label).size(11.0 * sc).color(lbl_color),
                    LBL_W * sc,
                    14.0 * sc,
                );
                theme::thin_bar_scaled(ui, frac, bar_w, bar_color, sc);
                theme::fixed_label_r(
                    ui,
                    RichText::new(val).size(11.0 * sc).color(val_color),
                    VAL_W * sc,
                    14.0 * sc,
                );
            });
        };

        if has_data {
            let cpu_frac = (cpu_w / total_w) as f32;
            let gpu_frac = (gpu_w / total_w) as f32;
            let other_frac = (other_w / total_w) as f32;

            let cpu_lbl = theme::avail_color(&stats.cpu_power, th.stat_label);
            let gpu_lbl = theme::avail_color(&stats.total_gpu_power, th.stat_label);
            let cpu_val = theme::avail_color(&stats.cpu_power, theme::C_TEXT);
            let gpu_val = theme::avail_color(&stats.total_gpu_power, theme::C_TEXT);

            let cpu_s = stats
                .cpu_power
                .map_or("-- W".to_string(), |w| format!("{w:.0} W"));
            let gpu_s = stats
                .total_gpu_power
                .map_or("-- W".to_string(), |w| format!("{w:.0} W"));
            let other_s = format!("~{other_w:.0} W");

            draw_bar(ui, "CPU", cpu_frac, &cpu_s, th.accent, cpu_lbl, cpu_val);
            draw_bar(ui, "GPU", gpu_frac, &gpu_s, theme::C_AMD, gpu_lbl, gpu_val);
            draw_bar(
                ui,
                "OTHER",
                other_frac,
                &other_s,
                th.text_muted,
                th.stat_label,
                th.text_muted,
            );

            if !complete {
                let note = if stats.cpu_power.is_none() {
                    "CPU sensor n/a"
                } else {
                    "GPU sensor n/a"
                };
                ui.label(RichText::new(note).size(10.0 * sc).color(theme::C_DIM));
            }
        } else {
            for label in ["CPU", "GPU", "OTHER"] {
                draw_bar(
                    ui,
                    label,
                    0.0,
                    "-- W",
                    theme::C_UNAVAILABLE,
                    th.stat_label,
                    theme::C_UNAVAILABLE,
                );
            }
            if !stats.lhm_connected {
                ui.label(
                    RichText::new("LibreHardwareMonitor not running.")
                        .size(10.0 * sc)
                        .color(theme::C_DIM),
                );
            }
        }

        // Push PWR bar to the bottom (battery-panel anchor pattern).
        let cursor = ui.cursor().top();
        let bar_row_h = 14.0 * sc;
        let filler =
            (theme::PANEL_DATA_H * sc - (cursor - ui.min_rect().top()) - bar_row_h).max(2.0 * sc);
        ui.add_space(filler);

        let (total_str, bar_color) = if has_data {
            (format!("~{total_w:.0} W"), theme::C_TEXT)
        } else {
            ("~-- W".to_string(), theme::C_UNAVAILABLE)
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new("PWR").size(11.0 * sc).color(th.stat_label));
            let bar_w =
                (ui.available_width() - VAL_W * sc - ui.spacing().item_spacing.x).max(4.0 * sc);
            theme::thin_bar_scaled(ui, total_frac, bar_w, bar_color, sc);
            theme::fixed_label_r(
                ui,
                RichText::new(&total_str).size(11.0 * sc).color(bar_color),
                VAL_W * sc,
                14.0 * sc,
            );
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_none_returns_none() {
        assert!(estimate_total(None, None, false).is_none());
        assert!(estimate_total(None, None, true).is_none());
    }

    #[test]
    fn desktop_uses_larger_overhead() {
        let (total, cpu_w, gpu_w, other, _) =
            estimate_total(Some(100.0), Some(50.0), false).unwrap();
        assert_eq!(other, DESKTOP_OVERHEAD_W);
        assert_eq!(cpu_w, 100.0);
        assert_eq!(gpu_w, 50.0);
        assert_eq!(total, 100.0 + 50.0 + DESKTOP_OVERHEAD_W);
    }

    #[test]
    fn laptop_uses_smaller_overhead() {
        let (total, _, _, other, _) = estimate_total(Some(20.0), Some(10.0), true).unwrap();
        assert_eq!(other, LAPTOP_OVERHEAD_W);
        assert_eq!(total, 20.0 + 10.0 + LAPTOP_OVERHEAD_W);
    }

    #[test]
    fn one_missing_sensor_still_estimates() {
        let (_, _, gpu_w, _, complete) = estimate_total(Some(80.0), None, false).unwrap();
        assert_eq!(gpu_w, 0.0);
        assert!(!complete);
    }

    #[test]
    fn complete_flag_true_when_both_present() {
        let (_, _, _, _, complete) = estimate_total(Some(100.0), Some(50.0), false).unwrap();
        assert!(complete);
    }

    #[test]
    fn fracs_sum_to_one() {
        let (total, cpu_w, gpu_w, other, _) =
            estimate_total(Some(100.0), Some(50.0), false).unwrap();
        let sum = cpu_w / total + gpu_w / total + other / total;
        assert!((sum - 1.0).abs() < 1e-10);
    }
}
