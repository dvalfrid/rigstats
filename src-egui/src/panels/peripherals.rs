//! PERIPHERALS panel (#290): the battery of wireless devices the sensor
//! service reads — ROG headsets, and keyboards and mice on the ROG Omni
//! receiver (`PollStats.peripherals`, about once a minute). One row per
//! device: name, a charge bar, and the percentage.

use egui::{pos2, Align2, FontId, Rect, RichText, Sense, Ui, Vec2};
use rigstats_backend::lhm::Peripheral;

use super::battery::charge_color;
use crate::{theme, PollStats};

const ROW_H: f32 = 18.0;
const CELL_PAD: f32 = 6.0;

/// The right column: "82%", or "CHG 82%" while charging (the font has no
/// bolt or arrow glyphs).
fn percent_text(device: &Peripheral) -> String {
    if device.charging {
        format!("CHG {}%", device.battery)
    } else {
        format!("{}%", device.battery)
    }
}

/// One device row painted at fixed x positions (as the PROCESSES and GPU APPS
/// panels do), so the columns line up whatever the names: NAME 46%, bar in
/// the middle, percentage right-aligned in the last 22%.
fn paint_row(
    ui: &mut Ui,
    inner_w: f32,
    device: &Peripheral,
    charge_warn: u8,
    charge_crit: u8,
    sc: f32,
) {
    let row_h = (ROW_H * sc).round();
    let pad = CELL_PAD * sc;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(inner_w, row_h), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let color = charge_color(device.battery, device.charging, charge_warn, charge_crit);
    let font = FontId::proportional(12.0 * sc);
    let cy = rect.center().y;
    let name_w = inner_w * 0.46;
    let pct_w = inner_w * 0.22;

    let name_clip = Rect::from_min_size(rect.min, Vec2::new(name_w - pad, row_h));
    ui.painter().with_clip_rect(name_clip).text(
        pos2(rect.min.x, cy),
        Align2::LEFT_CENTER,
        &device.name,
        font.clone(),
        theme::C_TEXT,
    );

    let bar_x0 = rect.min.x + name_w;
    let bar_w = (inner_w - name_w - pct_w - pad).max(4.0 * sc);
    let bar_h = (4.0 * sc).max(2.0);
    let track = Rect::from_min_size(pos2(bar_x0, cy - bar_h / 2.0), Vec2::new(bar_w, bar_h));
    ui.painter()
        .rect_filled(track, 0.0, egui::Color32::from_gray(42));
    let fill_w = bar_w * f32::from(device.battery.min(100)) / 100.0;
    if fill_w > 0.0 {
        let fill = Rect::from_min_size(track.min, Vec2::new(fill_w, bar_h));
        ui.painter().rect_filled(fill, 0.0, color);
    }

    ui.painter().text(
        pos2(rect.max.x, cy),
        Align2::RIGHT_CENTER,
        percent_text(device),
        font,
        color,
    );
}

pub fn draw(
    ui: &mut Ui,
    stats: &PollStats,
    opacity: f32,
    th: &theme::AppTheme,
    sc: f32,
    charge_warn: u8,
    charge_crit: u8,
) -> egui::Rect {
    theme::panel_frame(ui, opacity, th, sc, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H * sc);
        ui.label(
            RichText::new("PERIPHERALS")
                .strong()
                .color(theme::C_PANEL_TITLE)
                .size(theme::FONT_PANEL_TITLE * sc),
        );
        ui.add_space(4.0 * sc);

        if stats.peripherals.is_empty() {
            ui.label(
                RichText::new("NO WIRELESS DEVICES")
                    .size(14.0 * sc)
                    .color(th.text_muted),
            );
            return;
        }

        let inner_w = ui.available_width();
        for device in &stats.peripherals {
            paint_row(ui, inner_w, device, charge_warn, charge_crit, sc);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(battery: u8, charging: bool) -> Peripheral {
        Peripheral {
            id: "asus-keyboard-1ace-1".into(),
            name: "ROG Azoth X".into(),
            kind: "keyboard".into(),
            battery,
            charging,
        }
    }

    #[test]
    fn percent_text_marks_charging() {
        assert_eq!(percent_text(&device(82, false)), "82%");
        assert_eq!(percent_text(&device(24, true)), "CHG 24%");
    }
}
