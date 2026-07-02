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

pub fn draw(
    ui: &mut Ui,
    stats: &PollStats,
    tex: &Textures,
    opacity: f32,
    th: &theme::AppTheme,
    sc: f32,
) -> egui::Rect {
    theme::panel_frame(ui, opacity, th, sc, |ui| {
        ui.set_min_height(theme::PANEL_HEADER_H * sc);

        let subtitle = brand_subtitle(&stats.system_brand);
        let logo = tex.rig_logo(&stats.system_brand);
        let logo_w = logo.map_or(0.0, |logo| {
            let [lw, lh] = logo.size();
            let target_h = theme::PANEL_HEADER_H * sc;
            lw as f32 * (target_h / lh as f32)
        });

        // Single horizontal row spanning full panel height so the logo fills edge-to-edge.
        // Reserve the logo's width up front so a long hostname truncates instead of
        // crowding into (or overlapping) the logo.
        ui.horizontal(|ui| {
            let text_w = (ui.available_width() - logo_w - ui.spacing().item_spacing.x).max(0.0);
            ui.vertical(|ui| {
                ui.set_max_width(text_w);
                ui.add_space(6.0 * sc);
                ui.label(RichText::new(subtitle).size(11.0 * sc).color(th.stat_label));
                ui.add(
                    egui::Label::new(
                        RichText::new(&stats.hostname)
                            .size(36.0 * sc)
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                    .truncate(),
                );
                if !stats.model_name.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&stats.model_name)
                                .size(16.0 * sc)
                                .color(theme::C_ACCENT),
                        )
                        .truncate(),
                    );
                }
            });

            if let Some(logo) = logo {
                let target_h = theme::PANEL_HEADER_H * sc;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let sized =
                        egui::load::SizedTexture::new(logo.id(), egui::Vec2::new(logo_w, target_h));
                    ui.add(egui::Image::new(sized));
                });
            }
        });
    })
}
