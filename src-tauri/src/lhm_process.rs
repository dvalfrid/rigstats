//! LibreHardwareMonitor process lifecycle management.
//!
//! Responsibilities:
//! - Check whether LHM's HTTP endpoint is reachable on port 8085.
//! - Start LHM via the installer-created scheduled task (preferred, no UAC).
//! - Fall back to direct process spawn from known install locations.
//! - Track connection state transitions and throttle repeated log messages.

use crate::debug::{append_debug_log, run_hidden_command, unix_now_secs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[cfg(windows)]
const LHM_TASK_NAMES: [&str; 3] = [
  "LibreHardwareMonitor",
  "RIGStats\\LibreHardwareMonitor",
  "RigStats\\LibreHardwareMonitor",
];

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
  use std::time::Duration;
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

/// Returns a short diagnosis string for the LHM task situation, used by the Status UI.
/// Possible values: "ok", "access_denied", "missing".
pub(crate) fn get_lhm_task_diagnosis(_app: &tauri::AppHandle) -> &'static str {
  #[cfg(windows)]
  {
    let mut any_access_denied = false;
    for task_name in LHM_TASK_NAMES {
      match run_hidden_command("schtasks", &["/Query", "/TN", task_name]) {
        Ok(out) if out.status.success() => return "ok",
        Ok(out) => {
          let stderr = String::from_utf8_lossy(&out.stderr).to_string();
          if stderr.contains("nekad") || stderr.contains("denied") || stderr.contains("Access") {
            any_access_denied = true;
          }
        }
        Err(_) => {}
      }
    }
    if any_access_denied {
      "access_denied"
    } else {
      "missing"
    }
  }
  #[cfg(not(windows))]
  {
    "missing"
  }
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
