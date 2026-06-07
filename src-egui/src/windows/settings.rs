use rigstats_backend::{autostart, settings};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const ALL_PANELS: &[(&str, &str)] = &[
  ("header", "Header"),
  ("clock", "Clock"),
  ("cpu", "CPU"),
  ("gpu", "GPU"),
  ("ram", "RAM"),
  ("net", "Network"),
  ("disk", "Disk"),
  ("motherboard", "Motherboard"),
  ("process", "Processes"),
  ("battery", "Battery"),
];

const ALL_PROFILES: &[(&str, &str)] = &[
  ("portrait-xl", "Portrait XL (450×1920)"),
  ("portrait-slim", "Portrait Slim (480×1920)"),
  ("portrait-hd", "Portrait HD (720×1280)"),
  ("portrait-wxga", "Portrait WXGA (800×1280)"),
  ("portrait-fhd", "Portrait FHD (1080×1920)"),
  ("portrait-wuxga", "Portrait WUXGA (1200×1920)"),
  ("portrait-qhd", "Portrait QHD (1440×2560)"),
  ("portrait-hdplus", "Portrait HD+ (768×1366)"),
  ("portrait-900x1600", "Portrait 900×1600"),
  ("portrait-1050x1680", "Portrait 1050×1680"),
  ("portrait-1600x2560", "Portrait 1600×2560"),
  ("portrait-4k", "Portrait 4K (2160×3840)"),
  ("portrait-fhd-side", "FHD Side (253×1080)"),
  ("portrait-qhd-side", "QHD Side (338×1440)"),
  ("portrait-4k-side", "4K Side (506×2160)"),
];

pub struct SettingsWindow {
  pub draft: settings::Settings,
  pub tab: usize,
  pub error: Option<String>,
}

impl SettingsWindow {
  pub fn from_settings(s: &settings::Settings) -> Self {
    let mut draft = s.clone();
    // Sync autostart_enabled from registry at open time.
    draft.autostart_enabled = autostart::is_run_key_present();
    Self { draft, tab: 0, error: None }
  }
}

#[allow(deprecated)] // CentralPanel::show() is correct for deferred viewport callbacks
pub fn show(
  ctx: &egui::Context,
  main_ctx: &egui::Context,
  open: &Arc<AtomicBool>,
  state: &Arc<Mutex<SettingsWindow>>,
  dir: &Arc<PathBuf>,
  saved: &Arc<Mutex<settings::Settings>>,
  reload: &Arc<AtomicBool>,
) {
  if ctx.input(|i| i.viewport().close_requested()) {
    open.store(false, Ordering::Relaxed);
    main_ctx.request_repaint();
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    return;
  }

  egui::CentralPanel::default().show(ctx, |ui| {
    let mut state = state.lock().unwrap();

    // ── Tab bar ──────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
      for (i, label) in ["Dashboard", "Panels", "Alerts", "Appearance"].iter().enumerate() {
        if ui.selectable_label(state.tab == i, *label).clicked() {
          state.tab = i;
        }
      }
    });
    ui.separator();
    ui.add_space(6.0);

    // ── Tab content ───────────────────────────────────────────────────────
    let available_h = ui.available_height() - 56.0; // reserve footer
    egui::ScrollArea::vertical().max_height(available_h).show(ui, |ui| {
      let tab = state.tab;
      let draft = &mut state.draft;
      match tab {
        0 => draw_dashboard(ui, draft),
        1 => draw_panels(ui, draft),
        2 => draw_alerts(ui, draft),
        3 => draw_appearance(ui, draft),
        _ => {}
      }
    });

    // ── Footer ────────────────────────────────────────────────────────────
    ui.add_space(4.0);
    ui.separator();

    if let Some(err) = &state.error.clone() {
      ui.label(egui::RichText::new(err.as_str()).color(egui::Color32::RED).small());
    }

    ui.horizontal(|ui| {
      if ui.button("  Save  ").clicked() {
        let autostart_result = if state.draft.autostart_enabled {
          autostart::register_autostart()
        } else {
          autostart::unregister_autostart()
        };
        let save_result = settings::persist_settings(dir.as_ref(), &state.draft);

        match (save_result, autostart_result) {
          (Ok(()), Ok(())) => {
            *saved.lock().unwrap() = state.draft.clone();
            reload.store(true, Ordering::Relaxed);
            main_ctx.request_repaint(); // apply changes immediately
            state.error = None;
            open.store(false, Ordering::Relaxed);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
          }
          (Err(e), _) | (Ok(()), Err(e)) => {
            state.error = Some(format!("Save failed: {e}"));
          }
        }
      }

      if ui.button("Cancel").clicked() {
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint();
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
      }
    });
  });
}

// ── Dashboard tab ─────────────────────────────────────────────────────────────

fn draw_dashboard(ui: &mut egui::Ui, draft: &mut settings::Settings) {
  ui.label(egui::RichText::new("Dashboard Profile").strong());
  ui.add_space(4.0);

  let current_label = ALL_PROFILES
    .iter()
    .find(|(k, _)| *k == draft.dashboard_profile.as_str())
    .map(|(_, l)| *l)
    .unwrap_or(draft.dashboard_profile.as_str());

  egui::ComboBox::from_id_salt("profile_combo")
    .selected_text(current_label)
    .width(280.0)
    .show_ui(ui, |ui| {
      for &(key, label) in ALL_PROFILES {
        ui.selectable_value(&mut draft.dashboard_profile, key.to_string(), label);
      }
    });

  ui.add_space(4.0);
  ui.label(
    egui::RichText::new("Profile changes take effect after restarting RigStats.")
      .small()
      .color(egui::Color32::from_gray(130)),
  );

  ui.add_space(16.0);
  ui.label(egui::RichText::new("Floating Mode").strong());
  ui.add_space(4.0);
  ui.checkbox(&mut draft.floating_mode, "Open each panel as its own window");
  ui.label(
    egui::RichText::new("Floating mode will be fully implemented in a future update.")
      .small()
      .color(egui::Color32::from_gray(130)),
  );
}

// ── Panels tab ────────────────────────────────────────────────────────────────

fn draw_panels(ui: &mut egui::Ui, draft: &mut settings::Settings) {
  ui.label(egui::RichText::new("Panel Visibility & Order").strong());
  ui.label(
    egui::RichText::new("Use ↑ / ↓ to reorder visible panels.")
      .small()
      .color(egui::Color32::from_gray(130)),
  );
  ui.add_space(8.0);

  // Build ordered list: visible panels first (in order), then hidden
  let mut ordered: Vec<(&str, &str, bool)> = Vec::new();
  for key in &draft.visible_panels {
    if let Some(&(_, label)) = ALL_PANELS.iter().find(|(k, _)| k == key) {
      ordered.push((key, label, true));
    }
  }
  for &(key, label) in ALL_PANELS {
    if !draft.visible_panels.iter().any(|p| p == key) {
      ordered.push((key, label, false));
    }
  }

  let vis_count = draft.visible_panels.len();
  let mut toggle: Option<(String, bool)> = None;
  let mut reorder: Option<(String, i32)> = None;

  for &(key, label, visible) in &ordered {
    ui.horizontal(|ui| {
      let mut v = visible;
      if ui.checkbox(&mut v, label).changed() {
        toggle = Some((key.to_string(), v));
      }

      if visible {
        let idx = draft.visible_panels.iter().position(|p| p == key).unwrap_or(0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
          if idx + 1 < vis_count && ui.small_button("↓").clicked() {
            reorder = Some((key.to_string(), 1));
          }
          if idx > 0 && ui.small_button("↑").clicked() {
            reorder = Some((key.to_string(), -1));
          }
        });
      }
    });
  }

  if let Some((key, add)) = toggle {
    if add {
      if !draft.visible_panels.contains(&key) {
        draft.visible_panels.push(key);
      }
    } else {
      draft.visible_panels.retain(|p| p != &key);
    }
  }

  if let Some((key, dir)) = reorder {
    if let Some(idx) = draft.visible_panels.iter().position(|p| p == &key) {
      let new_idx = if dir < 0 {
        idx.saturating_sub(1)
      } else {
        (idx + 1).min(draft.visible_panels.len().saturating_sub(1))
      };
      draft.visible_panels.swap(idx, new_idx);
    }
  }
}

// ── Alerts tab ────────────────────────────────────────────────────────────────

fn draw_alerts(ui: &mut egui::Ui, draft: &mut settings::Settings) {
  ui.label(egui::RichText::new("Notifications").strong());
  ui.add_space(4.0);
  ui.checkbox(&mut draft.notify_on_crit, "Notify when critical threshold is exceeded");
  ui.add_space(8.0);

  ui.horizontal(|ui| {
    ui.label("Alert cooldown:");
    let mut secs = draft.alert_cooldown_secs as i32;
    if ui.add(egui::DragValue::new(&mut secs).range(60..=3600).suffix(" s")).changed() {
      draft.alert_cooldown_secs = secs.max(60) as u64;
    }
  });

  ui.add_space(16.0);
  ui.label(egui::RichText::new("Thresholds").strong());

  for &(key, label) in &[
    ("cpu", "CPU Temperature"),
    ("gpu", "GPU Temperature"),
    ("ram", "RAM Temperature"),
    ("disk", "Disk Temperature"),
    ("battery", "Battery Level"),
  ] {
    ui.add_space(10.0);
    ui.label(egui::RichText::new(label).underline());

    let entry = draft.thresholds.entry(key.to_string()).or_insert_with(|| {
      settings::default_thresholds().remove(key).unwrap_or_default()
    });

    let unit = if key == "battery" { "%" } else { "°C" };
    let (warn_lbl, crit_lbl) = if key == "battery" {
      ("Warn below", "Crit below")
    } else {
      ("Warn above", "Crit above")
    };

    let mut warn_on = entry.warn.is_some();
    let mut crit_on = entry.crit.is_some();
    let mut warn_val = entry.warn.unwrap_or(80) as i32;
    let mut crit_val = entry.crit.unwrap_or(90) as i32;

    egui::Grid::new(format!("t_{key}")).num_columns(3).min_col_width(90.0).show(ui, |ui| {
      if ui.checkbox(&mut warn_on, warn_lbl).changed() && !warn_on {
        warn_val = entry.warn.unwrap_or(80) as i32;
      }
      ui.add_enabled(warn_on, egui::DragValue::new(&mut warn_val).range(0..=255).suffix(unit));
      ui.end_row();

      if ui.checkbox(&mut crit_on, crit_lbl).changed() && !crit_on {
        crit_val = entry.crit.unwrap_or(90) as i32;
      }
      ui.add_enabled(crit_on, egui::DragValue::new(&mut crit_val).range(0..=255).suffix(unit));
      ui.end_row();
    });

    entry.warn = if warn_on { Some(warn_val as u8) } else { None };
    entry.crit = if crit_on { Some(crit_val as u8) } else { None };
  }
}

// ── Appearance tab ────────────────────────────────────────────────────────────

fn draw_appearance(ui: &mut egui::Ui, draft: &mut settings::Settings) {
  egui::Grid::new("appearance")
    .num_columns(2)
    .min_col_width(150.0)
    .spacing([12.0, 8.0])
    .show(ui, |ui| {
      // Rig name
      ui.label("Rig name:");
      ui.text_edit_singleline(&mut draft.model_name);
      ui.end_row();

      // Opacity
      ui.label("Opacity:");
      let mut opacity = draft.opacity as f32;
      ui.add(egui::Slider::new(&mut opacity, 0.1_f32..=1.0_f32).step_by(0.05));
      draft.opacity = opacity as f64;
      ui.end_row();

      // Window layer
      ui.label("Window layer:");
      let layer_label = match draft.window_layer.as_str() {
        "on_top" => "Always on top",
        "behind" => "Behind other windows",
        _ => "Normal",
      };
      egui::ComboBox::from_id_salt("window_layer_combo")
        .selected_text(layer_label)
        .show_ui(ui, |ui| {
          ui.selectable_value(&mut draft.window_layer, "normal".to_string(), "Normal");
          ui.selectable_value(&mut draft.window_layer, "on_top".to_string(), "Always on top");
          ui.selectable_value(
            &mut draft.window_layer,
            "behind".to_string(),
            "Behind other windows",
          );
        });
      ui.end_row();

      // Autostart
      ui.label("Autostart:");
      ui.checkbox(&mut draft.autostart_enabled, "Launch at login");
      ui.end_row();

      // Stats logging
      ui.label("Stats logging:");
      ui.checkbox(&mut draft.logging_enabled, "Write CSV log each second");
      ui.end_row();

      // Log retention (only shown when logging is on)
      if draft.logging_enabled {
        ui.label("Log retention:");
        let mut days = draft.log_retention_days as i32;
        ui.add(egui::DragValue::new(&mut days).range(1..=365).suffix(" days"));
        draft.log_retention_days = days.max(1) as u32;
        ui.end_row();
      }
    });
}
