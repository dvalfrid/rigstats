//! Update checking and installation via tauri-plugin-updater.
//!
//! On startup a background task checks for a new release after a short delay.
//! If a newer version is available the event `update-available` is emitted so
//! the renderer can show a badge. The user can then open the updater window to
//! review the changelog and confirm the download.

#![allow(clippy::needless_pass_by_value)]

use crate::debug::append_debug_log;
use crate::windows::ensure_updater_window;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
  pub current_version: String,
  pub version: String,
  pub body: Option<String>,
}

const CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60; // 6 hours

/// Spawns a background task that checks for updates on startup and then every
/// 6 hours of active runtime. Using a short loop interval means the check also
/// fires within a few hours after the computer wakes from sleep.
pub fn spawn_background_check(app: &AppHandle) {
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    // Short initial delay so startup is not slowed down.
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    loop {
      match check_update_inner(&app).await {
        Ok(Some(ref info)) => {
          append_debug_log(&app, &format!("Update available: v{}", info.version));
          let _ = app.emit("update-available", &info.version);
        }
        Ok(None) => {}
        Err(ref e) => {
          append_debug_log(&app, &format!("Update check failed: {}", e));
        }
      }
      tokio::time::sleep(std::time::Duration::from_secs(CHECK_INTERVAL_SECS)).await;
    }
  });
}

async fn check_update_inner(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
  let updater = app.updater().map_err(|e| e.to_string())?;
  let update = updater.check().await.map_err(|e| e.to_string())?;
  Ok(update.map(|u| UpdateInfo {
    current_version: u.current_version.to_string(),
    version: u.version.to_string(),
    body: u.body,
  }))
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
  check_update_inner(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
  append_debug_log(&app, "install_update: checking for update");
  let updater = app.updater().map_err(|e| {
    let msg = format!("install_update: updater init failed: {}", e);
    append_debug_log(&app, &msg);
    e.to_string()
  })?;

  let update = updater.check().await.map_err(|e| {
    let msg = format!("install_update: check failed: {}", e);
    append_debug_log(&app, &msg);
    e.to_string()
  })?;

  let Some(update) = update else {
    let msg = "install_update: no update found (version is current)";
    append_debug_log(&app, msg);
    return Err("No update is available. You may already be on the latest version.".to_string());
  };

  append_debug_log(&app, &format!("install_update: downloading v{}", update.version));

  let app_for_progress = app.clone();
  let app_for_log = app.clone();
  let mut downloaded = 0usize;

  update
    .download_and_install(
      move |chunk_length, content_length| {
        downloaded += chunk_length;
        let _ = app_for_progress.emit(
          "update-progress",
          serde_json::json!({
            "downloaded": downloaded,
            "total": content_length
          }),
        );
      },
      move || {
        append_debug_log(&app_for_log, "install_update: download complete, launching installer");
        // Notify the frontend before the process exits so the user knows to
        // look for a Windows UAC prompt (required for perMachine NSIS install).
        let _ = app_for_log.emit("update-download-complete", ());
      },
    )
    .await
    .map_err(|e| {
      let msg = format!("install_update: download_and_install failed: {}", e);
      append_debug_log(&app, &msg);
      e.to_string()
    })
}

#[tauri::command]
pub fn open_updater_window(app: AppHandle) -> Result<(), String> {
  ensure_updater_window(&app)
}
