use egui::RichText;

use crate::tempcolor::temp_color;
use crate::theme;
use crate::PollStats;

const GIB: f64 = 1_073_741_824.0;

#[allow(clippy::too_many_arguments)] // like the other panels: the frame's inputs
pub fn draw(
    ui: &mut egui::Ui,
    stats: &PollStats,
    opacity: f32,
    warn: u8,
    crit: u8,
    th: &theme::AppTheme,
    sc: f32,
    // The compact layout (#596): a usage ring with USED and FREE (and TEMP
    // when it's read), no spec lines and no bar.
    compact: bool,
) -> egui::Rect {
    theme::panel_frame(ui, opacity, th, sc, |ui| {
        if !compact {
            ui.set_min_height(theme::PANEL_DATA_H * sc);
        }

        let used_gb = stats.ram_used as f64 / GIB;
        let total_gb = stats.ram_total as f64 / GIB;
        let free_gb = total_gb - used_gb;
        let frac = if stats.ram_total > 0 {
            stats.ram_used as f32 / stats.ram_total as f32
        } else {
            0.0
        };
        let pct = (frac * 100.0) as u8;

        // Title
        ui.label(
            RichText::new("RAM USAGE")
                .strong()
                .color(theme::C_PANEL_TITLE)
                .size(theme::FONT_PANEL_TITLE * sc),
        );
        // Spec  (e.g. "DDR5 6000 MT/s (2 DIMMs)") — compact too, where it is
        // the line under the title like the CPU's and GPU's model names, so
        // the rings line up beside them.
        if !stats.ram_spec.is_empty() {
            ui.label(
                RichText::new(&stats.ram_spec)
                    .size(11.0 * sc)
                    .color(th.text_muted),
            );
        }
        if compact {
            compact_body(ui, stats, frac, used_gb, free_gb, warn, crit, th, sc);
            return;
        }
        // Details  (e.g. "2x16 GB | Team Group Inc | UD5-6000")
        if !stats.ram_details.is_empty() {
            ui.label(
                RichText::new(&stats.ram_details)
                    .size(11.0 * sc)
                    .color(th.text_muted),
            );
        }

        // Used GB (large, left) + FREE | TEMP grid (right) — mirrors CPU/GPU ring+grid pattern.
        let temp_s = stats
            .ram_temp
            .map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
        let tc = stats.ram_temp.map_or(theme::C_UNAVAILABLE, |_| {
            temp_color(stats.ram_temp, warn, crit)
        });

        ui.horizontal(|ui| {
            // Left: big used number
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(format!("{used_gb:.1}"))
                        .size(48.0 * sc)
                        .color(theme::C_RAM),
                );
                ui.label(
                    RichText::new(format!("/ {total_gb:.0} GB"))
                        .size(13.0 * sc)
                        .color(th.text_muted),
                );
            });

            ui.add_space(14.0 * sc);

            // Right: FREE | TEMP stats grid
            ui.vertical(|ui| {
                ui.add_space(12.0 * sc);
                egui::Grid::new("ram_meta")
                    .num_columns(2)
                    .min_col_width(44.0 * sc)
                    .show(ui, |ui| {
                        ui.label(RichText::new("FREE").size(11.0 * sc).color(th.stat_label));
                        ui.label(
                            RichText::new("TEMP")
                                .size(11.0 * sc)
                                .color(theme::avail_color(&stats.ram_temp, th.stat_label)),
                        );
                        ui.end_row();
                        ui.label(
                            RichText::new(format!("{free_gb:.1}G"))
                                .size(14.0 * sc)
                                .color(theme::C_TEXT),
                        );
                        ui.label(RichText::new(&temp_s).size(14.0 * sc).color(tc));
                        ui.end_row();
                    });
            });
        });

        // Push MEM bar to the bottom.
        let cursor_after_meta = ui.cursor().top();
        let bar_row_h = 14.0 * sc;
        let filler =
            (theme::PANEL_DATA_H * sc - (cursor_after_meta - ui.min_rect().top()) - bar_row_h)
                .max(2.0 * sc);
        ui.add_space(filler);

        ui.horizontal(|ui| {
            ui.label(RichText::new("MEM").size(11.0 * sc).color(th.stat_label));
            let bar_w = (ui.available_width() - 44.0 * sc).max(4.0 * sc);
            theme::thin_bar_scaled(ui, frac, bar_w, theme::C_RAM, sc);
            ui.label(
                RichText::new(format!("{pct}%"))
                    .size(11.0 * sc)
                    .color(theme::C_RAM),
            );
        });
    })
}

/// The compact RAM panel's body (#596): memory in use as a ring, then USED
/// and FREE, and TEMP only on boards that read it.
#[allow(clippy::too_many_arguments)] // the panel's own values, passed through
fn compact_body(
    ui: &mut egui::Ui,
    stats: &PollStats,
    frac: f32,
    used_gb: f64,
    free_gb: f64,
    warn: u8,
    crit: u8,
    th: &theme::AppTheme,
    sc: f32,
) {
    use super::CompactCell;
    let mut cells = vec![
        CompactCell::Ring(frac, theme::C_RAM, format!("{:.0}%", frac * 100.0)),
        CompactCell::Stat {
            label: "USED",
            label_color: th.stat_label,
            value: format!("{used_gb:.1}G"),
            value_color: theme::C_RAM,
            note: None,
        },
        CompactCell::Stat {
            label: "FREE",
            label_color: th.stat_label,
            value: format!("{free_gb:.1}G"),
            value_color: theme::C_VALUE,
            note: None,
        },
    ];
    if let Some(t) = stats.ram_temp {
        cells.push(CompactCell::Stat {
            label: "TEMP",
            label_color: th.stat_label,
            value: format!("{t:.0}°C"),
            value_color: temp_color(stats.ram_temp, warn, crit),
            note: None,
        });
    }
    super::compact_row(ui, &cells, th, sc);
}
