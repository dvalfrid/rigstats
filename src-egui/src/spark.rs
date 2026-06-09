use std::collections::VecDeque;

use egui::{Color32, Mesh, Pos2, Sense, Shape, Stroke, Ui, Vec2};

pub struct Sparkline {
    values: VecDeque<f32>,
}

impl Sparkline {
    /// Creates a sparkline pre-filled with zeros so the graph spans the full width from frame 1.
    pub fn new(capacity: usize) -> Self {
        let mut values = VecDeque::with_capacity(capacity);
        values.extend(std::iter::repeat(0.0_f32).take(capacity));
        Self { values }
    }

    pub fn push(&mut self, value: f32) {
        self.values.pop_front();
        self.values.push_back(value);
    }

    pub fn values(&self) -> &VecDeque<f32> {
        &self.values
    }

    /// Draw the sparkline spanning full available width at the given height.
    pub fn draw(&self, ui: &mut Ui, height: f32, color: Color32) {
        let w = {
            let w = ui.available_width();
            if w.is_finite() && w > 0.0 {
                w
            } else {
                ui.ctx().content_rect().width().max(1.0)
            }
        };
        self.draw_inner(ui, w, height, color);
    }

    fn draw_inner(&self, ui: &mut Ui, w: f32, height: f32, color: Color32) {
        let (response, painter) = ui.allocate_painter(Vec2::new(w, height), Sense::hover());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, Color32::from_gray(18));

        let n = self.values.len();
        if n < 2 {
            return;
        }

        // Dynamic scale: max of actual data, floor of 1.0 (matches JS: Math.max(...data, 1))
        let data_max = self.values.iter().cloned().fold(0.0f32, f32::max).max(1.0);

        // JS formula: y = height - 0.88 * (v/max) * height - 4  (4px bottom padding)
        let eff_h = (rect.height() - 4.0).max(0.0) * 0.88;
        let points: Vec<Pos2> = self
            .values
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let x = rect.left() + (i as f32 / (n - 1) as f32) * rect.width();
                let y = rect.bottom() - 4.0 - (v / data_max).clamp(0.0, 1.0) * eff_h;
                Pos2::new(x, y)
            })
            .collect();

        // Gradient fill area under the line (matches JS linear gradient top→bottom)
        let fill_y = rect.bottom();
        let mut mesh = Mesh::default();
        for pair in points.windows(2) {
            let (p0, p1) = (pair[0], pair[1]);
            let b0 = Pos2::new(p0.x, fill_y);
            let b1 = Pos2::new(p1.x, fill_y);
            let a0 = ((fill_y - p0.y) / rect.height() * 0x44 as f32) as u8;
            let a1 = ((fill_y - p1.y) / rect.height() * 0x44 as f32) as u8;
            let c0 = premul_color(color, a0);
            let c1 = premul_color(color, a1);
            let v = mesh.vertices.len() as u32;
            mesh.colored_vertex(p0, c0);
            mesh.colored_vertex(p1, c1);
            mesh.colored_vertex(b0, Color32::TRANSPARENT);
            mesh.colored_vertex(b1, Color32::TRANSPARENT);
            mesh.add_triangle(v, v + 2, v + 1);
            mesh.add_triangle(v + 1, v + 2, v + 3);
        }
        painter.add(Shape::mesh(mesh));

        // Line on top
        painter.add(Shape::line(points, Stroke::new(1.5, color)));
    }
}

pub fn premul_color(c: Color32, a: u8) -> Color32 {
    let af = a as u32;
    Color32::from_rgba_premultiplied(
        (c.r() as u32 * af / 255) as u8,
        (c.g() as u32 * af / 255) as u8,
        (c.b() as u32 * af / 255) as u8,
        a,
    )
}
