use egui::{Color32, CornerRadius, Frame, Margin, Rect, Sense, Stroke, Ui, Vec2};

// ── Panel accent colours (from panel-base.css) ────────────────────────────────
pub const C_ACCENT: Color32 = Color32::from_rgb(0x00, 0xc8, 0xff); // CPU, Header
pub const C_AMD: Color32 = Color32::from_rgb(0xff, 0x3a, 0x1f);    // GPU
pub const C_RAM: Color32 = Color32::from_rgb(0xff, 0xb3, 0x00);    // RAM
pub const C_GRN: Color32 = Color32::from_rgb(0x39, 0xff, 0x88);    // Net, Clock, Battery
pub const C_PUR: Color32 = Color32::from_rgb(0xbf, 0x7f, 0xff);    // Disk
pub const C_PROC: Color32 = Color32::from_rgb(0xff, 0x9f, 0x2f);   // Process
pub const C_MB: Color32 = Color32::from_rgb(0x3a, 0x99, 0xb8);     // Motherboard

// ── Text colours ─────────────────────────────────────────────────────────────
pub const C_TEXT: Color32 = Color32::from_rgb(0xb8, 0xcc, 0xe8);
pub const C_TEXT_MUTED: Color32 = Color32::from_rgb(0x5b, 0x8f, 0xa3);
pub const C_STAT_LABEL: Color32 = Color32::from_rgb(0x70, 0xb0, 0xc8);
pub const C_DIM: Color32 = Color32::from_rgb(0x2e, 0x3d, 0x5a);

// ── Panel card colours ────────────────────────────────────────────────────────
pub const PANEL_FILL: Color32 = Color32::from_rgb(11, 13, 18);
pub const PANEL_BORDER: Color32 = Color32::from_rgb(22, 28, 42);

/// Height of the drag handle strip at the top of the main window (logical px).
pub const DRAG_HANDLE_H: f32 = 14.0;

/// Draw a thin (4 px high) progress bar with dark background.
pub fn thin_bar(ui: &mut Ui, frac: f32, width: f32, color: Color32) {
  const H: f32 = 4.0;
  let (resp, painter) = ui.allocate_painter(Vec2::new(width, H), Sense::hover());
  let r = resp.rect;
  painter.rect_filled(r, 0.0, Color32::from_gray(28));
  let fw = frac.clamp(0.0, 1.0) * r.width();
  if fw > 0.5 {
    painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(fw, r.height())), 0.0, color);
  }
}

/// Wrap `add_contents` in a styled panel card:
/// dark fill, thin border, gradient accent line at top, TL/BR corner brackets.
pub fn panel_frame(ui: &mut Ui, accent: Color32, add_contents: impl FnOnce(&mut Ui)) {
  let frame = Frame {
    inner_margin: Margin::symmetric(12, 8),
    outer_margin: Margin::ZERO,
    corner_radius: CornerRadius::ZERO,
    fill: PANEL_FILL,
    stroke: Stroke::new(0.5, PANEL_BORDER),
    ..Default::default()
  };

  let fr = frame.show(ui, add_contents);
  let rect = fr.response.rect;
  let painter = ui.painter();

  draw_accent_line(painter, rect, accent);
  draw_corner_brackets(painter, rect, accent);
}

/// Horizontal gradient line at the top edge: transparent → accent → transparent.
fn draw_accent_line(painter: &egui::Painter, rect: egui::Rect, accent: Color32) {
  let peak = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 180);
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

/// Small L-shaped brackets at TL and BR corners of the panel.
fn draw_corner_brackets(painter: &egui::Painter, rect: egui::Rect, accent: Color32) {
  let c = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 128);
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
