//! Per-user Windows autostart via the registry Run key.
//!
//! Uses the `winreg` crate for direct registry access — no subprocesses are
//! spawned so there is no risk of the operation hanging.
//!
//! Writes/removes `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run\RigStats`.
//!
//! Also manages `StartupApproved\Run\RigStats` so the toggle stays in sync
//! with the Windows "Apps > Startup" page, which marks disabled entries with
//! a REG_BINARY value whose first byte is `0x03`.

use std::io;
use winreg::{
  enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE},
  RegKey,
};

const RUN_PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const VALUE_NAME: &str = "RigStats";

fn hkcu() -> RegKey {
  RegKey::predef(HKEY_CURRENT_USER)
}

/// Adds (or updates) the autostart registry value pointing at the current exe,
/// and removes any StartupApproved entry that Windows may have set to disabled.
pub fn register_autostart() -> Result<(), String> {
  let exe = std::env::current_exe()
    .map_err(|e| format!("Cannot resolve exe path: {e}"))?
    .to_string_lossy()
    .to_string();

  let value = format!("\"{}\"", exe);

  let run = hkcu()
    .open_subkey_with_flags(RUN_PATH, KEY_WRITE)
    .map_err(|e| format!("Cannot open Run key for writing: {e}"))?;
  run
    .set_value(VALUE_NAME, &value)
    .map_err(|e| format!("Cannot write Run value: {e}"))?;

  // Remove StartupApproved entry — absence means "enabled" to Windows.
  if let Ok(approved) = hkcu().open_subkey_with_flags(APPROVED_PATH, KEY_WRITE) {
    let _ = approved.delete_value(VALUE_NAME); // ignore "not found"
  }

  Ok(())
}

/// Removes the autostart registry value and any StartupApproved entry.
/// Succeeds silently if the values are already absent.
pub fn unregister_autostart() -> Result<(), String> {
  let run = hkcu()
    .open_subkey_with_flags(RUN_PATH, KEY_WRITE)
    .map_err(|e| format!("Cannot open Run key for writing: {e}"))?;

  match run.delete_value(VALUE_NAME) {
    Ok(()) => {}
    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
    Err(e) => return Err(format!("Cannot delete Run value: {e}")),
  }

  // Clean up StartupApproved (best-effort).
  if let Ok(approved) = hkcu().open_subkey_with_flags(APPROVED_PATH, KEY_WRITE) {
    let _ = approved.delete_value(VALUE_NAME);
  }

  Ok(())
}

/// Returns `true` if the Run key value exists, regardless of StartupApproved.
/// Used at startup to decide whether to re-register a missing entry, without
/// overriding a Windows Settings > Startup disable.
pub fn is_run_key_present() -> bool {
  hkcu()
    .open_subkey_with_flags(RUN_PATH, KEY_READ)
    .ok()
    .and_then(|key| key.get_value::<String, _>(VALUE_NAME).ok())
    .is_some()
}

/// Returns `true` only if the autostart entry exists **and** has not been
/// disabled via the Windows "Apps > Startup" page or Task Manager.
///
/// Windows marks a disabled entry in `StartupApproved\Run` with a REG_BINARY
/// value whose first byte is `0x03`. Absent or `0x02` = enabled.
///
/// The `debug_log` callback receives diagnostic lines so callers can write
/// them to the application log without this module depending on AppHandle.
pub fn is_autostart_registered_with_log(mut debug_log: impl FnMut(&str)) -> bool {
  let hkcu = hkcu();

  // 1. The Run value must exist.
  let run_present = hkcu
    .open_subkey_with_flags(RUN_PATH, KEY_READ)
    .ok()
    .and_then(|key| key.get_value::<String, _>(VALUE_NAME).ok())
    .is_some();

  debug_log(&format!("autostart: Run key present = {run_present}"));
  if !run_present {
    return false;
  }

  // 2. Check StartupApproved — first byte 0x03 means Windows has disabled it.
  match hkcu.open_subkey_with_flags(APPROVED_PATH, KEY_READ) {
    Err(_) => debug_log("autostart: StartupApproved key absent (= enabled)"),
    Ok(approved) => match approved.get_raw_value(VALUE_NAME) {
      Err(_) => debug_log("autostart: StartupApproved entry absent (= enabled)"),
      Ok(raw) => {
        let first = raw.bytes.first().copied().unwrap_or(0x02);
        debug_log(&format!("autostart: StartupApproved first byte = 0x{first:02x}"));
        // 0x02 = explicitly enabled; absent = default enabled.
        // Any other byte (0x00, 0x01, 0x03, …) is some form of disabled.
        if first != 0x02 {
          debug_log("autostart: disabled via Windows Settings");
          return false;
        }
      }
    },
  }

  true
}
