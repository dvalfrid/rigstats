//! Debug logging and low-level process-spawning primitives.
//!
//! Imported by all other modules — keep this file free of dependencies on
//! other crate modules to avoid circular imports.

use std::fs::{create_dir_all, File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    let _ = create_dir_all(dir);
    let _lock = DebugLogLock::acquire(dir);
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

/// The debug log is cut once it grows past this while the app runs (#563).
/// A normal session writes a few KB, so only something logging over and
/// over gets here — one 150-byte line a second is ~13 MB a day.
const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// What a cut keeps: the newest 256 KB, ~1 700 lines — hours of a noisy
/// line, and far more than the Status window shows.
const KEEP_LOG_BYTES: usize = 256 * 1024;

/// Serialises writes to the debug log across the two processes that append
/// to it (`rigstats` and `rigstats-wallpaper`), so a cut can't drop a line
/// the other process appends meanwhile. An OS lock on a separate file
/// (`File::try_lock`), not on the log itself: readers (Status window,
/// diagnostics export) aren't blocked, and Windows releases it when a
/// process dies, so a crash can't leave it held.
struct DebugLogLock(Option<File>);

impl DebugLogLock {
    /// Waits up to 250 ms; a log line takes microseconds, so only a stalled
    /// disk gets there. Then the caller goes on without the lock (and skips
    /// the cut) rather than hang — it may be the UI thread.
    fn acquire(dir: &Path) -> Self {
        let Ok(file) = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("rigstats-debug.lock"))
        else {
            return Self(None);
        };
        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            match file.try_lock() {
                Ok(()) => return Self(Some(file)),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(_) => return Self(None),
            }
        }
    }

    fn held(&self) -> bool {
        self.0.is_some()
    }
}

/// Appends `line` and, holding the lock, cuts the file to its newest `keep`
/// bytes once it is over `max` — checked from the open handle's metadata,
/// not by reading the file.
fn append_bounded(dir: &Path, line: &str, max: u64, keep: usize) {
    let path = debug_log_path(dir);
    let _ = create_dir_all(dir);
    let lock = DebugLogLock::acquire(dir);
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let _ = writeln!(file, "{line}");
    let over = file.metadata().is_ok_and(|m| m.len() > max);
    drop(file);
    if over && lock.held() {
        cut_to_tail(&path, keep);
    }
}

/// Rewrites the log as its last `keep` bytes, starting at a whole line, after
/// a line saying it was cut. Through a temp file, so a crash mid-cut can't
/// leave the log empty. A failed rename (a reader holding the file without
/// delete sharing) is simply tried again at the next line.
fn cut_to_tail(path: &Path, keep: usize) {
    let Ok(content) = std::fs::read(path) else {
        return;
    };
    let from = content.len().saturating_sub(keep);
    let start = content[from..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(content.len(), |i| from + i + 1);
    let notice = format_line(
        LogLevel::Info,
        &format!(
            "debug log: over {} KB — older lines cut, the newest {} KB kept (#563)",
            content.len() / 1024,
            (content.len() - start) / 1024
        ),
    );
    let temp = path.with_extension("log.tmp");
    let written = (|| {
        let mut out = File::create(&temp)?;
        writeln!(out, "{notice}")?;
        out.write_all(&content[start..])?;
        out.sync_all()
    })();
    if written.is_err() || std::fs::rename(&temp, path).is_err() {
        let _ = std::fs::remove_file(&temp);
    }
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
pub fn fmt_local_timestamp(secs: u64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(secs as i64, 0) {
        chrono::offset::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => secs.to_string(),
    }
}

fn format_line(level: LogLevel, message: &str) -> String {
    // Pad the bracketed tag to the width of the longest level ("[WARNING]")
    // so message text lines up in the monospace Status view.
    let tag = format!("[{}]", level.as_str());
    format!(
        "[{}] {tag:<9} {message}",
        fmt_local_timestamp(unix_now_secs())
    )
}

/// Appends a line to the debug log with a human-readable local timestamp and a
/// severity tag, e.g. `[2026-06-16 16:18:33] [WARNING] pipe: read timed out`.
/// Keeps the file under [`MAX_LOG_BYTES`] while the app runs (#563).
pub fn append_debug_log_lvl(dir: &Path, level: LogLevel, message: &str) {
    append_bounded(
        dir,
        &format_line(level, message),
        MAX_LOG_BYTES,
        KEEP_LOG_BYTES,
    );
}

/// At most `max` lines of one kind per `window_secs` (#563): a value that
/// flaps every second would otherwise write a line a second for as long as it
/// flaps. A window starts at the first line after the previous one ended, so
/// a change after a quiet spell is always logged.
#[derive(Debug, Clone)]
pub struct LineLimit {
    max: u32,
    window_secs: u64,
    window_start: Option<u64>,
    count: u32,
}

/// What [`LineLimit::check`] says to do with a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineGate {
    Write,
    /// The first line over the limit: instead of it, say that changes keep
    /// coming and that none are logged until `until` (Unix seconds).
    MuteUntil(u64),
    Mute,
}

impl LineLimit {
    pub const fn new(max: u32, window_secs: u64) -> Self {
        Self {
            max,
            window_secs,
            window_start: None,
            count: 0,
        }
    }

    /// Counts a line at `now` (Unix seconds).
    pub fn check(&mut self, now: u64) -> LineGate {
        let start = match self.window_start {
            Some(start) if now < start.saturating_add(self.window_secs) => start,
            _ => {
                self.window_start = Some(now);
                self.count = 0;
                now
            }
        };
        self.count = self.count.saturating_add(1);
        if self.count <= self.max {
            LineGate::Write
        } else if self.count == self.max + 1 {
            LineGate::MuteUntil(start.saturating_add(self.window_secs))
        } else {
            LineGate::Mute
        }
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

    #[test]
    fn a_line_limit_lets_max_through_then_says_so_once_until_the_hour_ends() {
        let mut limit = LineLimit::new(10, 3600);
        for s in 0..10 {
            assert_eq!(limit.check(1_000 + s), LineGate::Write);
        }
        assert_eq!(limit.check(1_010), LineGate::MuteUntil(4_600));
        assert_eq!(limit.check(1_011), LineGate::Mute);
        assert_eq!(limit.check(4_599), LineGate::Mute);
        // A new hour: logged again, and the window starts there.
        assert_eq!(limit.check(4_600), LineGate::Write);
        for s in 1..10 {
            assert_eq!(limit.check(4_600 + s), LineGate::Write);
        }
        assert_eq!(limit.check(5_000), LineGate::MuteUntil(8_200));
    }

    #[test]
    fn a_change_after_a_quiet_spell_is_always_logged() {
        let mut limit = LineLimit::new(10, 3600);
        assert_eq!(limit.check(0), LineGate::Write);
        assert_eq!(limit.check(10_000), LineGate::Write);
    }

    #[test]
    fn the_log_is_cut_to_its_newest_whole_lines_once_over_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..200 {
            append_bounded(
                dir.path(),
                &format!("line {i:04} {}", "x".repeat(40)),
                4096,
                1024,
            );
        }
        let log = std::fs::read_to_string(debug_log_path(dir.path())).unwrap();
        assert!(log.len() <= 4096 + 60, "{}", log.len());
        let lines: Vec<&str> = log.lines().collect();
        // Every line whole: the notice of the last cut, or one written above.
        assert!(lines
            .iter()
            .all(|l| l.contains("older lines cut") || (l.starts_with("line ") && l.len() == 50)));
        assert!(lines.iter().any(|l| l.contains("older lines cut")));
        assert!(lines.last().unwrap().starts_with("line 0199 "), "{log}");
        assert!(!log.contains("line 0000 "));
        assert!(!dir.path().join("rigstats-debug.log.tmp").exists());
    }

    #[test]
    fn writers_on_several_handles_lose_no_line_after_the_last_cut() {
        // Each thread opens its own handles, as the two processes do — the
        // OS lock is per handle, so this is the cross-process case.
        let dir = tempfile::tempdir().unwrap();
        let threads: Vec<_> = (0..4)
            .map(|t| {
                let dir = dir.path().to_path_buf();
                std::thread::spawn(move || {
                    for i in 0..300 {
                        append_bounded(
                            &dir,
                            &format!("t{t} {i:04} {}", "x".repeat(40)),
                            8192,
                            4096,
                        );
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let log = std::fs::read_to_string(debug_log_path(dir.path())).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert!(lines
            .iter()
            .all(|l| l.contains("older lines cut") || (l.starts_with('t') && l.len() == 48)));
        // A cut keeps the newest part, so per thread what is left must be
        // its newest lines without a gap — a line lost to a race with a
        // cut would leave one — ending at its last.
        let mut kept = 0;
        for t in 0..4 {
            let ours: Vec<u32> = lines
                .iter()
                .filter(|l| l.starts_with(&format!("t{t} ")))
                .map(|l| l[3..7].parse().unwrap())
                .collect();
            assert!(ours.windows(2).all(|w| w[1] == w[0] + 1), "{log}");
            assert!(ours.last().is_none_or(|&n| n == 299), "{log}");
            kept += ours.len();
        }
        assert!(kept > 0 && log.len() <= 8192 + 60, "{}", log.len());
    }
}
