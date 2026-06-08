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

pub fn draw(ui: &mut Ui, stats: &PollStats, tex: &Textures) {
  theme::panel_frame(ui, theme::C_ACCENT, |ui| {
    // Brand subtitle
    let subtitle = brand_subtitle(&stats.system_brand);
    ui.label(RichText::new(subtitle).small().color(theme::C_STAT_LABEL));

    // Hostname + LHM indicator on same row; brand logo on the right
    ui.horizontal(|ui| {
      ui.vertical(|ui| {
        // Hostname large
        ui.label(RichText::new(&stats.hostname).size(32.0).strong().color(egui::Color32::WHITE));
        // Model name in accent colour
        if !stats.model_name.is_empty() {
          ui.label(RichText::new(&stats.model_name).size(14.0).color(theme::C_ACCENT));
        }
        // CPU model in muted text
        if !stats.cpu_model.is_empty() {
          let model = &stats.cpu_model;
          let model = if model.len() > 44 { &model[..44] } else { model };
          ui.label(RichText::new(model).small().color(theme::C_TEXT_MUTED));
        }
      });

      // Brand logo: right-aligned, vertically centred
      if let Some(logo) = tex.rig_logo(&stats.system_brand) {
        let [lw, lh] = logo.size();
        let target_h = 56.0;
        let w = lw as f32 * (target_h / lh as f32);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
          ui.vertical(|ui| {
            let sized = egui::load::SizedTexture::new(logo.id(), egui::Vec2::new(w, target_h));
            ui.add(egui::Image::new(sized));
            // "REPUBLIC OF GAMERS" caption for ROG
            if matches!(stats.system_brand.as_str(), "rog" | "asus-rog") {
              ui.label(
                RichText::new("REPUBLIC OF\nGAMERS")
                  .size(8.0)
                  .color(theme::C_AMD)
                  .text_style(egui::TextStyle::Small),
              );
            }
          });
        });
      } else {
        // No image — show LHM indicator aligned right
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
          let (dot, color) = if stats.lhm_connected {
            ("[LHM]", egui::Color32::from_rgb(0x39, 0xff, 0x88))
          } else {
            ("[LHM]", theme::C_DIM)
          };
          ui.label(RichText::new(dot).small().color(color));
        });
      }
    });

    // LHM status dot when logo is shown
    if tex.rig_logo(&stats.system_brand).is_some() {
      let (dot, color) = if stats.lhm_connected {
        ("● LHM", egui::Color32::from_rgb(0x39, 0xff, 0x88))
      } else {
        ("○ LHM", theme::C_DIM)
      };
      ui.label(RichText::new(dot).small().color(color));
    }
  });
}
