use egui::{pos2, Color32, CornerRadius, Frame, Margin, Rect, Sense, Stroke, Ui, Vec2};

/// Per-frame colour theme — derived from the accent colour via HSL.
/// Passed to every panel `draw()` so text label colours reflect the active theme.
#[derive(Clone, Copy, PartialEq)]
pub struct AppTheme {
    pub accent: Color32,
    pub stat_label: Color32,
    pub text_muted: Color32,
    pub mb_accent: Color32,
}

/// All valid theme keys, in display order.
pub const THEME_KEYS: &[&str] = &[
    "dark-cyan",
    "amber",
    "green",
    "purple",
    "slate",
    "red",
    "blue",
];

impl AppTheme {
    pub fn from_key(key: &str) -> Self {
        let accent = match key {
            "amber" => Color32::from_rgb(0xff, 0xb3, 0x00),
            "green" => Color32::from_rgb(0x39, 0xff, 0x88),
            "purple" => Color32::from_rgb(0xbf, 0x7f, 0xff),
            "slate" => Color32::from_rgb(0x90, 0xaa, 0xc4),
            "red" => Color32::from_rgb(0xff, 0x3a, 0x1f),
            "blue" => Color32::from_rgb(0x40, 0x8c, 0xff),
            _ => Color32::from_rgb(0x00, 0xc8, 0xff), // dark-cyan
        };
        Self::from_accent(accent)
    }

    fn from_accent(accent: Color32) -> Self {
        let h = rgb_to_hue(accent.r(), accent.g(), accent.b());
        Self {
            accent,
            stat_label: hsl_to_rgb(h, 32.0, 64.0),
            text_muted: hsl_to_rgb(h, 20.0, 55.0),
            mb_accent: hsl_to_rgb(h, 40.0, 52.0),
        }
    }
}

impl Default for AppTheme {
    fn default() -> Self {
        Self::from_key("dark-cyan")
    }
}

/// Extract hue (0–360) from an RGB color.
fn rgb_to_hue(r: u8, g: u8, b: u8) -> f32 {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d < 1e-6 {
        return 0.0;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h * 60.0
}

/// Convert HSL (h: 0–360, s: 0–100, l: 0–100) to Color32.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Color32 {
    let hn = h / 360.0;
    let sn = s / 100.0;
    let ln = l / 100.0;
    if sn < 1e-6 {
        let v = (ln * 255.0) as u8;
        return Color32::from_rgb(v, v, v);
    }
    let q = if ln < 0.5 {
        ln * (1.0 + sn)
    } else {
        ln + sn - ln * sn
    };
    let p = 2.0 * ln - q;
    let r = hue2rgb(p, q, hn + 1.0 / 3.0);
    let g = hue2rgb(p, q, hn);
    let b = hue2rgb(p, q, hn - 1.0 / 3.0);
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn hue2rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

// ── Panel accent colours (from panel-base.css) ────────────────────────────────
pub const C_ACCENT: Color32 = Color32::from_rgb(0x00, 0xc8, 0xff); // CPU, Header
pub const C_AMD: Color32 = Color32::from_rgb(0xff, 0x3a, 0x1f); // GPU
pub const C_RAM: Color32 = Color32::from_rgb(0xff, 0xb3, 0x00); // RAM
pub const C_GRN: Color32 = Color32::from_rgb(0x39, 0xff, 0x88); // Net, Clock, Battery
pub const C_PUR: Color32 = Color32::from_rgb(0xbf, 0x7f, 0xff); // Disk
pub const C_PROC: Color32 = Color32::from_rgb(0xff, 0x9f, 0x2f); // Process
pub const C_NET_DOWN: Color32 = Color32::from_rgb(0x3a, 0xa5, 0xff); // Network DOWN

// ── Text colours ─────────────────────────────────────────────────────────────
pub const C_TEXT: Color32 = Color32::from_rgb(0xb8, 0xcc, 0xe8);
pub const C_DIM: Color32 = Color32::from_rgb(0x2e, 0x3d, 0x5a);
/// Colour for sensors/fields not supported by the current hardware.
/// Use via `avail_color` rather than directly.
pub const C_UNAVAILABLE: Color32 = Color32::from_gray(80);

/// Returns `active` when `val` is `Some`, `C_UNAVAILABLE` when `None`.
///
/// Call this for every stat label and value that maps to an `Option` field so
/// that unsupported sensors dim consistently across all panels.
pub fn avail_color<T>(val: &Option<T>, active: Color32) -> Color32 {
    if val.is_some() {
        active
    } else {
        C_UNAVAILABLE
    }
}

// ── Panel card colours ────────────────────────────────────────────────────────
pub const PANEL_FILL: Color32 = Color32::from_rgb(11, 13, 18);
pub const PANEL_BORDER: Color32 = Color32::from_rgb(22, 28, 42);

/// Panel section title size (e.g. "CPU LOAD", "GPU LOAD").
/// Must stay above Body (14 px) so titles read as headings.
pub const FONT_PANEL_TITLE: f32 = 17.0;

/// Colour for panel section titles — near-white for clear visual weight.
pub const C_PANEL_TITLE: Color32 = Color32::from_rgb(0xe4, 0xee, 0xfa);

/// Height of the drag handle strip at the top of the main window (logical px).
pub const DRAG_HANDLE_H: f32 = 14.0;

/// Minimum content height for header/clock panels (equalises their visual size).
pub const PANEL_HEADER_H: f32 = 105.0;
/// Minimum content height for all data panels (CPU, GPU, RAM, Net, Disk, etc.).
pub const PANEL_DATA_H: f32 = 200.0;

/// Shared ring gauge diameter used by CPU and GPU panels (keeps them visually consistent).
pub const RING_SIZE: f32 = 64.0;

/// Draw a filled upward-pointing triangle (↑ substitute — Ubuntu font subset lacks U+2191).
pub fn arrow_up(ui: &mut Ui, size: f32, color: Color32) {
    let (resp, painter) = ui.allocate_painter(Vec2::new(size * 0.7, size), Sense::hover());
    let r = resp.rect;
    let cx = r.center().x;
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(cx, r.top()),
            pos2(r.right(), r.bottom()),
            pos2(r.left(), r.bottom()),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Draw a filled downward-pointing triangle (↓ substitute — Ubuntu font subset lacks U+2193).
pub fn arrow_down(ui: &mut Ui, size: f32, color: Color32) {
    let (resp, painter) = ui.allocate_painter(Vec2::new(size * 0.7, size), Sense::hover());
    let r = resp.rect;
    let cx = r.center().x;
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(r.left(), r.top()),
            pos2(r.right(), r.top()),
            pos2(cx, r.bottom()),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Draw a thin progress bar with dark background and manual content scale.
pub fn thin_bar_scaled(ui: &mut Ui, frac: f32, width: f32, color: Color32, sc: f32) {
    let h = 4.0 * sc;
    let (resp, painter) = ui.allocate_painter(Vec2::new(width, h), Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 0.0, Color32::from_gray(42));
    let fw = frac.clamp(0.0, 1.0) * r.width();
    if fw > 0.5 {
        painter.rect_filled(
            Rect::from_min_size(r.min, Vec2::new(fw, r.height())),
            0.0,
            color,
        );
    }
}

/// Right-aligned label in a fixed-width slot.
/// Eliminates the repetitive `allocate_ui_with_layout` pattern for metric labels.
pub fn fixed_label_r(ui: &mut Ui, text: impl Into<egui::WidgetText>, width: f32, height: f32) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, height),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.label(text);
        },
    );
}

/// Compute how many pixels are available for a bar after subtracting fixed-width
/// columns and the actual (non-scaled) egui item_spacing between them.
///
/// `fixed_w_sum` = sum of all non-bar column widths already multiplied by `sc`.
/// `gaps`        = number of item_spacing gaps between columns.
pub fn bar_avail(ui: &Ui, fixed_w_sum: f32, gaps: u32) -> f32 {
    let sp = ui.spacing().item_spacing.x;
    let avail = {
        let w = ui.available_width();
        if w.is_finite() && w > 0.0 {
            w
        } else {
            ui.ctx().content_rect().width().max(1.0)
        }
    };
    (avail - fixed_w_sum - sp * gaps as f32).max(4.0)
}

/// Premultiply `color` (assumed fully opaque) by `opacity`.
pub fn premul(color: Color32, opacity: f32) -> Color32 {
    let a = opacity.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied(
        (color.r() as f32 * a) as u8,
        (color.g() as f32 * a) as u8,
        (color.b() as f32 * a) as u8,
        (255.0 * a) as u8,
    )
}

/// Wrap `add_contents` in a styled panel card.
/// `opacity` (0.0–1.0) is baked into all fills, borders, and decorations so
/// the panel blends correctly over the transparent window background.
/// Returns the outer bounding rect of the card (useful for overlay painting).
pub fn panel_frame(
    ui: &mut Ui,
    opacity: f32,
    th: &AppTheme,
    sc: f32,
    add_contents: impl FnOnce(&mut Ui),
) -> egui::Rect {
    let frame = Frame {
        inner_margin: Margin::symmetric((12.0 * sc).round() as i8, (8.0 * sc).round() as i8),
        outer_margin: Margin::ZERO,
        corner_radius: CornerRadius::ZERO,
        fill: premul(PANEL_FILL, opacity),
        stroke: Stroke::new(0.5_f32, premul(PANEL_BORDER, opacity)),
        ..Default::default()
    };

    let fr = frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        add_contents(ui);
    });
    let rect = fr.response.rect;
    let painter = ui.painter();

    draw_accent_line(painter, rect, th.accent, opacity);
    draw_corner_brackets(painter, rect, th.accent, opacity);
    rect
}

/// Horizontal gradient line at the top edge: transparent → accent → transparent.
fn draw_accent_line(painter: &egui::Painter, rect: egui::Rect, accent: Color32, opacity: f32) {
    let peak_a = (180.0 * opacity) as u8;
    let peak = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), peak_a);
    let cx = rect.center().x;
    let y0 = rect.top();
    let y1 = rect.top() + 1.5;

    let mut mesh = egui::Mesh::default();

    // Left half: transparent → peak
    let v = mesh.vertices.len() as u32;
    mesh.colored_vertex(egui::Pos2::new(rect.left(), y0), Color32::TRANSPARENT);
    mesh.colored_vertex(egui::Pos2::new(rect.left(), y1), Color32::TRANSPARENT);
    mesh.colored_vertex(egui::Pos2::new(cx, y0), peak);
    mesh.colored_vertex(egui::Pos2::new(cx, y1), peak);
    mesh.add_triangle(v, v + 1, v + 2);
    mesh.add_triangle(v + 1, v + 3, v + 2);

    // Right half: peak → transparent
    let v = mesh.vertices.len() as u32;
    mesh.colored_vertex(egui::Pos2::new(cx, y0), peak);
    mesh.colored_vertex(egui::Pos2::new(cx, y1), peak);
    mesh.colored_vertex(egui::Pos2::new(rect.right(), y0), Color32::TRANSPARENT);
    mesh.colored_vertex(egui::Pos2::new(rect.right(), y1), Color32::TRANSPARENT);
    mesh.add_triangle(v, v + 1, v + 2);
    mesh.add_triangle(v + 1, v + 3, v + 2);

    painter.add(egui::Shape::mesh(mesh));
}

// ── Dialog colour theme ───────────────────────────────────────────────────────

/// Colour palette for dialog windows (Settings, About, Status, Updater).
/// Two factory functions cover the two cases: `dark()` for the existing
/// dark palette and `light()` for OS light-mode.  Pass `&DialogColors` to
/// every dialog `show()` function instead of using module-level `const`s.
#[derive(Clone, Copy)]
pub struct DialogColors {
    pub is_dark: bool,
    // Surfaces
    pub bg: Color32,          // dialog background
    pub card: Color32,        // content card fill
    pub card_border: Color32, // content card border
    pub inner: Color32,       // inner-row bg (settings toggle rows)
    pub inset: Color32,       // deepest inset bg (log/scroll areas)
    // Text
    pub text: Color32,      // primary text
    pub muted: Color32,     // secondary/hint text
    pub label: Color32,     // small section label
    pub title: Color32,     // dialog heading
    pub link: Color32,      // hyperlink colour
    pub item: Color32,      // changelog list item text
    pub item_hash: Color32, // commit hash text
    // Interactive
    pub toggle_on: Color32,
    pub toggle_off: Color32,
    pub tab_active: Color32,
    pub tab_active_text: Color32,
    pub btn_sec_fill: Color32,
    pub btn_sec_hover: Color32,
    pub btn_sec_border: Color32,
    pub btn_sec_text: Color32,
}

impl DialogColors {
    pub fn dark() -> Self {
        Self {
            is_dark: true,
            bg: Color32::from_gray(38),
            card: Color32::from_gray(27),
            card_border: Color32::from_gray(55),
            inner: Color32::from_gray(33),
            inset: Color32::from_gray(22),
            text: Color32::from_rgb(155, 180, 210),
            muted: Color32::from_gray(115),
            label: Color32::from_gray(140),
            title: Color32::from_rgb(210, 220, 235),
            link: Color32::from_rgb(80, 160, 220),
            item: Color32::from_gray(162),
            item_hash: Color32::from_rgb(86, 142, 188),
            toggle_on: Color32::from_rgb(0, 188, 212),
            toggle_off: Color32::from_gray(55),
            tab_active: Color32::from_rgb(0, 188, 212),
            tab_active_text: Color32::from_gray(15),
            btn_sec_fill: Color32::from_gray(52),
            btn_sec_hover: Color32::from_gray(65),
            btn_sec_border: Color32::from_gray(88),
            btn_sec_text: C_TEXT,
        }
    }

    pub fn light() -> Self {
        Self {
            is_dark: false,
            bg: Color32::from_gray(245),
            card: Color32::from_gray(232),
            card_border: Color32::from_gray(200),
            inner: Color32::from_gray(220),
            inset: Color32::from_gray(215),
            text: Color32::from_rgb(30, 45, 70),
            muted: Color32::from_gray(90),
            label: Color32::from_gray(65),
            title: Color32::from_rgb(15, 30, 55),
            link: Color32::from_rgb(0, 90, 180),
            item: Color32::from_gray(55),
            item_hash: Color32::from_rgb(0, 90, 160),
            toggle_on: Color32::from_rgb(0, 140, 180),
            toggle_off: Color32::from_gray(185),
            tab_active: Color32::from_rgb(0, 140, 180),
            tab_active_text: Color32::WHITE,
            btn_sec_fill: Color32::from_gray(215),
            btn_sec_hover: Color32::from_gray(200),
            btn_sec_border: Color32::from_gray(160),
            btn_sec_text: Color32::from_rgb(30, 45, 70),
        }
    }

    /// Apply egui native widget visuals matching this palette to `ctx`.
    /// No-op if `ctx` is already in the correct dark/light mode — avoids the
    /// per-frame repaint loop that causes jerky window dragging.
    pub fn apply_to_ctx(&self, ctx: &egui::Context) {
        if ctx.global_style().visuals.dark_mode == self.is_dark {
            return;
        }
        let mut vis = if self.is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        // Keep popups/combo-box drops solid and matching the dialog surface.
        vis.window_fill = self.bg;
        ctx.set_visuals(vis);
    }
}

/// Apply the dashboard's larger text sizes (tuned for a portrait monitor) to
/// `ctx`. Shared by the main `rigstats` app and the `rigstats-wallpaper` host so
/// both render panels identically.
pub fn apply_dashboard_fonts(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};
    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(18.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
    ]
    .into();
    ctx.set_global_style(style);
}

// ── Dialog button components ──────────────────────────────────────────────────
//
// All secondary windows (Settings, About, Status, Updater) must use these helpers
// instead of raw `ui.button()`. They apply Windows 11-style hover/active state by
// temporarily overriding `ui.visuals_mut().widgets.*` inside a `ui.scope()`, which
// keeps the override local and does not leak to surrounding UI.
//
// Usage:
//   if theme::dialog_btn_primary(ui, "OK").clicked() { ... }
//   if theme::dialog_btn_secondary(ui, "Cancel").clicked() { ... }
//   theme::dialog_btn_secondary_disabled(ui, "Update Now"); // grayed out, unclickable
//
// Button layout convention: use `ui.with_layout(right_to_left, ...)` so the primary
// action sits on the far right and secondary actions are to its left.  Add
// `ui.separator()` immediately before the button row.

const DBTN_PRIMARY: Color32 = Color32::from_rgb(0, 120, 212);
const DBTN_PRIMARY_HOV: Color32 = Color32::from_rgb(26, 134, 219);
const DBTN_PRIMARY_ACT: Color32 = Color32::from_rgb(0, 108, 190);
const DBTN_PRIMARY_BORDER: Color32 = Color32::from_rgb(0, 90, 170);
const DBTN_SEC_ACT_DARKEN: u8 = 10; // how much to darken fill for active state
const DBTN_MIN_SIZE: Vec2 = Vec2::new(90.0, 26.0);
const DBTN_RADIUS: u8 = 4;

fn with_btn_visuals<R>(
    ui: &mut Ui,
    fill: Color32,
    fill_hov: Color32,
    fill_act: Color32,
    border: Color32,
    f: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.scope(|ui| {
        let cr = CornerRadius::same(DBTN_RADIUS);
        let w = &mut ui.visuals_mut().widgets;
        w.inactive.bg_fill = fill;
        w.inactive.weak_bg_fill = fill;
        w.inactive.bg_stroke = Stroke::new(1.0_f32, border);
        w.inactive.corner_radius = cr;
        w.hovered.bg_fill = fill_hov;
        w.hovered.weak_bg_fill = fill_hov;
        w.hovered.bg_stroke = Stroke::new(1.0_f32, border);
        w.hovered.corner_radius = cr;
        w.active.bg_fill = fill_act;
        w.active.weak_bg_fill = fill_act;
        w.active.bg_stroke = Stroke::new(1.0_f32, border);
        w.active.corner_radius = cr;
        f(ui)
    })
    .inner
}

/// Windows 11-style primary button (blue `#0078D4`, white text, hover `#1A86DB`).
pub fn dialog_btn_primary(ui: &mut Ui, label: &str) -> egui::Response {
    with_btn_visuals(
        ui,
        DBTN_PRIMARY,
        DBTN_PRIMARY_HOV,
        DBTN_PRIMARY_ACT,
        DBTN_PRIMARY_BORDER,
        |ui| {
            ui.visuals_mut().override_text_color = Some(Color32::WHITE);
            ui.add(egui::Button::new(label).min_size(DBTN_MIN_SIZE))
        },
    )
}

/// Windows 11-style secondary button — colours adapt to `dc` (dark or light mode).
pub fn dialog_btn_secondary(ui: &mut Ui, label: &str, dc: &DialogColors) -> egui::Response {
    let fill = dc.btn_sec_fill;
    let hover = dc.btn_sec_hover;
    // Active: darken by clamped subtraction on each channel
    let act = Color32::from_rgb(
        fill.r().saturating_sub(DBTN_SEC_ACT_DARKEN),
        fill.g().saturating_sub(DBTN_SEC_ACT_DARKEN),
        fill.b().saturating_sub(DBTN_SEC_ACT_DARKEN),
    );
    let border = dc.btn_sec_border;
    let text_col = dc.btn_sec_text;
    with_btn_visuals(ui, fill, hover, act, border, |ui| {
        ui.visuals_mut().override_text_color = Some(text_col);
        ui.add(egui::Button::new(label).min_size(DBTN_MIN_SIZE))
    })
}

/// Disabled variant of the secondary button (grayed out, unclickable).
pub fn dialog_btn_secondary_disabled(ui: &mut Ui, label: &str, dc: &DialogColors) {
    let fill = dc.btn_sec_fill;
    let border = dc.btn_sec_border;
    let text_col = dc.btn_sec_text;
    with_btn_visuals(ui, fill, fill, fill, border, |ui| {
        ui.visuals_mut().override_text_color = Some(text_col);
        ui.add_enabled(false, egui::Button::new(label).min_size(DBTN_MIN_SIZE))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: u8, b: u8, tol: u8) -> bool {
        a.abs_diff(b) <= tol
    }

    fn color_approx_eq(a: Color32, b: Color32, tol: u8) -> bool {
        approx_eq(a.r(), b.r(), tol) && approx_eq(a.g(), b.g(), tol) && approx_eq(a.b(), b.b(), tol)
    }

    #[test]
    fn hue_of_red() {
        assert!((rgb_to_hue(255, 0, 0) - 0.0).abs() < 1.0);
    }

    #[test]
    fn hue_of_green() {
        assert!((rgb_to_hue(0, 255, 0) - 120.0).abs() < 1.0);
    }

    #[test]
    fn hue_of_blue() {
        assert!((rgb_to_hue(0, 0, 255) - 240.0).abs() < 1.0);
    }

    #[test]
    fn hue_of_achromatic_is_zero() {
        assert_eq!(rgb_to_hue(128, 128, 128), 0.0);
    }

    #[test]
    fn hsl_red() {
        let c = hsl_to_rgb(0.0, 100.0, 50.0);
        assert!(color_approx_eq(c, Color32::from_rgb(255, 0, 0), 2));
    }

    #[test]
    fn hsl_achromatic_is_gray() {
        let c = hsl_to_rgb(0.0, 0.0, 50.0);
        assert_eq!(c.r(), c.g());
        assert_eq!(c.g(), c.b());
    }

    #[test]
    fn round_trip_dark_cyan() {
        let accent = Color32::from_rgb(0x00, 0xc8, 0xff);
        let h = rgb_to_hue(accent.r(), accent.g(), accent.b());
        // Hue should be in the cyan range (175–200°)
        assert!(h > 170.0 && h < 210.0, "hue={h}");
        // Round-trip: HSL with S=100, L=50 should recover a similar blue-cyan
        let back = hsl_to_rgb(h, 100.0, 50.0);
        assert!(color_approx_eq(back, accent, 20));
    }

    #[test]
    fn theme_keys_contains_all_presets() {
        assert_eq!(THEME_KEYS.len(), 7);
        assert!(THEME_KEYS.contains(&"dark-cyan"));
        assert!(THEME_KEYS.contains(&"amber"));
        assert!(THEME_KEYS.contains(&"purple"));
    }

    #[test]
    fn from_key_unknown_falls_back_to_dark_cyan() {
        let a = AppTheme::from_key("nonexistent");
        let b = AppTheme::from_key("dark-cyan");
        assert_eq!(a.accent, b.accent);
    }

    #[test]
    fn derived_colors_differ_from_accent() {
        let th = AppTheme::from_key("amber");
        // stat_label and text_muted must be desaturated vs accent
        assert_ne!(th.stat_label, th.accent);
        assert_ne!(th.text_muted, th.accent);
    }
}

/// Small L-shaped brackets at TL and BR corners of the panel.
fn draw_corner_brackets(painter: &egui::Painter, rect: egui::Rect, accent: Color32, opacity: f32) {
    let bracket_a = (128.0 * opacity) as u8;
    let c = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), bracket_a);
    let stroke = Stroke::new(1.0_f32, c);
    let size = 8.0;
    let inset = 5.0;

    let tl = rect.left_top() + egui::Vec2::new(inset, inset);
    painter.line_segment([tl, tl + egui::Vec2::new(size, 0.0)], stroke);
    painter.line_segment([tl, tl + egui::Vec2::new(0.0, size)], stroke);

    let br = rect.right_bottom() - egui::Vec2::new(inset, inset);
    painter.line_segment([br, br - egui::Vec2::new(size, 0.0)], stroke);
    painter.line_segment([br, br - egui::Vec2::new(0.0, size)], stroke);
}
