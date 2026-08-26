//! Procedurally-drawn glyph icons for the tray context-menu rows.
//!
//! Each icon is rasterized from simple shape primitives (circle/ring/
//! triangle/rect/line) at 4x supersample and box-filtered down to give
//! antialiased edges, rather than shipping a dozen hand-authored PNGs.
//! Coordinates for shape math are in icon space, `0.0..SIZE`.

use crate::theme;
use eframe::egui::Color32;
use tray_icon::menu::Icon;

const SIZE: u32 = 32;
const SS: u32 = 4;
const CENTER: f32 = SIZE as f32 / 2.0;

/// Neutral glyph color, readable on both light and dark native menu
/// backgrounds — only the recording row uses accent colors (green/red).
const NEUTRAL: Color32 = Color32::from_gray(140);

/// Rasterizes `paint` (icon-space coords, opaque return = covered) into an
/// `Icon`, supersampling `SS`x per axis and averaging alpha-weighted color
/// per output pixel for antialiased edges.
fn rasterize(paint: impl Fn(f32, f32) -> Option<Color32>) -> Icon {
    let mut out = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (mut r, mut g, mut b, mut a) = (0f32, 0f32, 0f32, 0f32);
            for dy in 0..SS {
                for dx in 0..SS {
                    let fx = (x * SS + dx) as f32 / SS as f32 + 0.5 / SS as f32;
                    let fy = (y * SS + dy) as f32 / SS as f32 + 0.5 / SS as f32;
                    if let Some(c) = paint(fx, fy) {
                        let af = c.a() as f32;
                        r += c.r() as f32 * af;
                        g += c.g() as f32 * af;
                        b += c.b() as f32 * af;
                        a += af;
                    }
                }
            }
            let n = (SS * SS) as f32;
            let alpha = (a / n).round().clamp(0.0, 255.0) as u8;
            let (rr, gg, bb) = if a > 0.0 {
                ((r / a) as u8, (g / a) as u8, (b / a) as u8)
            } else {
                (0, 0, 0)
            };
            let i = ((y * SIZE + x) * 4) as usize;
            out[i] = rr;
            out[i + 1] = gg;
            out[i + 2] = bb;
            out[i + 3] = alpha;
        }
    }
    Icon::from_rgba(out, SIZE, SIZE).expect("menu icon rgba")
}

fn dist(x: f32, y: f32, cx: f32, cy: f32) -> f32 {
    ((x - cx).powi(2) + (y - cy).powi(2)).sqrt()
}

fn seg_dist(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((x - ax) * dx + (y - ay) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist(x, y, ax + t * dx, ay + t * dy)
}

fn in_ring(x: f32, y: f32, cx: f32, cy: f32, r: f32, t: f32) -> bool {
    let d = dist(x, y, cx, cy);
    d >= r - t / 2.0 && d <= r + t / 2.0
}

/// Angle in degrees, clockwise from straight up, of `(x, y)` around `(cx, cy)`.
fn angle_from_top_deg(x: f32, y: f32, cx: f32, cy: f32) -> f32 {
    (x - cx).atan2(cy - y).to_degrees()
}

fn rect_fill(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> bool {
    x >= x0 && x <= x1 && y >= y0 && y <= y1
}

fn rect_stroke(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, t: f32) -> bool {
    let outer = x >= x0 - t / 2.0 && x <= x1 + t / 2.0 && y >= y0 - t / 2.0 && y <= y1 + t / 2.0;
    let inner = x >= x0 + t / 2.0 && x <= x1 - t / 2.0 && y >= y0 + t / 2.0 && y <= y1 - t / 2.0;
    outer && !inner
}

fn in_triangle(x: f32, y: f32, v0: (f32, f32), v1: (f32, f32), v2: (f32, f32)) -> bool {
    let sign = |p: (f32, f32), a: (f32, f32), b: (f32, f32)| {
        (p.0 - b.0) * (a.1 - b.1) - (a.0 - b.0) * (p.1 - b.1)
    };
    let d1 = sign((x, y), v0, v1);
    let d2 = sign((x, y), v1, v2);
    let d3 = sign((x, y), v2, v0);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

/// Two overlapping window outlines — Toggle Floating Mode.
pub fn floating() -> Icon {
    rasterize(|x, y| {
        let hit = rect_stroke(x, y, 7.0, 7.0, 21.0, 21.0, 2.0)
            || rect_stroke(x, y, 12.0, 12.0, 26.0, 26.0, 2.0);
        hit.then_some(NEUTRAL)
    })
}

/// Green play triangle — shown on the recording row while stopped.
pub fn record_start() -> Icon {
    rasterize(|x, y| {
        in_triangle(x, y, (11.0, 9.0), (11.0, 23.0), (23.0, 16.0)).then_some(theme::C_GRN)
    })
}

/// Red record dot at the given alpha — shown on the recording row while
/// active; `alpha` is pulsed between bright/dim in sync with the tray-icon
/// blink to give the menu row the same blinking cue.
pub fn record_dot(alpha: u8) -> Icon {
    let color = Color32::from_rgba_unmultiplied(
        theme::C_AMD.r(),
        theme::C_AMD.g(),
        theme::C_AMD.b(),
        alpha,
    );
    rasterize(move |x, y| (dist(x, y, CENTER, CENTER) <= 7.0).then_some(color))
}

/// Clock face — Session History.
pub fn history() -> Icon {
    rasterize(|x, y| {
        let ring = in_ring(x, y, CENTER, CENTER, 10.0, 2.2);
        let hour = seg_dist(x, y, CENTER, CENTER, CENTER, CENTER - 6.0) <= 1.1;
        let minute = seg_dist(x, y, CENTER, CENTER, CENTER + 5.0, CENTER) <= 1.1;
        (ring || hour || minute).then_some(NEUTRAL)
    })
}

/// Three sliders — Settings.
pub fn settings() -> Icon {
    rasterize(|x, y| {
        let track = |ty: f32| seg_dist(x, y, 7.0, ty, 25.0, ty) <= 0.9;
        let knob = |kx: f32, ky: f32| dist(x, y, kx, ky) <= 2.6;
        let hit = knob(13.0, 9.0)
            || knob(20.0, 16.0)
            || knob(11.0, 23.0)
            || track(9.0)
            || track(16.0)
            || track(23.0);
        hit.then_some(NEUTRAL)
    })
}

/// Circled "i" — About.
pub fn about() -> Icon {
    rasterize(|x, y| {
        let ring = in_ring(x, y, CENTER, CENTER, 10.0, 2.2);
        let dot = dist(x, y, CENTER, CENTER - 5.5) <= 1.6;
        let stem = seg_dist(x, y, CENTER, CENTER - 1.5, CENTER, CENTER + 5.5) <= 1.3;
        (ring || dot || stem).then_some(NEUTRAL)
    })
}

/// Ascending bar chart — Status.
pub fn status() -> Icon {
    rasterize(|x, y| {
        let bars = rect_fill(x, y, 8.0, 18.0, 12.0, 24.0)
            || rect_fill(x, y, 14.0, 12.0, 18.0, 24.0)
            || rect_fill(x, y, 20.0, 8.0, 24.0, 24.0);
        bars.then_some(NEUTRAL)
    })
}

/// Speech bubble — Help / Docs.
pub fn docs() -> Icon {
    rasterize(|x, y| {
        let body = rect_stroke(x, y, 7.0, 8.0, 25.0, 20.0, 2.2);
        let tail = in_triangle(x, y, (10.0, 20.0), (15.0, 20.0), (10.0, 25.0));
        (body || tail).then_some(NEUTRAL)
    })
}

/// Download arrow into a tray — Check for Updates.
pub fn updater() -> Icon {
    rasterize(|x, y| {
        let shaft = seg_dist(x, y, CENTER, 7.0, CENTER, 18.0) <= 1.3;
        let head = in_triangle(x, y, (10.0, 16.0), (22.0, 16.0), (CENTER, 24.0));
        let base = seg_dist(x, y, 8.0, 26.0, 24.0, 26.0) <= 1.3;
        (shaft || head || base).then_some(NEUTRAL)
    })
}

/// Power symbol — Quit.
pub fn quit() -> Icon {
    rasterize(|x, y| {
        let gap = angle_from_top_deg(x, y, CENTER, CENTER).abs() <= 28.0;
        let ring = in_ring(x, y, CENTER, CENTER, 9.0, 2.4) && !gap;
        let stem = seg_dist(x, y, CENTER, 6.0, CENTER, 15.0) <= 1.2;
        (ring || stem).then_some(NEUTRAL)
    })
}
