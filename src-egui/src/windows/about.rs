use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[allow(deprecated)] // CentralPanel::show() is correct for deferred viewport callbacks
pub fn show(
  ctx: &egui::Context,
  main_ctx: &egui::Context,
  open: &Arc<AtomicBool>,
  dir: &Arc<PathBuf>,
) {
  if ctx.input(|i| i.viewport().close_requested()) {
    open.store(false, Ordering::Relaxed);
    main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    return;
  }

  egui::CentralPanel::default().show(ctx, |ui| {
    ui.add_space(24.0);

    ui.vertical_centered(|ui| {
      ui.label(egui::RichText::new("RigStats").size(30.0).strong());
      ui.label(
        egui::RichText::new(format!("v{VERSION}"))
          .size(13.0)
          .color(egui::Color32::from_gray(150)),
      );
      ui.add_space(12.0);
      ui.label("Gaming rig stats dashboard for secondary display.");
      ui.add_space(6.0);
      ui.label(
        egui::RichText::new("github.com/dvalfrid/rigstats")
          .color(egui::Color32::from_rgb(80, 160, 220)),
      );
      ui.add_space(4.0);
      ui.label(
        egui::RichText::new("Built with Rust + egui")
          .small()
          .color(egui::Color32::from_gray(130)),
      );
    });

    ui.add_space(20.0);
    ui.separator();
    ui.add_space(12.0);

    ui.vertical_centered(|ui| {
      if ui.button("  Open Logs Folder  ").clicked() {
        let _ = std::process::Command::new("explorer").arg(dir.as_ref()).spawn();
      }
    });
  });
}
