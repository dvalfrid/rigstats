//! Sidecar pipe connection state tracking.
//!
//! Tracks whether the named pipe delivers data each stats tick
//! and logs connect/disconnect transitions (throttled to once per 30 s).

use crate::debug::{append_debug_log, unix_now_secs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static LHM_WAS_CONNECTED: AtomicBool = AtomicBool::new(true);
static LAST_LHM_OFFLINE_LOG_SECS: AtomicU64 = AtomicU64::new(0);

/// Called each stats tick to log sidecar pipe connect/disconnect transitions.
/// Throttles repeated "still offline" messages to once per 30 seconds.
pub(crate) fn track_lhm_connection_state(app: &tauri::AppHandle, connected: bool) {
  if connected {
    if !LHM_WAS_CONNECTED.swap(true, Ordering::Relaxed) {
      append_debug_log(app, "sidecar pipe connection restored");
    }
  } else {
    let was_connected = LHM_WAS_CONNECTED.swap(false, Ordering::Relaxed);
    if was_connected {
      append_debug_log(app, "sidecar pipe connection lost");
    }

    let now = unix_now_secs();
    let last = LAST_LHM_OFFLINE_LOG_SECS.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= 30 {
      LAST_LHM_OFFLINE_LOG_SECS.store(now, Ordering::Relaxed);
      append_debug_log(app, "sidecar pipe still offline");
    }
  }
}
