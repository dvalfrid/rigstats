use egui::{Color32, RichText, Ui};

use crate::PollStats;

const LABEL_COLOR: Color32 = Color32::from_gray(130);

fn fmt_ram(mb: u64) -> String {
  if mb >= 1024 {
    format!("{:.1}G", mb as f64 / 1024.0)
  } else {
    format!("{mb}M")
  }
}

pub fn draw(ui: &mut Ui, stats: &PollStats) {
  ui.label(RichText::new("PROCESSES").strong());

  if stats.processes.is_empty() {
    ui.label(RichText::new("no data").small().color(LABEL_COLOR));
    return;
  }

  egui::Grid::new("procs").num_columns(3).min_col_width(40.0).show(ui, |ui| {
    ui.label(RichText::new("NAME").small().color(LABEL_COLOR));
    ui.label(RichText::new("CPU").small().color(LABEL_COLOR));
    ui.label(RichText::new("RAM").small().color(LABEL_COLOR));
    ui.end_row();

    for p in &stats.processes {
      let name = p.name.trim_end_matches(".exe");
      let name: String = name.chars().take(18).collect();
      ui.label(RichText::new(name).small());
      ui.label(RichText::new(format!("{:.1}%", p.cpu)).small().color(Color32::from_rgb(100, 200, 255)));
      ui.label(RichText::new(fmt_ram(p.mem_mb)).small().color(Color32::from_gray(180)));
      ui.end_row();
    }
  });
}
