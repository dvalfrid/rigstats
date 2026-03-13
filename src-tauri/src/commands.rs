//! Tauri command handlers and window event helpers.
//!
//! Design notes:
//! - Commands are intentionally thin wrappers around shared state and helpers.
//! - Settings updates are emitted to the renderer immediately after persistence.
//! - Stats collection keeps the last successful LHM sample to avoid UI flicker
//!   when LibreHardwareMonitor is temporarily unavailable.

use crate::lhm::fetch_lhm;
use crate::settings::{persist_settings, Settings};
use crate::stats::{AppState, CpuStats, DiskDrive, DiskStats, GpuStats, NetStats, RamStats, StatsPayload};
use serde::Deserialize;
use std::process::Command;
use std::time::Instant;
use tauri::{Emitter, Manager, Position, Size, WebviewWindow, WebviewWindowBuilder, WebviewUrl, Window, WindowEvent};

fn normalize_profile(profile: &str) -> String {
  match profile {
    "portrait-xl" | "portrait-slim" | "portrait-hd" | "portrait-wxga" => {
      profile.to_string()
    }
    _ => "portrait-xl".to_string(),
  }
}

fn profile_dimensions(profile: &str) -> (u32, u32) {
  match normalize_profile(profile).as_str() {
    "portrait-slim" => (480, 1920),
    "portrait-hd" => (720, 1280),
    "portrait-wxga" => (800, 1280),
    _ => (450, 1920),
  }
}

pub fn pick_target_monitor(window: &WebviewWindow, profile: &str) -> bool {
  let (target_w, target_h) = profile_dimensions(profile);

  // Prefer exact match. If none exists, keep current position and let user place manually.
  if let Ok(monitors) = window.available_monitors() {
    let exact_monitor = monitors
      .iter()
      .find(|m| m.size().width == target_w && m.size().height == target_h)
      .cloned();
    let has_exact_match = exact_monitor.is_some();

    if let Some(monitor) = exact_monitor {
      // Dedicated portrait display: run borderless fullscreen to avoid frame artifacts.
      let _ = window.set_fullscreen(false);
      let _ = window.set_decorations(false);
      let _ = window.set_position(Position::Physical(*monitor.position()));
      let _ = window.set_size(Size::Physical(tauri::PhysicalSize {
        width: target_w,
        height: target_h,
      }));
      let _ = window.set_fullscreen(true);
    } else {
      // Standalone mode on other displays: use normal framed window behavior.
      let _ = window.set_fullscreen(false);
      let _ = window.set_decorations(true);
      let _ = window.set_size(Size::Physical(tauri::PhysicalSize {
        width: target_w,
        height: target_h,
      }));
    }

    return has_exact_match;
  }

  false
}

#[tauri::command]
pub fn get_settings(state: tauri::State<AppState>) -> Settings {
  state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn preview_opacity(app: tauri::AppHandle, value: f64) -> Result<(), String> {
  if let Some(main) = app.get_webview_window("main") {
    main
      .emit("apply-opacity", value)
      .map_err(|e| e.to_string())?;
  }
  Ok(())
}

#[tauri::command]
pub fn save_settings(
  app: tauri::AppHandle,
  state: tauri::State<AppState>,
  opacity: f64,
  model_name: Option<String>,
  #[allow(non_snake_case)] modelName: Option<String>,
  dashboard_profile: Option<String>,
  #[allow(non_snake_case)] dashboardProfile: Option<String>,
  always_on_top: Option<bool>,
  #[allow(non_snake_case)] alwaysOnTop: Option<bool>,
) -> Result<(), String> {
  // Clamp opacity to a valid CSS alpha range before persistence.
  let mut settings = state.settings.lock().unwrap();
  settings.opacity = opacity.clamp(0.0, 1.0);

  // Accept both snake_case and camelCase payload keys from the renderer.
  let incoming_name = model_name.or(modelName).unwrap_or_else(|| settings.model_name.clone());
  settings.model_name = incoming_name;
  let requested_profile = dashboard_profile
    .or(dashboardProfile)
    .unwrap_or_else(|| settings.dashboard_profile.clone());
  let applied_profile = normalize_profile(&requested_profile);
  let applied_always_on_top = always_on_top
    .or(alwaysOnTop)
    .unwrap_or(settings.always_on_top);
  if let Some(main) = app.get_webview_window("main") {
    let _ = pick_target_monitor(&main, &applied_profile);
    main
      .set_always_on_top(applied_always_on_top)
      .map_err(|e| e.to_string())?;
  }

  settings.dashboard_profile = applied_profile.clone();
  settings.always_on_top = applied_always_on_top;
  persist_settings(&app, &settings)?;

  if let Some(main) = app.get_webview_window("main") {
    main
      .emit("apply-opacity", settings.opacity)
      .map_err(|e| e.to_string())?;
    main
      .emit("apply-model-name", settings.model_name.clone())
      .map_err(|e| e.to_string())?;
    main
      .emit("apply-profile", applied_profile.clone())
      .map_err(|e| e.to_string())?;
  }

  Ok(())
}

#[tauri::command]
pub fn close_window(window: WebviewWindow) -> Result<(), String> {
  window.close().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_window_drag(window: WebviewWindow) -> Result<(), String> {
  window.start_dragging().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_system_name() -> String {
  hostname::get()
    .ok()
    .and_then(|s| s.into_string().ok())
    .unwrap_or_else(|| "RIG DASHBOARD".to_string())
}

#[tauri::command]
pub fn get_cpu_info(state: tauri::State<AppState>) -> String {
  let mut system = state.system.lock().unwrap();
  system.refresh_cpu();
  let cpu0 = system.cpus().first();
  cpu0
    .map(|c| c.brand().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "--".to_string())
}

#[cfg(windows)]
#[derive(Deserialize, Debug)]
struct VideoControllerName {
  #[serde(rename = "Name")]
  name: Option<String>,
}

#[cfg(windows)]
#[derive(Deserialize, Debug)]
struct VideoControllerMemory {
  #[serde(rename = "AdapterRAM")]
  adapter_ram: Option<u64>,
}

#[cfg(windows)]
#[derive(Deserialize, Debug)]
struct ComputerSystem {
  #[serde(rename = "Model")]
  model: Option<String>,
}

#[cfg(windows)]
#[derive(Deserialize, Debug)]
struct ComputerSystemProduct {
  #[serde(rename = "Version")]
  version: Option<String>,
  #[serde(rename = "Name")]
  name: Option<String>,
}

#[cfg(windows)]
#[derive(Deserialize, Debug)]
struct PhysicalMemory {
  #[serde(rename = "Speed")]
  speed: Option<u32>,
  #[serde(rename = "ConfiguredClockSpeed")]
  configured_clock_speed: Option<u32>,
  #[serde(rename = "SMBIOSMemoryType")]
  smbios_memory_type: Option<u16>,
  #[serde(rename = "MemoryType")]
  memory_type: Option<u16>,
  #[serde(rename = "Manufacturer")]
  manufacturer: Option<String>,
  #[serde(rename = "PartNumber")]
  part_number: Option<String>,
  #[serde(rename = "Capacity")]
  capacity: Option<u64>,
}

#[cfg(windows)]
fn map_memory_type(code: u16) -> Option<&'static str> {
  match code {
    18 => Some("DDR"),
    20 => Some("DDR2"),
    24 => Some("DDR3"),
    26 => Some("DDR4"),
    34 => Some("DDR5"),
    _ => None,
  }
}

#[cfg(windows)]
fn classify_board_brand(manufacturer: &str) -> &'static str {
  let lower = manufacturer.to_ascii_lowercase();
  if lower.contains("asus") || lower.contains("rog") {
    "rog"
  } else if lower.contains("msi") {
    "msi"
  } else if lower.contains("gigabyte") {
    "gigabyte"
  } else if lower.contains("asrock") {
    "asrock"
  } else if lower.contains("intel") {
    "intel"
  } else {
    "other"
  }
}

pub fn detect_system_brand() -> String {
  #[cfg(windows)]
  {
    // Try WMI first (no subprocess overhead).
    // raw_query() lets us specify the class name explicitly in WQL, avoiding
    // the limitation where the wmi crate derives the class name from the
    // Rust struct name.
    if let Ok(com) = wmi::COMLibrary::new() {
      if let Ok(conn) = wmi::WMIConnection::new(com.into()) {
        #[derive(Deserialize)]
        struct Row {
          #[serde(rename = "Manufacturer")]
          manufacturer: Option<String>,
        }
        let rows: Vec<Row> = conn
          .raw_query("SELECT Manufacturer FROM Win32_BaseBoard")
          .ok()
          .unwrap_or_default();
        if let Some(mfr) = rows.iter().filter_map(|r| r.manufacturer.as_deref()).next() {
          if !mfr.trim().is_empty() {
            return classify_board_brand(mfr).to_string();
          }
        }
      }
    }

    // PowerShell fallback — always available on Windows 10/11.
    let output = Command::new("powershell")
      .args([
        "-NoProfile",
        "-Command",
        "Get-CimInstance Win32_BaseBoard | Select-Object -ExpandProperty Manufacturer | Out-String",
      ])
      .output();

    if let Ok(out) = output {
      if out.status.success() {
        let mfr = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !mfr.is_empty() {
          return classify_board_brand(&mfr).to_string();
        }
      }
    }

    "other".to_string()
  }

  #[cfg(not(windows))]
  {
    "other".to_string()
  }
}

#[tauri::command]
pub fn get_system_brand(state: tauri::State<AppState>) -> String {
  state.system_brand.clone()
}

fn normalize_model_name(raw: &str) -> Option<String> {
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return None;
  }

  let invalid = ["to be filled by o.e.m.", "system product name", "default string", "unknown"];
  let lower = trimmed.to_ascii_lowercase();
  if invalid.iter().any(|x| lower == *x) {
    return None;
  }

  Some(trimmed.to_string())
}

pub fn detect_model_name() -> Option<String> {
  #[cfg(windows)]
  {
    let com = wmi::COMLibrary::new().ok()?;
    let conn = wmi::WMIConnection::new(com.into()).ok()?;

    let products: Vec<ComputerSystemProduct> = conn.query().ok().unwrap_or_default();
    if let Some(v) = products
      .iter()
      .filter_map(|p| p.version.as_deref().and_then(normalize_model_name))
      .next()
    {
      return Some(v);
    }

    if let Some(v) = products
      .iter()
      .filter_map(|p| p.name.as_deref().and_then(normalize_model_name))
      .next()
    {
      return Some(v);
    }

    let systems: Vec<ComputerSystem> = conn.query().ok().unwrap_or_default();
    systems
      .iter()
      .filter_map(|s| s.model.as_deref().and_then(normalize_model_name))
      .next()
  }

  #[cfg(not(windows))]
  {
    None
  }
}

pub fn detect_ram_spec() -> String {
  #[cfg(windows)]
  fn detect_ram_spec_from_shell() -> Option<String> {
    let output = Command::new("powershell")
      .args([
        "-NoProfile",
        "-Command",
        "$m = Get-CimInstance Win32_PhysicalMemory; if(-not $m){ return }; $dimms = $m.Count; $speed = ($m | ForEach-Object { if($_.ConfiguredClockSpeed){ $_.ConfiguredClockSpeed } else { $_.Speed } } | Measure-Object -Maximum).Maximum; $typeCode = ($m | Select-Object -First 1 -ExpandProperty SMBIOSMemoryType); if(-not $typeCode){ $typeCode = ($m | Select-Object -First 1 -ExpandProperty MemoryType) }; $type = switch([int]$typeCode){ 18 {'DDR'} 20 {'DDR2'} 24 {'DDR3'} 26 {'DDR4'} 34 {'DDR5'} default {''} }; if($type -and $speed){ \"$type $speed MT/s ($dimms DIMMs)\" } elseif($type){ \"$type ($dimms DIMMs)\" } elseif($speed){ \"$speed MT/s ($dimms DIMMs)\" } else { \"RAM ($dimms DIMMs)\" } | Out-String",
      ])
      .output()
      .ok()?;

    if !output.status.success() {
      return None;
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
      None
    } else {
      Some(text)
    }
  }

  #[cfg(windows)]
  {
    let com = match wmi::COMLibrary::new() {
      Ok(c) => c,
      Err(_) => return detect_ram_spec_from_shell().unwrap_or_else(|| "RAM".to_string()),
    };
    let conn = match wmi::WMIConnection::new(com.into()) {
      Ok(c) => c,
      Err(_) => return detect_ram_spec_from_shell().unwrap_or_else(|| "RAM".to_string()),
    };

    let sticks: Vec<PhysicalMemory> = match conn.query() {
      Ok(s) => s,
      Err(_) => return detect_ram_spec_from_shell().unwrap_or_else(|| "RAM".to_string()),
    };

    if sticks.is_empty() {
      return detect_ram_spec_from_shell().unwrap_or_else(|| "RAM".to_string());
    }

    let dimms = sticks.len();
    let max_speed = sticks
      .iter()
      .filter_map(|s| s.configured_clock_speed.or(s.speed))
      .max()
      .unwrap_or(0);
    let ram_type = sticks
      .iter()
      .find_map(|s| s.smbios_memory_type.or(s.memory_type).and_then(map_memory_type));

    let spec = match (ram_type, max_speed) {
      (Some(t), s) if s > 0 => format!("{} {} MT/s ({} DIMMs)", t, s, dimms),
      (Some(t), _) => format!("{} ({} DIMMs)", t, dimms),
      (None, s) if s > 0 => format!("{} MT/s ({} DIMMs)", s, dimms),
      _ => format!("RAM ({} DIMMs)", dimms),
    };

    if spec.starts_with("RAM") {
      detect_ram_spec_from_shell().unwrap_or(spec)
    } else {
      spec
    }
  }

  #[cfg(not(windows))]
  {
    "RAM".to_string()
  }
}

pub fn detect_ram_details() -> String {
  #[cfg(windows)]
  fn sanitize_ram_field(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
      return None;
    }

    let lower = value.to_ascii_lowercase();
    if lower == "unknown" || lower == "to be filled by o.e.m." || lower == "default string" {
      return None;
    }

    Some(value.to_string())
  }

  #[cfg(windows)]
  fn detect_ram_details_from_shell() -> Option<String> {
    let output = Command::new("powershell")
      .args([
        "-NoProfile",
        "-Command",
        "$m = Get-CimInstance Win32_PhysicalMemory; if(-not $m){ return }; $count = $m.Count; $caps = @($m | ForEach-Object { [math]::Round($_.Capacity / 1GB) }); $layout = if((@($caps | Select-Object -Unique)).Count -eq 1 -and $caps.Count -gt 0) { \"${count}x$($caps[0]) GB\" } else { \"${count} DIMMs\" }; $vendor = ($m | Select-Object -First 1 -ExpandProperty Manufacturer); $part = ($m | Select-Object -First 1 -ExpandProperty PartNumber); \"$layout|$vendor|$part\" | Out-String",
      ])
      .output()
      .ok()?;

    if !output.status.success() {
      return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text
      .trim()
      .split('|')
      .filter_map(sanitize_ram_field)
      .collect::<Vec<_>>();

    if parts.is_empty() {
      None
    } else {
      // Keep the output compact and predictable.
      parts.truncate(3);
      Some(parts.join(" | "))
    }
  }

  #[cfg(windows)]
  {
    let com = match wmi::COMLibrary::new() {
      Ok(c) => c,
      Err(_) => return detect_ram_details_from_shell().unwrap_or_default(),
    };
    let conn = match wmi::WMIConnection::new(com.into()) {
      Ok(c) => c,
      Err(_) => return detect_ram_details_from_shell().unwrap_or_default(),
    };

    let sticks: Vec<PhysicalMemory> = match conn.query() {
      Ok(s) => s,
      Err(_) => return detect_ram_details_from_shell().unwrap_or_default(),
    };
    if sticks.is_empty() {
      return detect_ram_details_from_shell().unwrap_or_default();
    }

    let mut pieces = Vec::new();

    let sizes_gb: Vec<u64> = sticks
      .iter()
      .filter_map(|s| s.capacity)
      .map(|bytes| ((bytes as f64) / 1_073_741_824.0).round() as u64)
      .filter(|gb| *gb > 0)
      .collect();

    if !sizes_gb.is_empty() {
      let first = sizes_gb[0];
      let all_equal = sizes_gb.iter().all(|v| *v == first);
      if all_equal {
        pieces.push(format!("{}x{} GB", sizes_gb.len(), first));
      } else {
        pieces.push(format!("{} DIMMs", sizes_gb.len()));
      }
    } else {
      pieces.push(format!("{} DIMMs", sticks.len()));
    }

    let vendor = sticks
      .iter()
      .filter_map(|s| s.manufacturer.as_deref())
      .filter_map(sanitize_ram_field)
      .next();
    if let Some(v) = vendor {
      pieces.push(v);
    }

    let part = sticks
      .iter()
      .filter_map(|s| s.part_number.as_deref())
      .filter_map(sanitize_ram_field)
      .next();
    if let Some(p) = part {
      pieces.push(p);
    }

    let details = pieces.join(" | ");
    if details.trim().is_empty() {
      detect_ram_details_from_shell().unwrap_or_default()
    } else {
      details
    }
  }

  #[cfg(not(windows))]
  {
    String::new()
  }
}

pub fn detect_gpu_vram_total_mb() -> f64 {
  #[cfg(windows)]
  {
    let com = match wmi::COMLibrary::new() {
      Ok(c) => c,
      Err(_) => return 16384.0,
    };
    let conn = match wmi::WMIConnection::new(com.into()) {
      Ok(c) => c,
      Err(_) => return 16384.0,
    };

    let rows: Vec<VideoControllerMemory> = match conn.query() {
      Ok(r) => r,
      Err(_) => return 16384.0,
    };

    let best = rows.iter().filter_map(|r| r.adapter_ram).max().unwrap_or(0);
    if best > 0 {
      (best as f64 / 1_048_576.0).round()
    } else {
      16384.0
    }
  }

  #[cfg(not(windows))]
  {
    16384.0
  }
}

#[cfg(windows)]
fn is_ignored_adapter_name(name: &str) -> bool {
  let lower = name.to_ascii_lowercase();
  lower.contains("microsoft basic display")
    || lower.contains("microsoft basic render")
    || lower.contains("remote display")
    || lower.contains("virtual display")
    || lower.contains("hyper-v")
}

#[cfg(windows)]
fn gpu_name_score(name: &str) -> i32 {
  let lower = name.to_ascii_lowercase();
  if is_ignored_adapter_name(name) {
    return -100;
  }
  if lower.contains("radeon rx") || lower.contains("geforce") || lower.contains("rtx") || lower.contains("arc") {
    return 100;
  }
  if lower.contains("radeon") || lower.contains("nvidia") || lower.contains("intel") {
    return 50;
  }
  10
}

#[cfg(windows)]
fn pick_best_gpu_name<I>(names: I) -> Option<String>
where
  I: IntoIterator<Item = String>,
{
  names
    .into_iter()
    .map(|n| n.trim().to_string())
    .filter(|n| !n.is_empty())
    .max_by_key(|n| gpu_name_score(n))
}

#[cfg(windows)]
fn get_gpu_info_from_shell() -> Option<String> {
  let output = Command::new("powershell")
    .args([
      "-NoProfile",
      "-Command",
      "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name | Out-String",
    ])
    .output()
    .ok()?;

  if !output.status.success() {
    return None;
  }

  let text = String::from_utf8_lossy(&output.stdout);
  let names = text
    .lines()
    .map(|line| line.trim().to_string())
    .filter(|line| !line.is_empty())
    .collect::<Vec<_>>();

  pick_best_gpu_name(names)
}

fn parse_ping_output_ms(output: &str) -> Option<f64> {
  let mut numbers = Vec::new();
  let mut current = String::new();

  for ch in output.chars() {
    if ch.is_ascii_digit() {
      current.push(ch);
    } else if !current.is_empty() {
      if let Ok(v) = current.parse::<f64>() {
        numbers.push(v);
      }
      current.clear();
    }
  }

  if !current.is_empty() {
    if let Ok(v) = current.parse::<f64>() {
      numbers.push(v);
    }
  }

  // Windows ping summary ends with average latency in ms.
  numbers.last().copied()
}

pub fn detect_ping_target() -> String {
  #[cfg(windows)]
  {
    let output = Command::new("powershell")
      .args([
        "-NoProfile",
        "-Command",
        "(Get-CimInstance Win32_NetworkAdapterConfiguration | Where-Object { $_.IPEnabled -and $_.DefaultIPGateway } | ForEach-Object { $_.DefaultIPGateway } | Where-Object { $_ -match '^\\d+\\.\\d+\\.\\d+\\.\\d+$' } | Select-Object -First 1) | Out-String",
      ])
      .output();

    if let Ok(out) = output {
      if out.status.success() {
        let candidate = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !candidate.is_empty() {
          return candidate;
        }
      }
    }

    "1.1.1.1".to_string()
  }

  #[cfg(not(windows))]
  {
    "1.1.1.1".to_string()
  }
}

fn sample_ping_ms(target: &str) -> Option<f64> {
  #[cfg(windows)]
  {
    let output = Command::new("ping")
      .args(["-n", "1", "-w", "500", target])
      .output()
      .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    parse_ping_output_ms(&text)
  }

  #[cfg(not(windows))]
  {
    None
  }
}

#[tauri::command]
pub fn get_gpu_info() -> Option<String> {
  #[cfg(windows)]
  {
    if let Ok(com) = wmi::COMLibrary::new() {
      if let Ok(conn) = wmi::WMIConnection::new(com.into()) {
        if let Ok(rows) = conn.query::<VideoControllerName>() {
          let names = rows.into_iter().filter_map(|r| r.name).collect::<Vec<_>>();
          if let Some(best) = pick_best_gpu_name(names) {
            return Some(best);
          }
        }
      }
    }

    return get_gpu_info_from_shell();
  }

  #[cfg(not(windows))]
  {
    None
  }
}

#[tauri::command]
pub async fn get_stats(state: tauri::State<'_, AppState>) -> Result<StatsPayload, String> {
  // Fetch a fresh LHM sample, then fall back to the last successful one if needed.
  let lhm_fresh = fetch_lhm().await;
  let lhm = {
    let mut last_lhm = state.last_lhm.lock().unwrap();
    if let Some(ref sample) = lhm_fresh {
      *last_lhm = Some(sample.clone());
    }
    (*last_lhm).clone()
  };

  // sysinfo values are refreshed each tick from this shared System instance.
  let mut system = state.system.lock().unwrap();
  system.refresh_cpu();
  system.refresh_memory();
  let system_uptime_secs = sysinfo::System::uptime();

  let total = system.total_memory();
  let used = system.used_memory();
  let free = system.free_memory();

  let avg_load = if system.cpus().is_empty() {
    0
  } else {
    let sum: f32 = system.cpus().iter().map(|c| c.cpu_usage()).sum();
    (sum / system.cpus().len() as f32).round() as u8
  };
  let cores: Vec<u8> = system
    .cpus()
    .iter()
    .map(|c| c.cpu_usage().round() as u8)
    .collect();
  let freq = system
    .cpus()
    .first()
    .map(|c| c.frequency() as f64 / 1000.0)
    .unwrap_or(0.0);
  drop(system);

  let mut disks = state.disks.lock().unwrap();
  disks.refresh();
  let mut drives = Vec::new();
  for d in disks.iter() {
    let total_space = d.total_space();
    if total_space <= 1_000_000_000 {
      continue;
    }
    let available = d.available_space();
    let used_space = total_space.saturating_sub(available);
    let pct = if total_space == 0 {
      0
    } else {
      ((used_space as f64 / total_space as f64) * 100.0).round() as u8
    };
    drives.push(DiskDrive {
      fs: d.mount_point().to_string_lossy().to_string(),
      size: total_space,
      used: used_space,
      pct,
    });
  }
  drop(disks);

  // Network throughput is computed from deltas over elapsed time between samples.
  let mut networks = state.networks.lock().unwrap();
  let mut last_sample = state.last_net_sample.lock().unwrap();
  let now = Instant::now();
  let elapsed = last_sample
    .map(|t| now.duration_since(t).as_secs_f64())
    .unwrap_or(1.0)
    .max(0.5);
  *last_sample = Some(now);

  networks.refresh();
  let mut best_iface = "--".to_string();
  let mut best_up = 0.0;
  let mut best_down = 0.0;
  for (name, data) in networks.iter() {
    let up_mbps = (data.transmitted() as f64 * 8.0 / 1_000_000.0) / elapsed;
    let down_mbps = (data.received() as f64 * 8.0 / 1_000_000.0) / elapsed;
    if up_mbps + down_mbps > best_up + best_down {
      best_up = up_mbps;
      best_down = down_mbps;
      best_iface = name.to_string();
    }
  }

  let ping_ms = {
    let mut cache = state.last_ping_sample.lock().unwrap();
    let should_refresh = cache
      .as_ref()
      .map(|(t, _)| now.duration_since(*t).as_secs_f64() >= 5.0)
      .unwrap_or(true);

    if should_refresh {
      let measured = sample_ping_ms(&state.ping_target);
      *cache = Some((now, measured));
      measured
    } else {
      cache.as_ref().map(|(_, value)| *value).unwrap_or(None)
    }
  };

  let (disk_read, disk_write, net_up, net_down, lhm_connected) = if let Some(ref l) = lhm {
    (l.disk_read, l.disk_write, l.net_up, l.net_down, true)
  } else {
    (0.0, 0.0, best_up, best_down, false)
  };

  Ok(StatsPayload {
    cpu: CpuStats {
      load: avg_load,
      cores,
      temp: lhm.as_ref().and_then(|l| l.cpu_temp),
      freq,
      power: lhm.as_ref().and_then(|l| l.cpu_power),
    },
    gpu: GpuStats {
      load: lhm.as_ref().and_then(|l| l.gpu_load),
      temp: lhm.as_ref().and_then(|l| l.gpu_temp),
      hotspot: lhm.as_ref().and_then(|l| l.gpu_hotspot),
      freq: lhm.as_ref().and_then(|l| l.gpu_freq),
      vram_used: lhm.as_ref().and_then(|l| l.vram_used),
      vram_total: lhm
        .as_ref()
        .and_then(|l| l.vram_total)
        .unwrap_or(state.gpu_vram_total_mb),
      fan_speed: lhm.as_ref().and_then(|l| l.gpu_fan),
      power: lhm.as_ref().and_then(|l| l.gpu_power),
    },
    ram: RamStats {
      total,
      used,
      free,
      spec: state.ram_spec.clone(),
      details: state.ram_details.clone(),
    },
    net: NetStats {
      up: net_up,
      down: net_down,
      iface: best_iface,
      ping_ms,
    },
    disk: DiskStats {
      read: disk_read,
      write: disk_write,
      drives,
    },
    system_uptime_secs,
    lhm_connected,
  })
}

pub fn ensure_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
  // Keep a single settings window instance; focus existing window if already open.
  if app.get_webview_window("settings").is_some() {
    if let Some(win) = app.get_webview_window("settings") {
      win.set_focus().map_err(|e| e.to_string())?;
    }
    return Ok(());
  }

  let main = app.get_webview_window("main").ok_or("Main window missing")?;
  let main_pos = main.outer_position().map_err(|e| e.to_string())?;
  let main_size = main.outer_size().map_err(|e| e.to_string())?;

  let width = 300.0;
  let height = 320.0;
  let x = main_pos.x as f64 + main_size.width as f64 - width - 16.0;
  let y = main_pos.y as f64 + 16.0;

  WebviewWindowBuilder::new(
    app,
    "settings",
    WebviewUrl::App("settings.html".into()),
  )
  .title("Settings")
  .inner_size(width, height)
  .position(x, y)
  .decorations(false)
  .resizable(false)
  .always_on_top(true)
  .skip_taskbar(true)
  .build()
  .map_err(|e| e.to_string())?;

  Ok(())
}

pub fn on_window_event(win: &Window, event: &WindowEvent) {
  if win.label() == "main" {
    // Closing the main window hides to tray instead of terminating the process.
    if let WindowEvent::CloseRequested { api, .. } = event {
      api.prevent_close();
      let _ = win.hide();
    }
  }

  if win.label() == "settings" {
    // Settings behaves like a popover: close when focus is lost.
    if let WindowEvent::Focused(false) = event {
      let _ = win.close();
    }
  }
}
