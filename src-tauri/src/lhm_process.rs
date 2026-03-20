//! LibreHardwareMonitor process lifecycle management.
//!
//! Responsibilities:
//! - Check whether LHM's HTTP endpoint is reachable on port 8085.
//! - Start LHM via the installer-created scheduled task (preferred, no UAC).
//! - Fall back to direct process spawn from known install locations.
//! - Track connection state transitions and throttle repeated log messages.

use crate::debug::{append_debug_log, run_hidden_command, unix_now_secs, CREATE_NO_WINDOW};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const LHM_TASK_NAMES: [&str; 3] = ["LibreHardwareMonitor", "RIGStats\\LibreHardwareMonitor", "RigStats\\LibreHardwareMonitor"];

/// Tracks whether the last `get_stats` tick had a live LHM connection.
/// Used to log connect/disconnect transitions exactly once.
static LHM_WAS_CONNECTED: AtomicBool = AtomicBool::new(true);

/// Unix timestamp of the last "LHM still offline" log message.
/// Limits repeated offline log spam to one entry per 30-second window.
static LAST_LHM_OFFLINE_LOG_SECS: AtomicU64 = AtomicU64::new(0);

// --- Endpoint reachability -------------------------------------------------

/// Returns `true` if LHM's HTTP server is accepting connections on port 8085.
#[cfg(windows)]
pub(crate) fn can_reach_lhm_endpoint() -> bool {
  use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
  let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8085);
  TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_ok()
}

// --- Scheduled task helpers ------------------------------------------------

fn task_field(output: &str, key: &str) -> Option<String> {
  output.lines().find_map(|line| {
    let trimmed = line.trim();
    if !trimmed.starts_with(key) {
      return None;
    }
    trimmed
      .split_once(':')
      .map(|(_, value)| value.trim().to_string())
      .filter(|value| !value.is_empty())
  })
}

/// Queries the Windows Task Scheduler for LHM task metadata.
/// Returns `(task_name, status, last_result, task_to_run)`.
pub(crate) fn get_lhm_task_details(
  app: &tauri::AppHandle,
) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
  #[cfg(windows)]
  {
    for task_name in LHM_TASK_NAMES {
      match run_hidden_command("schtasks", &["/Query", "/TN", task_name, "/V", "/FO", "LIST"]) {
        Ok(out) if out.status.success() => {
          let text = String::from_utf8_lossy(&out.stdout).to_string();
          return (
            task_field(&text, "TaskName"),
            task_field(&text, "Status"),
            task_field(&text, "Last Result"),
            task_field(&text, "Task To Run"),
          );
        }
        Ok(_) => continue,
        Err(e) => {
          append_debug_log(app, &format!("Failed to inspect LHM task {}: {}", task_name, e));
        }
      }
    }
  }

  (None, None, None, None)
}

#[cfg(windows)]
fn lhm_task_exists(app: &tauri::AppHandle) -> bool {
  for task_name in LHM_TASK_NAMES {
    match run_hidden_command("schtasks", &["/Query", "/TN", task_name]) {
      Ok(out) => {
        if out.status.success() {
          append_debug_log(app, &format!("LHM task exists: {}", task_name));
          return true;
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        append_debug_log(app, &format!("LHM task query failed ({}): {}", task_name, stderr));
      }
      Err(e) => {
        append_debug_log(app, &format!("LHM task query error ({}): {}", task_name, e));
      }
    }
  }
  false
}

#[cfg(windows)]
fn try_run_lhm_task(app: &tauri::AppHandle) -> bool {
  for task_name in LHM_TASK_NAMES {
    let output = run_hidden_command("schtasks", &["/Run", "/TN", task_name]);
    match output {
      Ok(out) => {
        if out.status.success() {
          append_debug_log(app, &format!("LHM task run command succeeded: {}", task_name));
          return true;
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        append_debug_log(app, &format!("LHM task run command failed ({}): {}", task_name, stderr));
      }
      Err(e) => {
        append_debug_log(app, &format!("LHM task run command error ({}): {}", task_name, e));
      }
    }
  }
  false
}

// --- Direct process spawn --------------------------------------------------

#[cfg(windows)]
fn candidate_lhm_paths(app: &tauri::AppHandle) -> Vec<PathBuf> {
  use tauri::Manager;

  let mut paths = Vec::new();

  if let Ok(resource_dir) = app.path().resource_dir() {
    paths.push(resource_dir.join("lhm").join("LibreHardwareMonitor.exe"));
    if let Some(parent) = resource_dir.parent() {
      paths.push(parent.join("lhm").join("LibreHardwareMonitor.exe"));
    }
  }

  if let Ok(current_exe) = std::env::current_exe() {
    if let Some(exe_dir) = current_exe.parent() {
      paths.push(exe_dir.join("lhm").join("LibreHardwareMonitor.exe"));
      paths.push(exe_dir.join("resources").join("lhm").join("LibreHardwareMonitor.exe"));
    }
  }

  if let Ok(program_files) = std::env::var("ProgramFiles") {
    paths.push(PathBuf::from(&program_files).join("RIGStats").join("lhm").join("LibreHardwareMonitor.exe"));
    paths.push(PathBuf::from(&program_files).join("RigStats").join("lhm").join("LibreHardwareMonitor.exe"));
    paths.push(PathBuf::from(program_files).join("LibreHardwareMonitor").join("LibreHardwareMonitor.exe"));
  }

  if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
    paths.push(PathBuf::from(program_files_x86).join("LibreHardwareMonitor").join("LibreHardwareMonitor.exe"));
  }

  if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
    paths.push(
      PathBuf::from(local_app_data)
        .join("Programs")
        .join("LibreHardwareMonitor")
        .join("LibreHardwareMonitor.exe"),
    );
  }

  paths
}

#[cfg(windows)]
fn spawn_lhm(exe_path: &Path) -> std::io::Result<()> {
  let mut command = std::process::Command::new(exe_path);
  command.creation_flags(CREATE_NO_WINDOW);
  command.spawn().map(|_| ())
}

// --- Public entry point ----------------------------------------------------

/// Ensures LHM is running. Called at startup as a fallback for cases where
/// the installer task did not launch LHM automatically.
///
/// Strategy (in order):
/// 1. If endpoint is already reachable, do nothing.
/// 2. Try to trigger the installer's scheduled task (no UAC required).
/// 3. Fall back to direct process spawn from known paths.
pub fn ensure_lhm_running(app: &tauri::AppHandle) {
  #[cfg(windows)]
  {
    append_debug_log(app, &format!("LHM ensure start"));

    if can_reach_lhm_endpoint() {
      append_debug_log(app, "LHM endpoint already reachable on :8085");
      return;
    }

    if try_run_lhm_task(app) {
      std::thread::sleep(Duration::from_millis(1200));
      if can_reach_lhm_endpoint() {
        append_debug_log(app, "LHM reachable after task run");
        return;
      }
      append_debug_log(app, "Task run succeeded but endpoint still unavailable");
    } else if !lhm_task_exists(app) {
      append_debug_log(app, "LHM task missing. Reinstall RIGStats as administrator to recreate task.");
    }

    let mut elevation_required = false;

    for path in candidate_lhm_paths(app) {
      if !path.is_file() {
        append_debug_log(app, &format!("LHM candidate missing: {}", path.display()));
        continue;
      }

      append_debug_log(app, &format!("LHM candidate found: {}", path.display()));
      match spawn_lhm(&path) {
        Ok(()) => {
          append_debug_log(app, &format!("LHM spawned from {}", path.display()));
          return;
        }
        Err(e) => {
          if e.raw_os_error() == Some(740) {
            elevation_required = true;
          }
          append_debug_log(app, &format!("LHM spawn failed from {}: {}", path.display(), e));
        }
      }
    }

    if elevation_required {
      append_debug_log(
        app,
        "Elevation required for direct LHM launch. App will not trigger UAC; using scheduled task only.",
      );
    }

    append_debug_log(app, "LHM ensure finished without successful spawn");
  }
}

// --- Connection state tracking ---------------------------------------------

/// Called each stats tick to log LHM connect/disconnect transitions.
/// Throttles repeated "still offline" messages to once per 30 seconds.
pub(crate) fn track_lhm_connection_state(app: &tauri::AppHandle, connected: bool) {
  if connected {
    if !LHM_WAS_CONNECTED.swap(true, Ordering::Relaxed) {
      append_debug_log(app, "LHM connection restored (data.json reachable)");
    }
  } else {
    let was_connected = LHM_WAS_CONNECTED.swap(false, Ordering::Relaxed);
    if was_connected {
      append_debug_log(app, "LHM connection lost (data.json unavailable)");
    }

    let now = unix_now_secs();
    let last = LAST_LHM_OFFLINE_LOG_SECS.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= 30 {
      LAST_LHM_OFFLINE_LOG_SECS.store(now, Ordering::Relaxed);
      append_debug_log(app, "LHM still offline after retry window");
    }
  }
}
