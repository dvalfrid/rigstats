use egui::{Color32, RichText, Ui};

use crate::tempcolor::temp_color;
use crate::theme;
use crate::PollStats;

fn short_label(label: &str) -> String {
    if let Some(rest) = label.strip_prefix("Temperature #") {
        return format!("T{rest}");
    }
    if let Some(rest) = label.strip_prefix("Fan #") {
        return format!("F{rest}");
    }
    label.chars().take(8).collect()
}

pub fn draw(
    ui: &mut Ui,
    stats: &PollStats,
    opacity: f32,
    warn: u8,
    crit: u8,
    th: &theme::AppTheme,
) {
    theme::panel_frame(ui, opacity, th, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("MOTHERBOARD")
                    .strong()
                    .color(theme::C_PANEL_TITLE)
                    .size(theme::FONT_PANEL_TITLE),
            );
        });

        if let Some(ref board) = stats.mb_board {
            ui.label(RichText::new(board.as_str()).small().color(th.text_muted));
        }
        if let Some(ref chip) = stats.mb_chip {
            ui.label(RichText::new(chip.as_str()).small().color(theme::C_DIM));
        }

        if !stats.mb_fans.is_empty() || !stats.mb_temps.is_empty() || !stats.mb_voltages.is_empty()
        {
            ui.add_space(4.0);
            let cursor_y = ui.cursor().top();
            let data_h = (theme::PANEL_DATA_H - (cursor_y - ui.min_rect().top())).max(20.0);
            egui::ScrollArea::vertical()
                .id_salt("mb_data")
                .max_height(data_h)
                .show(ui, |ui| {
                    ui.columns(3, |cols| {
                        // Col 0: FAN
                        if !stats.mb_fans.is_empty() {
                            egui::Grid::new("mb_fans")
                                .num_columns(2)
                                .min_col_width(24.0)
                                .show(&mut cols[0], |ui| {
                                    ui.label(RichText::new("FAN").small().color(th.mb_accent));
                                    ui.label(RichText::new("RPM").small().color(th.mb_accent));
                                    ui.end_row();
                                    for (label, rpm) in &stats.mb_fans {
                                        ui.label(
                                            RichText::new(short_label(label))
                                                .small()
                                                .color(th.text_muted),
                                        );
                                        ui.label(
                                            RichText::new(format!("{rpm:.0}"))
                                                .small()
                                                .color(th.mb_accent),
                                        );
                                        ui.end_row();
                                    }
                                });
                        }

                        // Col 1: SENSOR
                        if !stats.mb_temps.is_empty() {
                            egui::Grid::new("mb_temps")
                                .num_columns(2)
                                .min_col_width(24.0)
                                .show(&mut cols[1], |ui| {
                                    ui.label(RichText::new("SENSOR").small().color(th.mb_accent));
                                    ui.label(RichText::new("°C").small().color(th.mb_accent));
                                    ui.end_row();
                                    for (label, t) in &stats.mb_temps {
                                        ui.label(
                                            RichText::new(short_label(label))
                                                .small()
                                                .color(th.text_muted),
                                        );
                                        ui.label(
                                            RichText::new(format!("{t:.0}"))
                                                .small()
                                                .color(temp_color(Some(*t), warn, crit)),
                                        );
                                        ui.end_row();
                                    }
                                });
                        }

                        // Col 2: VOLT
                        if !stats.mb_voltages.is_empty() {
                            egui::Grid::new("mb_volts")
                                .num_columns(2)
                                .min_col_width(24.0)
                                .show(&mut cols[2], |ui| {
                                    ui.label(RichText::new("RAIL").small().color(th.mb_accent));
                                    ui.label(RichText::new("V").small().color(th.mb_accent));
                                    ui.end_row();
                                    for (label, v) in &stats.mb_voltages {
                                        ui.label(
                                            RichText::new(short_label(label))
                                                .small()
                                                .color(th.text_muted),
                                        );
                                        ui.label(
                                            RichText::new(format!("{v:.2}"))
                                                .small()
                                                .color(Color32::from_rgb(0xc8, 0xc8, 0x64)),
                                        );
                                        ui.end_row();
                                    }
                                });
                        }
                    });
                });
        }

        // Fill remaining space to reach PANEL_DATA_H (same pattern as NET/RAM/DISK).
        let cursor_y = ui.cursor().top();
        let filler = (theme::PANEL_DATA_H - (cursor_y - ui.min_rect().top())).max(0.0);
        if filler > 0.0 {
            ui.add_space(filler);
        }
    });
}
