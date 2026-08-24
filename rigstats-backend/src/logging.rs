//! Stats logging — session-based CSV recording.
//!
//! Each recording session is an explicit start/stop span, stored as its own
//! CSV file (`rigstats-session-<id>.csv`) plus a metadata entry in a JSON
//! index (`rigstats-sessions.json`). Pre-session-model daily-rolling logs
//! (`rigstats-log-YYYY-MM-DD.csv`) are imported as read-only legacy sessions.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::settings::atomic_write;
use crate::stats::StatsPayload;

/// Small cross-process mutual-exclusion lock guarding the read-modify-write
/// cycle on `rigstats-sessions.json`. That file is written by more than one
/// process (this app and the `rigstats-wallpaper` host both prune once a day,
/// and either can start/end/rename/pin/delete a session), and without this,
/// two racing writes could silently discard each other's change — e.g. a
/// rename overwritten by a concurrent prune. Built on plain atomic file
/// creation so it needs no extra dependency and no `unsafe` code.
struct SessionsLock {
  path: PathBuf,
  held: bool,
}

impl SessionsLock {
  /// Retries for up to ~50ms, which is far longer than a normal
  /// load-mutate-save cycle takes. A lock file older than a few seconds is
  /// assumed to be left behind by a process that crashed while holding it,
  /// and is cleared rather than waited on. If the deadline is still reached
  /// (e.g. another process is doing unusually large I/O, like startup
  /// reconciliation), the caller proceeds without the lock — a missed update
  /// in that rare case is far better than hanging the caller, which may be
  /// the UI thread.
  fn acquire(dir: &Path) -> Self {
    let path = dir.join("rigstats-sessions.lock");
    let deadline = Instant::now() + Duration::from_millis(50);
    loop {
      match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(_) => return Self { path, held: true },
        Err(_) => {
          let stale = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(3));
          if stale {
            let _ = fs::remove_file(&path);
            continue;
          }
          if Instant::now() >= deadline {
            return Self { path, held: false };
          }
          std::thread::sleep(Duration::from_millis(2));
        }
      }
    }
  }
}

impl Drop for SessionsLock {
  fn drop(&mut self) {
    if self.held {
      let _ = fs::remove_file(&self.path);
    }
  }
}

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

/// Converts a Unix timestamp in seconds to `(hour, minute)` within its day (UTC).
fn hm_from_unix(secs: u64) -> (u32, u32) {
  let s = secs % 86400;
  ((s / 3600) as u32, (s % 3600 / 60) as u32)
}

fn fmt_opt(v: Option<f64>, precision: usize) -> String {
  v.map_or_else(String::new, |f| format!("{:.prec$}", f, prec = precision))
}

fn parse_opt_f64(s: &str) -> Option<f64> {
  if s.is_empty() {
    None
  } else {
    s.parse().ok()
  }
}

// --- Session model -----------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
pub struct SessionSummary {
  pub row_count: u64,
  pub avg_cpu_load: f64,
  pub peak_cpu_load: f64,
  pub avg_gpu_load: Option<f64>,
  pub peak_gpu_load: Option<f64>,
  pub avg_ram_gb: f64,
  pub peak_ram_gb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionMeta {
  pub id: String,
  pub name: String,
  pub start_unix: u64,
  pub end_unix: Option<u64>,
  #[serde(default)]
  pub pinned: bool,
  /// True for sessions imported from a pre-session-model daily-rolling log —
  /// the id encodes the source file's date and is never written to directly.
  #[serde(default)]
  pub legacy: bool,
  #[serde(default)]
  pub summary: SessionSummary,
}

impl SessionMeta {
  pub fn is_active(&self) -> bool {
    self.end_unix.is_none()
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SessionsIndex {
  #[serde(default)]
  sessions: Vec<SessionMeta>,
}

/// One parsed row of a session's CSV file (mirrors [`CSV_HEADER`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionRow {
  pub timestamp_unix: u64,
  pub cpu_load: f64,
  pub cpu_temp: Option<f64>,
  pub cpu_freq_mhz: f64,
  pub gpu_load: Option<f64>,
  pub gpu_temp: Option<f64>,
  pub gpu_vram_used_mb: Option<f64>,
  pub ram_used_gb: f64,
  pub disk_read_mbs: f64,
  pub disk_write_mbs: f64,
  pub net_up_mbps: f64,
  pub net_down_mbps: f64,
  pub ping_ms: Option<f64>,
}

pub fn sessions_index_path(dir: &Path) -> PathBuf {
  dir.join("rigstats-sessions.json")
}

/// Returns the CSV file path for a (non-legacy) session id.
pub fn session_csv_path(dir: &Path, id: &str) -> PathBuf {
  dir.join(format!("rigstats-session-{id}.csv"))
}

/// Resolves the CSV file backing `meta` — a `rigstats-session-<id>.csv` for
/// sessions created under this model, or the original
/// `rigstats-log-YYYY-MM-DD.csv` for imported legacy sessions.
pub fn session_file_path(dir: &Path, meta: &SessionMeta) -> PathBuf {
  if meta.legacy {
    let date = meta.id.trim_start_matches("legacy-");
    dir.join(format!("rigstats-log-{date}.csv"))
  } else {
    session_csv_path(dir, &meta.id)
  }
}

fn backup_index_path(dir: &Path) -> PathBuf {
  dir.join("rigstats-sessions.json.bak")
}

fn parse_sessions_index(raw: &str) -> Option<Vec<SessionMeta>> {
  serde_json::from_str::<SessionsIndex>(raw).ok().map(|i| i.sessions)
}

/// Loads all sessions from the index, newest first. Returns an empty list if
/// the index is missing (a legitimate empty state). If the index exists but
/// fails to parse (corrupt), the corrupt content is preserved alongside it
/// for recovery and the last-known-good backup (kept by `save_sessions`) is
/// tried before falling back to empty — a single bad write can't otherwise
/// silently erase all session history the next time something saves.
pub fn load_sessions(dir: &Path) -> Vec<SessionMeta> {
  let path = sessions_index_path(dir);
  let mut sessions = match fs::read_to_string(&path) {
    Ok(raw) => match parse_sessions_index(&raw) {
      Some(sessions) => sessions,
      None => {
        crate::debug::log_error(dir, "logging: sessions index is corrupt — recovering from backup");
        let _ = fs::write(dir.join("rigstats-sessions.json.corrupt"), &raw);
        fs::read_to_string(backup_index_path(dir))
          .ok()
          .and_then(|raw| parse_sessions_index(&raw))
          .unwrap_or_default()
      }
    },
    Err(_) => Vec::new(),
  };
  sessions.sort_by(|a, b| b.start_unix.cmp(&a.start_unix));
  sessions
}

fn save_sessions(dir: &Path, sessions: &[SessionMeta]) -> Result<(), String> {
  let path = sessions_index_path(dir);
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
  }
  let json = serde_json::to_string_pretty(&SessionsIndex {
    sessions: sessions.to_vec(),
  })
  .map_err(|e| e.to_string())?;
  atomic_write(&path, &json)?;
  // Mirror the index we just wrote into a backup, so if the main file is
  // later found corrupt (external tampering, disk trouble) it can be healed
  // from the most recent known-good state instead of silently losing all
  // session history.
  let _ = fs::write(backup_index_path(dir), &json);
  Ok(())
}

fn log_persist_err(dir: &Path, e: &str) {
  crate::debug::log_error(dir, &format!("logging: failed to persist session index — {e}"));
}

fn new_session_id() -> String {
  let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
  format!("{}{:03}", now.as_secs(), now.subsec_millis())
}

/// Default display name for a session started at `start_unix`, e.g.
/// "Session 2026-08-22 14:32" (UTC).
pub fn default_session_name(start_unix: u64) -> String {
  let (y, m, d) = ymd_from_unix(start_unix);
  let (h, mi) = hm_from_unix(start_unix);
  format!("Session {y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
}

/// Starts a new recording session: creates its CSV file (with header) and adds
/// a [`SessionMeta`] entry to the index.
pub fn start_session(dir: &Path) -> std::io::Result<SessionMeta> {
  let start_unix = unix_now_secs();
  let id = new_session_id();
  let meta = SessionMeta {
    name: default_session_name(start_unix),
    id: id.clone(),
    start_unix,
    end_unix: None,
    pinned: false,
    legacy: false,
    summary: SessionSummary::default(),
  };

  let path = session_csv_path(dir, &id);
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)?;
  }
  let file = OpenOptions::new().create(true).append(true).open(&path)?;
  let mut w = BufWriter::new(file);
  w.write_all(CSV_HEADER.as_bytes())?;
  w.flush()?;

  let _lock = SessionsLock::acquire(dir);
  let mut sessions = load_sessions(dir);
  sessions.push(meta.clone());
  if let Err(e) = save_sessions(dir, &sessions) {
    log_persist_err(dir, &e);
  }
  Ok(meta)
}

/// Appends one CSV row to `session`'s file.
pub fn append_stats_row(payload: &StatsPayload, dir: &Path, session: &SessionMeta) -> std::io::Result<()> {
  let path = session_csv_path(dir, &session.id);
  let file = OpenOptions::new().create(true).append(true).open(&path)?;
  let mut w = BufWriter::new(file);
  let ram_used_gb = payload.ram.used as f64 / 1_073_741_824.0;
  let cpu_temp = fmt_opt(payload.cpu.temp, 1);
  let gpu_load = fmt_opt(payload.gpu.load, 1);
  let gpu_temp = fmt_opt(payload.gpu.temp, 1);
  let gpu_vram = fmt_opt(payload.gpu.vram_used, 0);
  let ping_ms = fmt_opt(payload.net.ping_ms, 1);
  writeln!(
    w,
    "{},{},{},{:.1},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{}",
    unix_now_secs(),
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

/// Finalizes the session with the given `id`: sets `end_unix`, recomputes its
/// summary from the CSV, and persists the index. Returns the updated meta, or
/// `None` if `id` was not found.
pub fn end_session(dir: &Path, id: &str, end_unix: u64) -> Option<SessionMeta> {
  let _lock = SessionsLock::acquire(dir);
  let mut sessions = load_sessions(dir);
  let idx = sessions.iter().position(|s| s.id == id)?;
  sessions[idx].end_unix = Some(end_unix);
  let path = session_file_path(dir, &sessions[idx]);
  sessions[idx].summary = compute_summary(&path);
  let updated = sessions[idx].clone();
  if let Err(e) = save_sessions(dir, &sessions) {
    log_persist_err(dir, &e);
  }
  Some(updated)
}

pub fn set_session_pinned(dir: &Path, id: &str, pinned: bool) {
  let _lock = SessionsLock::acquire(dir);
  let mut sessions = load_sessions(dir);
  if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
    s.pinned = pinned;
    if let Err(e) = save_sessions(dir, &sessions) {
      log_persist_err(dir, &e);
    }
  }
}

pub fn rename_session(dir: &Path, id: &str, name: String) {
  let _lock = SessionsLock::acquire(dir);
  let mut sessions = load_sessions(dir);
  if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
    s.name = name;
    if let Err(e) = save_sessions(dir, &sessions) {
      log_persist_err(dir, &e);
    }
  }
}

/// Removes a session's index entry and deletes its CSV file.
pub fn delete_session(dir: &Path, id: &str) {
  let _lock = SessionsLock::acquire(dir);
  let mut sessions = load_sessions(dir);
  if let Some(idx) = sessions.iter().position(|s| s.id == id) {
    let meta = sessions.remove(idx);
    let path = session_file_path(dir, &meta);
    if let Err(e) = fs::remove_file(&path) {
      if e.kind() != std::io::ErrorKind::NotFound {
        crate::debug::log_warn(dir, &format!("logging: failed to delete session file {} — {e}", path.display()));
      }
    }
    if let Err(e) = save_sessions(dir, &sessions) {
      log_persist_err(dir, &e);
    }
  }
}

/// Deletes finished, unpinned sessions whose `end_unix` is older than `days`.
/// Active (still-recording) and pinned sessions are always kept.
pub fn prune_old_sessions(dir: &Path, days: u32) {
  let _lock = SessionsLock::acquire(dir);
  let now = unix_now_secs();
  let cutoff = days as u64 * 86400;
  let mut sessions = load_sessions(dir);
  let before = sessions.len();
  let mut kept = Vec::with_capacity(sessions.len());
  for s in sessions.drain(..) {
    let expired = s.end_unix.map(|end| now.saturating_sub(end) > cutoff).unwrap_or(false);
    if expired && !s.pinned {
      let path = session_file_path(dir, &s);
      if let Err(e) = fs::remove_file(&path) {
        if e.kind() != std::io::ErrorKind::NotFound {
          crate::debug::log_warn(dir, &format!("logging: failed to prune session {} — {e}", s.id));
        }
      }
    } else {
      kept.push(s);
    }
  }
  if kept.len() != before {
    if let Err(e) = save_sessions(dir, &kept) {
      log_persist_err(dir, &e);
    }
  }
}

/// Reads and parses a session CSV's data rows (header skipped, malformed lines dropped).
fn read_csv_rows(path: &Path) -> Vec<SessionRow> {
  let Ok(file) = fs::File::open(path) else {
    return Vec::new();
  };
  let reader = BufReader::new(file);
  let mut rows = Vec::new();
  for (i, line) in reader.lines().enumerate() {
    let Ok(line) = line else { continue };
    if i == 0 || line.trim().is_empty() {
      continue;
    }
    let f: Vec<&str> = line.split(',').collect();
    if f.len() < 13 {
      continue;
    }
    let Ok(timestamp_unix) = f[0].parse::<u64>() else {
      continue;
    };
    rows.push(SessionRow {
      timestamp_unix,
      cpu_load: f[1].parse().unwrap_or(0.0),
      cpu_temp: parse_opt_f64(f[2]),
      cpu_freq_mhz: f[3].parse().unwrap_or(0.0),
      gpu_load: parse_opt_f64(f[4]),
      gpu_temp: parse_opt_f64(f[5]),
      gpu_vram_used_mb: parse_opt_f64(f[6]),
      ram_used_gb: f[7].parse().unwrap_or(0.0),
      disk_read_mbs: f[8].parse().unwrap_or(0.0),
      disk_write_mbs: f[9].parse().unwrap_or(0.0),
      net_up_mbps: f[10].parse().unwrap_or(0.0),
      net_down_mbps: f[11].parse().unwrap_or(0.0),
      ping_ms: parse_opt_f64(f[12]),
    });
  }
  rows
}

/// Reads and parses `meta`'s CSV data rows, for charting.
pub fn read_session_rows(dir: &Path, meta: &SessionMeta) -> Vec<SessionRow> {
  read_csv_rows(&session_file_path(dir, meta))
}

fn compute_summary(path: &Path) -> SessionSummary {
  summarize_rows(&read_csv_rows(path))
}

/// Computes summary stats (avg/peak per metric) from already-loaded rows —
/// used to render live stats for the still-recording session, whose cached
/// [`SessionMeta::summary`] isn't finalized until [`end_session`] runs.
pub fn summarize_rows(rows: &[SessionRow]) -> SessionSummary {
  let row_count = rows.len() as u64;
  if row_count == 0 {
    return SessionSummary::default();
  }
  let n = row_count as f64;
  let avg_cpu_load = rows.iter().map(|r| r.cpu_load).sum::<f64>() / n;
  let peak_cpu_load = rows.iter().map(|r| r.cpu_load).fold(0.0, f64::max);
  let gpu_vals: Vec<f64> = rows.iter().filter_map(|r| r.gpu_load).collect();
  let avg_gpu_load = (!gpu_vals.is_empty()).then(|| gpu_vals.iter().sum::<f64>() / gpu_vals.len() as f64);
  let peak_gpu_load = gpu_vals
    .iter()
    .copied()
    .fold(None, |acc: Option<f64>, v| Some(acc.map_or(v, |a| a.max(v))));
  let avg_ram_gb = rows.iter().map(|r| r.ram_used_gb).sum::<f64>() / n;
  let peak_ram_gb = rows.iter().map(|r| r.ram_used_gb).fold(0.0, f64::max);
  SessionSummary {
    row_count,
    avg_cpu_load,
    peak_cpu_load,
    avg_gpu_load,
    peak_gpu_load,
    avg_ram_gb,
    peak_ram_gb,
  }
}

/// Runs once at startup: closes any session left open by an unclean shutdown
/// (using its last row's timestamp as the end time) and best-effort imports
/// any pre-session-model daily-rolling logs (`rigstats-log-YYYY-MM-DD.csv`) as
/// read-only legacy sessions. Malformed/empty legacy files are skipped.
pub fn reconcile_sessions_on_startup(dir: &Path) {
  let _lock = SessionsLock::acquire(dir);
  let mut sessions = load_sessions(dir);
  let mut changed = false;

  for s in sessions.iter_mut() {
    if s.end_unix.is_none() {
      let path = session_file_path(dir, s);
      let rows = read_csv_rows(&path);
      s.end_unix = Some(rows.last().map(|r| r.timestamp_unix).unwrap_or(s.start_unix));
      s.summary = compute_summary(&path);
      changed = true;
    }
  }

  let known_legacy: HashSet<String> = sessions.iter().filter(|s| s.legacy).map(|s| s.id.clone()).collect();
  if let Ok(entries) = fs::read_dir(dir) {
    for entry in entries.flatten() {
      let path = entry.path();
      let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        continue;
      };
      if !(name.starts_with("rigstats-log-") && name.ends_with(".csv")) {
        continue;
      }
      let date = name.trim_start_matches("rigstats-log-").trim_end_matches(".csv");
      let legacy_id = format!("legacy-{date}");
      if known_legacy.contains(&legacy_id) {
        continue;
      }
      let rows = read_csv_rows(&path);
      let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        continue;
      };
      sessions.push(SessionMeta {
        id: legacy_id,
        name: format!("{date} (legacy)"),
        start_unix: first.timestamp_unix,
        end_unix: Some(last.timestamp_unix),
        pinned: false,
        legacy: true,
        summary: compute_summary(&path),
      });
      changed = true;
    }
  }

  if changed {
    if let Err(e) = save_sessions(dir, &sessions) {
      log_persist_err(dir, &e);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;

  // --- ymd_from_unix / hm_from_unix ---

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
    // 2000-03-01 00:00:00 UTC  (century year, not a leap year in non-400 centuries)
    assert_eq!(ymd_from_unix(951_868_800), (2000, 3, 1));
  }

  #[test]
  fn hm_known_time() {
    // 2024-01-01 14:32:00 UTC = 1704067200 + 14*3600 + 32*60
    assert_eq!(hm_from_unix(1_704_067_200 + 14 * 3600 + 32 * 60), (14, 32));
  }

  #[test]
  fn default_session_name_format() {
    let name = default_session_name(1_704_067_200 + 14 * 3600 + 32 * 60);
    assert_eq!(name, "Session 2024-01-01 14:32");
  }

  // --- fmt_opt / parse_opt_f64 ---

  #[test]
  fn fmt_opt_none_is_empty_string() {
    assert_eq!(fmt_opt(None, 1), "");
  }

  #[test]
  fn fmt_opt_rounds_to_precision() {
    assert_eq!(fmt_opt(Some(3.14159), 1), "3.1");
    assert_eq!(fmt_opt(Some(42.0), 0), "42");
  }

  #[test]
  fn parse_opt_f64_round_trips_fmt_opt() {
    assert_eq!(parse_opt_f64(&fmt_opt(None, 1)), None);
    assert_eq!(parse_opt_f64(&fmt_opt(Some(12.3), 1)), Some(12.3));
  }

  // --- start_session / append_stats_row / end_session ---

  fn sample_payload(cpu_load: u8, gpu_load: Option<f64>, ram_used_bytes: u64) -> StatsPayload {
    use crate::stats::*;
    StatsPayload {
      cpu: CpuStats {
        load: cpu_load,
        cores: vec![],
        temp: Some(55.0),
        freq: 4200.0,
        power: None,
      },
      gpu: GpuStats {
        name: None,
        load: gpu_load,
        temp: None,
        hotspot: None,
        freq: None,
        mem_freq: None,
        vram_used: None,
        vram_total: None,
        fan_speed: None,
        power: None,
        d3d_3d: None,
        d3d_vdec: None,
        available_gpus: vec![],
      },
      ram: RamStats {
        total: 34_359_738_368,
        used: ram_used_bytes,
        free: 0,
        spec: String::new(),
        details: String::new(),
        temp: None,
      },
      net: NetStats {
        up: 1.0,
        down: 2.0,
        iface: String::new(),
        ping_ms: None,
      },
      disk: DiskStats {
        read: 0.0,
        write: 0.0,
        drives: vec![],
      },
      motherboard: MotherboardStats {
        fans: vec![],
        temps: vec![],
        voltages: vec![],
        chip: None,
        board: None,
      },
      battery: BatteryStats {
        present: false,
        charge_pct: None,
        charging: None,
        time_remaining_mins: None,
        power_w: None,
      },
      top_processes: vec![],
      system_uptime_secs: 0,
      lhm_connected: true,
    }
  }

  #[test]
  fn start_session_creates_csv_with_header_and_index_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let meta = start_session(dir.path()).expect("start_session");
    assert!(!meta.id.is_empty());
    assert!(meta.is_active());

    let csv_path = session_csv_path(dir.path(), &meta.id);
    let content = fs::read_to_string(&csv_path).unwrap();
    assert!(content.starts_with("timestamp_unix,"));

    let sessions = load_sessions(dir.path());
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, meta.id);
  }

  #[test]
  fn append_and_end_session_computes_summary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let meta = start_session(dir.path()).unwrap();

    append_stats_row(&sample_payload(40, Some(60.0), 1_073_741_824), dir.path(), &meta).unwrap();
    append_stats_row(&sample_payload(60, Some(80.0), 2_147_483_648), dir.path(), &meta).unwrap();

    let updated = end_session(dir.path(), &meta.id, unix_now_secs()).expect("end_session");
    assert!(!updated.is_active());
    assert_eq!(updated.summary.row_count, 2);
    assert_eq!(updated.summary.avg_cpu_load, 50.0);
    assert_eq!(updated.summary.peak_cpu_load, 60.0);
    assert_eq!(updated.summary.avg_gpu_load, Some(70.0));
    assert_eq!(updated.summary.peak_gpu_load, Some(80.0));
  }

  #[test]
  fn end_session_missing_id_returns_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(end_session(dir.path(), "nonexistent", 0).is_none());
  }

  #[test]
  fn pin_rename_and_delete_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let meta = start_session(dir.path()).unwrap();

    set_session_pinned(dir.path(), &meta.id, true);
    assert!(load_sessions(dir.path())[0].pinned);

    rename_session(dir.path(), &meta.id, "My Benchmark".to_string());
    assert_eq!(load_sessions(dir.path())[0].name, "My Benchmark");

    let csv_path = session_csv_path(dir.path(), &meta.id);
    assert!(csv_path.exists());
    delete_session(dir.path(), &meta.id);
    assert!(load_sessions(dir.path()).is_empty());
    assert!(!csv_path.exists());
  }

  // --- prune_old_sessions ---

  #[test]
  fn prune_keeps_pinned_and_active_but_removes_expired() {
    let dir = tempfile::tempdir().expect("tempdir");
    let now = unix_now_secs();

    let expired = start_session(dir.path()).unwrap();
    end_session(dir.path(), &expired.id, now.saturating_sub(30 * 86400)).unwrap();

    let pinned = start_session(dir.path()).unwrap();
    end_session(dir.path(), &pinned.id, now.saturating_sub(30 * 86400)).unwrap();
    set_session_pinned(dir.path(), &pinned.id, true);

    let active = start_session(dir.path()).unwrap();

    prune_old_sessions(dir.path(), 7);

    let remaining: Vec<String> = load_sessions(dir.path()).into_iter().map(|s| s.id).collect();
    assert!(!remaining.contains(&expired.id), "expired unpinned session must be pruned");
    assert!(remaining.contains(&pinned.id), "pinned session must survive pruning");
    assert!(remaining.contains(&active.id), "active session must survive pruning");
    assert!(!session_csv_path(dir.path(), &expired.id).exists());
  }

  // --- reconcile_sessions_on_startup ---

  #[test]
  fn reconcile_closes_orphaned_open_session_using_last_row_timestamp() {
    let dir = tempfile::tempdir().expect("tempdir");
    let meta = start_session(dir.path()).unwrap();
    append_stats_row(&sample_payload(10, None, 1_000_000_000), dir.path(), &meta).unwrap();
    // Session left open (simulating a crash) — no end_session call.

    reconcile_sessions_on_startup(dir.path());

    let sessions = load_sessions(dir.path());
    assert_eq!(sessions.len(), 1);
    assert!(!sessions[0].is_active(), "orphaned session must be closed");
    assert_eq!(sessions[0].summary.row_count, 1);
  }

  #[test]
  fn reconcile_imports_legacy_daily_log_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let legacy_path = dir.path().join("rigstats-log-2024-01-01.csv");
    fs::write(
      &legacy_path,
      "timestamp_unix,cpu_load,cpu_temp,cpu_freq_mhz,gpu_load,gpu_temp,gpu_vram_used_mb,ram_used_gb,disk_read_mbs,disk_write_mbs,net_up_mbps,net_down_mbps,ping_ms\n\
       1704067200,10,,4000.0,,,,,0.0,0.0,0.0,0.0,\n\
       1704067260,20,,4000.0,,,,,0.0,0.0,0.0,0.0,\n",
    )
    .unwrap();

    reconcile_sessions_on_startup(dir.path());
    let sessions = load_sessions(dir.path());
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].legacy);
    assert_eq!(sessions[0].id, "legacy-2024-01-01");
    assert_eq!(sessions[0].summary.row_count, 2);

    // Re-running must not duplicate the import.
    reconcile_sessions_on_startup(dir.path());
    assert_eq!(load_sessions(dir.path()).len(), 1);
  }

  #[test]
  fn reconcile_skips_empty_legacy_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
      dir.path().join("rigstats-log-2024-01-01.csv"),
      "timestamp_unix,cpu_load,cpu_temp,cpu_freq_mhz,gpu_load,gpu_temp,gpu_vram_used_mb,ram_used_gb,disk_read_mbs,disk_write_mbs,net_up_mbps,net_down_mbps,ping_ms\n",
    )
    .unwrap();
    reconcile_sessions_on_startup(dir.path());
    assert!(load_sessions(dir.path()).is_empty());
  }

  // --- corrupted index recovery ---

  #[test]
  fn load_sessions_recovers_from_backup_when_index_is_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let meta = start_session(dir.path()).unwrap();
    set_session_pinned(dir.path(), &meta.id, true);
    assert!(backup_index_path(dir.path()).exists(), "backup must exist after a save");

    fs::write(sessions_index_path(dir.path()), "{ not json").unwrap();

    let recovered = load_sessions(dir.path());
    assert_eq!(recovered.len(), 1, "must recover the session from the backup");
    assert_eq!(recovered[0].id, meta.id);
    assert!(recovered[0].pinned);
    assert!(
      dir.path().join("rigstats-sessions.json.corrupt").exists(),
      "corrupt content must be preserved for forensics"
    );
  }

  #[test]
  fn load_sessions_falls_back_to_empty_when_no_backup_exists() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(sessions_index_path(dir.path()), "{ not json").unwrap();
    assert!(load_sessions(dir.path()).is_empty());
  }

  #[test]
  fn missing_index_is_empty_not_treated_as_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(load_sessions(dir.path()).is_empty());
    assert!(!dir.path().join("rigstats-sessions.json.corrupt").exists());
  }

  // --- SessionsLock ---

  #[test]
  fn lock_waits_for_release_then_succeeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = SessionsLock::acquire(dir.path());
    assert!(first.held);

    let dir_path = dir.path().to_path_buf();
    let handle = std::thread::spawn(move || {
      let second = SessionsLock::acquire(&dir_path);
      assert!(second.held, "must wait for the first lock to be released, not give up");
    });

    std::thread::sleep(Duration::from_millis(10));
    drop(first);
    handle.join().unwrap();
  }

  #[test]
  fn stale_lock_is_reclaimed_instead_of_blocking() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lock_path = dir.path().join("rigstats-sessions.lock");
    let file = fs::File::create(&lock_path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(10)).unwrap();
    drop(file);

    let started = Instant::now();
    let lock = SessionsLock::acquire(dir.path());
    assert!(lock.held, "a stale lock must be reclaimed, not just waited out");
    assert!(started.elapsed() < Duration::from_millis(50), "reclaiming a stale lock must be fast");
  }

  #[test]
  fn concurrent_pin_and_rename_do_not_lose_either_update() {
    let dir = tempfile::tempdir().expect("tempdir");
    let meta = start_session(dir.path()).unwrap();
    let id_a = meta.id.clone();
    let id_b = meta.id.clone();
    let dir_a = dir.path().to_path_buf();
    let dir_b = dir.path().to_path_buf();

    let renamer = std::thread::spawn(move || {
      for i in 0..50 {
        rename_session(&dir_a, &id_a, format!("Name {i}"));
      }
    });
    let pinner = std::thread::spawn(move || {
      for _ in 0..50 {
        set_session_pinned(&dir_b, &id_b, true);
      }
    });
    renamer.join().unwrap();
    pinner.join().unwrap();

    let sessions = load_sessions(dir.path());
    assert_eq!(sessions.len(), 1, "the session must not be duplicated or dropped");
    assert!(sessions[0].pinned, "the pin from the other thread must not be lost");
  }
}
