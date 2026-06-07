use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[allow(dead_code)] // Error variant reserved for Phase 7 actual update check
pub enum UpdateStatus {
  Idle,
  UpToDate,
  Error(String),
}

pub struct UpdaterState {
  pub status: UpdateStatus,
}

impl Default for UpdaterState {
  fn default() -> Self {
    Self { status: UpdateStatus::Idle }
  }
}

#[allow(deprecated)] // CentralPanel::show() is correct for deferred viewport callbacks
pub fn show(
  ctx: &egui::Context,
  main_ctx: &egui::Context,
  open: &Arc<AtomicBool>,
  state: &Arc<Mutex<UpdaterState>>,
) {
  if ctx.input(|i| i.viewport().close_requested()) {
    open.store(false, Ordering::Relaxed);
    main_ctx.request_repaint();
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    return;
  }

  egui::CentralPanel::default().show(ctx, |ui| {
    ui.heading("Updates");
    ui.separator();
    ui.add_space(12.0);

    egui::Grid::new("updater_info").num_columns(2).min_col_width(130.0).show(ui, |ui| {
      ui.label("Installed version:");
      ui.label(format!("v{VERSION}"));
      ui.end_row();
    });

    ui.add_space(16.0);

    let mut state = state.lock().unwrap();
    if ui.button("Check for Updates").clicked() {
      // Phase 7 will implement the actual HTTP check against latest.json.
      state.status = UpdateStatus::UpToDate;
    }

    ui.add_space(8.0);

    match &state.status {
      UpdateStatus::Idle => {}
      UpdateStatus::UpToDate => {
        ui.label(
          egui::RichText::new("✓ You are up to date.")
            .color(egui::Color32::from_rgb(100, 200, 100)),
        );
      }
      UpdateStatus::Error(e) => {
        ui.label(egui::RichText::new(format!("Error: {e}")).color(egui::Color32::RED));
      }
    }
  });
}
