use egui::{Color32, RichText, Ui};

use crate::tempcolor;
use crate::{theme, PollStats};

/// Charge colour: green when above warn, yellow when above crit, red when critical.
/// Charging always uses accent (cyan).
fn charge_color(pct: u8, charging: bool, charge_warn: u8, charge_crit: u8) -> Color32 {
    if charging {
        return theme::C_ACCENT;
    }
    if pct >= charge_warn {
        tempcolor::color_ok()
    } else if pct >= charge_crit {
        tempcolor::color_warn()
    } else {
        tempcolor::color_hot()
    }
}

/// Power colour: green below warn, yellow below crit, red at or above crit.
/// Charging uses muted (power draw is expected / irrelevant).
fn power_color(
    w: f64,
    charging: bool,
    power_warn: u8,
    power_crit: u8,
    th: &theme::AppTheme,
) -> Color32 {
    if charging {
        return th.text_muted;
    }
    tempcolor::temp_color(Some(w), power_warn, power_crit)
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut Ui,
    stats: &PollStats,
    opacity: f32,
    th: &theme::AppTheme,
    sc: f32,
    charge_warn: u8,
    charge_crit: u8,
    power_warn: u8,
    power_crit: u8,
) -> egui::Rect {
    theme::panel_frame(ui, opacity, th, sc, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H * sc);
        ui.label(
            RichText::new("BATTERY")
                .strong()
                .color(theme::C_PANEL_TITLE)
                .size(theme::FONT_PANEL_TITLE * sc),
        );

        #[cfg(not(debug_assertions))]
        if !stats.battery_present {
            ui.label(
                RichText::new("NO BATTERY")
                    .size(14.0 * sc)
                    .color(th.text_muted),
            );
            return;
        }

        #[cfg(debug_assertions)]
        let (charge, charging, fake_time, fake_power) = if !stats.battery_present {
            (71u8, false, Some(260u32), Some(11.7f64))
        } else {
            (
                stats.battery_charge_pct.unwrap_or(0),
                stats.battery_charging.unwrap_or(false),
                stats.battery_time_mins,
                stats.battery_power_w,
            )
        };
        #[cfg(not(debug_assertions))]
        let (charge, charging, fake_time, fake_power) = (
            stats.battery_charge_pct.unwrap_or(0),
            stats.battery_charging.unwrap_or(false),
            stats.battery_time_mins,
            stats.battery_power_w,
        );

        let frac = charge as f32 / 100.0;
        let color = charge_color(charge, charging, charge_warn, charge_crit);

        // Large percentage: big number + small subscript %
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{charge}"))
                    .size(48.0 * sc)
                    .color(color),
            );
            ui.vertical(|ui| {
                ui.add_space(20.0 * sc);
                ui.label(RichText::new("%").size(18.0 * sc).color(th.text_muted));
            });
        });

        // Three-column stat row: STATUS | TIME LEFT | POWER
        let status_str = if charging { "CHARGING" } else { "DISCHARGING" };
        let status_color = if charging { theme::C_ACCENT } else { color };
        let time_str = fake_time
            .map(|m| format!("{}h {:02}m", m / 60, m % 60))
            .unwrap_or_else(|| "\u{2014}".to_string());
        let (power_str, pw_color) = fake_power
            .map(|w| {
                (
                    format!("{w:.1} W"),
                    power_color(w, charging, power_warn, power_crit, th),
                )
            })
            .unwrap_or_else(|| ("\u{2014}".to_string(), th.text_muted));

        ui.add_space(4.0 * sc);
        let stat_label = th.stat_label;
        let text_muted = th.text_muted;
        ui.columns(3, |cols| {
            for (col_ui, (header, value, vc)) in cols.iter_mut().zip([
                ("STATUS", status_str, status_color),
                ("TIME LEFT", time_str.as_str(), text_muted),
                ("POWER", power_str.as_str(), pw_color),
            ]) {
                col_ui.label(RichText::new(header).size(11.0 * sc).color(stat_label));
                col_ui.label(RichText::new(value).size(13.0 * sc).color(vc));
            }
        });

        // Push PWR bar to the bottom of the panel.
        let cursor = ui.cursor().top();
        let bar_row_h = 14.0 * sc;
        let filler =
            (theme::PANEL_DATA_H * sc - (cursor - ui.min_rect().top()) - bar_row_h).max(2.0 * sc);
        ui.add_space(filler);

        ui.horizontal(|ui| {
            ui.label(RichText::new("PWR").size(11.0 * sc).color(th.stat_label));
            let bar_w = (ui.available_width() - 44.0 * sc).max(4.0 * sc);
            theme::thin_bar_scaled(ui, frac, bar_w, color, sc);
            ui.label(
                RichText::new(format!("{charge}%"))
                    .size(11.0 * sc)
                    .color(color),
            );
        });
    })
}
