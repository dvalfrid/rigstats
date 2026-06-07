use egui::{Color32, RichText, Ui};

use crate::tempcolor::color_accent;
use crate::PollStats;

pub fn draw(ui: &mut Ui, stats: &PollStats) {
  ui.horizontal(|ui| {
    ui.label(RichText::new("RIGSTATS").strong().color(color_accent()));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
      let (dot, color) = if stats.lhm_connected {
        ("●", Color32::from_rgb(57, 255, 136))
      } else {
        ("○", Color32::from_gray(120))
      };
      ui.label(RichText::new(format!("{dot} LHM")).small().color(color));
    });
  });

  if !stats.hostname.is_empty() {
    ui.label(RichText::new(&stats.hostname).small().color(Color32::from_gray(180)));
  }

  if !stats.cpu_model.is_empty() {
    let model = &stats.cpu_model;
    let model = if model.len() > 44 { &model[..44] } else { model };
    ui.label(RichText::new(model).small().color(Color32::from_gray(140)));
  }
}
