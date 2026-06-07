use egui::{Color32, RichText, Ui};

use crate::tempcolor::color_accent;

pub fn draw(ui: &mut Ui, uptime_secs: u64) {
  let now = chrono::Local::now();

  ui.horizontal(|ui| {
    ui.label(RichText::new("CLOCK").strong());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
      let h = uptime_secs / 3600;
      let m = (uptime_secs % 3600) / 60;
      let s = uptime_secs % 60;
      ui.label(
        RichText::new(format!("UP {h:02}:{m:02}:{s:02}"))
          .small()
          .color(Color32::from_gray(140)),
      );
    });
  });

  ui.label(RichText::new(now.format("%H:%M:%S").to_string()).heading().color(color_accent()));
  ui.label(
    RichText::new(now.format("%A, %d %B %Y").to_string())
      .small()
      .color(Color32::from_gray(160)),
  );
}
