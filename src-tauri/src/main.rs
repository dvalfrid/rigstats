//! Application entry point and high-level orchestration.
//! Responsibilities:
//! - Configure Tauri builder, tray, and lifecycle behavior.
//! - Initialize shared application state.
//! - Wire command handlers from the commands module.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod lhm;
mod settings;
mod stats;

use commands::{
  close_window, detect_gpu_vram_total_mb, detect_ping_target, detect_ram_details,
  detect_ram_spec, ensure_lhm_running, ensure_settings_window, get_cpu_info,
  get_gpu_info, get_settings, get_stats, get_system_brand, get_system_name, on_window_event, pick_target_monitor,
  preview_opacity, save_settings, start_window_drag, detect_model_name, detect_system_brand,
};
use settings::{load_settings, persist_settings, LEGACY_DEFAULT_MODEL_NAME};
use stats::AppState;
use std::sync::Mutex;
use sysinfo::{Disks, Networks, System};
use tauri::{
  menu::MenuBuilder,
  tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
  AppHandle, Manager, RunEvent,
};

const TRAY_SHOW_ID: &str = "show";
const TRAY_SETTINGS_ID: &str = "settings";
const TRAY_QUIT_ID: &str = "quit";

fn focus_main_window(app: &AppHandle) {
  if let Some(main) = app.get_webview_window("main") {
    let _ = main.show();
    let _ = main.set_focus();
  }
}

fn toggle_main_window(app: &AppHandle) {
  if let Some(main) = app.get_webview_window("main") {
    if main.is_visible().unwrap_or(true) {
      let _ = main.hide();
    } else {
      let _ = main.show();
      let _ = main.set_focus();
    }
  }
}

fn create_tray(app: &tauri::App) -> tauri::Result<()> {
  let tray_menu = MenuBuilder::new(app)
    .text(TRAY_SHOW_ID, "Show RigStats")
    .separator()
    .text(TRAY_SETTINGS_ID, "Settings")
    .separator()
    .text(TRAY_QUIT_ID, "Quit")
    .build()?;

  let mut tray_builder = TrayIconBuilder::with_id("main")
    .menu(&tray_menu)
    .show_menu_on_left_click(false)
    .on_menu_event(|app, event| match event.id().as_ref() {
      TRAY_SHOW_ID => focus_main_window(app),
      TRAY_SETTINGS_ID => {
        let _ = ensure_settings_window(app);
      }
      TRAY_QUIT_ID => std::process::exit(0),
      _ => {}
    })
    .on_tray_icon_event(|tray, event| {
      if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
      } = event
      {
        toggle_main_window(&tray.app_handle());
      }
    });

  if let Some(icon) = app.default_window_icon().cloned() {
    tray_builder = tray_builder.icon(icon);
  }

  tray_builder.build(app)?;
  Ok(())
}

fn main() {
  tauri::Builder::default()
    .setup(|app| {
      let app_handle = app.handle();
      let mut settings = load_settings(&app_handle);
      let should_autofill_model = settings.model_name.trim().is_empty()
        || settings.model_name.trim() == LEGACY_DEFAULT_MODEL_NAME;
      if should_autofill_model {
        if let Some(model_name) = detect_model_name() {
          settings.model_name = model_name;
          let _ = persist_settings(&app_handle, &settings);
        }
      }
      let startup_profile = settings.dashboard_profile.clone();
      let startup_always_on_top = settings.always_on_top;
      let ram_spec = detect_ram_spec();
      let ram_details = detect_ram_details();
      let gpu_vram_total_mb = detect_gpu_vram_total_mb();
      let ping_target = detect_ping_target();
      let system_brand = detect_system_brand();

      // Shared state is stored behind Mutex because commands run concurrently.
      app.manage(AppState {
        settings: Mutex::new(settings),
        system: Mutex::new(System::new_all()),
        disks: Mutex::new(Disks::new_with_refreshed_list()),
        networks: Mutex::new(Networks::new_with_refreshed_list()),
        last_net_sample: Mutex::new(None),
        last_ping_sample: Mutex::new(None),
        last_lhm: Mutex::new(None),
        ram_spec,
        ram_details,
        gpu_vram_total_mb,
        ping_target,
        system_brand,
      });

      if let Some(main) = app.get_webview_window("main") {
        // Place the dashboard on the preferred portrait monitor if present.
        let _ = pick_target_monitor(&main, &startup_profile);
        let _ = main.set_always_on_top(startup_always_on_top);
        let _ = main.show();
        let _ = main.set_focus();
      }

      // Fallback for cases where installer task did not launch LHM yet.
      ensure_lhm_running(&app_handle);

      create_tray(app)?;

      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_settings,
      preview_opacity,
      save_settings,
      close_window,
      start_window_drag,
      get_system_name,
      get_system_brand,
      get_cpu_info,
      get_gpu_info,
      get_stats
    ])
    .on_window_event(|window, event| on_window_event(window, event))
    .build(tauri::generate_context!())
    .expect("error while running tauri application")
    .run(|_app_handle, event| {
      if let RunEvent::ExitRequested { api, .. } = event {
        api.prevent_exit();
      }
    });
}
