//! Stats logging — appends a CSV row on every tick when enabled.
//! Log files roll daily: `<app-data>/rigstats-log-YYYY-MM-DD.csv`.

use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::stats::StatsPayload;

const CSV_HEADER: &str = "timestamp_unix,cpu_load,cpu_temp,cpu_freq_mhz,gpu_load,gpu_temp,\
gpu_vram_used_mb,ram_used_gb,disk_read_mbs,disk_write_mbs,net_up_mbps,net_down_mbps,ping_ms\n";

pub fn unix_now_secs() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_secs())
    .unwrap_or(0)
}

/// Converts a Unix timestamp in seconds to `(year, month, day)`.
///
/// Uses Howard Hinnant's civil_from_days algorithm; accurate from 1970 to 2200.
fn ymd_from_unix(secs: u64) -> (u32, u32, u32) {
  let days = (secs / 86400) as i64;
  let z = days + 719_468;
  let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
  let doe = z - era * 146_097;
  let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
  let y = yoe + era * 400;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let d = doy - (153 * mp + 2) / 5 + 1;
  let m = if mp < 10 { mp + 3 } else { mp - 9 };
  let y = if m <= 2 { y + 1 } else { y };
  (y as u32, m as u32, d as u32)
}

/// Returns the log file path for the given Unix timestamp: `<dir>/rigstats-log-YYYY-MM-DD.csv`.
pub fn current_log_path(dir: &Path, secs: u64) -> PathBuf {
  let (y, m, d) = ymd_from_unix(secs);
  dir.join(format!("rigstats-log-{y:04}-{m:02}-{d:02}.csv"))
}

fn fmt_opt(v: Option<f64>, precision: usize) -> String {
  v.map_or_else(String::new, |f| format!("{:.prec$}", f, prec = precision))
}

/// Appends one CSV row to today's log file, writing a header row if the file is new.
pub fn append_stats_row(payload: &StatsPayload, dir: &Path) -> std::io::Result<()> {
  let now = unix_now_secs();
  let path = current_log_path(dir, now);
  let is_new = !path.exists();
  if is_new {
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent)?;
    }
  }
  let file = OpenOptions::new().create(true).append(true).open(&path)?;
  let mut w = BufWriter::new(file);
  if is_new {
    w.write_all(CSV_HEADER.as_bytes())?;
  }
  let ram_used_gb = payload.ram.used as f64 / 1_073_741_824.0;
  let cpu_temp = fmt_opt(payload.cpu.temp, 1);
  let gpu_load = fmt_opt(payload.gpu.load, 1);
  let gpu_temp = fmt_opt(payload.gpu.temp, 1);
  let gpu_vram = fmt_opt(payload.gpu.vram_used, 0);
  let ping_ms = fmt_opt(payload.net.ping_ms, 1);
  writeln!(
    w,
    "{},{},{},{:.1},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{}",
    now,
    payload.cpu.load,
    cpu_temp,
    payload.cpu.freq,
    gpu_load,
    gpu_temp,
    gpu_vram,
    ram_used_gb,
    payload.disk.read,
    payload.disk.write,
    payload.net.up,
    payload.net.down,
    ping_ms,
  )
}

/// Deletes log files in `dir` older than `days` days, based on modification time.
/// Silently ignores entries it cannot stat; logs entries it fails to delete.
/// Silently returns if `dir` does not exist.
pub fn prune_old_logs(dir: &Path, days: u32) {
  let now_secs = unix_now_secs();
  let cutoff_secs = days as u64 * 86400;
  let Ok(entries) = fs::read_dir(dir) else {
    return;
  };
  for entry in entries.flatten() {
    let path = entry.path();
    let is_log = path
      .file_name()
      .and_then(|n| n.to_str())
      .map(|name| name.starts_with("rigstats-log-") && name.ends_with(".csv"))
      .unwrap_or(false);
    if !is_log {
      continue;
    }
    if let Ok(meta) = fs::metadata(&path) {
      if let Ok(mtime) = meta.modified() {
        let mtime_secs = mtime.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        if now_secs.saturating_sub(mtime_secs) > cutoff_secs {
          if let Err(e) = fs::remove_file(&path) {
            crate::debug::log_warn(
              dir,
              &format!("logging: failed to prune {} — {e}", path.display()),
            );
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use std::time::{Duration, SystemTime};

  // --- ymd_from_unix ---

  #[test]
  fn ymd_unix_epoch_is_1970_01_01() {
    assert_eq!(ymd_from_unix(0), (1970, 1, 1));
  }

  #[test]
  fn ymd_known_dates() {
    // 2024-01-01 00:00:00 UTC
    assert_eq!(ymd_from_unix(1_704_067_200), (2024, 1, 1));
    // 2024-02-29 00:00:00 UTC  (leap day)
    assert_eq!(ymd_from_unix(1_709_164_800), (2024, 2, 29));
    // 2025-01-01 00:00:00 UTC
    assert_eq!(ymd_from_unix(1_735_689_600), (2025, 1, 1));
    // 2000-03-01 00:00:00 UTC  (century year, not a leap year in non-400 centuries)
    assert_eq!(ymd_from_unix(951_868_800), (2000, 3, 1));
  }

  #[test]
  fn ymd_last_second_of_day_stays_on_same_day() {
    // 2024-01-01 23:59:59 UTC = 1704067200 + 86399
    assert_eq!(ymd_from_unix(1_704_067_200 + 86_399), (2024, 1, 1));
  }

  #[test]
  fn ymd_first_second_of_next_day_rolls_over() {
    // 2024-01-02 00:00:00 UTC = 1704067200 + 86400
    assert_eq!(ymd_from_unix(1_704_067_200 + 86_400), (2024, 1, 2));
  }

  // --- current_log_path ---

  #[test]
  fn log_path_filename_format() {
    let dir = std::path::Path::new("/logs");
    // 2024-03-15 = 1710460800
    let path = current_log_path(dir, 1_710_460_800);
    assert_eq!(path.file_name().unwrap(), "rigstats-log-2024-03-15.csv");
  }

  // --- fmt_opt ---

  #[test]
  fn fmt_opt_none_is_empty_string() {
    assert_eq!(fmt_opt(None, 1), "");
  }

  #[test]
  fn fmt_opt_rounds_to_precision() {
    assert_eq!(fmt_opt(Some(3.14159), 1), "3.1");
    assert_eq!(fmt_opt(Some(42.0), 0), "42");
    assert_eq!(fmt_opt(Some(0.0), 3), "0.000");
  }

  // --- prune_old_logs ---

  #[test]
  fn prune_silently_ignores_missing_directory() {
    // Should not panic on a non-existent path.
    prune_old_logs(std::path::Path::new("/nonexistent/path/xyz"), 7);
  }

  #[test]
  fn prune_only_deletes_log_files_not_other_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("rigstats-log-2024-01-01.csv");
    let other = dir.path().join("important.txt");
    let sublog = dir.path().join("rigstats-log-2024-01-02.csv");

    fs::write(&log, "data").unwrap();
    fs::write(&other, "keep me").unwrap();
    fs::write(&sublog, "data2").unwrap();

    // Back-date all log files to 30 days ago so they are past the 7-day cutoff.
    let old = SystemTime::now() - Duration::from_secs(30 * 86400);
    for path in [&log, &sublog] {
      let f = fs::OpenOptions::new().write(true).open(path).unwrap();
      f.set_modified(old).unwrap();
    }
    // Also back-date other.txt — it should still NOT be deleted (wrong prefix).
    let f = fs::OpenOptions::new().write(true).open(&other).unwrap();
    f.set_modified(old).unwrap();

    prune_old_logs(dir.path(), 7);

    assert!(!log.exists(), "old log should be pruned");
    assert!(!sublog.exists(), "old log should be pruned");
    assert!(other.exists(), "non-log file must never be touched");
  }

  #[test]
  fn prune_keeps_recent_log_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let recent = dir.path().join("rigstats-log-2099-12-31.csv");
    fs::write(&recent, "data").unwrap();
    // mtime is "now" by default — well within any retention window.
    prune_old_logs(dir.path(), 7);
    assert!(recent.exists(), "recent log must not be pruned");
  }
}
