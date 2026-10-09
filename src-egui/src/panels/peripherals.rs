//! PERIPHERALS panel (#290): the battery of wireless devices the sensor
//! service reads — ROG headsets, and keyboards and mice on the ROG Omni
//! receiver (`PollStats.peripherals`, about once a minute). One row per
//! device: name, a charge bar, and the percentage.

use egui::{
    pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, Vec2,
};
use rigstats_backend::lhm::Peripheral;

use super::battery::charge_color;
use crate::{theme, PollStats};

const ROW_H: f32 = 18.0;
const CELL_PAD: f32 = 6.0;
/// Share of the row the charge bar takes.
const BAR_SHARE: f32 = 0.30;
/// Slot for the charging marker between the bar and the percentage.
const MARK_W: f32 = 9.0;
/// Slot for the connection icon in front of the name.
const ICON_W: f32 = 16.0;

/// How a device reaches the PC, from the sidecar's `connection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    Usb,
    Bluetooth,
    Radio,
}

fn link_of(connection: &str) -> Option<Link> {
    match connection {
        "usb" => Some(Link::Usb),
        "bluetooth" => Some(Link::Bluetooth),
        "2.4ghz" => Some(Link::Radio),
        _ => None,
    }
}

/// A small line-drawn connection icon centred at `c` in a `size` box —
/// the font has no USB, Bluetooth or radio glyphs.
fn paint_link_icon(painter: &egui::Painter, c: Pos2, size: f32, link: Link, color: Color32) {
    let s = size / 2.0;
    let stroke = Stroke::new((size / 9.0).max(1.0), color);
    match link {
        Link::Bluetooth => {
            // The Bluetooth rune: a bar with two chevrons on its right.
            let (l, r) = (c.x - s * 0.5, c.x + s * 0.5);
            painter.add(Shape::line(
                vec![
                    pos2(l, c.y - s * 0.45),
                    pos2(r, c.y + s * 0.45),
                    pos2(c.x, c.y + s),
                    pos2(c.x, c.y - s),
                    pos2(r, c.y - s * 0.45),
                    pos2(l, c.y + s * 0.45),
                ],
                stroke,
            ));
        }
        Link::Radio => {
            // A transmitter with two waves above it.
            let base = pos2(c.x, c.y + s * 0.6);
            painter.circle_filled(base, stroke.width * 1.2, color);
            for radius in [s * 0.75, s * 1.4] {
                let points = (0..=8)
                    .map(|i| {
                        let a = -std::f32::consts::FRAC_PI_4 * 3.0
                            + std::f32::consts::FRAC_PI_2 * i as f32 / 8.0;
                        base + vec2(a.cos(), a.sin()) * radius
                    })
                    .collect();
                painter.add(Shape::line(points, stroke));
            }
        }
        Link::Usb => {
            // A plug: the head with its two contacts, the cable below.
            let head = Rect::from_center_size(pos2(c.x, c.y - s * 0.35), vec2(s * 1.2, s * 0.9));
            painter.rect_stroke(head, 0.0, stroke, egui::StrokeKind::Middle);
            for dx in [-0.25, 0.25] {
                let x = c.x + s * dx;
                painter.line_segment(
                    [pos2(x, head.top() - s * 0.35), pos2(x, head.top())],
                    stroke,
                );
            }
            painter.line_segment([pos2(c.x, head.bottom()), pos2(c.x, c.y + s)], stroke);
        }
    }
}

/// Where a row's columns go, as offsets from the row's left edge:
/// `(name_w, bar_x0, bar_w)`. Right to left: the percentage takes `pct_w`
/// (the width of "100%"), then the charging-marker slot, then the bar — so
/// every bar ends just before the numbers, whatever the value or charging
/// state — and the name gets everything left of the bar.
fn row_columns(inner_w: f32, pct_w: f32, sc: f32) -> (f32, f32, f32) {
    let pad = CELL_PAD * sc;
    let bar_x1 = inner_w - pct_w - MARK_W * sc;
    let bar_w = (inner_w * BAR_SHARE).max(24.0 * sc).min(bar_x1.max(0.0));
    let bar_x0 = bar_x1 - bar_w;
    let name_w = (bar_x0 - pad).max(0.0);
    (name_w, bar_x0, bar_w)
}

/// One device row painted at fixed x positions (as the PROCESSES and GPU APPS
/// panels do), so the columns line up whatever the names: connection icon,
/// name left, bar
/// ending just before the right-aligned percentage, and a small accent
/// triangle in front of the percentage while charging (the font has no bolt
/// or arrow glyphs). Also drawn by the tray hover card (`app/tray_card.rs`).
pub fn paint_device_row(
    ui: &mut Ui,
    inner_w: f32,
    device: &Peripheral,
    charge_warn: u8,
    charge_crit: u8,
    sc: f32,
) {
    let row_h = (ROW_H * sc).round();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(inner_w, row_h), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let color = charge_color(device.battery, device.charging, charge_warn, charge_crit);
    let font = FontId::proportional(12.0 * sc);
    let cy = rect.center().y;
    let pct_w = ui
        .painter()
        .layout_no_wrap("100%".to_string(), font.clone(), color)
        .size()
        .x;
    let (name_w, bar_x0, bar_w) = row_columns(inner_w, pct_w, sc);

    let icon_w = ICON_W * sc;
    if let Some(link) = link_of(&device.connection) {
        paint_link_icon(
            ui.painter(),
            pos2(rect.min.x + icon_w / 2.0 - sc, cy),
            10.0 * sc,
            link,
            Color32::from_gray(150),
        );
    }

    let name_clip = Rect::from_min_size(
        pos2(rect.min.x + icon_w, rect.min.y),
        Vec2::new((name_w - icon_w).max(0.0), row_h),
    );
    ui.painter().with_clip_rect(name_clip).text(
        pos2(rect.min.x + icon_w, cy),
        Align2::LEFT_CENTER,
        &device.name,
        font.clone(),
        theme::C_TEXT,
    );

    let bar_h = (4.0 * sc).max(2.0);
    let track = Rect::from_min_size(
        pos2(rect.min.x + bar_x0, cy - bar_h / 2.0),
        Vec2::new(bar_w, bar_h),
    );
    ui.painter()
        .rect_filled(track, 0.0, egui::Color32::from_gray(42));
    let fill_w = bar_w * f32::from(device.battery.min(100)) / 100.0;
    if fill_w > 0.0 {
        let fill = Rect::from_min_size(track.min, Vec2::new(fill_w, bar_h));
        ui.painter().rect_filled(fill, 0.0, color);
    }

    if device.charging {
        let cx = track.max.x + MARK_W * sc / 2.0;
        let half = 3.0 * sc;
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                pos2(cx, cy - half),
                pos2(cx + half, cy + half),
                pos2(cx - half, cy + half),
            ],
            color,
            Stroke::NONE,
        ));
    }

    ui.painter().text(
        pos2(rect.max.x, cy),
        Align2::RIGHT_CENTER,
        format!("{}%", device.battery),
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
            paint_device_row(ui, inner_w, device, charge_warn, charge_crit, sc);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_ends_just_before_the_percentage_and_the_name_gets_the_rest() {
        // 280 px row, "100%" 28 px wide, scale 1.
        let (name_w, bar_x0, bar_w) = row_columns(280.0, 28.0, 1.0);
        assert_eq!(bar_x0 + bar_w, 280.0 - 28.0 - MARK_W);
        assert_eq!(bar_w, 280.0 * BAR_SHARE);
        assert_eq!(name_w, bar_x0 - CELL_PAD);
    }

    #[test]
    fn connection_strings_map_to_icons() {
        assert_eq!(link_of("usb"), Some(Link::Usb));
        assert_eq!(link_of("bluetooth"), Some(Link::Bluetooth));
        assert_eq!(link_of("2.4ghz"), Some(Link::Radio));
        assert_eq!(link_of(""), None); // an older sidecar: no icon
    }

    #[test]
    fn narrow_rows_never_go_negative() {
        let (name_w, bar_x0, bar_w) = row_columns(30.0, 28.0, 1.0);
        assert!(name_w >= 0.0 && bar_w >= 0.0 && bar_x0 + bar_w <= 30.0);
    }
}
