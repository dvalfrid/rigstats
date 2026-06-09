use egui::{Color32, RichText, Ui};

use crate::{theme, PollStats};

fn charge_color(pct: u8, charging: bool) -> Color32 {
  if charging {
    return theme::C_ACCENT;
  }
  if pct > 50 {
    theme::C_GRN
  } else if pct > 20 {
    theme::C_RAM
  } else {
    theme::C_AMD
  }
}

fn power_color(w: f64, charging: bool) -> Color32 {
  if charging {
    return theme::C_TEXT_MUTED;
  }
  if w < 12.0 {
    theme::C_GRN
  } else if w < 20.0 {
    theme::C_RAM
  } else {
    theme::C_AMD
  }
}

pub fn draw(ui: &mut Ui, stats: &PollStats) {
  theme::panel_frame(ui, theme::C_GRN, |ui| {
    ui.set_min_height(theme::PANEL_DATA_H);
    ui.label(RichText::new("BATTERY").strong().color(theme::C_PANEL_TITLE).size(theme::FONT_PANEL_TITLE));

    if !stats.battery_present {
      ui.label(RichText::new("NO BATTERY").color(theme::C_TEXT_MUTED));
      return;
    }

    let charge = stats.battery_charge_pct.unwrap_or(0);
    let frac = charge as f32 / 100.0;
    let charging = stats.battery_charging.unwrap_or(false);
    let color = charge_color(charge, charging);

    ui.horizontal(|ui| {
      ui.label(RichText::new(format!("{charge}%")).color(color).size(22.0));
      if charging {
        ui.label(RichText::new("⚡ CHARGING").small().color(theme::C_ACCENT));
      }
    });

    let bw = (ui.available_width()).max(4.0);
    theme::thin_bar(ui, frac, bw, color);

    if let Some(mins) = stats.battery_time_mins {
      let h = mins / 60;
      let m = mins % 60;
      ui.label(
        RichText::new(format!("{h}h {m:02}m remaining")).small().color(theme::C_TEXT_MUTED),
      );
    }

    if let Some(w) = stats.battery_power_w {
      ui.label(
        RichText::new(format!("{w:.1} W")).small().color(power_color(w, charging)),
      );
    }
  });
}
