use egui::{RichText, Ui};

use crate::brand::Textures;
use crate::theme;
use crate::PollStats;
use rigstats_backend::control::ControlState;

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
    control: &ControlState,
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
                draw_profile_chip(ui, control, stats.service_profile.as_ref(), sc);
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

/// Active-profile chip (a painted dot + `GAMING`) — Control Center phase 0
/// (#187). Click opens the Control Center window; consumed the same way the
/// clock panel's
/// `"open_updater"` badge-click flag is (see `RigStatsApp::draw_one_panel`
/// in main.rs) — the wallpaper host never consumes it, so clicking there is
/// a harmless no-op. Nothing renders until there is an active profile.
/// The chip's text: the control pipe's active profile (the main app — it
/// follows a switch at once), else the one the sensor service reports in the
/// telemetry (the wallpaper host, which has no control pipe, #302).
fn chip_label(
    control: &ControlState,
    service: Option<&rigstats_backend::lhm::ActiveProfile>,
) -> Option<String> {
    if let Some(active_id) = &control.active_profile {
        return Some(
            control
                .profiles
                .iter()
                .find(|p| &p.id == active_id)
                .map_or_else(|| active_id.to_uppercase(), |p| p.name.to_uppercase()),
        );
    }
    service.map(|p| p.name.to_uppercase())
}

fn draw_profile_chip(
    ui: &mut Ui,
    control: &ControlState,
    service: Option<&rigstats_backend::lhm::ActiveProfile>,
    sc: f32,
) {
    let Some(label) = chip_label(control, service) else {
        return;
    };

    let color = egui::Color32::from_rgb(0x39, 0xff, 0x88);
    let border = egui::Color32::from_rgba_unmultiplied(0x39, 0xff, 0x88, 115);
    let bg = egui::Color32::from_rgba_unmultiplied(0x39, 0xff, 0x88, 26);

    ui.add_space(4.0 * sc);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0 * sc;
        // Painted dot rather than a Unicode bullet glyph, which the embedded
        // font doesn't have (renders as a tofu box) — same fix as the
        // "Recording" indicator in windows/history.rs.
        let (dot_rect, _) =
            ui.allocate_exact_size(egui::vec2(8.0 * sc, 11.0 * sc), egui::Sense::hover());
        ui.painter()
            .circle_filled(dot_rect.center(), 2.5 * sc, color);

        let resp = ui.add(
            egui::Button::new(RichText::new(label).size(11.0 * sc).color(color))
                .fill(bg)
                .stroke(egui::Stroke::new(1.0_f32, border))
                .corner_radius(egui::CornerRadius::same(2)),
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            ui.ctx()
                .data_mut(|d| d.insert_temp(egui::Id::new("open_control_center"), true));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rigstats_backend::lhm::ActiveProfile;

    fn service(name: &str) -> ActiveProfile {
        ActiveProfile {
            id: name.to_lowercase(),
            name: name.into(),
        }
    }

    #[test]
    fn the_wallpaper_host_shows_the_service_profile() {
        // No control pipe (the host): the telemetry's profile.
        let control = ControlState::default();
        assert_eq!(
            chip_label(&control, Some(&service("Balanced"))),
            Some("BALANCED".into())
        );
    }

    #[test]
    fn the_control_pipe_wins_over_the_telemetry() {
        // The main app follows a switch at once through the control pipe.
        let control = ControlState {
            active_profile: Some("gaming".into()),
            ..Default::default()
        };
        assert_eq!(
            chip_label(&control, Some(&service("Balanced"))),
            Some("GAMING".into())
        );
    }

    #[test]
    fn no_profile_no_chip() {
        assert_eq!(chip_label(&ControlState::default(), None), None);
    }
}
