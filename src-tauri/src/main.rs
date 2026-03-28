//! Application entry point and high-level orchestration.
//! Responsibilities:
//! - Configure Tauri builder, tray, and lifecycle behavior.
//! - Initialize shared application state.
//! - Wire command handlers from the commands module.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod commands;
mod debug;
mod diagnostics;
mod hardware;
mod lhm;
mod lhm_process;
mod monitor;
mod settings;
mod stats;
mod streamdeck;
mod updater;
mod windows;

use commands::{
  close_window, get_about_info, get_changelog, get_cpu_info, get_gpu_info, get_settings, get_stats, get_system_brand,
  get_system_name, log_frontend_error, preview_opacity, preview_profile, preview_visible_panels, save_settings,
  set_main_height, start_window_drag, test_temp_alert,
};
use debug::{append_debug_log, reset_debug_log};
use diagnostics::collect_diagnostics;
use hardware::{
  detect_disk_model_map, detect_gpu_vram_total_mb, detect_model_name, detect_motherboard_name, detect_ping_target,
  detect_ram_details, detect_ram_spec, detect_system_brand, is_placeholder_model_name, probe_wmi_status,
};
use lhm_process::ensure_lhm_running;
use monitor::pick_target_monitor;
use settings::{load_settings, persist_settings};
use stats::AppState;
use std::collections::HashMap;
use std::sync::Mutex;
use sysinfo::{Disks, Networks, System};
use tauri::{
  menu::MenuBuilder,
  tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
  AppHandle, Manager, RunEvent,
};
use updater::{check_for_update, install_update, open_updater_window};
use windows::{
  ensure_about_window, ensure_settings_window, ensure_status_window, ensure_updater_window, on_window_event,
  set_last_tray_click_position,
};

/// Registers the app's AppUserModelID in HKCU so that Windows toast notifications
/// display "RIGStats" as the source instead of the parent process (e.g. PowerShell).
/// The NSIS installer creates a Start-menu shortcut that carries the same AUMID,
/// so in production builds this registry entry is redundant but harmless.
#[cfg(windows)]
fn register_notification_app_id() {
  use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
  use winreg::RegKey;
  let hkcu = RegKey::predef(HKEY_CURRENT_USER);
  let key_path = r"Software\Classes\AppUserModelId\se.codeby.rigstats";
  if let Ok((key, _)) = hkcu.create_subkey_with_flags(key_path, KEY_WRITE) {
    let _ = key.set_value("DisplayName", &"RIGStats");
  }
}

const TRAY_SHOW_ID: &str = "show";
const TRAY_SETTINGS_ID: &str = "settings";
const TRAY_ABOUT_ID: &str = "about";
const TRAY_STATUS_ID: &str = "status";
const TRAY_UPDATES_ID: &str = "updates";
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
    .text(TRAY_SHOW_ID, "Show RIGStats")
    .separator()
    .text(TRAY_SETTINGS_ID, "Settings")
    .text(TRAY_STATUS_ID, "Status")
    .text(TRAY_ABOUT_ID, "About")
    .text(TRAY_UPDATES_ID, "Updates & Changelog")
    .separator()
    .text(TRAY_QUIT_ID, "Quit")
    .build()?;

  let mut tray_builder = TrayIconBuilder::with_id("main")
    .menu(&tray_menu)
    .show_menu_on_left_click(false)
    .on_menu_event(|app, event| match event.id().as_ref() {
      TRAY_SHOW_ID => focus_main_window(app),
      TRAY_SETTINGS_ID => {
        append_debug_log(app, "Tray menu: Settings clicked");
        if let Err(error) = ensure_settings_window(app) {
          append_debug_log(app, &format!("Settings window failed: {}", error));
        }
      }
      TRAY_STATUS_ID => {
        append_debug_log(app, "Tray menu: Status clicked");
        if let Err(error) = ensure_status_window(app) {
          append_debug_log(app, &format!("Status window failed: {}", error));
        }
      }
      TRAY_ABOUT_ID => {
        append_debug_log(app, "Tray menu: About clicked");
        if let Err(error) = ensure_about_window(app) {
          append_debug_log(app, &format!("About window failed: {}", error));
        }
      }
      TRAY_UPDATES_ID => {
        append_debug_log(app, "Tray menu: Updates & Changelog clicked");
        if let Err(e) = ensure_updater_window(app) {
          append_debug_log(app, &format!("Updater window failed: {}", e));
        }
      }
      TRAY_QUIT_ID => std::process::exit(0),
      _ => {}
    })
    .on_tray_icon_event(|tray, event| {
      if let TrayIconEvent::Click { position, .. } = event.clone() {
        set_last_tray_click_position(position.x, position.y);
      }

      if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
      } = event
      {
        toggle_main_window(tray.app_handle());
      }
    });

  if let Some(icon) = app.default_window_icon().cloned() {
    tray_builder = tray_builder.icon(icon);
  }

  tray_builder.build(app)?;
  Ok(())
}

fn main() {
  #[cfg(windows)]
  register_notification_app_id();

  tauri::Builder::default()
    .plugin(tauri_plugin_notification::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_opener::init())
    .setup(|app| {
      let app_handle = app.handle();
      reset_debug_log(app_handle);
      append_debug_log(app_handle, "RIGStats startup");

      let mut settings = load_settings(app_handle);
      let should_autofill_model =
        settings.model_name.trim().is_empty() || is_placeholder_model_name(settings.model_name.trim());
      if should_autofill_model {
        if let Some(model_name) = detect_model_name() {
          settings.model_name = model_name;
          let _ = persist_settings(app_handle, &settings);
        }
      }
      let startup_profile = settings.dashboard_profile.clone();
      let startup_always_on_top = settings.always_on_top;
      let startup_autostart_enabled = settings.autostart_enabled;
      let current_version = env!("CARGO_PKG_VERSION").to_string();
      let last_seen = settings.last_seen_version.clone();
      let should_show_changelog = !last_seen.is_empty() && last_seen != current_version;
      if last_seen != current_version {
        settings.last_seen_version = current_version;
        let _ = persist_settings(app_handle, &settings);
      }
      let system = System::new_all();
      let sysinfo_available = !system.cpus().is_empty() || system.total_memory() > 0;
      let ram_spec = detect_ram_spec();
      let ram_details = detect_ram_details();
      let gpu_vram_total_mb = detect_gpu_vram_total_mb();
      let disk_model_map = detect_disk_model_map();
      if disk_model_map.is_empty() {
        append_debug_log(app_handle, "disk_model_map: empty (WMI join returned no results)");
      } else {
        for (letter, model) in &disk_model_map {
          append_debug_log(app_handle, &format!("disk_model_map: {} => {}", letter, model));
        }
      }
      let ping_target = detect_ping_target();
      let system_brand = detect_system_brand();
      let mb_name = detect_motherboard_name();
      let wmi_available = match probe_wmi_status() {
        Ok(()) => true,
        Err(error) => {
          append_debug_log(app_handle, &format!("WMI dependency probe failed: {}", error));
          false
        }
      };

      // Shared state is stored behind Mutex because commands run concurrently.
      app.manage(AppState {
        disk_model_map,
        lhm_client: reqwest::Client::builder()
          .timeout(std::time::Duration::from_millis(800))
          .build()
          .unwrap_or_default(),
        settings: Mutex::new(settings),
        system: Mutex::new(system),
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
        mb_name,
        sysinfo_available,
        wmi_available,
        last_alert: Mutex::new(HashMap::new()),
        last_stats: Mutex::new(None),
      });

      if let Some(main) = app.get_webview_window("main") {
        // Place the dashboard on the preferred portrait monitor if present.
        let _ = pick_target_monitor(&main, &startup_profile);
        let _ = main.set_always_on_top(startup_always_on_top);
        let _ = main.show();
        let _ = main.set_focus();
        if should_show_changelog {
          let app_for_cl = app_handle.clone();
          tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let _ = ensure_updater_window(&app_for_cl);
          });
        }
      }

      // Fallback for cases where installer task did not launch LHM yet.
      ensure_lhm_running(app_handle);
      updater::spawn_background_check(app_handle);
      streamdeck::spawn_streamdeck_loop(app_handle);

      // Re-register only if the Run key is completely absent (e.g. after a
      // reinstall). If Windows Settings has disabled the entry (StartupApproved
      // byte = 0x03) we leave it alone — that is the user's intentional choice.
      if startup_autostart_enabled && !autostart::is_run_key_present() {
        match autostart::register_autostart() {
          Ok(()) => append_debug_log(app_handle, "autostart: re-registered missing entry"),
          Err(e) => append_debug_log(app_handle, &format!("autostart: startup re-register failed: {e}")),
        }
      }

      create_tray(app)?;

      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_settings,
      get_about_info,
      preview_opacity,
      preview_profile,
      preview_visible_panels,
      save_settings,
      close_window,
      start_window_drag,
      get_system_name,
      get_system_brand,
      get_cpu_info,
      get_gpu_info,
      get_stats,
      log_frontend_error,
      test_temp_alert,
      collect_diagnostics,
      get_changelog,
      check_for_update,
      install_update,
      open_updater_window,
      set_main_height
    ])
    .on_window_event(on_window_event)
    .build(tauri::generate_context!())
    .expect("error while running tauri application")
    .run(|_app_handle, event| {
      if let RunEvent::ExitRequested { api, .. } = event {
        api.prevent_exit();
      }
    });
}
