//! Windows hardware detection via WMI and PowerShell fallbacks.
//!
//! All public functions here are called once at startup; their results are
//! stored in `AppState` so the per-tick hot path pays no WMI/process cost.
//! Each function tries WMI first and falls back to a PowerShell CIM call on
//! any COM/WMI failure, keeping the app functional even on locked-down systems.

use crate::debug::run_hidden_command;
use serde::Deserialize;

// --- WMI row structs -------------------------------------------------------
// Field names must match the WMI property names exactly (PascalCase).

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
  #[serde(rename = "Manufacturer")]
  manufacturer: Option<String>,
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
struct BaseBoardInfo {
  #[serde(rename = "Manufacturer")]
  manufacturer: Option<String>,
  #[serde(rename = "Product")]
  product: Option<String>,
}

#[cfg(windows)]
#[derive(Deserialize, Debug, Default)]
struct PowerShellBrandInfo {
  #[serde(rename = "computerSystemManufacturer")]
  computer_system_manufacturer: Option<String>,
  #[serde(rename = "computerSystemModel")]
  computer_system_model: Option<String>,
  #[serde(rename = "productName")]
  product_name: Option<String>,
  #[serde(rename = "productVersion")]
  product_version: Option<String>,
  #[serde(rename = "baseBoardManufacturer")]
  base_board_manufacturer: Option<String>,
  #[serde(rename = "baseBoardProduct")]
  base_board_product: Option<String>,
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

// --- WMI availability probe ------------------------------------------------

/// Verifies that WMI/CIM is reachable on the current system.
/// Called once at startup; the result is stored in `AppState.wmi_available`.
pub fn probe_wmi_status() -> Result<(), String> {
  #[cfg(windows)]
  {
    let com_probe_result = (|| -> Result<(), String> {
      let com = wmi::COMLibrary::new().map_err(|e| format!("COM init failed: {}", e))?;
      let conn = wmi::WMIConnection::new(com).map_err(|e| format!("WMI connection failed: {}", e))?;

      #[derive(Deserialize)]
      struct ProbeRow {
        #[serde(rename = "Caption")]
        caption: Option<String>,
      }

      let rows: Vec<ProbeRow> = conn
        .raw_query("SELECT Caption FROM Win32_OperatingSystem")
        .map_err(|e| format!("WMI query failed: {}", e))?;

      if rows
        .iter()
        .any(|r| r.caption.as_deref().is_some_and(|v| !v.trim().is_empty()))
      {
        Ok(())
      } else {
        Err("WMI query returned no usable rows".to_string())
      }
    })();

    if com_probe_result.is_ok() {
      return Ok(());
    }

    // Fallback: even if COM apartment init fails, CIM may still be available.
    let shell_probe = run_hidden_command(
      "powershell",
      &[
        "-NoProfile",
        "-Command",
        "(Get-CimInstance Win32_OperatingSystem | Select-Object -First 1 -ExpandProperty Caption) | Out-String",
      ],
    );

    if let Ok(out) = shell_probe {
      if out.status.success() {
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !text.is_empty() {
          return Ok(());
        }
      }
    }

    let com_error = com_probe_result
      .err()
      .unwrap_or_else(|| "Unknown WMI COM probe failure".to_string());
    Err(format!("{}; CIM fallback failed", com_error))
  }

  #[cfg(not(windows))]
  {
    Err("WMI is only available on Windows".to_string())
  }
}

// --- GPU name detection ----------------------------------------------------

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
fn get_gpu_name_from_shell() -> Option<String> {
  let output = run_hidden_command(
    "powershell",
    &[
      "-NoProfile",
      "-Command",
      "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name | Out-String",
    ],
  )
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

/// Detects the primary discrete GPU name.
/// Prefers WMI; falls back to PowerShell `Get-CimInstance`.
pub fn detect_gpu_name() -> Option<String> {
  #[cfg(windows)]
  {
    if let Ok(com) = wmi::COMLibrary::new() {
      if let Ok(conn) = wmi::WMIConnection::new(com) {
        if let Ok(rows) = conn.query::<VideoControllerName>() {
          let names = rows.into_iter().filter_map(|r| r.name).collect::<Vec<_>>();
          if let Some(best) = pick_best_gpu_name(names) {
            return Some(best);
          }
        }
      }
    }

    get_gpu_name_from_shell()
  }

  #[cfg(not(windows))]
  {
    None
  }
}

/// Detects the total VRAM in MB for the primary GPU.
/// Returns `None` when WMI is unavailable or reports no usable value.
/// LHM live data (`vram_total` in `LhmData`) is the preferred source; this is
/// only used as a startup fallback before the first LHM tick arrives.
pub fn detect_gpu_vram_total_mb() -> Option<f64> {
  #[cfg(windows)]
  {
    let com = wmi::COMLibrary::new().ok()?;
    let conn = wmi::WMIConnection::new(com).ok()?;
    let rows: Vec<VideoControllerMemory> = conn.query().ok()?;
    let best = rows.iter().filter_map(|r| r.adapter_ram).max().unwrap_or(0);
    if best > 0 {
      Some((best as f64 / 1_048_576.0).round())
    } else {
      None
    }
  }

  #[cfg(not(windows))]
  {
    None
  }
}

// --- System brand detection ------------------------------------------------

/// Maps OEM/product strings to a canonical brand slug used for logo selection.
#[cfg(windows)]
pub(crate) fn classify_system_brand(fields: &[&str]) -> &'static str {
  let normalized: Vec<String> = fields
    .iter()
    .map(|v| v.trim().to_ascii_lowercase())
    .filter(|v| !v.is_empty())
    .collect();

  let has_any = |needles: &[&str]| {
    normalized
      .iter()
      .any(|v| needles.iter().any(|needle| v.contains(needle)))
  };

  if has_any(&["alienware"]) {
    "alienware"
  } else if has_any(&["razer"]) {
    "razer"
  } else if has_any(&["legion"]) {
    "legion"
  } else if has_any(&["omen"]) {
    "omen"
  } else if has_any(&["predator"]) {
    "predator"
  } else if has_any(&["aorus"]) {
    "aorus"
  } else if has_any(&["asus", "rog", "republic of gamers"]) {
    "rog"
  } else if has_any(&["msi", "micro-star", "micro star"]) {
    "msi"
  } else if has_any(&["gigabyte"]) {
    "gigabyte"
  } else if has_any(&["asrock"]) {
    "asrock"
  } else if has_any(&["corsair"]) {
    "corsair"
  } else if has_any(&["nzxt"]) {
    "nzxt"
  } else if has_any(&["intel"]) {
    "intel"
  } else if has_any(&["dell"]) {
    "dell"
  } else if has_any(&["lenovo"]) {
    "lenovo"
  } else if has_any(&["hewlett-packard", "hp ", " hp", "hp-"]) {
    "hp"
  } else if has_any(&["acer"]) {
    "acer"
  } else {
    "other"
  }
}

/// Detects the system brand by querying WMI manufacturer/model/board fields.
pub fn detect_system_brand() -> String {
  #[cfg(windows)]
  {
    if let Ok(com) = wmi::COMLibrary::new() {
      if let Ok(conn) = wmi::WMIConnection::new(com) {
        let systems: Vec<ComputerSystem> = conn.query().ok().unwrap_or_default();
        let products: Vec<ComputerSystemProduct> = conn.query().ok().unwrap_or_default();
        let boards: Vec<BaseBoardInfo> = conn.query().ok().unwrap_or_default();

        let mut fields = Vec::new();
        if let Some(s) = systems.first() {
          if let Some(v) = s.manufacturer.as_deref() {
            fields.push(v);
          }
          if let Some(v) = s.model.as_deref() {
            fields.push(v);
          }
        }
        if let Some(p) = products.first() {
          if let Some(v) = p.name.as_deref() {
            fields.push(v);
          }
          if let Some(v) = p.version.as_deref() {
            fields.push(v);
          }
        }
        if let Some(b) = boards.first() {
          if let Some(v) = b.manufacturer.as_deref() {
            fields.push(v);
          }
          if let Some(v) = b.product.as_deref() {
            fields.push(v);
          }
        }

        if !fields.is_empty() {
          return classify_system_brand(&fields).to_string();
        }
      }
    }

    let output = run_hidden_command(
      "powershell",
      &[
        "-NoProfile",
        "-Command",
        "$cs = Get-CimInstance Win32_ComputerSystem; $csp = Get-CimInstance Win32_ComputerSystemProduct; $bb = Get-CimInstance Win32_BaseBoard; [pscustomobject]@{ computerSystemManufacturer = $cs.Manufacturer; computerSystemModel = $cs.Model; productName = $csp.Name; productVersion = $csp.Version; baseBoardManufacturer = $bb.Manufacturer; baseBoardProduct = $bb.Product } | ConvertTo-Json -Compress",
      ],
    );

    if let Ok(out) = output {
      if out.status.success() {
        let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Ok(info) = serde_json::from_str::<PowerShellBrandInfo>(&raw) {
          let mut fields = Vec::new();
          if let Some(v) = info.computer_system_manufacturer.as_deref() {
            fields.push(v);
          }
          if let Some(v) = info.computer_system_model.as_deref() {
            fields.push(v);
          }
          if let Some(v) = info.product_name.as_deref() {
            fields.push(v);
          }
          if let Some(v) = info.product_version.as_deref() {
            fields.push(v);
          }
          if let Some(v) = info.base_board_manufacturer.as_deref() {
            fields.push(v);
          }
          if let Some(v) = info.base_board_product.as_deref() {
            fields.push(v);
          }
          if !fields.is_empty() {
            return classify_system_brand(&fields).to_string();
          }
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

// --- Motherboard name detection --------------------------------------------

/// Normalises common OEM board manufacturer strings to a short display name.
/// Returns the trimmed input unchanged for vendors not explicitly listed.
#[cfg(windows)]
fn normalize_manufacturer(raw: &str) -> String {
  let lower = raw.trim().to_ascii_lowercase();
  if lower.contains("asustek") || lower.contains("asus") {
    "ASUS".to_string()
  } else if lower.contains("micro-star") || lower.contains("micro star") || lower == "msi" {
    "MSI".to_string()
  } else if lower.contains("gigabyte") {
    "Gigabyte".to_string()
  } else if lower.contains("asrock") {
    "ASRock".to_string()
  } else if lower.contains("evga") {
    "EVGA".to_string()
  } else {
    raw.trim().to_string()
  }
}

/// Detects the motherboard name as "Manufacturer Product" (e.g. "ASUS PRIME B650M-A AX6 II").
/// Returns `None` when WMI is unavailable or the board fields are BIOS placeholders.
/// Falls back to PowerShell `Get-CimInstance` if WMI fails.
pub fn detect_motherboard_name() -> Option<String> {
  #[cfg(windows)]
  {
    if let Ok(com) = wmi::COMLibrary::new() {
      if let Ok(conn) = wmi::WMIConnection::new(com) {
        let boards: Vec<BaseBoardInfo> = conn.query().ok().unwrap_or_default();
        if let Some(b) = boards.first() {
          let product = b.product.as_deref().and_then(normalize_model_name)?;
          let mfr = normalize_manufacturer(b.manufacturer.as_deref().unwrap_or(""));
          return Some(if mfr.is_empty() {
            product
          } else {
            format!("{mfr} {product}")
          });
        }
      }
    }

    // Fallback: query via PowerShell CIM if WMI is unavailable.
    let output = run_hidden_command(
      "powershell",
      &[
        "-NoProfile",
        "-Command",
        "$bb=Get-CimInstance Win32_BaseBoard;[pscustomobject]@{Manufacturer=$bb.Manufacturer;Product=$bb.Product}|ConvertTo-Json -Compress",
      ],
    )
    .ok()?;
    if !output.status.success() {
      return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let b = serde_json::from_str::<BaseBoardInfo>(&raw).ok()?;
    let product = b.product.as_deref().and_then(normalize_model_name)?;
    let mfr = normalize_manufacturer(b.manufacturer.as_deref().unwrap_or(""));
    Some(if mfr.is_empty() {
      product
    } else {
      format!("{mfr} {product}")
    })
  }

  #[cfg(not(windows))]
  {
    None
  }
}

// --- Model name detection --------------------------------------------------

#[cfg(windows)]
#[derive(Deserialize, Debug, Default)]
struct ModelNameInfo {
  #[serde(rename = "cspVersion")]
  csp_version: Option<String>,
  #[serde(rename = "cspName")]
  csp_name: Option<String>,
  #[serde(rename = "csModel")]
  cs_model: Option<String>,
}

fn normalize_model_name(raw: &str) -> Option<String> {
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return None;
  }
  let invalid = [
    "to be filled by o.e.m.",
    "system product name",
    "system version",
    "default string",
    "unknown",
    "none",
    "n/a",
    "not applicable",
  ];
  let lower = trimmed.to_ascii_lowercase();
  if invalid.iter().any(|x| lower == *x) {
    return None;
  }
  Some(trimmed.to_string())
}

/// Returns true if the given model name is a known BIOS placeholder that
/// should be replaced by auto-detection on the next startup.
pub(crate) fn is_placeholder_model_name(name: &str) -> bool {
  normalize_model_name(name).is_none()
}

/// Detects the system model name from WMI `Win32_ComputerSystemProduct`.
/// Falls back to PowerShell `Get-CimInstance` if WMI is unavailable.
pub fn detect_model_name() -> Option<String> {
  #[cfg(windows)]
  {
    if let Ok(com) = wmi::COMLibrary::new() {
      if let Ok(conn) = wmi::WMIConnection::new(com) {
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
        if let Some(v) = systems
          .iter()
          .filter_map(|s| s.model.as_deref().and_then(normalize_model_name))
          .next()
        {
          return Some(v);
        }
      }
    }

    // Fallback: query via PowerShell CIM if WMI is unavailable.
    let output = run_hidden_command(
      "powershell",
      &[
        "-NoProfile",
        "-Command",
        "$csp=Get-CimInstance Win32_ComputerSystemProduct;$cs=Get-CimInstance Win32_ComputerSystem;[pscustomobject]@{cspVersion=$csp.Version;cspName=$csp.Name;csModel=$cs.Model}|ConvertTo-Json -Compress",
      ],
    )
    .ok()?;
    if !output.status.success() {
      return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let info = serde_json::from_str::<ModelNameInfo>(&raw).ok()?;
    if let Some(v) = info.csp_version.as_deref().and_then(normalize_model_name) {
      return Some(v);
    }
    if let Some(v) = info.csp_name.as_deref().and_then(normalize_model_name) {
      return Some(v);
    }
    info.cs_model.as_deref().and_then(normalize_model_name)
  }

  #[cfg(not(windows))]
  {
    None
  }
}

// --- RAM detection ---------------------------------------------------------

#[cfg(windows)]
fn map_memory_type(code: u16) -> Option<&'static str> {
  // Codes apply to both Win32_PhysicalMemory.MemoryType and .SMBIOSMemoryType.
  // SMBIOSMemoryType follows the SMBIOS spec; MemoryType uses WMI-specific values
  // that mostly overlap for DDR3+.  Both sources are tried in order, so this
  // single table must work for both.  LPDDR variants only appear in SMBIOSMemoryType.
  match code {
    18 => Some("DDR"),    // MemoryType=18 (WMI), SMBIOSMemoryType overlaps are rare
    20 => Some("DDR2"),   // MemoryType=20 (WMI DDR2 FB-DIMM, close enough)
    24 => Some("DDR3"),   // MemoryType=24 / SMBIOSMemoryType=24
    26 => Some("DDR4"),   // MemoryType=26 / SMBIOSMemoryType=26
    27 => Some("LPDDR"),  // SMBIOSMemoryType=27
    28 => Some("LPDDR2"), // SMBIOSMemoryType=28
    29 => Some("LPDDR3"), // SMBIOSMemoryType=29
    30 => Some("LPDDR4"), // SMBIOSMemoryType=30
    34 => Some("DDR5"),   // MemoryType=34 / SMBIOSMemoryType=34
    35 => Some("LPDDR5"), // SMBIOSMemoryType=35
    _ => None,
  }
}

/// Detects the installed RAM spec string (e.g. "DDR5 6000 MT/s (2 DIMMs)").
pub fn detect_ram_spec() -> String {
  #[cfg(windows)]
  fn detect_ram_spec_from_shell() -> Option<String> {
    let output = run_hidden_command(
      "powershell",
      &[
        "-NoProfile",
        "-Command",
        "$m = Get-CimInstance Win32_PhysicalMemory; if(-not $m){ return }; $dimms = $m.Count; $speed = ($m | ForEach-Object { if($_.ConfiguredClockSpeed){ $_.ConfiguredClockSpeed } else { $_.Speed } } | Measure-Object -Maximum).Maximum; $typeCode = ($m | Select-Object -First 1 -ExpandProperty SMBIOSMemoryType); if(-not $typeCode){ $typeCode = ($m | Select-Object -First 1 -ExpandProperty MemoryType) }; $type = switch([int]$typeCode){ 18 {'DDR'} 20 {'DDR2'} 24 {'DDR3'} 26 {'DDR4'} 34 {'DDR5'} default {''} }; if($type -and $speed){ \"$type $speed MT/s ($dimms DIMMs)\" } elseif($type){ \"$type ($dimms DIMMs)\" } elseif($speed){ \"$speed MT/s ($dimms DIMMs)\" } else { \"RAM ($dimms DIMMs)\" } | Out-String",
      ],
    )
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
    let conn = match wmi::WMIConnection::new(com) {
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
    // SMBIOSMemoryType returns 0 on many DDR5 boards (a known BIOS quirk).
    // Try it first, but fall through to MemoryType if it doesn't map.
    let ram_type = sticks.iter().find_map(|s| {
      s.smbios_memory_type
        .and_then(map_memory_type)
        .or_else(|| s.memory_type.and_then(map_memory_type))
    });

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

/// Detects RAM module details (e.g. "2x16 GB | Kingston | KF560C36-16").
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
    let output = run_hidden_command(
      "powershell",
      &[
        "-NoProfile",
        "-Command",
        "$m = Get-CimInstance Win32_PhysicalMemory; if(-not $m){ return }; $count = $m.Count; $caps = @($m | ForEach-Object { [math]::Round($_.Capacity / 1GB) }); $layout = if((@($caps | Select-Object -Unique)).Count -eq 1 -and $caps.Count -gt 0) { \"${count}x$($caps[0]) GB\" } else { \"${count} DIMMs\" }; $vendor = ($m | Select-Object -First 1 -ExpandProperty Manufacturer); $part = ($m | Select-Object -First 1 -ExpandProperty PartNumber); \"$layout|$vendor|$part\" | Out-String",
      ],
    )
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
    let conn = match wmi::WMIConnection::new(com) {
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
      if sizes_gb.iter().all(|v| *v == first) {
        pieces.push(format!("{}x{} GB", sizes_gb.len(), first));
      } else {
        pieces.push(format!("{} DIMMs", sizes_gb.len()));
      }
    } else {
      pieces.push(format!("{} DIMMs", sticks.len()));
    }

    if let Some(v) = sticks
      .iter()
      .filter_map(|s| s.manufacturer.as_deref())
      .find_map(sanitize_ram_field)
    {
      pieces.push(v);
    }
    if let Some(p) = sticks
      .iter()
      .filter_map(|s| s.part_number.as_deref())
      .find_map(sanitize_ram_field)
    {
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

// --- Ping target detection -------------------------------------------------

/// Detects the default network gateway to use as the ping target.
/// Falls back to `1.1.1.1` if no gateway is found.
pub fn detect_ping_target() -> String {
  #[cfg(windows)]
  {
    let output = run_hidden_command(
      "powershell",
      &[
        "-NoProfile",
        "-Command",
        "(Get-CimInstance Win32_NetworkAdapterConfiguration | Where-Object { $_.IPEnabled -and $_.DefaultIPGateway } | ForEach-Object { $_.DefaultIPGateway } | Where-Object { $_ -match '^\\d+\\.\\d+\\.\\d+\\.\\d+$' } | Select-Object -First 1) | Out-String",
      ],
    );

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

  // Windows ping summary ends with the average latency in ms.
  numbers.last().copied()
}

/// Sends a single ICMP ping and returns the round-trip time in milliseconds.
pub(crate) fn sample_ping_ms(target: &str) -> Option<f64> {
  #[cfg(windows)]
  {
    let output = run_hidden_command("ping", &["-n", "1", "-w", "500", target]).ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    parse_ping_output_ms(&text)
  }

  #[cfg(not(windows))]
  {
    None
  }
}

// --- Tests -----------------------------------------------------------------

#[cfg(test)]
mod cross_platform_tests {
  use super::{is_placeholder_model_name, normalize_model_name, parse_ping_output_ms};

  #[test]
  fn normalize_model_name_accepts_real_names() {
    assert_eq!(normalize_model_name("ROG GM700TZ"), Some("ROG GM700TZ".to_string()));
    assert_eq!(
      normalize_model_name("PRIME B650M-A AX6 II"),
      Some("PRIME B650M-A AX6 II".to_string())
    );
  }

  #[test]
  fn normalize_model_name_trims_whitespace() {
    assert_eq!(normalize_model_name("  ROG GM700TZ  "), Some("ROG GM700TZ".to_string()));
  }

  #[test]
  fn normalize_model_name_rejects_empty_and_whitespace() {
    assert_eq!(normalize_model_name(""), None);
    assert_eq!(normalize_model_name("   "), None);
  }

  #[test]
  fn normalize_model_name_rejects_all_known_placeholders() {
    let placeholders = [
      "To Be Filled By O.E.M.",
      "System Product Name",
      "System Version",
      "Default String",
      "Unknown",
      "None",
      "N/A",
      "Not Applicable",
    ];
    for p in &placeholders {
      assert_eq!(normalize_model_name(p), None, "expected None for placeholder: {p}");
    }
  }

  #[test]
  fn normalize_model_name_placeholder_check_is_case_insensitive() {
    assert_eq!(normalize_model_name("SYSTEM VERSION"), None);
    assert_eq!(normalize_model_name("system version"), None);
    assert_eq!(normalize_model_name("TO BE FILLED BY O.E.M."), None);
  }

  #[test]
  fn is_placeholder_true_for_known_placeholders() {
    assert!(is_placeholder_model_name("System Version"));
    assert!(is_placeholder_model_name(""));
    assert!(is_placeholder_model_name("  "));
    assert!(is_placeholder_model_name("Unknown"));
  }

  #[test]
  fn is_placeholder_false_for_real_model_names() {
    assert!(!is_placeholder_model_name("ROG GM700TZ"));
    assert!(!is_placeholder_model_name("PRIME B650M-A AX6 II"));
  }

  #[test]
  fn parse_ping_output_ms_extracts_average_from_windows_output() {
    let output = "Pinging 1.1.1.1 with 32 bytes of data:\r\n\
      Reply from 1.1.1.1: bytes=32 time=12ms TTL=57\r\n\
      Ping statistics for 1.1.1.1:\r\n\
          Packets: Sent = 1, Received = 1, Lost = 0 (0% loss),\r\n\
      Approximate round trip times in milli-seconds:\r\n\
          Minimum = 12ms, Maximum = 12ms, Average = 12ms";
    assert_eq!(parse_ping_output_ms(output), Some(12.0));
  }

  #[test]
  fn parse_ping_output_ms_last_number_is_average() {
    assert_eq!(
      parse_ping_output_ms("Minimum = 5ms, Maximum = 15ms, Average = 10ms"),
      Some(10.0)
    );
  }

  #[test]
  fn parse_ping_output_ms_returns_none_for_empty() {
    assert_eq!(parse_ping_output_ms(""), None);
  }

  #[test]
  fn parse_ping_output_ms_returns_none_for_no_numbers() {
    assert_eq!(parse_ping_output_ms("Request timed out."), None);
  }
}

#[cfg(all(test, windows))]
mod windows_tests {
  use super::normalize_manufacturer;

  #[test]
  fn normalize_manufacturer_maps_asustek_variants() {
    assert_eq!(normalize_manufacturer("ASUSTeK COMPUTER INC."), "ASUS");
    assert_eq!(normalize_manufacturer("ASUS"), "ASUS");
    assert_eq!(normalize_manufacturer("  ASUSTeK  "), "ASUS");
  }

  #[test]
  fn normalize_manufacturer_maps_msi_variants() {
    assert_eq!(normalize_manufacturer("Micro-Star International Co., Ltd."), "MSI");
    assert_eq!(normalize_manufacturer("Micro Star International"), "MSI");
    assert_eq!(normalize_manufacturer("MSI"), "MSI");
  }

  #[test]
  fn normalize_manufacturer_maps_gigabyte() {
    assert_eq!(normalize_manufacturer("Gigabyte Technology Co., Ltd."), "Gigabyte");
    assert_eq!(normalize_manufacturer("GIGABYTE"), "Gigabyte");
  }

  #[test]
  fn normalize_manufacturer_maps_asrock() {
    assert_eq!(normalize_manufacturer("ASRock Incorporation"), "ASRock");
    assert_eq!(normalize_manufacturer("asrock"), "ASRock");
  }

  #[test]
  fn normalize_manufacturer_maps_evga() {
    assert_eq!(normalize_manufacturer("EVGA"), "EVGA");
    assert_eq!(normalize_manufacturer("evga"), "EVGA");
  }

  #[test]
  fn normalize_manufacturer_passes_through_unknown_trimmed() {
    assert_eq!(normalize_manufacturer("  SuperMicro  "), "SuperMicro");
    assert_eq!(normalize_manufacturer("Biostar"), "Biostar");
  }
}

// --- Disk letter → model map -----------------------------------------------

/// Returns a map from drive letter (e.g. `"C:"`) to physical disk model name
/// (e.g. `"Samsung SSD 980 PRO"`).  Used to match LHM temperature readings to
/// sysinfo volumes without relying on fragile index ordering.
///
/// Queries three WMI association tables and joins them in memory:
///   Win32_DiskDrive → Win32_DiskDriveToDiskPartition → Win32_LogicalDiskToPartition
///
/// Falls back to a PowerShell CIM command on any WMI failure.
/// Returns an empty map when both paths fail so callers degrade gracefully.
pub fn detect_disk_model_map() -> std::collections::HashMap<String, String> {
  #[cfg(windows)]
  {
    // --- WMI path -----------------------------------------------------------
    if let Some(map) = try_disk_model_map_via_wmi() {
      if !map.is_empty() {
        return map;
      }
    }

    // --- PowerShell fallback ------------------------------------------------
    try_disk_model_map_via_shell().unwrap_or_default()
  }

  #[cfg(not(windows))]
  {
    std::collections::HashMap::new()
  }
}

#[cfg(windows)]
fn try_disk_model_map_via_wmi() -> Option<std::collections::HashMap<String, String>> {
  use serde::Deserialize;

  #[derive(Deserialize)]
  struct DiskDriveRow {
    #[serde(rename = "DeviceID")]
    device_id: Option<String>,
    #[serde(rename = "Model")]
    model: Option<String>,
  }

  // WMI association rows return references as plain strings (the WMI object path).
  // We only need the two DeviceID values embedded in those paths, so we store
  // them as strings and parse out the IDs afterwards.
  #[derive(Deserialize)]
  struct DiskToPartRow {
    #[serde(rename = "Antecedent")]
    antecedent: Option<String>, // Win32_DiskDrive path
    #[serde(rename = "Dependent")]
    dependent: Option<String>, // Win32_DiskPartition path
  }

  #[derive(Deserialize)]
  struct PartToLogicalRow {
    #[serde(rename = "Antecedent")]
    antecedent: Option<String>, // Win32_DiskPartition path
    #[serde(rename = "Dependent")]
    dependent: Option<String>, // Win32_LogicalDisk path (contains drive letter)
  }

  // Extract the bare DeviceID value from a WMI object path string like:
  //   \\HOST\root\cimv2:Win32_DiskDrive.DeviceID="\\\\.\\PHYSICALDRIVE0"
  // Returns the value between the outer quotes, or None.
  fn extract_device_id(path: &str) -> Option<String> {
    let eq = path.find(".DeviceID=")?;
    let after = &path[eq + ".DeviceID=".len()..];
    // Value may be quoted or unquoted.
    let value = if after.starts_with('"') {
      after
        .trim_start_matches('"')
        .trim_end_matches('"')
        .replace("\\\\", "\\")
    } else {
      after.trim_end_matches('"').to_string()
    };
    Some(value)
  }

  let com = wmi::COMLibrary::new().ok()?;
  let conn = wmi::WMIConnection::new(com).ok()?;

  let drives: Vec<DiskDriveRow> = conn.raw_query("SELECT DeviceID, Model FROM Win32_DiskDrive").ok()?;
  let mut disk_id_to_model: std::collections::HashMap<String, String> = drives
    .into_iter()
    .filter_map(|r| Some((r.device_id?.trim().to_string(), r.model?.trim().to_string())))
    .collect();

  let disk_to_part: Vec<DiskToPartRow> = conn
    .raw_query("SELECT Antecedent, Dependent FROM Win32_DiskDriveToDiskPartition")
    .ok()?;
  // partition_id → disk_model
  let mut part_to_model: std::collections::HashMap<String, String> = disk_to_part
    .into_iter()
    .filter_map(|r| {
      let disk_path = r.antecedent?;
      let part_path = r.dependent?;
      let disk_id = extract_device_id(&disk_path)?;
      let part_id = extract_device_id(&part_path)?;
      let model = disk_id_to_model.remove(&disk_id)?;
      Some((part_id, model))
    })
    .collect();

  let part_to_logical: Vec<PartToLogicalRow> = conn
    .raw_query("SELECT Antecedent, Dependent FROM Win32_LogicalDiskToPartition")
    .ok()?;
  let map: std::collections::HashMap<String, String> = part_to_logical
    .into_iter()
    .filter_map(|r| {
      let part_path = r.antecedent?;
      let logical_path = r.dependent?;
      let part_id = extract_device_id(&part_path)?;
      let drive_letter = extract_device_id(&logical_path)?;
      let model = part_to_model.remove(&part_id)?;
      Some((drive_letter, model))
    })
    .collect();

  Some(map)
}

#[cfg(windows)]
fn try_disk_model_map_via_shell() -> Option<std::collections::HashMap<String, String>> {
  // Builds the same three-table join via CIM cmdlets and outputs compact JSON:
  // [{"letter":"C:","model":"Samsung SSD 980 PRO"},...]
  let output = run_hidden_command(
    "powershell",
    &[
      "-NoProfile",
      "-Command",
      concat!(
        "$m=@{};",
        "Get-CimInstance Win32_DiskDrive|ForEach-Object{$m[$_.DeviceID]=$_.Model};",
        "$pd=@{};",
        "Get-CimInstance Win32_DiskDriveToDiskPartition|ForEach-Object{$pd[$_.Dependent.DeviceID]=$m[$_.Antecedent.DeviceID]};",
        "$out=@();",
        "Get-CimInstance Win32_LogicalDiskToPartition|ForEach-Object{",
        "  $model=$pd[$_.Antecedent.DeviceID];",
        "  if($model){$out+=[PSCustomObject]@{letter=$_.Dependent.DeviceID;model=$model}}",
        "};",
        "@($out)|ConvertTo-Json -Compress"
      ),
    ],
  )
  .ok()?;

  if !output.status.success() {
    return None;
  }

  #[derive(serde::Deserialize)]
  struct Entry {
    letter: String,
    model: String,
  }

  let text = String::from_utf8_lossy(&output.stdout);
  let trimmed = text.trim();
  if trimmed.is_empty() || trimmed == "null" {
    return None;
  }
  let entries: Vec<Entry> = serde_json::from_str(trimmed).ok()?;
  Some(
    entries
      .into_iter()
      .map(|e| (e.letter.trim().to_string(), e.model.trim().to_string()))
      .collect(),
  )
}

#[cfg(all(test, windows))]
mod tests {
  use super::{classify_system_brand, gpu_name_score, map_memory_type, pick_best_gpu_name};

  // map_memory_type

  #[test]
  fn map_memory_type_returns_correct_labels_for_all_ddr_codes() {
    // Codes that must map correctly for desktop/server RAM.
    assert_eq!(map_memory_type(18), Some("DDR"));
    assert_eq!(map_memory_type(20), Some("DDR2"));
    assert_eq!(map_memory_type(24), Some("DDR3"));
    assert_eq!(map_memory_type(26), Some("DDR4"));
    assert_eq!(map_memory_type(34), Some("DDR5"));
  }

  #[test]
  fn map_memory_type_returns_correct_labels_for_lpddr_smbios_codes() {
    // LPDDR variants are reported via SMBIOSMemoryType on laptops.
    // These codes were missing before the fix and caused "RAM" to be shown.
    assert_eq!(map_memory_type(27), Some("LPDDR"));
    assert_eq!(map_memory_type(28), Some("LPDDR2"));
    assert_eq!(map_memory_type(29), Some("LPDDR3"));
    assert_eq!(map_memory_type(30), Some("LPDDR4"));
    assert_eq!(map_memory_type(35), Some("LPDDR5"));
  }

  #[test]
  fn map_memory_type_returns_none_for_zero_so_smbios_fallback_works() {
    // Many DDR5 boards report SMBIOSMemoryType = 0 (unknown).
    // Returning None here allows the caller to fall through to MemoryType,
    // which typically carries the correct code.  A non-None result would
    // suppress the fallback and leave the type field empty.
    assert_eq!(map_memory_type(0), None, "code 0 must not map to a label");
  }

  #[test]
  fn map_memory_type_returns_none_for_unknown_codes() {
    assert_eq!(map_memory_type(1), None);
    assert_eq!(map_memory_type(255), None);
  }

  #[test]
  fn classify_system_brand_recognizes_rog_aliases() {
    assert_eq!(classify_system_brand(&["ASUSTeK COMPUTER INC."]), "rog");
    assert_eq!(classify_system_brand(&["Republic of Gamers"]), "rog");
  }

  #[test]
  fn classify_system_brand_recognizes_product_lines_before_oem() {
    assert_eq!(
      classify_system_brand(&["Dell Inc.", "Alienware Aurora R16"]),
      "alienware"
    );
    assert_eq!(classify_system_brand(&["LENOVO", "Legion T7 34IRZ8"]), "legion");
    assert_eq!(classify_system_brand(&["HP", "OMEN 45L Desktop GT22"]), "omen");
    assert_eq!(classify_system_brand(&["Acer", "Predator Orion 7000"]), "predator");
    assert_eq!(
      classify_system_brand(&["Gigabyte Technology Co., Ltd.", "AORUS MODEL X"]),
      "aorus"
    );
  }

  #[test]
  fn classify_system_brand_recognizes_oem_brands() {
    assert_eq!(classify_system_brand(&["Micro-Star International Co., Ltd"]), "msi");
    assert_eq!(classify_system_brand(&["Gigabyte Technology Co., Ltd."]), "gigabyte");
    assert_eq!(classify_system_brand(&["Razer"]), "razer");
    assert_eq!(classify_system_brand(&["NZXT"]), "nzxt");
    assert_eq!(classify_system_brand(&["Corsair"]), "corsair");
  }

  #[test]
  fn classify_system_brand_falls_back_to_other() {
    assert_eq!(classify_system_brand(&["Some Unknown Vendor"]), "other");
  }

  #[test]
  fn gpu_name_score_prefers_discrete_over_integrated() {
    assert!(gpu_name_score("NVIDIA GeForce RTX 4090") > gpu_name_score("Intel UHD Graphics 770"));
    assert!(gpu_name_score("AMD Radeon RX 7900 XTX") > gpu_name_score("AMD Radeon Graphics"));
  }

  #[test]
  fn gpu_name_score_rejects_virtual_adapters() {
    assert!(gpu_name_score("Microsoft Basic Display Adapter") < 0);
    assert!(gpu_name_score("Hyper-V Video") < 0);
    assert!(gpu_name_score("Microsoft Basic Render Driver") < 0);
  }

  #[test]
  fn pick_best_gpu_name_selects_discrete_gpu() {
    let names = vec![
      "Intel UHD Graphics 770".to_string(),
      "NVIDIA GeForce RTX 4090".to_string(),
    ];
    assert_eq!(pick_best_gpu_name(names), Some("NVIDIA GeForce RTX 4090".to_string()));
  }

  #[test]
  fn pick_best_gpu_name_skips_empty_strings() {
    let names = vec!["".to_string(), "  ".to_string(), "AMD Radeon RX 7900 XTX".to_string()];
    assert_eq!(pick_best_gpu_name(names), Some("AMD Radeon RX 7900 XTX".to_string()));
  }

  #[test]
  fn pick_best_gpu_name_returns_none_for_empty_list() {
    let names: Vec<String> = vec![];
    assert_eq!(pick_best_gpu_name(names), None);
  }
}
