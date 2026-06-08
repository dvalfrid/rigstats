use egui::{Color32, RichText, Ui};

use crate::tempcolor::temp_color;
use crate::theme;
use crate::PollStats;

fn short_label(label: &str) -> String {
  if let Some(rest) = label.strip_prefix("Temperature #") {
    return format!("T{rest}");
  }
  if let Some(rest) = label.strip_prefix("Fan #") {
    return format!("F{rest}");
  }
  label.chars().take(8).collect()
}

pub fn draw(ui: &mut Ui, stats: &PollStats) {
  theme::panel_frame(ui, theme::C_MB, |ui| {
    ui.horizontal(|ui| {
      ui.label(RichText::new("MOTHERBOARD").strong().color(theme::C_TEXT).size(13.0));
    });

    if let Some(ref board) = stats.mb_board {
      ui.label(RichText::new(board.as_str()).small().color(theme::C_TEXT_MUTED));
    }
    if let Some(ref chip) = stats.mb_chip {
      ui.label(RichText::new(chip.as_str()).small().color(theme::C_DIM));
    }

    if !stats.mb_fans.is_empty() {
      ui.add_space(4.0);
      egui::Grid::new("mb_fans").num_columns(2).min_col_width(60.0).show(ui, |ui| {
        ui.label(RichText::new("FAN").small().color(theme::C_STAT_LABEL));
        ui.label(RichText::new("RPM").small().color(theme::C_STAT_LABEL));
        ui.end_row();
        for (label, rpm) in &stats.mb_fans {
          ui.label(RichText::new(short_label(label)).small().color(theme::C_TEXT_MUTED));
          ui.label(RichText::new(format!("{rpm:.0}")).small().color(theme::C_MB));
          ui.end_row();
        }
      });
    }

    if !stats.mb_temps.is_empty() {
      ui.add_space(4.0);
      egui::Grid::new("mb_temps").num_columns(2).min_col_width(60.0).show(ui, |ui| {
        ui.label(RichText::new("SENSOR").small().color(theme::C_STAT_LABEL));
        ui.label(RichText::new("TEMP").small().color(theme::C_STAT_LABEL));
        ui.end_row();
        for (label, t) in &stats.mb_temps {
          ui.label(RichText::new(short_label(label)).small().color(theme::C_TEXT_MUTED));
          ui.label(RichText::new(format!("{t:.0}°C")).small().color(temp_color(Some(*t), 70, 90)));
          ui.end_row();
        }
      });
    }

    if !stats.mb_voltages.is_empty() {
      ui.add_space(4.0);
      egui::Grid::new("mb_volts").num_columns(2).min_col_width(60.0).show(ui, |ui| {
        ui.label(RichText::new("RAIL").small().color(theme::C_STAT_LABEL));
        ui.label(RichText::new("VOLT").small().color(theme::C_STAT_LABEL));
        ui.end_row();
        for (label, v) in &stats.mb_voltages {
          ui.label(RichText::new(short_label(label)).small().color(theme::C_TEXT_MUTED));
          ui.label(
            RichText::new(format!("{v:.3} V"))
              .small()
              .color(Color32::from_rgb(0xc8, 0xc8, 0x64)),
          );
          ui.end_row();
        }
      });
    }
  });
}
