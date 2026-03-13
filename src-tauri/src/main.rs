//! Application entry point and high-level orchestration.
//! Responsibilities:
//! - Configure Tauri builder, tray, and lifecycle behavior.
//! - Initialize shared application state.
//! - Wire command handlers from the commands module.

mod commands;
mod lhm;
mod settings;
mod stats;

use commands::{
  close_window, ensure_settings_window, get_cpu_info, get_gpu_info, get_settings, get_stats,
  get_system_name, on_window_event, pick_target_monitor, preview_opacity, save_settings,
};
use settings::load_settings;
use stats::AppState;
use std::sync::Mutex;
use sysinfo::{Disks, Networks, System};
use tauri::{CustomMenuItem, Manager, RunEvent, SystemTray, SystemTrayEvent, SystemTrayMenu};

fn main() {
  // Build the tray menu once. Individual click behavior is handled below.
  let tray_menu = SystemTrayMenu::new()
    .add_item(CustomMenuItem::new("show".to_string(), "Show RigStats"))
    .add_native_item(tauri::SystemTrayMenuItem::Separator)
    .add_item(CustomMenuItem::new("settings".to_string(), "Settings"))
    .add_native_item(tauri::SystemTrayMenuItem::Separator)
    .add_item(CustomMenuItem::new("quit".to_string(), "Quit"));

  tauri::Builder::default()
    .setup(|app| {
      let app_handle = app.handle();
      let settings = load_settings(&app_handle);

      // Shared state is stored behind Mutex because commands run concurrently.
      app.manage(AppState {
        settings: Mutex::new(settings),
        system: Mutex::new(System::new_all()),
        disks: Mutex::new(Disks::new_with_refreshed_list()),
        networks: Mutex::new(Networks::new_with_refreshed_list()),
        last_net_sample: Mutex::new(None),
        last_lhm: Mutex::new(None),
      });

      if let Some(main) = app.get_window("main") {
        // Place the dashboard on the preferred portrait monitor if present.
        pick_target_monitor(&main);
      }

      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_settings,
      preview_opacity,
      save_settings,
      close_window,
      get_system_name,
      get_cpu_info,
      get_gpu_info,
      get_stats
    ])
    .system_tray(SystemTray::new().with_menu(tray_menu))
    .on_system_tray_event(|app, event| match event {
      SystemTrayEvent::LeftClick { .. } => {
        if let Some(main) = app.get_window("main") {
          if main.is_visible().unwrap_or(true) {
            let _ = main.hide();
          } else {
            let _ = main.show();
            let _ = main.set_focus();
          }
        }
      }
      SystemTrayEvent::MenuItemClick { id, .. } => {
        match id.as_str() {
          "show" => {
            if let Some(main) = app.get_window("main") {
              let _ = main.show();
              let _ = main.set_focus();
            }
          }
          "settings" => {
            let _ = ensure_settings_window(app);
          }
          "quit" => {
            std::process::exit(0);
          }
          _ => {}
        }
      }
      _ => {}
    })
    .on_window_event(on_window_event)
    .build(tauri::generate_context!())
    .expect("error while running tauri application")
    .run(|_app_handle, event| {
      if let RunEvent::ExitRequested { api, .. } = event {
        api.prevent_exit();
      }
    });
}
