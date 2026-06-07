use egui::{Color32, RichText, Ui};

use crate::tempcolor::temp_color;
use crate::PollStats;

const LABEL_COLOR: Color32 = Color32::from_gray(130);

/// Maps long LHM sensor labels to short display names that fit in a narrow column.
/// Mirrors the JS `shortLabel()` in motherboard.js.
fn short_label(label: &str) -> String {
  // "Temperature #3" → "T3"
  if let Some(rest) = label.strip_prefix("Temperature #") {
    return format!("T{rest}");
  }
  // "Fan #2" → "F2"
  if let Some(rest) = label.strip_prefix("Fan #") {
    return format!("F{rest}");
  }
  // Truncate everything else to 8 chars.
  label.chars().take(8).collect()
}

pub fn draw(ui: &mut Ui, stats: &PollStats) {
  ui.horizontal(|ui| {
    ui.label(RichText::new("MOTHERBOARD").strong());
  });

  if let Some(ref board) = stats.mb_board {
    ui.label(RichText::new(board.as_str()).small().color(Color32::from_gray(160)));
  }
  if let Some(ref chip) = stats.mb_chip {
    ui.label(RichText::new(chip.as_str()).small().color(Color32::from_gray(120)));
  }

  // Fans
  if !stats.mb_fans.is_empty() {
    ui.add_space(4.0);
    egui::Grid::new("mb_fans").num_columns(2).min_col_width(60.0).show(ui, |ui| {
      ui.label(RichText::new("FAN").small().color(LABEL_COLOR));
      ui.label(RichText::new("RPM").small().color(LABEL_COLOR));
      ui.end_row();
      for (label, rpm) in &stats.mb_fans {
        ui.label(RichText::new(short_label(label)).small());
        ui.label(RichText::new(format!("{rpm:.0}")).small().color(egui::Color32::from_rgb(100, 200, 255)));
        ui.end_row();
      }
    });
  }

  // Temps
  if !stats.mb_temps.is_empty() {
    ui.add_space(4.0);
    egui::Grid::new("mb_temps").num_columns(2).min_col_width(60.0).show(ui, |ui| {
      ui.label(RichText::new("SENSOR").small().color(LABEL_COLOR));
      ui.label(RichText::new("TEMP").small().color(LABEL_COLOR));
      ui.end_row();
      for (label, t) in &stats.mb_temps {
        ui.label(RichText::new(short_label(label)).small());
        ui.label(RichText::new(format!("{t:.0}°C")).small().color(temp_color(Some(*t), 70, 90)));
        ui.end_row();
      }
    });
  }

  // Voltages
  if !stats.mb_voltages.is_empty() {
    ui.add_space(4.0);
    egui::Grid::new("mb_volts").num_columns(2).min_col_width(60.0).show(ui, |ui| {
      ui.label(RichText::new("RAIL").small().color(LABEL_COLOR));
      ui.label(RichText::new("VOLT").small().color(LABEL_COLOR));
      ui.end_row();
      for (label, v) in &stats.mb_voltages {
        ui.label(RichText::new(short_label(label)).small());
        ui.label(RichText::new(format!("{v:.3} V")).small().color(Color32::from_rgb(200, 200, 100)));
        ui.end_row();
      }
    });
  }
}
