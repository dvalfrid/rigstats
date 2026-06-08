use egui::{RichText, Ui};

use crate::theme;
use crate::PollStats;

fn fmt_ram(mb: u64) -> String {
  if mb >= 1024 {
    format!("{:.1}G", mb as f64 / 1024.0)
  } else {
    format!("{mb}M")
  }
}

pub fn draw(ui: &mut Ui, stats: &PollStats) {
  theme::panel_frame(ui, theme::C_PROC, |ui| {
    ui.label(RichText::new("PROCESSES").strong().color(theme::C_TEXT).size(13.0));

    if stats.processes.is_empty() {
      ui.label(RichText::new("no data").small().color(theme::C_TEXT_MUTED));
      return;
    }

    egui::Grid::new("procs")
      .num_columns(3)
      .min_col_width(40.0)
      .show(ui, |ui| {
        ui.label(RichText::new("NAME").small().color(theme::C_STAT_LABEL));
        ui.label(RichText::new("CPU").small().color(theme::C_STAT_LABEL));
        ui.label(RichText::new("RAM").small().color(theme::C_STAT_LABEL));
        ui.end_row();

        for p in &stats.processes {
          let name = p.name.trim_end_matches(".exe");
          let name: String = name.chars().take(18).collect();
          ui.label(RichText::new(name).small().color(theme::C_TEXT));
          ui.label(RichText::new(format!("{:.1}%", p.cpu)).small().color(theme::C_PROC));
          ui.label(RichText::new(fmt_ram(p.mem_mb)).small().color(theme::C_TEXT_MUTED));
          ui.end_row();
        }
      });
  });
}
