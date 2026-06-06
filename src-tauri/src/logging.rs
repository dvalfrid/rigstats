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
/// Silently ignores entries it cannot stat or delete.
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
          let _ = fs::remove_file(&path);
        }
      }
    }
  }
}
