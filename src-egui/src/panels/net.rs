use std::collections::VecDeque;

use egui::{Color32, RichText, Sense, Stroke, Ui, Vec2};

use crate::spark::Sparkline;
use crate::theme;
use crate::PollStats;

const SPARK_H: f32 = 40.0;

fn fmt_mbps(v: f64) -> String {
  if v >= 1000.0 {
    format!("{:.2} Gbps", v / 1000.0)
  } else if v >= 1.0 {
    format!("{v:.1} Mbps")
  } else {
    format!("{:.0} Kbps", v * 1000.0)
  }
}

/// Draw two sparklines on the same canvas (both with gradient fill).
fn draw_dual(
  ui: &mut Ui,
  height: f32,
  up: &Sparkline,
  dn: &Sparkline,
  up_color: Color32,
  dn_color: Color32,
) {
  let w = {
    let w = ui.available_width();
    if w.is_finite() && w > 0.0 { w } else { ui.ctx().content_rect().width().max(1.0) }
  };
  let (resp, painter) = ui.allocate_painter(Vec2::new(w, height), Sense::hover());
  let rect = resp.rect;
  painter.rect_filled(rect, 0.0, Color32::from_gray(18));

  // Shared scale so both series are comparable.
  let up_max = up.values().iter().cloned().fold(0.0f32, f32::max);
  let dn_max = dn.values().iter().cloned().fold(0.0f32, f32::max);
  let shared_max = up_max.max(dn_max).max(1.0);

  let eff_h = (rect.height() - 4.0).max(0.0) * 0.88;

  let make_points = |vals: &VecDeque<f32>| -> Vec<egui::Pos2> {
    let n = vals.len();
    if n < 2 {
      return Vec::new();
    }
    vals
      .iter()
      .enumerate()
      .map(|(i, &v)| {
        let x = rect.left() + (i as f32 / (n - 1) as f32) * rect.width();
        let y = rect.bottom() - 4.0 - (v / shared_max).clamp(0.0, 1.0) * eff_h;
        egui::Pos2::new(x, y)
      })
      .collect()
  };

  let fill_series = |pts: &[egui::Pos2], color: Color32| {
    if pts.len() < 2 {
      return None;
    }
    let fill_y = rect.bottom();
    let mut mesh = egui::Mesh::default();
    for pair in pts.windows(2) {
      let (p0, p1) = (pair[0], pair[1]);
      let a0 = (((fill_y - p0.y) / rect.height()) * 0x55 as f32) as u8;
      let a1 = (((fill_y - p1.y) / rect.height()) * 0x55 as f32) as u8;
      let c0 = crate::spark::premul_color(color, a0);
      let c1 = crate::spark::premul_color(color, a1);
      let v = mesh.vertices.len() as u32;
      mesh.colored_vertex(p0, c0);
      mesh.colored_vertex(p1, c1);
      mesh.colored_vertex(egui::Pos2::new(p0.x, fill_y), Color32::TRANSPARENT);
      mesh.colored_vertex(egui::Pos2::new(p1.x, fill_y), Color32::TRANSPARENT);
      mesh.add_triangle(v, v + 2, v + 1);
      mesh.add_triangle(v + 1, v + 2, v + 3);
    }
    Some((mesh, egui::Shape::line(pts.to_vec(), Stroke::new(1.5, color))))
  };

  let dn_pts = make_points(dn.values());
  let up_pts = make_points(up.values());

  // Draw DOWN first (behind), then UP on top.
  if let Some((mesh, line)) = fill_series(&dn_pts, dn_color) {
    painter.add(egui::Shape::mesh(mesh));
    painter.add(line);
  }
  if let Some((mesh, line)) = fill_series(&up_pts, up_color) {
    painter.add(egui::Shape::mesh(mesh));
    painter.add(line);
  }
}

pub fn draw(ui: &mut Ui, stats: &PollStats, up_spark: &Sparkline, dn_spark: &Sparkline) {
  theme::panel_frame(ui, theme::C_GRN, |ui| {
    ui.set_min_height(theme::PANEL_DATA_H);

    ui.label(RichText::new("NETWORK").strong().color(theme::C_PANEL_TITLE).size(theme::FONT_PANEL_TITLE));
    ui.add_space(4.0);

    // UP (green ↑) and DOWN (blue ↓) side by side in two columns.
    ui.columns(2, |cols| {
      // UP column
      cols[0].horizontal(|ui| {
        theme::arrow_up(ui, 12.0, theme::C_GRN);
        ui.add_space(2.0);
        ui.label(RichText::new("UP").small().color(theme::C_GRN));
      });
      cols[0].label(
        RichText::new(fmt_mbps(stats.net_up_mbps)).size(20.0).color(egui::Color32::WHITE),
      );

      // DOWN column
      cols[1].horizontal(|ui| {
        theme::arrow_down(ui, 12.0, theme::C_NET_DOWN);
        ui.add_space(2.0);
        ui.label(RichText::new("DOWN").small().color(theme::C_NET_DOWN));
      });
      cols[1].label(
        RichText::new(fmt_mbps(stats.net_down_mbps)).size(20.0).color(egui::Color32::WHITE),
      );
    });

    ui.add_space(4.0);

    // PING + IFACE — above the sparkline.
    ui.horizontal(|ui| {
      if let Some(p) = stats.net_ping_ms {
        ui.label(RichText::new("PING").small().color(theme::C_STAT_LABEL));
        ui.label(RichText::new(format!("{p:.0} ms")).small().color(theme::C_TEXT));
        ui.add_space(12.0);
      }
      if !stats.net_iface.is_empty() {
        ui.label(RichText::new("IFACE").small().color(theme::C_STAT_LABEL));
        ui.label(RichText::new(&stats.net_iface).small().color(theme::C_TEXT));
      }
    });

    // Push sparkline to bottom using cursor tracking.
    let cursor_y = ui.cursor().top();
    let filler = (theme::PANEL_DATA_H - (cursor_y - ui.min_rect().top()) - SPARK_H).max(2.0);
    ui.add_space(filler);

    draw_dual(ui, SPARK_H, up_spark, dn_spark, theme::C_GRN, theme::C_NET_DOWN);
  });
}
