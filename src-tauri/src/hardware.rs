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
      let conn = wmi::WMIConnection::new(com.into())
        .map_err(|e| format!("WMI connection failed: {}", e))?;

      #[derive(Deserialize)]
      struct ProbeRow {
        #[serde(rename = "Caption")]
        caption: Option<String>,
      }

      let rows: Vec<ProbeRow> = conn
        .raw_query("SELECT Caption FROM Win32_OperatingSystem")
        .map_err(|e| format!("WMI query failed: {}", e))?;

      if rows.iter().any(|r| r.caption.as_deref().is_some_and(|v| !v.trim().is_empty())) {
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
      if let Ok(conn) = wmi::WMIConnection::new(com.into()) {
        if let Ok(rows) = conn.query::<VideoControllerName>() {
          let names = rows.into_iter().filter_map(|r| r.name).collect::<Vec<_>>();
          if let Some(best) = pick_best_gpu_name(names) {
            return Some(best);
          }
        }
      }
    }

    return get_gpu_name_from_shell();
  }

  #[cfg(not(windows))]
  {
    None
  }
}

/// Detects the total VRAM in MB for the primary GPU.
/// Falls back to 16 384 MB (16 GB) when WMI is unavailable.
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

  if has_any(&["alienware"]) { "alienware" }
  else if has_any(&["razer"]) { "razer" }
  else if has_any(&["legion"]) { "legion" }
  else if has_any(&["omen"]) { "omen" }
  else if has_any(&["predator"]) { "predator" }
  else if has_any(&["aorus"]) { "aorus" }
  else if has_any(&["asus", "rog", "republic of gamers"]) { "rog" }
  else if has_any(&["msi", "micro-star", "micro star"]) { "msi" }
  else if has_any(&["gigabyte"]) { "gigabyte" }
  else if has_any(&["asrock"]) { "asrock" }
  else if has_any(&["corsair"]) { "corsair" }
  else if has_any(&["nzxt"]) { "nzxt" }
  else if has_any(&["intel"]) { "intel" }
  else if has_any(&["dell"]) { "dell" }
  else if has_any(&["lenovo"]) { "lenovo" }
  else if has_any(&["hewlett-packard", "hp ", " hp", "hp-"]) { "hp" }
  else if has_any(&["acer"]) { "acer" }
  else { "other" }
}

/// Detects the system brand by querying WMI manufacturer/model/board fields.
pub fn detect_system_brand() -> String {
  #[cfg(windows)]
  {
    if let Ok(com) = wmi::COMLibrary::new() {
      if let Ok(conn) = wmi::WMIConnection::new(com.into()) {
        let systems: Vec<ComputerSystem> = conn.query().ok().unwrap_or_default();
        let products: Vec<ComputerSystemProduct> = conn.query().ok().unwrap_or_default();
        let boards: Vec<BaseBoardInfo> = conn.query().ok().unwrap_or_default();

        let mut fields = Vec::new();
        if let Some(s) = systems.first() {
          if let Some(v) = s.manufacturer.as_deref() { fields.push(v); }
          if let Some(v) = s.model.as_deref() { fields.push(v); }
        }
        if let Some(p) = products.first() {
          if let Some(v) = p.name.as_deref() { fields.push(v); }
          if let Some(v) = p.version.as_deref() { fields.push(v); }
        }
        if let Some(b) = boards.first() {
          if let Some(v) = b.manufacturer.as_deref() { fields.push(v); }
          if let Some(v) = b.product.as_deref() { fields.push(v); }
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
          if let Some(v) = info.computer_system_manufacturer.as_deref() { fields.push(v); }
          if let Some(v) = info.computer_system_model.as_deref() { fields.push(v); }
          if let Some(v) = info.product_name.as_deref() { fields.push(v); }
          if let Some(v) = info.product_version.as_deref() { fields.push(v); }
          if let Some(v) = info.base_board_manufacturer.as_deref() { fields.push(v); }
          if let Some(v) = info.base_board_product.as_deref() { fields.push(v); }
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

// --- Model name detection --------------------------------------------------

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

/// Detects the system model name from WMI `Win32_ComputerSystemProduct`.
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

// --- RAM detection ---------------------------------------------------------

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
    if text.is_empty() { None } else { Some(text) }
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
      if sizes_gb.iter().all(|v| *v == first) {
        pieces.push(format!("{}x{} GB", sizes_gb.len(), first));
      } else {
        pieces.push(format!("{} DIMMs", sizes_gb.len()));
      }
    } else {
      pieces.push(format!("{} DIMMs", sticks.len()));
    }

    if let Some(v) = sticks.iter().filter_map(|s| s.manufacturer.as_deref()).find_map(sanitize_ram_field) {
      pieces.push(v);
    }
    if let Some(p) = sticks.iter().filter_map(|s| s.part_number.as_deref()).find_map(sanitize_ram_field) {
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

#[cfg(all(test, windows))]
mod tests {
  use super::classify_system_brand;

  #[test]
  fn classify_system_brand_recognizes_rog_aliases() {
    assert_eq!(classify_system_brand(&["ASUSTeK COMPUTER INC."]), "rog");
    assert_eq!(classify_system_brand(&["Republic of Gamers"]), "rog");
  }

  #[test]
  fn classify_system_brand_recognizes_product_lines_before_oem() {
    assert_eq!(classify_system_brand(&["Dell Inc.", "Alienware Aurora R16"]), "alienware");
    assert_eq!(classify_system_brand(&["LENOVO", "Legion T7 34IRZ8"]), "legion");
    assert_eq!(classify_system_brand(&["HP", "OMEN 45L Desktop GT22"]), "omen");
    assert_eq!(classify_system_brand(&["Acer", "Predator Orion 7000"]), "predator");
    assert_eq!(classify_system_brand(&["Gigabyte Technology Co., Ltd.", "AORUS MODEL X"]), "aorus");
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
}
