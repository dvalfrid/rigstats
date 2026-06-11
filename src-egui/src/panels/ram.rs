use egui::{RichText, Ui};

use crate::tempcolor::temp_color;
use crate::theme;
use crate::PollStats;

const GIB: f64 = 1_073_741_824.0;

pub fn draw(ui: &mut Ui, stats: &PollStats, opacity: f32, warn: u8, crit: u8) {
    theme::panel_frame(ui, theme::C_RAM, opacity, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H);
        let used_gb = stats.ram_used as f64 / GIB;
        let total_gb = stats.ram_total as f64 / GIB;
        let frac = if stats.ram_total > 0 {
            (stats.ram_used as f32) / (stats.ram_total as f32)
        } else {
            0.0
        };
        let pct = (frac * 100.0) as u8;

        // Header + spec (like CPU model / GPU model below title)
        ui.label(
            RichText::new("RAM USAGE")
                .strong()
                .color(theme::C_PANEL_TITLE)
                .size(theme::FONT_PANEL_TITLE),
        );
        if !stats.ram_spec.is_empty() {
            ui.label(
                RichText::new(&stats.ram_spec)
                    .small()
                    .color(theme::C_TEXT_MUTED),
            );
        }

        // Large number
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{used_gb:.1}"))
                    .size(48.0)
                    .color(theme::C_RAM),
            );
            ui.vertical(|ui| {
                ui.add_space(20.0);
                ui.label(
                    RichText::new(format!("/ {total_gb:.0} GB"))
                        .size(18.0)
                        .color(theme::C_TEXT_MUTED),
                );
            });
        });

        // Optional temperature row
        if let Some(t) = stats.ram_temp {
            let tc = temp_color(Some(t), warn, crit);
            ui.horizontal(|ui| {
                ui.label(RichText::new("TEMP").small().color(theme::C_STAT_LABEL));
                ui.label(RichText::new(format!("{t:.0}°C")).color(tc));
            });
        }

        // Push MEM bar to the bottom of the panel.
        let cursor_after_meta = ui.cursor().top();
        let bar_row_h = 14.0;
        let filler =
            (theme::PANEL_DATA_H - (cursor_after_meta - ui.min_rect().top()) - bar_row_h).max(2.0);
        ui.add_space(filler);

        ui.horizontal(|ui| {
            ui.label(RichText::new("MEM").small().color(theme::C_STAT_LABEL));
            // Reserve space for "100%" label (~30 px) + auto gap (~8 px) at 12 px Small.
            let bar_w = (ui.available_width() - 44.0).max(4.0);
            theme::thin_bar(ui, frac, bar_w, theme::C_RAM);
            ui.label(RichText::new(format!("{pct}%")).small().color(theme::C_RAM));
        });
    });
}
