use egui::{RichText, Ui};

use crate::theme;

pub fn draw(ui: &mut Ui, uptime_secs: u64) {
  theme::panel_frame(ui, theme::C_GRN, |ui| {
    ui.set_min_height(theme::PANEL_HEADER_H);
    let now = chrono::Local::now();

    // Large time display
    ui.label(
      RichText::new(now.format("%H:%M:%S").to_string())
        .size(40.0)
        .color(egui::Color32::WHITE),
    );

    // Day name and date on one row
    ui.horizontal(|ui| {
      ui.label(
        RichText::new(now.format("%A").to_string().to_uppercase())
          .size(16.0)
          .color(theme::C_GRN),
      );
      ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(
          RichText::new(now.format("%Y·%m·%d").to_string())
            .small()
            .color(theme::C_TEXT_MUTED),
        );
      });
    });

    // Uptime
    let h = uptime_secs / 3600;
    let m = (uptime_secs % 3600) / 60;
    let s = uptime_secs % 60;
    ui.label(
      RichText::new(format!("UP  {h:02}:{m:02}:{s:02}"))
        .small()
        .color(theme::C_GRN),
    );
  });
}
