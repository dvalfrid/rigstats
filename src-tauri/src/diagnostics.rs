//! Diagnostics collection and ZIP export.
//!
//! The `collect_diagnostics` command gathers hardware info, the debug log,
//! current settings, the raw LHM sensor tree, and environment details into a
//! single ZIP file that users can attach to bug reports.

use crate::debug::{append_debug_log, debug_log_path, run_hidden_command};
use crate::monitor::{normalize_profile, profile_dimensions};
use crate::stats::AppState;
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
      "@{",
      "os=@{caption=$os.Caption;version=$os.Version;build=$os.BuildNumber;arch=$os.OSArchitecture};",
      "cpu=@($cpu|%{@{name=$_.Name;cores=$_.NumberOfCores;threads=$_.NumberOfLogicalProcessors;maxMHz=$_.MaxClockSpeed}});",
      "gpu=@($gpu|%{@{name=$_.Name;ramBytes=$_.AdapterRAM;driver=$_.DriverVersion}});",
      "board=@{csMfr=$cs.Manufacturer;csModel=$cs.Model;bbMfr=$bb.Manufacturer;bbProd=$bb.Product;cspName=$csp.Name;cspVer=$csp.Version};",
      "ram=@($mem|%{@{capBytes=$_.Capacity;speed=$_.Speed;configured=$_.ConfiguredClockSpeed;typeCode=$_.SMBIOSMemoryType;mfr=$_.Manufacturer;part=$_.PartNumber}})",
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
    let task_names = ["LibreHardwareMonitor", "RIGStats\\LibreHardwareMonitor", "RigStats\\LibreHardwareMonitor"];
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
  network_interfaces: Vec<String>,
  system_brand: String,
  sysinfo_available: bool,
  wmi_available: bool,
  ram_spec: String,
  ram_details: String,
  ping_target: String,
}

fn diag_collect_installer_log(app: &tauri::AppHandle) -> Vec<u8> {
  let path = debug_log_path(app).with_file_name("rigstats-install.log");
  std::fs::read(path).unwrap_or_else(|_| b"(install log not found)".to_vec())
}

fn diag_collect_sysinfo(state: &AppState) -> String {
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
    disks.iter().map(|d| d.mount_point().to_string_lossy().to_string()).collect()
  };
  let network_interfaces: Vec<String> = {
    let networks = state.networks.lock().unwrap_or_else(|e| e.into_inner());
    networks.iter().map(|(name, _)| name.clone()).collect()
  };
  let snap = SysinfoSnapshot {
    cpu_brand,
    cpu_count,
    total_memory_mb,
    used_memory_mb,
    disk_mount_points,
    network_interfaces,
    system_brand: state.system_brand.clone(),
    sysinfo_available: state.sysinfo_available,
    wmi_available: state.wmi_available,
    ram_spec: state.ram_spec.clone(),
    ram_details: state.ram_details.clone(),
    ping_target: state.ping_target.clone(),
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

  let payload = DiagDisplays { current_profile: profile, target_w, target_h, monitors: diag_monitors };
  serde_json::to_string_pretty(&payload).unwrap_or_else(|e| format!("{{\"error\":\"{}\"}}", e))
}

// --- Tauri command ---------------------------------------------------------

/// Opens a native save-file dialog, collects hardware/software diagnostics,
/// and writes everything into a ZIP archive for bug reports.
#[tauri::command]
pub async fn collect_diagnostics(
  app: tauri::AppHandle,
  state: tauri::State<'_, AppState>,
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
    Ok(resp) => pretty_json(&resp.text().await.unwrap_or_else(|e| format!("{{\"error\":\"body: {}\"}}", e))),
    Err(e) => format!("{{\"error\":\"request: {}\"}}", e),
  };

  let hardware_json = pretty_json(&diag_collect_hardware());
  let tasks_txt = diag_collect_tasks();
  let env_txt = diag_collect_environment();
  let sysinfo_json = diag_collect_sysinfo(&state);
  let install_log_bytes = diag_collect_installer_log(&app);
  let displays_json = {
    let profile = state.settings.lock().unwrap_or_else(|e| e.into_inner()).dashboard_profile.clone();
    diag_collect_displays(&app, &profile)
  };

  let zip_file = std::fs::File::create(&path).map_err(|e| format!("Cannot create zip: {}", e))?;
  let mut writer = zip::ZipWriter::new(zip_file);
  let opts = zip::write::SimpleFileOptions::default()
    .compression_method(zip::CompressionMethod::Deflated);

  let entries: &[(&str, &[u8])] = &[
    ("manifest.json", manifest.as_bytes()),
    ("debug.log", &log_bytes),
    ("install.log", &install_log_bytes),
    ("settings.json", settings_json.as_bytes()),
    ("lhm-data.json", lhm_json.as_bytes()),
    ("hardware.json", hardware_json.as_bytes()),
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
