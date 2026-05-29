//! Diagnostics collection and ZIP export.
//!
//! The `collect_diagnostics` command gathers hardware info, the debug log,
//! current settings, the raw LHM sensor tree, and environment details into a
//! single ZIP file that users can attach to bug reports.

use crate::debug::{append_debug_log, debug_log_path, run_hidden_command};
use crate::monitor::{normalize_profile, profile_dimensions};
use crate::stats::{AppState, HardwareInfo};
use serde::Serialize;
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// --- Helpers ---------------------------------------------------------------

/// Re-parses a JSON string and returns it pretty-printed.
/// Falls back to the original string if it isn't valid JSON.
fn pretty_json(s: &str) -> String {
  serde_json::from_str::<serde_json::Value>(s)
    .and_then(|v| serde_json::to_string_pretty(&v))
    .unwrap_or_else(|_| s.to_string())
}

// --- Shell probe helper ----------------------------------------------------

/// Runs a PowerShell `-Command` and captures stdout, stderr, and exit code as JSON.
/// Used by diagnostic probes so the exact command output is visible in the ZIP.
fn run_ps_capture(cmd: &str) -> serde_json::Value {
  #[cfg(windows)]
  {
    match run_hidden_command("powershell", &["-NoProfile", "-Command", cmd]) {
      Ok(out) => serde_json::json!({
        "exit_code": out.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&out.stdout).trim().to_string(),
        "stderr": String::from_utf8_lossy(&out.stderr).trim().to_string(),
      }),
      Err(e) => serde_json::json!({ "error": e.to_string() }),
    }
  }
  #[cfg(not(windows))]
  {
    let _ = cmd;
    serde_json::json!({ "error": "not windows" })
  }
}

// --- Data collection helpers -----------------------------------------------

fn diag_collect_hardware() -> String {
  #[cfg(windows)]
  {
    let script = concat!(
      "try{",
      "$os=Get-CimInstance Win32_OperatingSystem -EA Stop;",
      "$cpu=Get-CimInstance Win32_Processor -EA Stop;",
      "$gpu=Get-CimInstance Win32_VideoController -EA Stop;",
      "$cs=Get-CimInstance Win32_ComputerSystem -EA Stop;",
      "$csp=Get-CimInstance Win32_ComputerSystemProduct -EA Stop;",
      "$bb=Get-CimInstance Win32_BaseBoard -EA Stop;",
      "$mem=Get-CimInstance Win32_PhysicalMemory -EA Stop;",
      "$disk=Get-CimInstance Win32_DiskDrive -EA Stop;",
      "@{",
      "os=@{caption=$os.Caption;version=$os.Version;build=$os.BuildNumber;arch=$os.OSArchitecture};",
      "cpu=@($cpu|%{@{name=$_.Name;cores=$_.NumberOfCores;threads=$_.NumberOfLogicalProcessors;maxMHz=$_.MaxClockSpeed}});",
      "gpu=@($gpu|%{@{name=$_.Name;ramBytes=$_.AdapterRAM;driver=$_.DriverVersion}});",
      "board=@{csMfr=$cs.Manufacturer;csModel=$cs.Model;bbMfr=$bb.Manufacturer;bbProd=$bb.Product;cspName=$csp.Name;cspVer=$csp.Version};",
      "ram=@($mem|%{@{capBytes=$_.Capacity;speed=$_.Speed;configured=$_.ConfiguredClockSpeed;typeCode=$_.SMBIOSMemoryType;mfr=$_.Manufacturer;part=$_.PartNumber}});",
      "disk=@($disk|%{@{deviceId=$_.DeviceID;model=$_.Model;mediaType=$_.MediaType;sizeBytes=$_.Size;interfaceType=$_.InterfaceType}})",
      "}|ConvertTo-Json -Depth 4",
      "}catch{'{ \"error\": \"collection failed\" }'}"
    );
    match run_hidden_command("powershell", &["-NoProfile", "-Command", script]) {
      Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
      Ok(out) => format!("{{\"error\":\"exit {}\"}}", out.status),
      Err(e) => format!("{{\"error\":\"{}\"}}", e),
    }
  }
  #[cfg(not(windows))]
  {
    r#"{"error":"not windows"}"#.to_string()
  }
}

fn diag_collect_tasks() -> String {
  #[cfg(windows)]
  {
    let task_names = [
      "LibreHardwareMonitor",
      "RIGStats\\LibreHardwareMonitor",
      "RigStats\\LibreHardwareMonitor",
    ];
    let mut out = String::new();
    for task_name in task_names {
      out.push_str(&format!("=== {} ===\n", task_name));
      match run_hidden_command("schtasks", &["/Query", "/TN", task_name, "/V", "/FO", "LIST"]) {
        Ok(result) => {
          out.push_str(&String::from_utf8_lossy(&result.stdout));
          if !result.stderr.is_empty() {
            out.push_str(&String::from_utf8_lossy(&result.stderr));
          }
        }
        Err(e) => out.push_str(&format!("Error: {}\n", e)),
      }
      out.push('\n');
    }
    out
  }
  #[cfg(not(windows))]
  {
    "not windows\n".to_string()
  }
}

fn diag_collect_environment() -> String {
  let mut lines = Vec::<String>::new();
  for var in &[
    "OS",
    "PROCESSOR_ARCHITECTURE",
    "PROCESSOR_IDENTIFIER",
    "NUMBER_OF_PROCESSORS",
    "COMPUTERNAME",
    "SystemRoot",
    "ProgramFiles",
  ] {
    lines.push(format!(
      "{}={}",
      var,
      std::env::var(var).unwrap_or_else(|_| "(not set)".to_string())
    ));
  }
  lines.push(format!(
    "hostname={}",
    hostname::get()
      .ok()
      .and_then(|s| s.into_string().ok())
      .unwrap_or_else(|| "(unknown)".to_string())
  ));
  #[cfg(windows)]
  {
    if let Ok(out) = run_hidden_command(
      "powershell",
      &[
        "-NoProfile",
        "-Command",
        "Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion' | Select-Object CurrentBuild,DisplayVersion,ProductName | ConvertTo-Json -Compress | Out-String",
      ],
    ) {
      if out.status.success() {
        lines.push(format!(
          "windows-version={}",
          String::from_utf8_lossy(&out.stdout).trim()
        ));
      }
    }
  }
  lines.join("\n")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SysinfoSnapshot {
  cpu_brand: String,
  cpu_count: usize,
  total_memory_mb: u64,
  used_memory_mb: u64,
  disk_mount_points: Vec<String>,
  /// WMI drive-letter → model-name map built at startup; empty when the WMI join failed.
  disk_model_map: std::collections::HashMap<String, String>,
  network_interfaces: Vec<String>,
  system_brand: String,
  sysinfo_available: bool,
  wmi_available: bool,
  /// What detect_ram_spec() produced at startup. "RAM" means detection failed.
  ram_spec: String,
  ram_details: String,
  /// Runs the exact PowerShell command used by detect_ram_spec at startup.
  /// Captures stdout, stderr, and exit_code so any syntax error is immediately visible.
  /// exit_code 0 + non-empty stdout = success. Anything else explains the failure.
  ram_spec_shell_test: serde_json::Value,
  /// Runs the WMI three-table join used to build disk_model_map.
  /// Empty result means the join returned no rows (common on some laptop BIOSes).
  disk_model_map_probe: serde_json::Value,
  ping_target: String,
}

fn diag_collect_installer_log(_app: &tauri::AppHandle) -> Vec<u8> {
  // The NSIS installer runs elevated (perMachine), so $APPDATA resolves to the
  // system account profile, not the user's. Use %PROGRAMDATA% instead — it is
  // machine-wide and always accessible regardless of which account ran the installer.
  let path = std::path::PathBuf::from(std::env::var("PROGRAMDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string()))
    .join("se.codeby.rigstats")
    .join("rigstats-install.log");
  std::fs::read(path).unwrap_or_else(|_| b"(install log not found)".to_vec())
}

fn diag_collect_battery(state: &AppState) -> String {
  // Cached state: what the frontend is currently seeing.
  let cached = {
    let guard = state.last_battery_sample.lock().unwrap_or_else(|e| e.into_inner());
    match &*guard {
      Some((instant, s)) => serde_json::json!({
        "age_secs": instant.elapsed().as_secs(),
        "present": s.present,
        "charge_pct": s.charge_pct,
        "charging": s.charging,
        "time_remaining_mins": s.time_remaining_mins,
        "power_w": s.power_w,
      }),
      None => serde_json::json!({ "status": "not yet sampled" }),
    }
  };

  // Win32_Battery — charge %, status code, estimated runtime.
  let win32_battery = run_ps_capture(
    "$b = Get-CimInstance Win32_Battery -EA SilentlyContinue; \
     if(-not $b){ '(no battery detected)' } \
     else { $b | Select-Object EstimatedChargeRemaining, BatteryStatus, EstimatedRunTime, Name | ConvertTo-Json -Depth 2 }",
  );

  // root\wmi BatteryStatus — charge/discharge rate in mW.
  // This is where the live watt reading comes from; absence means driver doesn't expose it.
  let wmi_battery_status = run_ps_capture(
    "$s = Get-CimInstance -Namespace root\\wmi -Class BatteryStatus -EA SilentlyContinue; \
     if(-not $s){ '(no data — driver may not expose root\\wmi BatteryStatus)' } \
     else { $s | Select-Object ChargeRate, DischargeRate, Charging, Discharging, PowerOnline, Voltage | ConvertTo-Json -Depth 2 }",
  );

  serde_json::to_string_pretty(&serde_json::json!({
    "cached": cached,
    "win32_battery": win32_battery,
    "wmi_battery_status": wmi_battery_status,
  }))
  .unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
}

fn diag_collect_sysinfo(state: &AppState, hw: &HardwareInfo) -> String {
  let (cpu_brand, cpu_count, total_memory_mb, used_memory_mb) = {
    let system = state.system.lock().unwrap_or_else(|e| e.into_inner());
    let brand = system.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
    let count = system.cpus().len();
    let total = system.total_memory() / 1_048_576;
    let used = system.used_memory() / 1_048_576;
    (brand, count, total, used)
  };
  let disk_mount_points: Vec<String> = {
    let disks = state.disks.lock().unwrap_or_else(|e| e.into_inner());
    disks
      .iter()
      .map(|d| d.mount_point().to_string_lossy().to_string())
      .collect()
  };
  let network_interfaces: Vec<String> = {
    let networks = state.networks.lock().unwrap_or_else(|e| e.into_inner());
    networks.keys().cloned().collect()
  };
  // Run the exact PowerShell command from detect_ram_spec_from_shell, capturing
  // stdout + stderr + exit_code. A non-zero exit or non-empty stderr immediately
  // explains why ram_spec shows "RAM" even when WMI has the data.
  let ram_spec_shell_test = run_ps_capture(
    "$m = Get-CimInstance Win32_PhysicalMemory; if(-not $m){ return }; \
     $dimms = $m.Count; \
     $speed = ($m | ForEach-Object { if($_.ConfiguredClockSpeed){ $_.ConfiguredClockSpeed } else { $_.Speed } } | Measure-Object -Maximum).Maximum; \
     $typeCode = ($m | Select-Object -First 1 -ExpandProperty SMBIOSMemoryType); \
     if(-not $typeCode){ $typeCode = ($m | Select-Object -First 1 -ExpandProperty MemoryType) }; \
     $type = switch([int]$typeCode){ 18 {'DDR'} 20 {'DDR2'} 24 {'DDR3'} 26 {'DDR4'} 27 {'LPDDR'} 28 {'LPDDR2'} 29 {'LPDDR3'} 30 {'LPDDR4'} 34 {'DDR5'} 35 {'LPDDR5'} 36 {'LPDDR5X'} default {''} }; \
     $r = if($type -and $speed){ \"$type $speed MT/s ($dimms DIMMs)\" } elseif($type){ \"$type ($dimms DIMMs)\" } elseif($speed){ \"$speed MT/s ($dimms DIMMs)\" } else { \"RAM ($dimms DIMMs)\" }; $r",
  );

  // Run the WMI drive-letter→model join used by detect_disk_model_map.
  // Empty result pinpoints which part of the association chain is broken.
  let disk_model_map_probe = run_ps_capture(
    "try { \
       $r = Get-CimInstance Win32_DiskDrive | ForEach-Object { \
         $d = $_; \
         Get-CimAssociatedInstance $d -ResultClassName Win32_DiskPartition -EA Stop | ForEach-Object { \
           $p = $_; \
           Get-CimAssociatedInstance $p -ResultClassName Win32_LogicalDisk -EA Stop | ForEach-Object { \
             [pscustomobject]@{letter=$_.DeviceID;model=$d.Model} \
           } \
         } \
       }; \
       if(-not $r){'(empty — join returned no rows)'} else { $r | ConvertTo-Json -Depth 2 } \
     } catch { \"(error: $_)\" }",
  );
  let snap = SysinfoSnapshot {
    cpu_brand,
    cpu_count,
    total_memory_mb,
    used_memory_mb,
    disk_mount_points,
    disk_model_map: hw.disk_model_map.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    network_interfaces,
    system_brand: hw.system_brand.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    sysinfo_available: hw.sysinfo_available,
    wmi_available: hw.wmi_available,
    ram_spec: hw.ram_spec.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    ram_details: hw.ram_details.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    ram_spec_shell_test,
    disk_model_map_probe,
    ping_target: hw.ping_target.clone(),
  };
  serde_json::to_string_pretty(&snap).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
}

// --- Display topology ------------------------------------------------------

#[derive(Serialize)]
struct DiagMonitor {
  name: String,
  width_px: u32,
  height_px: u32,
  position_x: i32,
  position_y: i32,
  scale_factor: f64,
  is_portrait: bool,
  fit_score: f64,
  selected: bool,
}

#[derive(Serialize)]
struct DiagDisplays {
  current_profile: String,
  target_w: u32,
  target_h: u32,
  monitors: Vec<DiagMonitor>,
}

fn fit_score(mw: u32, mh: u32, tw: u32, th: u32) -> f64 {
  let aspect_cost = ((mw as f64 / mh as f64) / (tw as f64 / th as f64)).ln().abs();
  let area_cost = ((mw as f64 * mh as f64) / (tw as f64 * th as f64)).ln().abs();
  (0.7 * aspect_cost) + (0.3 * area_cost)
}

fn diag_collect_displays(app: &tauri::AppHandle, profile: &str) -> String {
  use tauri::Manager;
  let profile = normalize_profile(profile);
  let (target_w, target_h) = profile_dimensions(&profile);

  let monitors = app
    .get_webview_window("main")
    .and_then(|w| w.available_monitors().ok())
    .unwrap_or_default();

  // Determine which monitor pick_target_monitor would select.
  let target_portrait = target_h >= target_w;
  let selected_pos = monitors
    .iter()
    .enumerate()
    .find(|(_, m)| m.size().width == target_w && m.size().height == target_h)
    .or_else(|| {
      monitors
        .iter()
        .enumerate()
        .filter(|(_, m)| (m.size().height >= m.size().width) == target_portrait)
        .min_by(|(_, a), (_, b)| {
          fit_score(a.size().width, a.size().height, target_w, target_h)
            .partial_cmp(&fit_score(b.size().width, b.size().height, target_w, target_h))
            .unwrap_or(std::cmp::Ordering::Equal)
        })
    })
    .or_else(|| {
      monitors.iter().enumerate().min_by(|(_, a), (_, b)| {
        fit_score(a.size().width, a.size().height, target_w, target_h)
          .partial_cmp(&fit_score(b.size().width, b.size().height, target_w, target_h))
          .unwrap_or(std::cmp::Ordering::Equal)
      })
    })
    .map(|(i, _)| i);

  let diag_monitors: Vec<DiagMonitor> = monitors
    .iter()
    .enumerate()
    .map(|(i, m)| {
      let w = m.size().width;
      let h = m.size().height;
      DiagMonitor {
        name: m.name().cloned().unwrap_or_default(),
        width_px: w,
        height_px: h,
        position_x: m.position().x,
        position_y: m.position().y,
        scale_factor: m.scale_factor(),
        is_portrait: h >= w,
        fit_score: (fit_score(w, h, target_w, target_h) * 1000.0).round() / 1000.0,
        selected: selected_pos == Some(i),
      }
    })
    .collect();

  let payload = DiagDisplays {
    current_profile: profile,
    target_w,
    target_h,
    monitors: diag_monitors,
  };
  serde_json::to_string_pretty(&payload).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
}

// --- Tauri command ---------------------------------------------------------

/// Opens a native save-file dialog, collects hardware/software diagnostics,
/// and writes everything into a ZIP archive for bug reports.
#[tauri::command]
pub async fn collect_diagnostics(
  app: tauri::AppHandle,
  state: tauri::State<'_, AppState>,
  hw: tauri::State<'_, HardwareInfo>,
) -> Result<Option<String>, String> {
  let ts = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs();
  let default_name = format!("rigstats-diag-{}.zip", ts);

  // Open a native save dialog on an OS thread (Win32 requires STA/message loop).
  let save_path = tokio::task::spawn_blocking(move || {
    rfd::FileDialog::new()
      .set_file_name(&default_name)
      .add_filter("ZIP Archive", &["zip"])
      .save_file()
  })
  .await
  .map_err(|e| format!("Dialog spawn error: {}", e))?;

  let Some(path) = save_path else {
    return Ok(None); // user cancelled
  };

  let manifest = serde_json::to_string_pretty(&serde_json::json!({
    "collected_at_unix": ts,
    "rigstats_version": env!("CARGO_PKG_VERSION"),
  }))
  .unwrap_or_default();

  let log_bytes = std::fs::read(debug_log_path(&app)).unwrap_or_else(|_| b"(log not found)".to_vec());

  let settings_json = {
    let s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
    serde_json::to_string_pretty(&*s).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
  };

  // Raw LHM sensor tree — the most useful data for adding new sensor support.
  let lhm_json = match state
    .lhm_client
    .get("http://localhost:8085/data.json")
    .timeout(Duration::from_secs(3))
    .send()
    .await
  {
    Ok(resp) => pretty_json(
      &resp
        .text()
        .await
        .unwrap_or_else(|e| format!("{{\"error\":\"body: {}\"}}", e)),
    ),
    Err(e) => format!("{{\"error\":\"request: {}\"}}", e),
  };

  let hardware_json = pretty_json(&diag_collect_hardware());
  let tasks_txt = diag_collect_tasks();
  let env_txt = diag_collect_environment();
  let battery_json = diag_collect_battery(&state);
  let sysinfo_json = diag_collect_sysinfo(&state, &hw);
  let install_log_bytes = diag_collect_installer_log(&app);
  let displays_json = {
    let profile = state
      .settings
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .dashboard_profile
      .clone();
    diag_collect_displays(&app, &profile)
  };
  // Parsed LHM snapshot: shows exactly what values the app derived from the sensor
  // tree (disk_temps, cpu_temp, gpu_temp, ram_temp, etc.). Faster to read than the
  // raw tree and directly pinpoints sensor extraction failures.
  let lhm_parsed_json = {
    let guard = state.last_lhm.lock().unwrap_or_else(|e| e.into_inner());
    match &*guard {
      Some(data) => serde_json::to_string_pretty(data).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e)),
      None => "{\"error\":\"no LHM sample available\"}".to_string(),
    }
  };

  let zip_file = std::fs::File::create(&path).map_err(|e| format!("Cannot create zip: {}", e))?;
  let mut writer = zip::ZipWriter::new(zip_file);
  let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

  let entries: &[(&str, &[u8])] = &[
    ("manifest.json", manifest.as_bytes()),
    ("debug.log", &log_bytes),
    ("install.log", &install_log_bytes),
    ("settings.json", settings_json.as_bytes()),
    ("lhm-data.json", lhm_json.as_bytes()),
    ("lhm-parsed.json", lhm_parsed_json.as_bytes()),
    ("hardware.json", hardware_json.as_bytes()),
    ("battery.json", battery_json.as_bytes()),
    ("sched-task.txt", tasks_txt.as_bytes()),
    ("environment.txt", env_txt.as_bytes()),
    ("sysinfo.json", sysinfo_json.as_bytes()),
    ("displays.json", displays_json.as_bytes()),
  ];

  for (name, data) in entries {
    writer
      .start_file(*name, opts)
      .map_err(|e| format!("zip start_file {}: {}", name, e))?;
    writer
      .write_all(data)
      .map_err(|e| format!("zip write {}: {}", name, e))?;
  }
  writer.finish().map_err(|e| format!("zip finish: {}", e))?;

  append_debug_log(&app, &format!("Diagnostics saved: {}", path.display()));
  Ok(Some(path.display().to_string()))
}
