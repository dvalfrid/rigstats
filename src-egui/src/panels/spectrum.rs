//! The Now Playing spectrum's styles (#732): each draws the same band
//! heights (`rigstats_backend::spectrum::bars`) its own way into the panel's
//! spectrum area, except the oscilloscope, which draws the wave itself
//! (`rigstats_backend::spectrum::wave`). Only "falling peaks" keeps state
//! between frames (in egui's memory). The star field's stars are placed by a
//! fixed hash of their number and twinkle with the clock, so they stay put
//! and twinkle on without any state; every other style is a pure drawing of
//! this frame's bars.

use egui::epaint::PathStroke;
use egui::{pos2, Color32, Mesh, Pos2, Rect, Shape, Stroke, Ui};
use rigstats_backend::settings::{SpectrumColor, SpectrumStyle};

use crate::theme;

/// What the profile chose for the spectrum, carried with the dashboard's
/// view: frames a second (0 = off), style and colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpectrumChoice {
    pub fps: u8,
    pub style: SpectrumStyle,
    pub color: SpectrumColor,
}

impl SpectrumChoice {
    pub fn from_settings(s: &rigstats_backend::settings::Settings) -> Self {
        Self {
            fps: s.spectrum_fps,
            style: s.spectrum_style,
            color: s.spectrum_color,
        }
    }
}

const CYAN: Color32 = Color32::from_rgb(0, 200, 255);
const VIOLET: Color32 = Color32::from_rgb(168, 85, 247);
const PINK: Color32 = Color32::from_rgb(236, 72, 153);

/// The colour at `t` (0 = lowest note, 1 = highest).
fn color_at(choice: SpectrumColor, th: &theme::AppTheme, t: f32) -> Color32 {
    match choice {
        SpectrumColor::Accent => th.accent,
        SpectrumColor::Gradient => gradient(t),
    }
}

/// Cyan to violet to pink.
pub fn gradient(t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        lerp(CYAN, VIOLET, t * 2.0)
    } else {
        lerp(VIOLET, PINK, (t - 0.5) * 2.0)
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Draws `bars` (0–1 each) into `rect` in `choice`'s style; the
/// oscilloscope draws `wave` (−1 to 1 each) instead.
pub fn paint(
    ui: &Ui,
    rect: Rect,
    bars: &[f32],
    wave: &[f32],
    choice: SpectrumChoice,
    th: &theme::AppTheme,
    sc: f32,
) {
    let p = ui.painter();
    let n = bars.len();
    if n == 0 {
        return;
    }
    let t = |i: usize, of: usize| {
        if of > 1 {
            i as f32 / (of - 1) as f32
        } else {
            0.0
        }
    };
    match choice.style {
        SpectrumStyle::Bars | SpectrumStyle::Peaks => {
            let peaks = (choice.style == SpectrumStyle::Peaks).then(|| falling_peaks(ui, bars));
            for (i, (x0, x1)) in columns(rect.left(), rect.width(), 3.0 * sc, n)
                .into_iter()
                .enumerate()
            {
                let h = (bars[i] * rect.height()).max(2.0 * sc);
                let c = color_at(choice.color, th, t(i, n));
                p.rect_filled(
                    Rect::from_min_max(pos2(x0, rect.bottom() - h), pos2(x1, rect.bottom())),
                    1.0,
                    c,
                );
                if let Some(peaks) = &peaks {
                    let y = rect.bottom() - peaks[i] * rect.height() - 4.0 * sc;
                    p.rect_filled(
                        Rect::from_min_max(
                            pos2(x0, y.max(rect.top())),
                            pos2(x1, (y + 2.5 * sc).max(rect.top() + 2.5 * sc)),
                        ),
                        0.0,
                        Color32::WHITE,
                    );
                }
            }
        }
        SpectrumStyle::Mirror => {
            let v = resample(bars, 48);
            let mid = rect.center().y;
            for (i, (x0, x1)) in columns(rect.left(), rect.width(), 2.0 * sc, v.len())
                .into_iter()
                .enumerate()
            {
                let h = (v[i] * rect.height() / 2.0).max(1.0 * sc);
                p.rect_filled(
                    Rect::from_min_max(pos2(x0, mid - h), pos2(x1, mid + h)),
                    1.0,
                    color_at(choice.color, th, t(i, v.len())),
                );
            }
        }
        SpectrumStyle::Neon => {
            let count = ((rect.width() / (3.5 * sc)) as usize).clamp(24, 160);
            let v = resample(bars, count);
            let mid = rect.center().y;
            for (i, h) in v.iter().enumerate() {
                let x = rect.left() + (i as f32 + 0.5) * rect.width() / count as f32;
                // A fixed texture per line, so neighbours differ like the lines
                // of a real waveform; it doesn't flicker between frames.
                let h = (h * texture(i) * rect.height() / 2.0).max(1.0 * sc);
                let c = color_at(choice.color, th, t(i, count));
                p.line_segment(
                    [pos2(x, mid - h), pos2(x, mid + h)],
                    Stroke::new(4.0 * sc, c.gamma_multiply(0.28)),
                );
                p.line_segment(
                    [pos2(x, mid - h), pos2(x, mid + h)],
                    Stroke::new(1.3 * sc, c),
                );
            }
        }
        SpectrumStyle::Mountain => {
            let v = resample(bars, 64);
            let pts: Vec<Pos2> = v
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    pos2(
                        rect.left() + t(i, v.len()) * rect.width(),
                        rect.bottom() - h * rect.height(),
                    )
                })
                .collect();
            let mut mesh = Mesh::default();
            for (i, pt) in pts.iter().enumerate() {
                let c = color_at(choice.color, th, t(i, pts.len()));
                mesh.colored_vertex(*pt, c.gamma_multiply(0.55));
                mesh.colored_vertex(pos2(pt.x, rect.bottom()), c.gamma_multiply(0.04));
                if i > 0 {
                    let k = (i * 2) as u32;
                    mesh.add_triangle(k - 2, k - 1, k);
                    mesh.add_triangle(k - 1, k + 1, k);
                }
            }
            p.add(mesh);
            for (i, w) in pts.windows(2).enumerate() {
                p.line_segment(
                    [w[0], w[1]],
                    Stroke::new(2.2 * sc, color_at(choice.color, th, t(i, pts.len()))),
                );
            }
        }
        SpectrumStyle::Led => {
            const SEGMENTS: usize = 10;
            let gap = 2.0 * sc;
            let seg_h = (rect.height() - gap * (SEGMENTS - 1) as f32) / SEGMENTS as f32;
            for (i, (x0, x1)) in columns(rect.left(), rect.width(), 3.0 * sc, n)
                .into_iter()
                .enumerate()
            {
                let lit = lit_segments(bars[i], SEGMENTS);
                for k in 0..SEGMENTS {
                    let mut c = led_color(k, SEGMENTS);
                    if k >= lit {
                        c = c.gamma_multiply(0.12);
                    }
                    let y1 = rect.bottom() - k as f32 * (seg_h + gap);
                    p.rect_filled(
                        Rect::from_min_max(pos2(x0, y1 - seg_h), pos2(x1, y1)),
                        1.0,
                        c,
                    );
                }
            }
        }
        SpectrumStyle::Dots => {
            const ROWS: usize = 8;
            let cell_h = rect.height() / ROWS as f32;
            let cols = columns(rect.left(), rect.width(), 0.0, n);
            let r = ((cols[0].1 - cols[0].0).min(cell_h) * 0.32).max(1.0);
            for (i, (x0, x1)) in cols.into_iter().enumerate() {
                let lit = lit_segments(bars[i], ROWS);
                let c = color_at(choice.color, th, t(i, n));
                for k in 0..ROWS {
                    let centre = pos2((x0 + x1) / 2.0, rect.bottom() - (k as f32 + 0.5) * cell_h);
                    p.circle_filled(centre, r, if k < lit { c } else { c.gamma_multiply(0.14) });
                }
            }
        }
        SpectrumStyle::Stars => {
            // Seconds, kept small so f32 keeps its precision.
            let time = (ui.input(|i| i.time) % 3600.0) as f32;
            let bass = bars[..BASS_BANDS.min(n)].iter().sum::<f32>() / BASS_BANDS.min(n) as f32;
            let margin = 3.0 * sc;
            let area = rect.shrink(margin);
            for j in 0..n * STARS_PER_BAND {
                let star = star(j, n);
                let level = bars[star.band];
                let lit = star_lit(level, star.threshold);
                let a = (lit * twinkle(time, star.speed, star.phase)).max(0.1);
                let centre = pos2(
                    area.left() + star.x * area.width(),
                    area.top() + star.y * area.height(),
                );
                let c = color_at(choice.color, th, t(star.band, n));
                let mut r = star.size * sc;
                if star.big {
                    // Bass makes the big ones flare: larger, with a halo.
                    let flare = bass * lit;
                    r *= 1.0 + flare;
                    if flare > 0.05 {
                        p.circle_filled(centre, r * 2.2, c.gamma_multiply(0.2 * flare));
                    }
                }
                p.circle_filled(
                    centre,
                    r,
                    lerp(c, Color32::WHITE, 0.45 * lit).gamma_multiply(a),
                );
            }
        }
        SpectrumStyle::Scope => {
            if wave.len() < 2 {
                return;
            }
            let mid = rect.center().y;
            let half = rect.height() / 2.0 - 2.0 * sc;
            let pts: Vec<Pos2> = wave
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    pos2(
                        rect.left() + t(i, wave.len()) * rect.width(),
                        mid - v.clamp(-1.0, 1.0) * half,
                    )
                })
                .collect();
            // A soft glow under a thin line.
            for (width, alpha) in [(5.0 * sc, 0.22), (1.6 * sc, 1.0)] {
                let stroke = match choice.color {
                    SpectrumColor::Accent => {
                        PathStroke::new(width, th.accent.gamma_multiply(alpha))
                    }
                    SpectrumColor::Gradient => {
                        let (left, w) = (rect.left(), rect.width().max(1.0));
                        PathStroke::new_uv(width, move |_, pos| {
                            gradient((pos.x - left) / w).gamma_multiply(alpha)
                        })
                    }
                };
                p.add(Shape::line(pts.clone(), stroke));
            }
        }
    }
}

/// How many stars each band has; louder bands light more of them.
pub const STARS_PER_BAND: usize = 6;

/// The lowest bands, whose mean makes the big stars flare.
const BASS_BANDS: usize = 4;

/// One star of the star field, fixed by its number: where it is (0–1 in
/// the area, inside its band's column), how loud its band must be to light
/// it, how it twinkles, and whether it is one of the big ones.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Star {
    pub band: usize,
    pub x: f32,
    pub y: f32,
    pub threshold: f32,
    pub size: f32,
    pub speed: f32,
    pub phase: f32,
    pub big: bool,
}

/// Star `j` of a field over `bands` bands.
pub fn star(j: usize, bands: usize) -> Star {
    let bands = bands.max(1);
    let band = (j / STARS_PER_BAND).min(bands - 1);
    let k = j % STARS_PER_BAND;
    let big = hash01(j, 6) < 0.08;
    Star {
        band,
        x: (band as f32 + hash01(j, 1)) / bands as f32,
        y: hash01(j, 2),
        // Spread over 0–0.85, star k of a band in its own slice, so a band
        // at height h lights about h of its stars.
        threshold: (k as f32 + hash01(j, 3)) / STARS_PER_BAND as f32 * 0.85,
        size: if big { 1.8 } else { 0.8 + 0.6 * hash01(j, 7) },
        speed: 1.5 + 2.5 * hash01(j, 4),
        phase: std::f32::consts::TAU * hash01(j, 5),
        big,
    }
}

/// How lit a star is (0–1) at its band's `level`: off below its threshold,
/// fully lit a little above it.
pub fn star_lit(level: f32, threshold: f32) -> f32 {
    ((level - threshold) / 0.12).clamp(0.0, 1.0)
}

/// A star's brightness factor at `time`: 0.55–1.0, never off.
fn twinkle(time: f32, speed: f32, phase: f32) -> f32 {
    0.775 + 0.225 * (time * speed + phase).sin()
}

/// A fixed number 0–1 for `(i, salt)`.
fn hash01(i: usize, salt: u32) -> f32 {
    let mut x = (i as u32)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(salt.wrapping_mul(0x85EB_CA6B));
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x % 10_000) as f32 / 9_999.0
}

/// LED colours from the bottom: green, then yellow, then red at the top.
fn led_color(k: usize, of: usize) -> Color32 {
    let f = k as f32 / of as f32;
    if f >= 0.8 {
        Color32::from_rgb(239, 68, 68)
    } else if f >= 0.6 {
        Color32::from_rgb(250, 204, 21)
    } else {
        Color32::from_rgb(34, 197, 94)
    }
}

/// How many of `of` segments a bar of height `h` (0–1) lights.
pub fn lit_segments(h: f32, of: usize) -> usize {
    ((h.clamp(0.0, 1.0) * of as f32).round() as usize).min(of)
}

/// `n` columns side by side across `width` from `left`, `gap` between them:
/// each column's left and right edge.
pub fn columns(left: f32, width: f32, gap: f32, n: usize) -> Vec<(f32, f32)> {
    if n == 0 {
        return Vec::new();
    }
    let w = ((width - gap * (n as f32 - 1.0)) / n as f32).max(1.0);
    (0..n)
        .map(|i| {
            let x0 = left + i as f32 * (w + gap);
            (x0, x0 + w)
        })
        .collect()
}

/// `values` spread over `n` points with a smooth (Catmull-Rom) curve, each
/// kept within 0–1.
pub fn resample(values: &[f32], n: usize) -> Vec<f32> {
    let m = values.len();
    if m == 0 || n == 0 {
        return vec![0.0; n];
    }
    if m == 1 || n == 1 {
        return vec![values[0].clamp(0.0, 1.0); n];
    }
    let at = |i: isize| values[i.clamp(0, m as isize - 1) as usize];
    (0..n)
        .map(|j| {
            let x = j as f32 * (m - 1) as f32 / (n - 1) as f32;
            let i = x.floor() as isize;
            let f = x - i as f32;
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let v = 0.5
                * (2.0 * p1
                    + (p2 - p0) * f
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f * f
                    + (3.0 * p1 - p0 - 3.0 * p2 + p3) * f * f * f);
            v.clamp(0.0, 1.0)
        })
        .collect()
}

/// A fixed factor 0.6–1.0 per line index.
fn texture(i: usize) -> f32 {
    let x = (i as u32).wrapping_mul(2_654_435_761).rotate_left(13) % 1000;
    0.6 + 0.4 * x as f32 / 999.0
}

/// How long a peak stays before it falls, and how fast it falls (heights a
/// second).
const PEAK_HOLD: f32 = 0.4;
const PEAK_FALL: f32 = 0.7;

/// The peaks after `dt` seconds: a bar at or above its peak lifts it and
/// starts its hold; after the hold a peak falls, never below its bar.
pub fn update_peaks(peaks: &mut [f32], held: &mut [f32], bars: &[f32], dt: f32) {
    for i in 0..peaks.len().min(bars.len()).min(held.len()) {
        if bars[i] >= peaks[i] {
            peaks[i] = bars[i];
            held[i] = 0.0;
        } else {
            held[i] += dt;
            if held[i] > PEAK_HOLD {
                peaks[i] = (peaks[i] - PEAK_FALL * dt).max(bars[i]);
            }
        }
    }
}

/// The falling peaks for this frame, kept in egui's memory between frames.
fn falling_peaks(ui: &Ui, bars: &[f32]) -> Vec<f32> {
    let id = egui::Id::unique("spectrum_peaks");
    let now = ui.input(|i| i.time);
    let (mut peaks, mut held, last) = ui
        .ctx()
        .data(|d| d.get_temp::<(Vec<f32>, Vec<f32>, f64)>(id))
        .filter(|(p, _, _)| p.len() == bars.len())
        .unwrap_or_else(|| (bars.to_vec(), vec![0.0; bars.len()], now));
    let dt = ((now - last) as f32).clamp(0.0, 0.5);
    update_peaks(&mut peaks, &mut held, bars, dt);
    ui.ctx()
        .data_mut(|d| d.insert_temp(id, (peaks.clone(), held, now)));
    peaks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_fill_the_width_with_their_gaps() {
        let c = columns(10.0, 380.0, 3.0, 24);
        assert_eq!(c.len(), 24);
        assert!((c[0].0 - 10.0).abs() < 0.01);
        assert!((c[23].1 - 390.0).abs() < 0.01, "{:?}", c[23]);
        assert!((c[1].0 - c[0].1 - 3.0).abs() < 0.01);
    }

    #[test]
    fn resampling_keeps_the_ends_and_stays_in_range() {
        let v = resample(&[0.0, 1.0, 0.2, 0.9], 40);
        assert_eq!(v.len(), 40);
        assert!((v[0] - 0.0).abs() < 1e-5);
        assert!((v[39] - 0.9).abs() < 1e-5);
        assert!(v.iter().all(|x| (0.0..=1.0).contains(x)));
        assert_eq!(resample(&[], 5), vec![0.0; 5]);
        assert_eq!(resample(&[0.4], 3), vec![0.4; 3]);
    }

    #[test]
    fn a_peak_holds_then_falls_but_never_below_its_bar() {
        let mut peaks = vec![0.0; 2];
        let mut held = vec![0.0; 2];
        update_peaks(&mut peaks, &mut held, &[0.8, 0.5], 0.03);
        assert_eq!(peaks, [0.8, 0.5]);
        update_peaks(&mut peaks, &mut held, &[0.1, 0.45], 0.3);
        assert_eq!(peaks, [0.8, 0.5], "still held");
        update_peaks(&mut peaks, &mut held, &[0.1, 0.45], 0.2);
        assert!(
            (peaks[0] - (0.8 - PEAK_FALL * 0.2)).abs() < 1e-5,
            "{peaks:?}"
        );
        assert!(
            (peaks[1] - 0.45).abs() < 1e-5,
            "stops at its bar: {peaks:?}"
        );
    }

    #[test]
    fn segments_light_by_height() {
        assert_eq!(lit_segments(0.0, 10), 0);
        assert_eq!(lit_segments(0.54, 10), 5);
        assert_eq!(lit_segments(1.0, 10), 10);
        assert_eq!(lit_segments(3.0, 10), 10);
    }

    #[test]
    fn the_gradient_runs_cyan_violet_pink() {
        assert_eq!(gradient(0.0), CYAN);
        assert_eq!(gradient(0.5), VIOLET);
        assert_eq!(gradient(1.0), PINK);
    }

    #[test]
    fn every_star_sits_in_its_bands_column_and_stays_there() {
        let n = 24;
        for j in 0..n * STARS_PER_BAND {
            let s = star(j, n);
            assert_eq!(s.band, j / STARS_PER_BAND);
            let (lo, hi) = (s.band as f32 / n as f32, (s.band + 1) as f32 / n as f32);
            assert!(s.x >= lo && s.x <= hi, "{j}: {s:?}");
            assert!((0.0..=1.0).contains(&s.y));
            assert!((0.0..0.86).contains(&s.threshold));
            assert_eq!(star(j, n), s, "the same star every frame");
        }
        let big = (0..n * STARS_PER_BAND).filter(|&j| star(j, n).big).count();
        assert!((4..=25).contains(&big), "{big} big stars");
    }

    #[test]
    fn louder_lights_more_stars_and_silence_none() {
        let lit_at = |level: f32| {
            (0..STARS_PER_BAND * 24)
                .filter(|&j| star_lit(level, star(j, 24).threshold) > 0.5)
                .count()
        };
        assert_eq!(lit_at(0.0), 0);
        assert!(
            lit_at(0.3) < lit_at(0.8),
            "{} vs {}",
            lit_at(0.3),
            lit_at(0.8)
        );
        assert_eq!(lit_at(1.0), STARS_PER_BAND * 24);
    }

    #[test]
    fn a_star_twinkles_but_never_goes_out() {
        let v: Vec<f32> = (0..200)
            .map(|i| twinkle(i as f32 * 0.05, 2.0, 1.0))
            .collect();
        assert!(v.iter().all(|x| (0.54..=1.0).contains(x)));
        let (min, max) = v
            .iter()
            .fold((1.0f32, 0.0f32), |(a, b), x| (a.min(*x), b.max(*x)));
        assert!(max - min > 0.3, "it twinkles: {min}–{max}");
    }

    #[test]
    fn led_colours_go_green_yellow_red() {
        assert_eq!(led_color(0, 10), Color32::from_rgb(34, 197, 94));
        assert_eq!(led_color(6, 10), Color32::from_rgb(250, 204, 21));
        assert_eq!(led_color(9, 10), Color32::from_rgb(239, 68, 68));
    }
}
