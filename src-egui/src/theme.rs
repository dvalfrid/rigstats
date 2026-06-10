use egui::{pos2, Color32, CornerRadius, Frame, Margin, Rect, Sense, Stroke, Ui, Vec2};

// ── Panel accent colours (from panel-base.css) ────────────────────────────────
pub const C_ACCENT: Color32 = Color32::from_rgb(0x00, 0xc8, 0xff); // CPU, Header
pub const C_AMD: Color32 = Color32::from_rgb(0xff, 0x3a, 0x1f); // GPU
pub const C_RAM: Color32 = Color32::from_rgb(0xff, 0xb3, 0x00); // RAM
pub const C_GRN: Color32 = Color32::from_rgb(0x39, 0xff, 0x88); // Net, Clock, Battery
pub const C_PUR: Color32 = Color32::from_rgb(0xbf, 0x7f, 0xff); // Disk
pub const C_PROC: Color32 = Color32::from_rgb(0xff, 0x9f, 0x2f); // Process
pub const C_MB: Color32 = Color32::from_rgb(0x3a, 0x99, 0xb8); // Motherboard
pub const C_NET_DOWN: Color32 = Color32::from_rgb(0x3a, 0xa5, 0xff); // Network DOWN

// ── Text colours ─────────────────────────────────────────────────────────────
pub const C_TEXT: Color32 = Color32::from_rgb(0xb8, 0xcc, 0xe8);
pub const C_TEXT_MUTED: Color32 = Color32::from_rgb(0x5b, 0x8f, 0xa3);
pub const C_STAT_LABEL: Color32 = Color32::from_rgb(0x70, 0xb0, 0xc8);
pub const C_DIM: Color32 = Color32::from_rgb(0x2e, 0x3d, 0x5a);

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

/// Draw a thin (4 px high) progress bar with dark background.
pub fn thin_bar(ui: &mut Ui, frac: f32, width: f32, color: Color32) {
    const H: f32 = 4.0;
    let (resp, painter) = ui.allocate_painter(Vec2::new(width, H), Sense::hover());
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

/// Premultiply `color` (assumed fully opaque) by `opacity`.
fn premul(color: Color32, opacity: f32) -> Color32 {
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
pub fn panel_frame(ui: &mut Ui, accent: Color32, opacity: f32, add_contents: impl FnOnce(&mut Ui)) {
    let frame = Frame {
        inner_margin: Margin::symmetric(12, 8),
        outer_margin: Margin::ZERO,
        corner_radius: CornerRadius::ZERO,
        fill: premul(PANEL_FILL, opacity),
        stroke: Stroke::new(0.5, premul(PANEL_BORDER, opacity)),
        ..Default::default()
    };

    let fr = frame.show(ui, add_contents);
    let rect = fr.response.rect;
    let painter = ui.painter();

    draw_accent_line(painter, rect, accent, opacity);
    draw_corner_brackets(painter, rect, accent, opacity);
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
const DBTN_SEC: Color32 = Color32::from_gray(52);
const DBTN_SEC_HOV: Color32 = Color32::from_gray(65);
const DBTN_SEC_ACT: Color32 = Color32::from_gray(42);
const DBTN_SEC_BORDER: Color32 = Color32::from_gray(88);
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
        w.inactive.bg_stroke = Stroke::new(1.0, border);
        w.inactive.corner_radius = cr;
        w.hovered.bg_fill = fill_hov;
        w.hovered.weak_bg_fill = fill_hov;
        w.hovered.bg_stroke = Stroke::new(1.0, border);
        w.hovered.corner_radius = cr;
        w.active.bg_fill = fill_act;
        w.active.weak_bg_fill = fill_act;
        w.active.bg_stroke = Stroke::new(1.0, border);
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

/// Windows 11-style secondary button (gray with border, hover lightens fill).
pub fn dialog_btn_secondary(ui: &mut Ui, label: &str) -> egui::Response {
    with_btn_visuals(
        ui,
        DBTN_SEC,
        DBTN_SEC_HOV,
        DBTN_SEC_ACT,
        DBTN_SEC_BORDER,
        |ui| ui.add(egui::Button::new(label).min_size(DBTN_MIN_SIZE)),
    )
}

/// Disabled variant of the secondary button (grayed out, unclickable).
pub fn dialog_btn_secondary_disabled(ui: &mut Ui, label: &str) {
    with_btn_visuals(
        ui,
        DBTN_SEC,
        DBTN_SEC,
        DBTN_SEC,
        DBTN_SEC_BORDER,
        |ui| ui.add_enabled(false, egui::Button::new(label).min_size(DBTN_MIN_SIZE)),
    );
}

/// Small L-shaped brackets at TL and BR corners of the panel.
fn draw_corner_brackets(painter: &egui::Painter, rect: egui::Rect, accent: Color32, opacity: f32) {
    let bracket_a = (128.0 * opacity) as u8;
    let c = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), bracket_a);
    let stroke = Stroke::new(1.0, c);
    let size = 8.0;
    let inset = 5.0;

    let tl = rect.left_top() + egui::Vec2::new(inset, inset);
    painter.line_segment([tl, tl + egui::Vec2::new(size, 0.0)], stroke);
    painter.line_segment([tl, tl + egui::Vec2::new(0.0, size)], stroke);

    let br = rect.right_bottom() - egui::Vec2::new(inset, inset);
    painter.line_segment([br, br - egui::Vec2::new(size, 0.0)], stroke);
    painter.line_segment([br, br - egui::Vec2::new(0.0, size)], stroke);
}
