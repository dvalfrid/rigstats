use egui::{RichText, Ui};

use crate::brand::Textures;
use crate::theme;
use crate::PollStats;

fn brand_subtitle(brand: &str) -> &'static str {
    match brand {
        "rog" | "asus-rog" => "// ASUS ROG",
        "asus" => "// ASUS",
        "alienware" => "// ALIENWARE",
        "razer" => "// RAZER",
        "legion" => "// LENOVO LEGION",
        "omen" => "// HP OMEN",
        "predator" => "// ACER PREDATOR",
        "aorus" => "// GIGABYTE AORUS",
        "msi" => "// MSI",
        "gigabyte" => "// GIGABYTE",
        "asrock" => "// ASROCK",
        "corsair" => "// CORSAIR",
        "nzxt" => "// NZXT",
        "intel" => "// INTEL",
        "dell" => "// DELL",
        "lenovo" => "// LENOVO",
        "hp" => "// HP",
        "acer" => "// ACER",
        _ => "// GAMING RIG",
    }
}

pub fn draw(ui: &mut Ui, stats: &PollStats, tex: &Textures, opacity: f32, th: &theme::AppTheme) {
    theme::panel_frame(ui, opacity, th, |ui| {
        ui.set_min_height(theme::PANEL_HEADER_H);

        let subtitle = brand_subtitle(&stats.system_brand);

        // Single horizontal row spanning full panel height so the logo fills edge-to-edge.
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(subtitle).small().color(th.stat_label));
                ui.label(
                    RichText::new(&stats.hostname)
                        .size(36.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
                if !stats.model_name.is_empty() {
                    ui.label(
                        RichText::new(&stats.model_name)
                            .size(16.0)
                            .color(theme::C_ACCENT),
                    );
                }
            });

            if let Some(logo) = tex.rig_logo(&stats.system_brand) {
                let [lw, lh] = logo.size();
                let target_h = theme::PANEL_HEADER_H;
                let w = lw as f32 * (target_h / lh as f32);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let sized =
                        egui::load::SizedTexture::new(logo.id(), egui::Vec2::new(w, target_h));
                    ui.add(egui::Image::new(sized));
                });
            }
        });
    });
}
