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
pub enum Link {
    Usb,
    Bluetooth,
    Radio,
}

pub fn link_of(connection: &str) -> Option<Link> {
    match connection {
        "usb" => Some(Link::Usb),
        "bluetooth" => Some(Link::Bluetooth),
        "2.4ghz" => Some(Link::Radio),
        _ => None,
    }
}

/// A small line-drawn connection icon centred at `c` in a `size` box —
/// the font has no USB, Bluetooth or radio glyphs.
/// "USB", "Bluetooth", "2.4 GHz".
pub fn link_label(link: Link) -> &'static str {
    match link {
        Link::Usb => "USB",
        Link::Bluetooth => "Bluetooth",
        Link::Radio => "2.4 GHz",
    }
}

/// A small battery outline filled to `pct` in `color`, in `rect`.
pub fn paint_battery_glyph(
    painter: &egui::Painter,
    rect: Rect,
    pct: u8,
    color: Color32,
    outline: Color32,
) {
    let body = Rect::from_min_max(rect.min, pos2(rect.max.x - 2.5, rect.max.y));
    painter.rect_stroke(
        body,
        egui::CornerRadius::same(2),
        Stroke::new(1.2_f32, outline),
        egui::StrokeKind::Inside,
    );
    let nub = Rect::from_min_max(
        pos2(body.max.x, rect.center().y - rect.height() * 0.2),
        pos2(rect.max.x, rect.center().y + rect.height() * 0.2),
    );
    painter.rect_filled(nub, egui::CornerRadius::same(1), outline);
    let inner = body.shrink(2.5);
    let fill_w = inner.width() * f32::from(pct.min(100)) / 100.0;
    if fill_w > 0.0 {
        painter.rect_filled(
            Rect::from_min_size(inner.min, vec2(fill_w, inner.height())),
            egui::CornerRadius::same(1),
            color,
        );
    }
}

pub fn paint_link_icon(painter: &egui::Painter, c: Pos2, size: f32, link: Link, color: Color32) {
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
            // The USB trident (the standard port symbol): a stem with an
            // arrowhead, a branch ending in a circle, one ending in a
            // square, and a dot at the base.
            let (top, bottom) = (c.y - s, c.y + s);
            painter.line_segment(
                [pos2(c.x, bottom - s * 0.3), pos2(c.x, top + s * 0.3)],
                stroke,
            );
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(c.x, top),
                    pos2(c.x + s * 0.32, top + s * 0.42),
                    pos2(c.x - s * 0.32, top + s * 0.42),
                ],
                color,
                Stroke::NONE,
            ));
            painter.circle_filled(pos2(c.x, bottom - s * 0.2), s * 0.24, color);
            let (lx, rx) = (c.x - s * 0.6, c.x + s * 0.6);
            painter.add(Shape::line(
                vec![
                    pos2(c.x, c.y + s * 0.4),
                    pos2(lx, c.y),
                    pos2(lx, c.y - s * 0.25),
                ],
                stroke,
            ));
            painter.circle_filled(pos2(lx, c.y - s * 0.38), s * 0.17, color);
            painter.add(Shape::line(
                vec![
                    pos2(c.x, c.y + s * 0.15),
                    pos2(rx, c.y - s * 0.25),
                    pos2(rx, c.y - s * 0.45),
                ],
                stroke,
            ));
            painter.rect_filled(
                Rect::from_center_size(pos2(rx, c.y - s * 0.6), vec2(s * 0.32, s * 0.32)),
                0.0,
                color,
            );
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
/// or arrow glyphs). The tray hover card has its own row, `paint_card_row`.
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

/// One device row for the tray hover card, in the Control Center's style:
/// name left; then, in fixed columns so every row lines up, the connection
/// (icon and "USB" / "Bluetooth" / "2.4 GHz"), a battery glyph and the
/// percentage, coloured by the battery alert levels (accent while charging,
/// with a small triangle).
pub fn paint_card_row(
    ui: &mut Ui,
    inner_w: f32,
    device: &Peripheral,
    charge_warn: u8,
    charge_crit: u8,
) {
    const H: f32 = 24.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(inner_w, H), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    let font = FontId::proportional(12.0);
    let muted = Color32::from_gray(140);
    let color = charge_color(device.battery, device.charging, charge_warn, charge_crit);
    let cy = rect.center().y;
    let width = |t: &str| p.layout_no_wrap(t.to_owned(), font.clone(), muted).size().x;

    // Right to left: percentage, charging mark, battery, connection.
    let pct_w = width("100 %");
    p.text(
        pos2(rect.max.x, cy),
        Align2::RIGHT_CENTER,
        format!("{} %", device.battery),
        font.clone(),
        color,
    );
    let mark_x = rect.max.x - pct_w - 7.0;
    if device.charging {
        p.add(Shape::convex_polygon(
            vec![
                pos2(mark_x, cy - 3.0),
                pos2(mark_x + 3.0, cy + 3.0),
                pos2(mark_x - 3.0, cy + 3.0),
            ],
            color,
            Stroke::NONE,
        ));
    }
    let glyph = Rect::from_min_size(pos2(mark_x - 8.0 - 22.0, cy - 6.0), vec2(22.0, 12.0));
    paint_battery_glyph(p, glyph, device.battery, color, muted);
    let link_w = width("Bluetooth") + 18.0;
    let link_x = glyph.min.x - 12.0 - link_w;
    if let Some(link) = link_of(&device.connection) {
        paint_link_icon(p, pos2(link_x + 6.0, cy), 11.0, link, muted);
        p.text(
            pos2(link_x + 16.0, cy),
            Align2::LEFT_CENTER,
            link_label(link),
            font.clone(),
            muted,
        );
    }
    let name_clip = Rect::from_min_max(rect.min, pos2(link_x - 8.0, rect.max.y));
    p.with_clip_rect(name_clip).text(
        pos2(rect.min.x, cy),
        Align2::LEFT_CENTER,
        &device.name,
        FontId::proportional(13.0),
        theme::C_TEXT,
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
