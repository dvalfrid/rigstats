use egui::{Color32, RichText, Ui};

use crate::spark::Sparkline;
use crate::tempcolor::color_accent;
use crate::PollStats;

const SPARK_H: f32 = 24.0;
const UP_COLOR: Color32 = Color32::from_rgb(100, 220, 130);

fn fmt_mbps(v: f64) -> String {
  if v >= 1000.0 {
    format!("{:.2} Gbps", v / 1000.0)
  } else if v >= 1.0 {
    format!("{v:.2} Mbps")
  } else {
    format!("{:.0} Kbps", v * 1000.0)
  }
}

pub fn draw(ui: &mut Ui, stats: &PollStats, up_spark: &mut Sparkline, dn_spark: &mut Sparkline) {
  up_spark.push(stats.net_up_mbps as f32);
  dn_spark.push(stats.net_down_mbps as f32);

  ui.horizontal(|ui| {
    ui.label(RichText::new("NETWORK").strong());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
      ui.label(RichText::new(&stats.net_iface).small().color(Color32::from_gray(140)));
    });
  });

  // Upload
  ui.horizontal(|ui| {
    ui.label(RichText::new("↑").color(UP_COLOR));
    ui.label(RichText::new(fmt_mbps(stats.net_up_mbps)).color(UP_COLOR));
  });
  up_spark.draw(ui, SPARK_H, UP_COLOR);

  // Download
  ui.horizontal(|ui| {
    ui.label(RichText::new("↓").color(color_accent()));
    ui.label(RichText::new(fmt_mbps(stats.net_down_mbps)).color(color_accent()));
  });
  dn_spark.draw(ui, SPARK_H, color_accent());

  // Ping
  if let Some(p) = stats.net_ping_ms {
    ui.label(
      RichText::new(format!("PING {p:.0} ms"))
        .small()
        .color(Color32::from_gray(160)),
    );
  }
}
