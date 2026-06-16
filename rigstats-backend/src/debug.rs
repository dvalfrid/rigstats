//! Debug logging and low-level process-spawning primitives.
//!
//! Imported by all other modules — keep this file free of dependencies on
//! other crate modules to avoid circular imports.

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Windows flag that suppresses the console window for spawned child processes.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Runs a subprocess and captures its output without showing a console window.
pub fn run_hidden_command(program: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
  let mut command = Command::new(program);
  command.args(args);
  #[cfg(windows)]
  {
    command.creation_flags(CREATE_NO_WINDOW);
  }
  command.output()
}

pub fn unix_now_secs() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_secs())
    .unwrap_or(0)
}

pub fn debug_log_path(dir: &Path) -> PathBuf {
  dir.join("rigstats-debug.log")
}

/// Rotates the debug log at startup so crash data from the previous session is preserved.
///
/// The existing `rigstats-debug.log` is renamed to `rigstats-debug-prev.log` before
/// a fresh log is created, so a crash in the previous run does not wipe its own evidence.
pub fn reset_debug_log(dir: &Path) {
  let path = debug_log_path(dir);
  let prev_path = dir.join("rigstats-debug-prev.log");
  if let Some(parent) = path.parent() {
    let _ = create_dir_all(parent);
  }
  // Rename current log to prev (silently ignore if it doesn't exist yet).
  if path.exists() {
    let _ = std::fs::rename(&path, &prev_path);
  }
  let _ = OpenOptions::new().create(true).write(true).truncate(true).open(path);
}

/// Severity of a debug-log line. Rendered as a `[LEVEL]` tag so the Status
/// window's debug log is scannable at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
  /// Verbose detail useful only when diagnosing (e.g. hardware-detection dumps).
  Debug,
  /// Normal lifecycle events (startup, shutdown, connections established).
  Info,
  /// Recoverable problems — connectivity loss, transient probe failures.
  Warning,
  /// Failures that lose data or fall back to defaults.
  Error,
}

impl LogLevel {
  pub fn as_str(self) -> &'static str {
    match self {
      LogLevel::Debug => "DEBUG",
      LogLevel::Info => "INFO",
      LogLevel::Warning => "WARNING",
      LogLevel::Error => "ERROR",
    }
  }
}

/// Formats a Unix timestamp as local `YYYY-MM-DD HH:MM:SS`, falling back to the
/// raw seconds if the conversion ever fails (it should not).
fn fmt_local_timestamp(secs: u64) -> String {
  use chrono::{Local, TimeZone};
  match Local.timestamp_opt(secs as i64, 0) {
    chrono::offset::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
    _ => secs.to_string(),
  }
}

/// Appends a line to the debug log with a human-readable local timestamp and a
/// severity tag, e.g. `[2026-06-16 16:18:33] [WARNING] pipe: read timed out`.
pub fn append_debug_log_lvl(dir: &Path, level: LogLevel, message: &str) {
  let path = debug_log_path(dir);
  if let Some(parent) = path.parent() {
    let _ = create_dir_all(parent);
  }
  if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
    // Pad the bracketed tag to the width of the longest level ("[WARNING]")
    // so message text lines up in the monospace Status view.
    let tag = format!("[{}]", level.as_str());
    let _ = writeln!(file, "[{}] {tag:<9} {message}", fmt_local_timestamp(unix_now_secs()));
  }
}

/// Logs at [INFO] — the default level for normal lifecycle events.
pub fn append_debug_log(dir: &Path, message: &str) {
  append_debug_log_lvl(dir, LogLevel::Info, message);
}

/// Logs at [DEBUG] — verbose diagnostic detail.
pub fn log_debug(dir: &Path, message: &str) {
  append_debug_log_lvl(dir, LogLevel::Debug, message);
}

/// Logs at [WARNING] — recoverable problems.
pub fn log_warn(dir: &Path, message: &str) {
  append_debug_log_lvl(dir, LogLevel::Warning, message);
}

/// Logs at [ERROR] — failures that lose data or fall back to defaults.
pub fn log_error(dir: &Path, message: &str) {
  append_debug_log_lvl(dir, LogLevel::Error, message);
}

pub fn read_debug_log_tail(dir: &Path, line_limit: usize) -> String {
  let path = debug_log_path(dir);
  let content = std::fs::read_to_string(path).unwrap_or_default();
  let lines = content.lines().collect::<Vec<_>>();
  let start = lines.len().saturating_sub(line_limit);
  lines[start..].join("\n")
}
