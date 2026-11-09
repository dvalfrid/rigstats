//! NOW PLAYING (#718): the cover picture, title, artist, album and whether it
//! plays, from `PollStats::media` (Windows' current media session, read in
//! `rigstats_backend::media`). Below them, when the profile turns it on, a
//! spectrum of what the PC plays (#719, `rigstats_backend::spectrum`). The
//! ring styles (#732 round 3) change the layout instead: a round cover in
//! the middle with the spectrum around it, and the title below.

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
    // The spectrum (#719, #732, the profile's look): rate (0 = none), style
    // and colour. The compact layout shows a small one on its title row.
    spectrum: super::spectrum::SpectrumChoice,
) -> egui::Rect {
    theme::panel_frame(ui, opacity, th, sc, |ui| {
        if !compact {
            ui.set_min_height(theme::PANEL_DATA_H * sc);
        }
        let title = RichText::new("NOW PLAYING")
            .strong()
            .color(theme::C_PANEL_TITLE)
            .size(theme::FONT_PANEL_TITLE * sc);
        if compact && spectrum.fps > 0 {
            let playing = stats.media.as_ref().is_some_and(|m| m.playing);
            title_with_mini_spectrum(ui, title, playing, spectrum, th, sc);
        } else {
            ui.label(title);
        }
        ui.add_space(4.0 * sc);
        if compact {
            super::body_pad(ui);
        }
        if !compact && spectrum.fps > 0 && spectrum.style.around_cover() {
            ring_layout(ui, stats.media.as_ref(), spectrum, th, sc, opacity);
            return;
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
        if !compact && spectrum.fps > 0 {
            draw_spectrum(ui, now.playing, spectrum, th, sc);
        }
    })
}

/// The spectrum's height at scale 1.
const SPECTRUM_H: f32 = 44.0;

/// The spectrum, listening only while something plays: when it doesn't, the
/// bars rest at the bottom and nothing asks for audio or extra frames, so the
/// capture thread ends. While it plays, the next frame is asked for after
/// `1 / fps`.
fn draw_spectrum(
    ui: &mut Ui,
    playing: bool,
    choice: super::spectrum::SpectrumChoice,
    th: &theme::AppTheme,
    sc: f32,
) {
    let (bars, wave) = spectrum_data(ui, playing, choice);
    ui.add_space(10.0 * sc);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), SPECTRUM_H * sc),
        Sense::hover(),
    );
    super::spectrum::paint(ui, rect, &bars, &wave, choice, th, sc);
}

/// This frame's bands and wave while something plays (the oscilloscope gets
/// the wave, every other style the bands), asking for the next frame after
/// `1 / fps`; at rest (all zero) otherwise, with no capture and no extra
/// frames.
fn spectrum_data(
    ui: &Ui,
    playing: bool,
    choice: super::spectrum::SpectrumChoice,
) -> (
    [f32; rigstats_backend::spectrum::BANDS],
    [f32; rigstats_backend::spectrum::WAVE_POINTS],
) {
    use rigstats_backend::spectrum::{BANDS, WAVE_POINTS};
    let mut bars = [0.0; BANDS];
    let mut wave = [0.0; WAVE_POINTS];
    if playing && choice.fps > 0 {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f32(
                1.0 / f32::from(choice.fps),
            ));
        if choice.style == rigstats_backend::settings::SpectrumStyle::Scope {
            wave = rigstats_backend::spectrum::wave();
        } else {
            bars = rigstats_backend::spectrum::bars();
        }
    }
    (bars, wave)
}

/// The space between NOW PLAYING and the compact layout's spectrum, which
/// takes the rest of the title row, and how far it reaches down into the
/// space between the title row and the cover (drawn there, not laid out, so
/// the row and the panel keep their height); at scale 1.
const MINI_GAP: f32 = 20.0;
const MINI_DROP: f32 = 6.0;

/// The compact layout's title row: NOW PLAYING on the left and, filling the
/// rest of the row, a small spectrum a little taller than the title, so the
/// panel stays at the shared compact height (#604).
fn title_with_mini_spectrum(
    ui: &mut Ui,
    title: RichText,
    playing: bool,
    choice: super::spectrum::SpectrumChoice,
    th: &theme::AppTheme,
    sc: f32,
) {
    ui.scope(|ui| {
        // A horizontal row is otherwise a button high; keep it the title's.
        ui.spacing_mut().interact_size.y = 0.0;
        ui.horizontal(|ui| {
            let h = ui.label(title).rect.height();
            let w = (ui.available_width() - MINI_GAP * sc).max(0.0);
            if w < 24.0 * sc {
                return;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (row, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
                let rect = egui::Rect::from_min_max(
                    egui::pos2(row.left(), row.top() + 1.0 * sc),
                    egui::pos2(row.right(), row.bottom() + MINI_DROP * sc),
                );
                let (bars, wave) = spectrum_data(ui, playing, choice);
                let mini = super::spectrum::SpectrumChoice {
                    style: super::spectrum::compact_style(choice.style),
                    ..choice
                };
                super::spectrum::paint(ui, rect, &bars, &wave, mini, th, sc);
            });
        });
    });
}

/// This frame's bars while something plays, asking for the next frame after
/// `1 / fps`; at rest (all zero) otherwise, with no capture and no extra
/// frames.
fn spectrum_bars(ui: &Ui, playing: bool, fps: u8) -> [f32; rigstats_backend::spectrum::BANDS] {
    if playing && fps > 0 {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f32(1.0 / f32::from(fps)));
        rigstats_backend::spectrum::bars()
    } else {
        [0.0; rigstats_backend::spectrum::BANDS]
    }
}

/// The ring layout's square at scale 1 (cover and spectrum around it): as
/// wide as the panel up to this, and the smallest it shrinks to in a
/// landscape cell.
const RING_AREA: f32 = 300.0;
const RING_AREA_MIN: f32 = 80.0;

/// The cover's radius at scale 1, and at most this share of the ring area's
/// radius (a ring shrunk into a landscape cell keeps room for its bars).
/// The cover keeps its size as the area grows, so the room for the spectrum
/// around it grows instead.
const RING_COVER_R: f32 = 61.0;
const RING_COVER: f32 = 0.58;

/// The spectrum around a round cover in the middle, the title and artist
/// centred below (#732 round 3). Taller than the other panels where the
/// window or card grows with it; in the landscape grid's fixed cell the
/// ring shrinks so the text still fits (`theme::panel_room`).
fn ring_layout(
    ui: &mut Ui,
    now: Option<&rigstats_backend::media::NowPlaying>,
    choice: super::spectrum::SpectrumChoice,
    th: &theme::AppTheme,
    sc: f32,
    opacity: f32,
) {
    let gap = 6.0 * sc;
    let title_h = ui.fonts_mut(|f| f.row_height(&egui::FontId::proportional(16.0 * sc)));
    let artist_h = ui.fonts_mut(|f| f.row_height(&egui::FontId::proportional(13.0 * sc)));
    let text_h = title_h + 2.0 * sc + artist_h;
    let mut side = RING_AREA * sc;
    if let Some(room) = theme::panel_room(ui, sc) {
        // egui's own spacing comes after the ring and after the gap.
        let spacing = 2.0 * ui.spacing().item_spacing.y;
        side = side.min(room - gap - spacing - text_h);
    }
    let side = side.max(RING_AREA_MIN * sc).min(ui.available_width());

    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), side), Sense::hover());
    let centre = rect.center();
    let r_out = side / 2.0;
    let r_cover = (RING_COVER_R * sc).min(r_out * RING_COVER);
    let bars = spectrum_bars(ui, now.is_some_and(|n| n.playing), choice.fps);
    super::spectrum::paint_around(ui, centre, r_cover + 4.0 * sc, r_out, &bars, choice, th, sc);
    let texture = now.and_then(|n| cover_texture(ui.ctx(), n.cover.as_ref()));
    paint_cover(
        ui,
        egui::Rect::from_center_size(centre, Vec2::splat(2.0 * r_cover)),
        texture.as_ref(),
        r_cover,
        th,
        opacity,
    );

    ui.add_space(gap);
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0 * sc;
        match now {
            Some(now) => {
                one_line(ui, &now.track.title, 16.0 * sc, theme::C_VALUE, true);
                // An empty artist still takes its line, so the panel's height
                // doesn't change with the track.
                let artist = if now.track.artist.is_empty() {
                    " "
                } else {
                    &now.track.artist
                };
                one_line(ui, artist, 13.0 * sc, th.text_muted, false);
            }
            None => {
                // The title's size, so the panel keeps its height.
                one_line(ui, "NOTHING PLAYING", 16.0 * sc, th.text_muted, false);
                one_line(ui, " ", 13.0 * sc, th.text_muted, false);
            }
        }
    });
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
    paint_cover(ui, rect, texture, side * 0.06, th, opacity);
}

/// The cover in `rect` with corners of `radius` (half the side makes it
/// round), or a quiet shape in its place.
fn paint_cover(
    ui: &Ui,
    rect: egui::Rect,
    texture: Option<&TextureHandle>,
    radius: f32,
    th: &theme::AppTheme,
    opacity: f32,
) {
    let radius = CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8);
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
