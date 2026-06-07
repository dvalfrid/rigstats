use egui::{Color32, ProgressBar, RichText, Ui};

use crate::tempcolor::color_accent;
use crate::PollStats;

const LABEL_COLOR: Color32 = Color32::from_gray(130);

fn safe_bar_width(ui: &Ui) -> f32 {
  let w = ui.available_width();
  if w.is_finite() && w > 0.0 { w } else { ui.ctx().content_rect().width().max(1.0) }
}

fn charge_color(pct: u8, charging: bool) -> Color32 {
  if charging {
    return color_accent();
  }
  if pct > 50 {
    Color32::from_rgb(57, 255, 136)
  } else if pct > 20 {
    Color32::from_rgb(255, 179, 0)
  } else {
    Color32::from_rgb(255, 58, 31)
  }
}

fn power_color(w: f64, charging: bool) -> Color32 {
  if charging {
    return Color32::from_gray(200);
  }
  if w < 12.0 {
    Color32::from_rgb(57, 255, 136)
  } else if w < 20.0 {
    Color32::from_rgb(255, 179, 0)
  } else {
    Color32::from_rgb(255, 58, 31)
  }
}

pub fn draw(ui: &mut Ui, stats: &PollStats) {
  ui.label(RichText::new("BATTERY").strong());

  if !stats.battery_present {
    ui.label(RichText::new("NO BATTERY").color(LABEL_COLOR));
    return;
  }

  let charge = stats.battery_charge_pct.unwrap_or(0);
  let frac = charge as f32 / 100.0;
  let charging = stats.battery_charging.unwrap_or(false);
  let color = charge_color(charge, charging);

  ui.horizontal(|ui| {
    ui.label(RichText::new(format!("{charge}%")).color(color));
    if charging {
      ui.label(RichText::new("⚡ CHARGING").small().color(color_accent()));
    }
  });

  let bw = safe_bar_width(ui);
  ui.add(ProgressBar::new(frac).desired_width(bw).fill(color));

  if let Some(mins) = stats.battery_time_mins {
    let h = mins / 60;
    let m = mins % 60;
    ui.label(
      RichText::new(format!("{h}h {m:02}m remaining"))
        .small()
        .color(Color32::from_gray(160)),
    );
  }

  if let Some(w) = stats.battery_power_w {
    ui.label(
      RichText::new(format!("{w:.1} W"))
        .small()
        .color(power_color(w, charging)),
    );
  }
}
