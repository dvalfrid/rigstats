use crate::update_check::{self, UpdateInfo};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    Downloading {
        downloaded: u64,
        total: u64,
    },
    Ready {
        info: UpdateInfo,
        installer_path: PathBuf,
    },
    UpToDate,
    Error(String),
}

pub struct UpdaterState {
    pub status: UpdateStatus,
}

impl Default for UpdaterState {
    fn default() -> Self {
        Self {
            status: UpdateStatus::Idle,
        }
    }
}

#[allow(deprecated)]
pub fn show(
    ctx: &egui::Context,
    main_ctx: &egui::Context,
    open: &Arc<AtomicBool>,
    needs_focus: &Arc<AtomicBool>,
    state: &Arc<Mutex<UpdaterState>>,
) {
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("Updates");
        ui.separator();
        ui.add_space(12.0);

        egui::Grid::new("updater_info")
            .num_columns(2)
            .min_col_width(130.0)
            .show(ui, |ui| {
                ui.label("Installed version:");
                ui.label(format!("v{VERSION}"));
                ui.end_row();
            });

        ui.add_space(16.0);

        let mut state = state.lock().unwrap();

        match &state.status {
            UpdateStatus::Idle => {
                if ui.button("Check for Updates").clicked() {
                    state.status = UpdateStatus::Checking;
                    // Trigger check on next frame via the Checking state —
                    // the caller polls this and spawns the async work.
                }
            }
            UpdateStatus::Checking => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Checking for updates…");
                });
            }
            UpdateStatus::Downloading { downloaded, total } => {
                ui.label("Downloading update…");
                ui.add_space(8.0);
                if *total > 0 {
                    let progress = *downloaded as f32 / *total as f32;
                    ui.add(egui::ProgressBar::new(progress).show_percentage());
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{:.1} / {:.1} MB",
                            *downloaded as f32 / 1_048_576.0,
                            *total as f32 / 1_048_576.0,
                        ))
                        .small()
                        .color(egui::Color32::from_gray(150)),
                    );
                } else {
                    ui.add(egui::ProgressBar::new(0.0).animate(true));
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{:.1} MB downloaded",
                            *downloaded as f32 / 1_048_576.0,
                        ))
                        .small()
                        .color(egui::Color32::from_gray(150)),
                    );
                }
            }
            UpdateStatus::Ready {
                info,
                installer_path,
            } => {
                ui.label(
                    egui::RichText::new(format!("v{} is ready to install!", info.version))
                        .color(egui::Color32::from_rgb(100, 220, 100))
                        .strong(),
                );
                ui.add_space(12.0);

                if !info.notes.is_empty() {
                    ui.label(egui::RichText::new("Release notes:").strong());
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&info.notes)
                                    .small()
                                    .color(egui::Color32::from_gray(180)),
                            );
                        });
                    ui.add_space(12.0);
                }

                let path = installer_path.clone();
                ui.horizontal(|ui| {
                    if ui.button("  Install Now  ").clicked() {
                        if let Err(e) = update_check::launch_installer(&path) {
                            state.status = UpdateStatus::Error(e);
                        }
                    }
                    ui.add_space(8.0);
                    if ui.button("Later").clicked() {
                        open.store(false, Ordering::Relaxed);
                        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
                    }
                });
            }
            UpdateStatus::UpToDate => {
                ui.label(
                    egui::RichText::new("✓ You are up to date.")
                        .color(egui::Color32::from_rgb(100, 200, 100)),
                );
                ui.add_space(8.0);
                if ui.button("Check again").clicked() {
                    state.status = UpdateStatus::Checking;
                }
            }
            UpdateStatus::Error(e) => {
                ui.label(egui::RichText::new(format!("Error: {e}")).color(egui::Color32::RED));
                ui.add_space(8.0);
                if ui.button("Try again").clicked() {
                    state.status = UpdateStatus::Checking;
                }
            }
        }
    });

    if ctx.input(|i| i.viewport().close_requested()) {
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}
