//! NOW PLAYING (#718): the cover picture, title, artist, album and whether it
//! plays, from `PollStats::media` (Windows' current media session, read in
//! `rigstats_backend::media`).

use egui::{Color32, CornerRadius, RichText, Sense, TextureHandle, Ui, Vec2};

use crate::{theme, PollStats};

/// The cover's side at scale 1: full and compact.
const COVER: f32 = 128.0;
const COVER_COMPACT: f32 = 44.0;

/// The largest side a cover is uploaded at: it's drawn at most `COVER` × the
/// floating scale (200 %), so more is wasted memory.
const COVER_MAX_PX: u32 = 256;

pub fn draw(
    ui: &mut Ui,
    stats: &PollStats,
    opacity: f32,
    th: &theme::AppTheme,
    sc: f32,
    // The compact layout: a smaller cover beside the title and artist, no
    // album or state row.
    compact: bool,
) -> egui::Rect {
    theme::panel_frame(ui, opacity, th, sc, |ui| {
        if !compact {
            ui.set_min_height(theme::PANEL_DATA_H * sc);
        }
        ui.label(
            RichText::new("NOW PLAYING")
                .strong()
                .color(theme::C_PANEL_TITLE)
                .size(theme::FONT_PANEL_TITLE * sc),
        );
        ui.add_space(4.0 * sc);
        if compact {
            super::body_pad(ui);
        }

        let Some(now) = &stats.media else {
            ui.horizontal(|ui| {
                cover_box(ui, None, side(compact, sc), th, opacity);
                ui.add_space(10.0 * sc);
                ui.label(
                    RichText::new("NOTHING PLAYING")
                        .size(14.0 * sc)
                        .color(th.text_muted),
                );
            });
            return;
        };

        let texture = cover_texture(ui.ctx(), now.cover.as_ref());
        ui.horizontal(|ui| {
            cover_box(ui, texture.as_ref(), side(compact, sc), th, opacity);
            ui.add_space(10.0 * sc);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0 * sc;
                one_line(ui, &now.track.title, 16.0 * sc, theme::C_VALUE, true);
                if !now.track.artist.is_empty() {
                    one_line(ui, &now.track.artist, 13.0 * sc, th.text_muted, false);
                }
                if !compact && !now.track.album.is_empty() {
                    one_line(ui, &now.track.album, 11.0 * sc, th.stat_label, false);
                }
                if !compact {
                    ui.add_space(6.0 * sc);
                    state_row(ui, now.playing, th, sc);
                }
            });
        });
    })
}

fn side(compact: bool, sc: f32) -> f32 {
    (if compact { COVER_COMPACT } else { COVER }) * sc
}

/// Text cut with an ellipsis at the panel's edge, never wrapped into other
/// text.
fn one_line(ui: &mut Ui, text: &str, size: f32, color: Color32, strong: bool) {
    let mut rich = RichText::new(text).size(size).color(color);
    if strong {
        rich = rich.strong();
    }
    ui.add(egui::Label::new(rich).truncate().selectable(false));
}

/// A small play triangle or pause bars, then PLAYING / PAUSED.
fn state_row(ui: &mut Ui, playing: bool, th: &theme::AppTheme, sc: f32) {
    let color = if playing { th.accent } else { th.text_muted };
    ui.horizontal(|ui| {
        let s = 10.0 * sc;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(s), Sense::hover());
        let p = ui.painter();
        if playing {
            p.add(egui::Shape::convex_polygon(
                vec![
                    rect.left_top(),
                    rect.left_bottom(),
                    egui::pos2(rect.right(), rect.center().y),
                ],
                color,
                egui::Stroke::NONE,
            ));
        } else {
            let w = s * 0.32;
            p.rect_filled(
                egui::Rect::from_min_size(rect.left_top(), Vec2::new(w, s)),
                0.0,
                color,
            );
            p.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(rect.right() - w, rect.top()),
                    Vec2::new(w, s),
                ),
                0.0,
                color,
            );
        }
        ui.label(
            RichText::new(if playing { "PLAYING" } else { "PAUSED" })
                .size(11.0 * sc)
                .color(color),
        );
    });
}

/// The cover, or a quiet square in its place, so the layout doesn't move
/// when a picture arrives or the music stops.
fn cover_box(
    ui: &mut Ui,
    texture: Option<&TextureHandle>,
    side: f32,
    th: &theme::AppTheme,
    opacity: f32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let radius = CornerRadius::same((side * 0.06).round() as u8);
    match texture {
        // Opaque whatever the panels' opacity: in the desktop wallpaper the
        // panels are often almost see-through, and a picture at that opacity
        // is all but gone. The quiet square in its place follows it.
        Some(tex) => {
            egui::Image::new(tex)
                .corner_radius(radius)
                .paint_at(ui, rect);
        }
        None => {
            ui.painter()
                .rect_filled(rect, radius, th.stat_label.gamma_multiply(0.18 * opacity));
        }
    }
}

/// The cover's texture, decoded and uploaded once per picture and kept in
/// egui's memory with the id it was made from. A picture that won't decode
/// is remembered as none, so it isn't tried every frame.
fn cover_texture(
    ctx: &egui::Context,
    cover: Option<&rigstats_backend::media::Cover>,
) -> Option<TextureHandle> {
    let cover = cover?;
    let key = egui::Id::unique("now_playing_cover");
    if let Some((id, tex)) = ctx.data(|d| d.get_temp::<(u64, Option<TextureHandle>)>(key)) {
        if id == cover.id {
            return tex;
        }
    }
    let tex = decode(&cover.bytes)
        .map(|image| ctx.load_texture("now-playing-cover", image, egui::TextureOptions::LINEAR));
    ctx.data_mut(|d| d.insert_temp(key, (cover.id, tex.clone())));
    tex
}

/// The picture as an egui image, scaled down to `COVER_MAX_PX` on its longer
/// side and cut to a square from its centre (video thumbnails are wide).
fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let (w, h) = (image.width(), image.height());
    if w == 0 || h == 0 {
        return None;
    }
    let side = w.min(h);
    let square = image.crop_imm((w - side) / 2, (h - side) / 2, side, side);
    let px = side.min(COVER_MAX_PX);
    let rgba = square
        .resize_exact(px, px, image::imageops::FilterType::Triangle)
        .to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [px as usize, px as usize],
        rgba.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 10, 10, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn a_large_cover_is_scaled_down() {
        let image = decode(&png(600, 600)).unwrap();
        assert_eq!(image.size, [COVER_MAX_PX as usize; 2]);
    }

    #[test]
    fn a_small_cover_keeps_its_size() {
        let image = decode(&png(64, 64)).unwrap();
        assert_eq!(image.size, [64, 64]);
    }

    #[test]
    fn a_wide_thumbnail_is_cut_to_a_square() {
        let image = decode(&png(320, 180)).unwrap();
        assert_eq!(image.size, [180, 180]);
    }

    #[test]
    fn bytes_that_are_no_picture_give_none() {
        assert!(decode(b"not a picture").is_none());
        assert!(decode(&[]).is_none());
    }
}
