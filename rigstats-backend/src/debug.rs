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

/// Truncates the debug log at startup so each session starts with a clean file.
pub fn reset_debug_log(dir: &Path) {
  let path = debug_log_path(dir);
  if let Some(parent) = path.parent() {
    let _ = create_dir_all(parent);
  }
  let _ = OpenOptions::new().create(true).write(true).truncate(true).open(path);
}

pub fn append_debug_log(dir: &Path, message: &str) {
  let path = debug_log_path(dir);
  if let Some(parent) = path.parent() {
    let _ = create_dir_all(parent);
  }
  if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
    let _ = writeln!(file, "[{}] {}", unix_now_secs(), message);
  }
}

pub fn read_debug_log_tail(dir: &Path, line_limit: usize) -> String {
  let path = debug_log_path(dir);
  let content = std::fs::read_to_string(path).unwrap_or_default();
  let lines = content.lines().collect::<Vec<_>>();
  let start = lines.len().saturating_sub(line_limit);
  lines[start..].join("\n")
}
