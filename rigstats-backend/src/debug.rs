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
    let _ = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path);
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
        let _ = writeln!(
            file,
            "[{}] {tag:<9} {message}",
            fmt_local_timestamp(unix_now_secs())
        );
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

/// Writes every panic — any thread — to the debug log with its thread,
/// file:line, message and backtrace, then runs the default hook (#219). A
/// GUI process has no console, so without this a panic leaves no trace but
/// an "Application Error" event.
pub fn install_panic_logger(dir: &Path) {
    let dir = dir.to_path_buf();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_owned());
        let location = info
            .location()
            .map_or_else(|| "unknown location".to_owned(), ToString::to_string);
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        // Enough frames to find the cause, not the whole runtime stack.
        let backtrace = backtrace.lines().take(60).collect::<Vec<_>>().join("\n");
        log_error(
            &dir,
            &format!(
                "PANIC in thread '{}' at {location}: {message}\n{backtrace}",
                thread.name().unwrap_or("unnamed")
            ),
        );
        previous(info);
    }));
}

/// Runs the task `make` builds, and builds and runs it again after a panic
/// (the panic itself is logged by [`install_panic_logger`]) — a long-lived
/// task such as the poll loop or the control pipe that died would otherwise
/// leave the dashboard frozen or the Control Center disconnected until a
/// restart, with nothing saying why (#219). Waits 2 s before restarting,
/// doubling up to 60 s while it keeps panicking. Ends when the task ends.
pub async fn supervise<F, Fut>(dir: PathBuf, name: &'static str, make: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    supervise_after(dir, name, make, std::time::Duration::from_secs(2)).await;
}

async fn supervise_after<F, Fut>(
    dir: PathBuf,
    name: &'static str,
    mut make: F,
    first_delay: std::time::Duration,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let mut delay = first_delay;
    loop {
        match tokio::spawn(make()).await {
            Err(e) if e.is_panic() => {
                log_error(
                    &dir,
                    &format!(
                        "{name} stopped by a panic — restarting it in {:.1} s",
                        delay.as_secs_f64()
                    ),
                );
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(std::time::Duration::from_secs(60));
            }
            _ => return,
        }
    }
}

pub fn read_debug_log_tail(dir: &Path, line_limit: usize) -> String {
    let path = debug_log_path(dir);
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let lines = content.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(line_limit);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn supervise_restarts_a_task_that_panicked_and_logs_it() {
        let dir = std::env::temp_dir().join(format!("rigstats-supervise-{}", std::process::id()));
        let runs = Arc::new(AtomicU32::new(0));
        let counted = runs.clone();
        supervise_after(
            dir.clone(),
            "Test task",
            move || {
                let counted = counted.clone();
                async move {
                    // Panics the first time, ends normally the second.
                    if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                        panic!("boom");
                    }
                }
            },
            std::time::Duration::from_millis(10),
        )
        .await;
        assert_eq!(runs.load(Ordering::SeqCst), 2);
        let log = read_debug_log_tail(&dir, 20);
        assert!(log.contains("Test task stopped by a panic"), "{log}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
