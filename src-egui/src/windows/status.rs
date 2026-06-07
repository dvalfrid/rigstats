use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct StatusState {
  pub log: String,
  pub service_status: String,
}

impl StatusState {
  pub fn load(dir: &std::path::Path) -> Self {
    let log = std::fs::read_to_string(dir.join("rigstats-debug.log"))
      .unwrap_or_else(|_| "(log file not found)".to_string());
    Self { log, service_status: query_service() }
  }
}

fn query_service() -> String {
  match std::process::Command::new("sc.exe").args(["query", "rigstats-sensor"]).output() {
    Ok(out) => {
      let s = String::from_utf8_lossy(&out.stdout);
      if s.contains("RUNNING") {
        "Running".to_string()
      } else if s.contains("STOPPED") {
        "Stopped".to_string()
      } else if s.contains("does not exist") || s.contains("1060") {
        "Not installed".to_string()
      } else {
        "Unknown".to_string()
      }
    }
    Err(_) => "Error querying service".to_string(),
  }
}

#[allow(deprecated)] // CentralPanel::show() is correct for deferred viewport callbacks
pub fn show(
  ctx: &egui::Context,
  main_ctx: &egui::Context,
  open: &Arc<AtomicBool>,
  needs_focus: &Arc<AtomicBool>,
  state: &Arc<Mutex<StatusState>>,
  dir: &Arc<PathBuf>,
) {
  if needs_focus.swap(false, Ordering::Relaxed) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
  }

  egui::CentralPanel::default().show(ctx, |ui| {
    ui.horizontal(|ui| {
      ui.heading("Status");
      ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button("Refresh").clicked() {
          *state.lock().unwrap() = StatusState::load(dir.as_ref());
        }
      });
    });

    ui.separator();
    ui.add_space(4.0);

    let state = state.lock().unwrap();

    egui::Grid::new("status_meta").num_columns(2).min_col_width(150.0).show(ui, |ui| {
      ui.label("Sensor service:");
      let color = match state.service_status.as_str() {
        "Running" => egui::Color32::from_rgb(100, 200, 100),
        "Stopped" | "Not installed" => egui::Color32::from_rgb(220, 80, 60),
        _ => egui::Color32::from_gray(160),
      };
      ui.label(egui::RichText::new(&state.service_status).color(color));
      ui.end_row();
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("Debug log").strong());
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
      .auto_shrink([false, false])
      .stick_to_bottom(true)
      .show(ui, |ui| {
        let mut log = state.log.as_str();
        ui.add(
          egui::TextEdit::multiline(&mut log)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .interactive(false),
        );
      });
  });

  if ctx.input(|i| i.viewport().close_requested()) {
    open.store(false, Ordering::Relaxed);
    main_ctx.request_repaint_of(egui::ViewportId::ROOT);
  }
}
