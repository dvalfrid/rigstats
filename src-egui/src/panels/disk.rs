use egui::{Color32, RichText, Ui, Vec2};

use crate::tempcolor::temp_color;
use crate::theme;
use crate::PollStats;

const LBL_W: f32 = 28.0; // "C:"
const PCT_W: f32 = 36.0; // "100%"
const TEMP_W: f32 = 46.0; // "100°C"
const ROW_H: f32 = 14.0;
const ROW_GAP: f32 = 2.0;
const MAX_VISIBLE: usize = 3;

fn fmt_speed_parts(mb: f64) -> (String, &'static str) {
    if mb >= 1000.0 {
        (format!("{:.2}", mb / 1000.0), "GB/s")
    } else if mb >= 1.0 {
        (format!("{mb:.1}"), "MB/s")
    } else {
        (format!("{:.0}", mb * 1000.0), "KB/s")
    }
}

pub fn draw(ui: &mut Ui, stats: &PollStats, opacity: f32) {
    theme::panel_frame(ui, theme::C_PUR, opacity, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H);
        let drives = &stats.disk_drives;

        ui.label(
            RichText::new("DISK")
                .strong()
                .color(theme::C_PANEL_TITLE)
                .size(theme::FONT_PANEL_TITLE),
        );
        ui.add_space(4.0);

        const NUMBER_W: f32 = 40.0;
        ui.columns(2, |cols| {
            let (val, unit) = fmt_speed_parts(stats.disk_read_mbps);
            cols[0].horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(NUMBER_W, 14.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        theme::arrow_up(ui, 10.0, theme::C_PUR);
                    },
                );
                ui.label(RichText::new("READ").small().color(theme::C_PUR));
            });
            cols[0].horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(NUMBER_W, 24.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.label(RichText::new(&val).size(20.0).color(Color32::WHITE));
                    },
                );
                ui.label(RichText::new(unit).small().color(theme::C_TEXT_MUTED));
            });

            let (val, unit) = fmt_speed_parts(stats.disk_write_mbps);
            cols[1].horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(NUMBER_W, 14.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        theme::arrow_down(ui, 10.0, theme::C_TEXT_MUTED);
                    },
                );
                ui.label(RichText::new("WRITE").small().color(theme::C_TEXT_MUTED));
            });
            cols[1].horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(NUMBER_W, 24.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.label(RichText::new(&val).size(20.0).color(Color32::WHITE));
                    },
                );
                ui.label(RichText::new(unit).small().color(theme::C_TEXT_MUTED));
            });
        });

        if drives.is_empty() {
            ui.label(
                RichText::new("no drives")
                    .small()
                    .color(theme::C_TEXT_MUTED),
            );
            return;
        }

        // Push drive bars to the bottom of the panel.
        let n_visible = drives.len().min(MAX_VISIBLE);
        let scroll_h = n_visible as f32 * ROW_H + (n_visible.saturating_sub(1)) as f32 * ROW_GAP;
        let cursor_y = ui.cursor().top();
        let filler = (theme::PANEL_DATA_H - (cursor_y - ui.min_rect().top()) - scroll_h).max(2.0);
        ui.add_space(filler);

        // Scrollable drive list — shows MAX_VISIBLE rows, scrolls if more.
        egui::ScrollArea::vertical()
            .id_salt("disk_drives")
            .max_height(scroll_h)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = ROW_GAP;

                for drive in drives.iter() {
                    let frac = drive.pct as f32 / 100.0;
                    let bar_color = if drive.pct >= 90 {
                        Color32::from_rgb(0xff, 0x3a, 0x1f)
                    } else if drive.pct >= 75 {
                        Color32::from_rgb(0xff, 0xb3, 0x00)
                    } else {
                        theme::C_PUR
                    };

                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(LBL_W, ROW_H),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.label(RichText::new(&drive.fs).small().color(theme::C_TEXT));
                            },
                        );
                        let spacing = ui.spacing().item_spacing.x;
                        let bar_w =
                            (ui.available_width() - PCT_W - TEMP_W - 2.0 * spacing).max(4.0);
                        theme::thin_bar(ui, frac, bar_w, bar_color);
                        ui.allocate_ui_with_layout(
                            Vec2::new(PCT_W, ROW_H),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    RichText::new(format!("{}%", drive.pct))
                                        .small()
                                        .color(theme::C_TEXT),
                                );
                            },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(TEMP_W, ROW_H),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let (s, c) = match drive.temp {
                                    Some(t) => (format!("{t:.0}°C"), temp_color(Some(t), 50, 60)),
                                    None => ("--".to_string(), theme::C_DIM),
                                };
                                ui.label(RichText::new(s).small().color(c));
                            },
                        );
                    });
                }
            });
    });
}
