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

fn fmt_speed(mb: f64) -> String {
    if mb >= 1000.0 {
        format!("{:.2} GB/s", mb / 1000.0)
    } else if mb >= 1.0 {
        format!("{mb:.1} MB/s")
    } else {
        format!("{:.0} KB/s", mb * 1000.0)
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

        // Read / Write columns — same layout as NET UP / DOWN.
        // Values come from sysinfo process-delta, available regardless of LHM.
        ui.columns(2, |cols| {
            cols[0].horizontal(|ui| {
                theme::arrow_up(ui, 12.0, theme::C_PUR);
                ui.add_space(2.0);
                ui.label(RichText::new("READ").small().color(theme::C_PUR));
            });
            cols[0].label(
                RichText::new(fmt_speed(stats.disk_read_mbps))
                    .size(20.0)
                    .color(Color32::WHITE),
            );

            cols[1].horizontal(|ui| {
                theme::arrow_down(ui, 12.0, theme::C_TEXT_MUTED);
                ui.add_space(2.0);
                ui.label(RichText::new("WRITE").small().color(theme::C_TEXT_MUTED));
            });
            cols[1].label(
                RichText::new(fmt_speed(stats.disk_write_mbps))
                    .size(20.0)
                    .color(Color32::WHITE),
            );
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
