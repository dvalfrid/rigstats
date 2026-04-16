//! LibreHardwareMonitor JSON parsing and transport helpers.
//!
//! LHM publishes a nested tree structure. We flatten it into simple nodes, then
//! extract metrics by parent/text pairs for stable lookup.

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct FlatNode {
  pub text: String,
  pub value: String,
  pub parent: String,
  /// One level above `parent` — used to recover the device name for grouped sensors.
  pub grandparent: String,
  /// LHM sensor ID (e.g. `/nvme/0/temperature/0`) — used to distinguish disk sensors
  /// from identically-named sensors on other hardware (motherboard, RAM, etc.).
  pub sensor_id: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LhmData {
  /// Device name of the GPU currently selected for display (grandparent in LHM tree).
  pub gpu_name: Option<String>,
  pub gpu_load: Option<f64>,
  pub gpu_temp: Option<f64>,
  pub gpu_hotspot: Option<f64>,
  pub gpu_freq: Option<f64>,
  pub gpu_mem_freq: Option<f64>,
  pub gpu_power: Option<f64>,
  pub gpu_fan: Option<f64>,
  pub vram_used: Option<f64>,
  pub vram_total: Option<f64>,
  pub gpu_d3d_3d: Option<f64>,
  pub gpu_d3d_vdec: Option<f64>,
  pub cpu_temp: Option<f64>,
  pub cpu_power: Option<f64>,
  pub ram_temp: Option<f64>,
  /// Active motherboard fan channels: `(label, rpm)`, sorted descending by RPM, capped at 5.
  /// Channels reporting 0 RPM are excluded (LHM sentinel for disconnected/inactive headers).
  /// Extracted from `/lpc/` sensors so any Super I/O chip variant is covered without naming it.
  pub mb_fans: Vec<(String, f64)>,
  /// Motherboard temperature sensors from the Super I/O chip.
  /// Values < 5 °C are filtered out — LHM uses near-zero as a sentinel for unconfigured slots.
  pub mb_temps: Vec<(String, f64)>,
  /// Named voltage rails from the Super I/O chip.
  /// Generic "Voltage #N" slots (unmapped hardware pins) are excluded.
  pub mb_voltages: Vec<(String, f64)>,
  /// Super I/O chip name (e.g. "Nuvoton NCT6799D"), taken from the grandparent of the first
  /// `/lpc/` sensor. `None` when no LPC sensors are present (laptops, LHM not running).
  pub mb_chip: Option<String>,
  pub disk_read: f64,
  pub disk_write: f64,
  pub net_up: f64,
  pub net_down: f64,
  /// Per-device disk temperatures: `(device_name, temp_celsius)`, in LHM device order.
  pub disk_temps: Vec<(String, f64)>,
}

fn parse_val(str_val: &str) -> Option<f64> {
  // LHM values can include units and locale commas; keep only numeric content.
  let cleaned = str_val.replace(',', ".");
  let filtered: String = cleaned
    .chars()
    .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
    .collect();
  if filtered.is_empty() {
    return None;
  }
  filtered.parse::<f64>().ok()
}

fn flatten_lhm(value: &Value, results: &mut Vec<FlatNode>, parent: &str, grandparent: &str) {
  // Recursively flatten the tree so sensor lookups become linear scans.
  let text = value
    .get("Text")
    .or_else(|| value.get("text"))
    .and_then(Value::as_str)
    .unwrap_or("")
    .to_string();

  let node_val = value
    .get("Value")
    .or_else(|| value.get("value"))
    .and_then(Value::as_str)
    .unwrap_or("")
    .to_string();

  let sensor_id = value
    .get("SensorId")
    .or_else(|| value.get("sensorId"))
    .and_then(Value::as_str)
    .unwrap_or("")
    .to_string();

  if !node_val.is_empty() && node_val != "Value" {
    results.push(FlatNode {
      text: text.clone(),
      value: node_val,
      parent: parent.to_string(),
      grandparent: grandparent.to_string(),
      sensor_id,
    });
  }

  if let Some(children) = value
    .get("Children")
    .or_else(|| value.get("children"))
    .and_then(Value::as_array)
  {
    let next_parent = if text.is_empty() { parent } else { &text };
    let next_grandparent = if text.is_empty() { grandparent } else { parent };
    for child in children {
      flatten_lhm(child, results, next_parent, next_grandparent);
    }
  }
}

// --- Helpers ---------------------------------------------------------------

/// Converts an LHM throughput value string to MB/s, handling KB and GB suffixes.
fn to_mbs(raw: &str) -> f64 {
  let v = parse_val(raw).unwrap_or(0.0);
  if raw.contains("KB") {
    v / 1024.0
  } else if raw.contains("GB") {
    v * 1024.0
  } else {
    v
  }
}

struct GpuData {
  name: Option<String>,
  load: Option<f64>,
  temp: Option<f64>,
  hotspot: Option<f64>,
  freq: Option<f64>,
  mem_freq: Option<f64>,
  power: Option<f64>,
  fan: Option<f64>,
  vram_used: Option<f64>,
  vram_total: Option<f64>,
  d3d_3d: Option<f64>,
  d3d_vdec: Option<f64>,
}

/// Extracts all GPU metrics from the sensor list.
///
/// Collects all GPU candidates (unique grandparents of "GPU Memory Total" sensors),
/// then picks the one that is currently active:
///   • Primary: highest "GPU Core" load — shows the iGPU when the dGPU is idle.
///   • Tiebreak: highest VRAM — selects the dGPU when both report 0 % load.
///
/// Intel iGPU + NVIDIA dGPU: Intel reports "D3D Shared Memory Total", not
/// "GPU Memory Total", so the Intel iGPU is not a candidate and the NVIDIA dGPU
/// is always shown regardless of load.
fn extract_gpu(nodes: &[FlatNode]) -> GpuData {
  // Collect all unique GPU device names that expose a "GPU Memory Total" sensor.
  let mut seen_devices: Vec<String> = Vec::new();
  for n in nodes.iter().filter(|n| n.text == "GPU Memory Total") {
    if !n.grandparent.is_empty() && !seen_devices.contains(&n.grandparent) {
      seen_devices.push(n.grandparent.clone());
    }
  }

  let load_for = |dev: &str| -> f64 {
    nodes
      .iter()
      .find(|n| n.grandparent == dev && n.parent == "Load" && n.text == "GPU Core")
      .and_then(|n| parse_val(&n.value))
      .unwrap_or(0.0)
  };
  let vram_for = |dev: &str| -> f64 {
    nodes
      .iter()
      .find(|n| n.grandparent == dev && n.text == "GPU Memory Total")
      .and_then(|n| parse_val(&n.value))
      .unwrap_or(0.0)
  };

  // Pick the GPU with the highest load; break ties by highest VRAM (dGPU wins when idle).
  let gpu_device: Option<String> = seen_devices.into_iter().max_by(|a, b| {
    let la = load_for(a);
    let lb = load_for(b);
    match la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal) {
      std::cmp::Ordering::Equal => vram_for(a)
        .partial_cmp(&vram_for(b))
        .unwrap_or(std::cmp::Ordering::Equal),
      other => other,
    }
  });

  // Identify the GPU device by the grandparent of the anchor node, then collect
  // all sensors that belong to the same device. A fixed window was fragile: GPUs
  // with many D3D load sensors (e.g. RTX 4090 reports 19) push temperature, clock
  // and power sensors far enough that they fell outside the old ±25 limit.
  let gpu_block: Vec<&FlatNode> = if let Some(ref dev) = gpu_device {
    nodes.iter().filter(|n| &n.grandparent == dev).collect()
  } else {
    vec![]
  };

  let find = |parent: &str, text: &str| {
    gpu_block
      .iter()
      .find(|n| n.parent == parent && n.text == text)
      .and_then(|n| parse_val(&n.value))
  };

  GpuData {
    name: gpu_device.clone(),
    load: find("Load", "GPU Core"),
    // AMD iGPUs (e.g. Radeon 890M) report "GPU VR SoC" instead of "GPU Core" temperature.
    temp: find("Temperatures", "GPU Core").or_else(|| find("Temperatures", "GPU VR SoC")),
    // "GPU Hot Spot" is present on desktop NVIDIA GPUs; laptop GPUs (e.g. RTX
    // 5070 Ti Laptop) expose "GPU Memory Junction" instead — use it as fallback.
    hotspot: find("Temperatures", "GPU Hot Spot").or_else(|| find("Temperatures", "GPU Memory Junction")),
    freq: find("Clocks", "GPU Core"),
    mem_freq: find("Clocks", "GPU Memory"),
    // AMD iGPUs expose the total GPU power as "GPU Core" under Powers rather than "GPU Package".
    power: find("Powers", "GPU Package").or_else(|| find("Powers", "GPU Core")),
    fan: gpu_block
      .iter()
      .find(|n| n.parent == "Fans" && n.text.starts_with("GPU Fan"))
      .and_then(|n| parse_val(&n.value)),
    vram_used: find("Data", "GPU Memory Used"),
    vram_total: find("Data", "GPU Memory Total"),
    d3d_3d: find("Load", "D3D 3D"),
    d3d_vdec: find("Load", "D3D Video Decode"),
  }
}

/// Returns total disk read and write throughput in MB/s across all drives.
fn extract_disk_throughput(nodes: &[FlatNode]) -> (f64, f64) {
  let read = nodes
    .iter()
    .filter(|n| n.parent == "Throughput" && n.text == "Read Rate")
    .map(|n| to_mbs(&n.value))
    .sum();
  let write = nodes
    .iter()
    .filter(|n| n.parent == "Throughput" && n.text == "Write Rate")
    .map(|n| to_mbs(&n.value))
    .sum();
  (read, write)
}

/// Returns the busiest network interface's upload and download speed in Mbit/s.
fn extract_network(nodes: &[FlatNode]) -> (f64, f64) {
  let uploads: Vec<&FlatNode> = nodes
    .iter()
    .filter(|n| n.parent == "Throughput" && n.text == "Upload Speed")
    .collect();
  let downloads: Vec<&FlatNode> = nodes
    .iter()
    .filter(|n| n.parent == "Throughput" && n.text == "Download Speed")
    .collect();

  let mut best_up = 0.0;
  let mut best_down = 0.0;
  for (i, up_node) in uploads.iter().enumerate() {
    let up = to_mbs(&up_node.value) * 8.0;
    let down = downloads.get(i).map(|n| to_mbs(&n.value) * 8.0).unwrap_or(0.0);
    if up + down > best_up + best_down {
      best_up = up;
      best_down = down;
    }
  }
  (best_up, best_down)
}

/// Returns per-device disk temperatures: `(device_name, temp_celsius)`.
///
/// Sensors are identified by SensorId prefix (/nvme/, /hdd/, /ata/, /scsi/, /ssd/).
/// "Warning Composite" and "Critical Composite" are NVMe thresholds, not readings — excluded.
/// LHM reports 0 as a sentinel for unsupported sensors — those are skipped too.
/// Multiple temperature entries for the same device are collapsed to the highest value.
fn extract_disk_temps(nodes: &[FlatNode]) -> Vec<(String, f64)> {
  let mut temps: Vec<(String, f64)> = Vec::new();
  for n in nodes.iter().filter(|n| {
    n.parent == "Temperatures"
      && (n.sensor_id.starts_with("/nvme/")
        || n.sensor_id.starts_with("/hdd/")
        || n.sensor_id.starts_with("/ata/")
        || n.sensor_id.starts_with("/scsi/")
        || n.sensor_id.starts_with("/ssd/"))
      && !n.text.contains("Warning")
      && !n.text.contains("Critical")
  }) {
    if let Some(t) = parse_val(&n.value).filter(|&v| v > 0.0) {
      if let Some(existing) = temps.iter_mut().find(|(name, _)| name == &n.grandparent) {
        if t > existing.1 {
          existing.1 = t;
        }
      } else if !n.grandparent.is_empty() {
        temps.push((n.grandparent.clone(), t));
      }
    }
  }
  temps
}

/// Returns `(cpu_temp, cpu_power)`.
///
/// AMD Ryzen reports "Core (Tctl/Tdie)"; Intel reports "CPU Package" or "Core Average".
/// All three sensor names also appear under "Powers", so temp lookup is restricted to
/// parent == "Temperatures" to avoid the Intel "CPU Package" power sensor.
fn extract_cpu(nodes: &[FlatNode]) -> (Option<f64>, Option<f64>) {
  let temp = ["Core (Tctl/Tdie)", "CPU Package", "Core Average"]
    .iter()
    .find_map(|&name| {
      nodes
        .iter()
        .find(|n| n.parent == "Temperatures" && n.text == name)
        .and_then(|n| parse_val(&n.value))
    });
  // Intel names the package power sensor "CPU Package"; AMD names it "Package".
  let power = ["CPU Package", "Package"].iter().find_map(|&name| {
    nodes
      .iter()
      .find(|n| n.parent == "Powers" && n.text == name)
      .and_then(|n| parse_val(&n.value))
  });
  (temp, power)
}

/// Returns the highest DIMM temperature across all populated slots, or `None`.
///
/// DDR5 (and some DDR4) DIMM sensors: the real reading is always /temperature/0
/// per slot. Indices 1–5 are resolution and threshold values — excluded.
fn extract_ram_temp(nodes: &[FlatNode]) -> Option<f64> {
  nodes
    .iter()
    .filter(|n| {
      n.parent == "Temperatures" && n.sensor_id.starts_with("/memory/dimm/") && n.sensor_id.ends_with("/temperature/0")
    })
    .filter_map(|n| parse_val(&n.value).filter(|&v| v > 0.0))
    .reduce(f64::max)
}

struct MbData {
  fans: Vec<(String, f64)>,
  temps: Vec<(String, f64)>,
  voltages: Vec<(String, f64)>,
  chip: Option<String>,
}

/// Extracts Super I/O motherboard metrics (fans, temps, voltages, chip name).
///
/// Primary source: /lpc/ SensorId prefix (chip-agnostic, covers NCT, ITE, Winbond, etc.).
/// Voltage fallback: AMD CPU SVI2 rails (/amdcpu/ prefix, parent "Voltages") when no LPC
/// chip is present — laptops use an embedded controller instead of a discrete Super I/O.
/// Per-core VID readouts ("… VID") are excluded as they are switching targets, not supply
/// rail measurements.
fn extract_motherboard(nodes: &[FlatNode]) -> MbData {
  // Fans: RPM > 0 required (0 is the LHM sentinel for disconnected headers), sorted descending.
  let mut fans: Vec<(String, f64)> = nodes
    .iter()
    .filter(|n| n.parent == "Fans" && n.sensor_id.starts_with("/lpc/"))
    .filter_map(|n| Some((n.text.clone(), parse_val(&n.value).filter(|&v| v > 0.0)?)))
    .collect();
  fans.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

  // Temperatures: values < 5 °C are LHM sentinels for unconfigured/absent sensors.
  let temps: Vec<(String, f64)> = nodes
    .iter()
    .filter(|n| n.parent == "Temperatures" && n.sensor_id.starts_with("/lpc/"))
    .filter_map(|n| Some((n.text.clone(), parse_val(&n.value).filter(|&v| v >= 5.0)?)))
    .collect();

  // Voltages: named rails only — generic "Voltage #N" entries are unmapped hardware pins.
  let mut voltages: Vec<(String, f64)> = nodes
    .iter()
    .filter(|n| {
      n.sensor_id.starts_with("/lpc/") && n.sensor_id.contains("/voltage/") && !n.text.starts_with("Voltage #")
    })
    .filter_map(|n| Some((n.text.clone(), parse_val(&n.value).filter(|&v| v > 0.1)?)))
    .collect();

  // No LPC chip present (laptop EC) — fall back to AMD CPU SVI2 voltage rails.
  // Per-core VID readouts (e.g. "Core #1 VID") are switching targets, not supply rail
  // measurements — exclude them to avoid flooding the panel with 12+ nearly identical rows.
  if voltages.is_empty() {
    voltages = nodes
      .iter()
      .filter(|n| {
        n.sensor_id.starts_with("/amdcpu/")
          && n.sensor_id.contains("/voltage/")
          && !n.text.contains("VID")
          && !n.text.starts_with("Voltage #")
      })
      .filter_map(|n| Some((n.text.clone(), parse_val(&n.value).filter(|&v| v > 0.1)?)))
      .collect();
  }

  // Chip name is the grandparent of any /lpc/ sensor (the Super I/O device node).
  let chip = nodes
    .iter()
    .find(|n| n.sensor_id.starts_with("/lpc/"))
    .map(|n| n.grandparent.clone())
    .filter(|s| !s.is_empty());

  MbData {
    fans,
    temps,
    voltages,
    chip,
  }
}

// --- Top-level parser ------------------------------------------------------

fn parse_lhm(data: &Value) -> LhmData {
  let mut nodes = Vec::new();
  flatten_lhm(data, &mut nodes, "", "");

  let gpu = extract_gpu(&nodes);
  let (disk_read, disk_write) = extract_disk_throughput(&nodes);
  let (net_up, net_down) = extract_network(&nodes);
  let disk_temps = extract_disk_temps(&nodes);
  let (cpu_temp, cpu_power) = extract_cpu(&nodes);
  let ram_temp = extract_ram_temp(&nodes);
  let mb = extract_motherboard(&nodes);

  LhmData {
    gpu_name: gpu.name,
    gpu_load: gpu.load,
    gpu_temp: gpu.temp,
    gpu_hotspot: gpu.hotspot,
    gpu_freq: gpu.freq,
    gpu_mem_freq: gpu.mem_freq,
    gpu_power: gpu.power,
    gpu_fan: gpu.fan,
    vram_used: gpu.vram_used,
    vram_total: gpu.vram_total,
    gpu_d3d_3d: gpu.d3d_3d,
    gpu_d3d_vdec: gpu.d3d_vdec,
    cpu_temp,
    cpu_power,
    ram_temp,
    disk_read,
    disk_write,
    net_up,
    net_down,
    disk_temps,
    mb_fans: mb.fans,
    mb_temps: mb.temps,
    mb_voltages: mb.voltages,
    mb_chip: mb.chip,
  }
}

// --- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::{flatten_lhm, parse_lhm, parse_val};
  use serde_json::json;

  // parse_val

  #[test]
  fn parse_val_parses_plain_number() {
    assert_eq!(parse_val("42.5"), Some(42.5));
    assert_eq!(parse_val("0"), Some(0.0));
  }

  #[test]
  fn parse_val_strips_unit_suffixes() {
    assert_eq!(parse_val("65.3 °C"), Some(65.3));
    assert_eq!(parse_val("1234 MHz"), Some(1234.0));
    assert_eq!(parse_val("100 %"), Some(100.0));
    assert_eq!(parse_val("8192 MB"), Some(8192.0));
  }

  #[test]
  fn parse_val_handles_locale_comma_as_decimal_separator() {
    assert_eq!(parse_val("65,3 °C"), Some(65.3));
    assert_eq!(parse_val("1 234,5 MHz"), Some(1234.5));
  }

  #[test]
  fn parse_val_returns_none_for_non_numeric_input() {
    assert_eq!(parse_val("N/A"), None);
    assert_eq!(parse_val(""), None);
    assert_eq!(parse_val("Value"), None);
  }

  #[test]
  fn parse_val_handles_negative_numbers() {
    assert_eq!(parse_val("-5.0"), Some(-5.0));
  }

  // flatten_lhm

  #[test]
  fn flatten_lhm_extracts_leaf_with_parent_name() {
    let tree = json!({
      "Text": "GPU",
      "Value": "",
      "Children": [{
        "Text": "GPU Core",
        "Value": "75 %",
        "Children": []
      }]
    });
    let mut nodes = vec![];
    flatten_lhm(&tree, &mut nodes, "", "");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].text, "GPU Core");
    assert_eq!(nodes[0].value, "75 %");
    assert_eq!(nodes[0].parent, "GPU");
  }

  #[test]
  fn flatten_lhm_skips_nodes_without_values() {
    let tree = json!({ "Text": "Container", "Value": "", "Children": [] });
    let mut nodes = vec![];
    flatten_lhm(&tree, &mut nodes, "", "");
    assert!(nodes.is_empty());
  }

  #[test]
  fn flatten_lhm_skips_sentinel_value_string() {
    // LHM uses the literal string "Value" as a sentinel for missing data.
    let tree = json!({ "Text": "GPU Core", "Value": "Value", "Children": [] });
    let mut nodes = vec![];
    flatten_lhm(&tree, &mut nodes, "", "");
    assert!(nodes.is_empty());
  }

  #[test]
  fn flatten_lhm_handles_nested_children() {
    let tree = json!({
      "Text": "Root",
      "Value": "",
      "Children": [{
        "Text": "Temperatures",
        "Value": "",
        "Children": [{
          "Text": "GPU Core",
          "Value": "72 °C",
          "Children": []
        }]
      }]
    });
    let mut nodes = vec![];
    flatten_lhm(&tree, &mut nodes, "", "");
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].parent, "Temperatures");
    assert_eq!(nodes[0].text, "GPU Core");
  }

  // parse_lhm

  #[test]
  fn parse_lhm_extracts_cpu_temp() {
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "AMD Ryzen 9 7950X", "Value": "",
        "Children": [{
          "Text": "Temperatures", "Value": "",
          "Children": [{
            "Text": "Core (Tctl/Tdie)",
            "Value": "72 °C",
            "Children": []
          }]
        }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.cpu_temp, Some(72.0));
  }

  #[test]
  fn parse_lhm_extracts_intel_cpu_package_temp() {
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Intel Core i9-13900K", "Value": "",
        "Children": [{
          "Text": "Temperatures", "Value": "",
          "Children": [{
            "Text": "CPU Package",
            "Value": "68 °C",
            "Children": []
          }]
        }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.cpu_temp, Some(68.0));
  }

  #[test]
  fn parse_lhm_cpu_package_power_sensor_does_not_bleed_into_cpu_temp() {
    // Intel CPUs expose "CPU Package" under both Temperatures and Powers.
    // The temperature lookup must select the one under Temperatures, not Powers.
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Intel Core i9-13900K", "Value": "",
        "Children": [
          {
            "Text": "Powers", "Value": "",
            "Children": [{
              "Text": "CPU Package", "Value": "95 W", "Children": []
            }]
          },
          {
            "Text": "Temperatures", "Value": "",
            "Children": [{
              "Text": "CPU Package", "Value": "68 °C", "Children": []
            }]
          }
        ]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.cpu_temp, Some(68.0), "temp must come from Temperatures section");
    assert_eq!(result.cpu_power, Some(95.0), "power must come from Powers section");
  }

  #[test]
  fn parse_lhm_prefers_amd_sensor_over_intel_when_both_present() {
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [
        {
          "Text": "Temperatures", "Value": "",
          "Children": [
            { "Text": "Core (Tctl/Tdie)", "Value": "72 °C", "Children": [] },
            { "Text": "CPU Package",       "Value": "68 °C", "Children": [] }
          ]
        }
      ]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.cpu_temp, Some(72.0));
  }

  #[test]
  fn parse_lhm_extracts_cpu_power_intel() {
    // Intel LHM sensor name: "CPU Package"
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Powers", "Value": "",
        "Children": [{ "Text": "CPU Package", "Value": "125 W", "Children": [] }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.cpu_power, Some(125.0));
  }

  #[test]
  fn parse_lhm_extracts_cpu_power_amd() {
    // AMD LHM sensor name: "Package"
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Powers", "Value": "",
        "Children": [{ "Text": "Package", "Value": "95 W", "Children": [] }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.cpu_power, Some(95.0));
  }

  #[test]
  fn parse_lhm_converts_disk_kb_to_mb() {
    // LHM reports slow disks in KB/s — must be divided by 1024 before display.
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Throughput", "Value": "",
        "Children": [
          { "Text": "Read Rate",  "Value": "2048 KB", "Children": [] },
          { "Text": "Write Rate", "Value": "1024 KB", "Children": [] }
        ]
      }]
    });
    let result = parse_lhm(&data);
    assert!(
      (result.disk_read - 2.0).abs() < 1e-9,
      "2048 KB should be 2.0 MB, got {}",
      result.disk_read
    );
    assert!(
      (result.disk_write - 1.0).abs() < 1e-9,
      "1024 KB should be 1.0 MB, got {}",
      result.disk_write
    );
  }

  #[test]
  fn parse_lhm_converts_disk_gb_to_mb() {
    // LHM reports fast disks in GB/s — must be multiplied by 1024 before display.
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Throughput", "Value": "",
        "Children": [
          { "Text": "Read Rate",  "Value": "2 GB", "Children": [] },
          { "Text": "Write Rate", "Value": "1 GB", "Children": [] }
        ]
      }]
    });
    let result = parse_lhm(&data);
    assert!(
      (result.disk_read - 2048.0).abs() < 1e-9,
      "2 GB should be 2048.0 MB, got {}",
      result.disk_read
    );
    assert!(
      (result.disk_write - 1024.0).abs() < 1e-9,
      "1 GB should be 1024.0 MB, got {}",
      result.disk_write
    );
  }

  #[test]
  fn parse_lhm_sums_all_disk_throughput() {
    // Previously only the first two Read Rate / Write Rate nodes were summed.
    // This test uses four drives to catch a regression back to that limit.
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Throughput", "Value": "",
        "Children": [
          { "Text": "Read Rate",  "Value": "10", "Children": [] },
          { "Text": "Write Rate", "Value": "5",  "Children": [] },
          { "Text": "Read Rate",  "Value": "20", "Children": [] },
          { "Text": "Write Rate", "Value": "15", "Children": [] },
          { "Text": "Read Rate",  "Value": "30", "Children": [] },
          { "Text": "Write Rate", "Value": "5",  "Children": [] },
          { "Text": "Read Rate",  "Value": "40", "Children": [] },
          { "Text": "Write Rate", "Value": "5",  "Children": [] }
        ]
      }]
    });
    let result = parse_lhm(&data);
    assert!(
      (result.disk_read - 100.0).abs() < 1e-9,
      "disk read should sum all four drives (10+20+30+40=100), got {}",
      result.disk_read
    );
    assert!(
      (result.disk_write - 30.0).abs() < 1e-9,
      "disk write should sum all four drives (5+15+5+5=30), got {}",
      result.disk_write
    );
  }

  #[test]
  fn parse_lhm_selects_network_interface_with_most_traffic() {
    // The interface with the highest combined up+down should win.
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Throughput", "Value": "",
        "Children": [
          { "Text": "Upload Speed",   "Value": "1",  "Children": [] },
          { "Text": "Download Speed", "Value": "2",  "Children": [] },
          { "Text": "Upload Speed",   "Value": "10", "Children": [] },
          { "Text": "Download Speed", "Value": "20", "Children": [] }
        ]
      }]
    });
    let result = parse_lhm(&data);
    // Network values are multiplied by 8 (bytes → bits), so 10+20 MB = 240 Mbit
    assert!(
      result.net_up > result.net_down * 0.0,
      "should pick the busier interface"
    );
    assert!(
      (result.net_up - 80.0).abs() < 1e-9,
      "10 MB * 8 = 80 Mbit/s upload, got {}",
      result.net_up
    );
    assert!(
      (result.net_down - 160.0).abs() < 1e-9,
      "20 MB * 8 = 160 Mbit/s download, got {}",
      result.net_down
    );
  }

  #[test]
  fn parse_lhm_extracts_disk_temperatures() {
    // Only /nvme/, /hdd/, /ata/, /scsi/, /ssd/ SensorIds are included.
    // Warning/Critical threshold sensors are excluded even though they share SensorId prefix.
    // Motherboard sensors (/lpc/...) are excluded regardless of text.
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Samsung SSD 980 PRO", "Value": "",
        "Children": [{
          "Text": "Temperatures", "Value": "",
          "Children": [
            { "Text": "Composite",           "Value": "44 °C", "SensorId": "/nvme/0/temperature/0", "Children": [] },
            { "Text": "Temperature 1",        "Value": "42 °C", "SensorId": "/nvme/0/temperature/1", "Children": [] },
            { "Text": "Temperature 2",        "Value": "38 °C", "SensorId": "/nvme/0/temperature/2", "Children": [] },
            { "Text": "Warning Composite",    "Value": "75 °C", "SensorId": "/nvme/0/temperature/3", "Children": [] },
            { "Text": "Critical Composite",   "Value": "85 °C", "SensorId": "/nvme/0/temperature/4", "Children": [] }
          ]
        }]
      }, {
        "Text": "WD Blue", "Value": "",
        "Children": [{
          "Text": "Temperatures", "Value": "",
          "Children": [{
            "Text": "Temperature",
            "Value": "35 °C",
            "SensorId": "/hdd/0/temperature/0",
            "Children": []
          }]
        }]
      }, {
        "Text": "Nuvoton NCT6799D", "Value": "",
        "Children": [{
          "Text": "Temperatures", "Value": "",
          "Children": [
            { "Text": "Temperature #1", "Value": "37 °C", "SensorId": "/lpc/nct6799d/0/temperature/1", "Children": [] }
          ]
        }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.disk_temps.len(), 2, "motherboard sensor must be excluded");
    assert_eq!(result.disk_temps[0].0, "Samsung SSD 980 PRO");
    assert_eq!(result.disk_temps[0].1, 44.0, "Composite wins (highest real sensor)");
    assert_eq!(result.disk_temps[1].0, "WD Blue");
    assert_eq!(result.disk_temps[1].1, 35.0);
  }

  #[test]
  fn parse_lhm_extracts_ram_temperature() {
    // Only /memory/dimm/N/temperature/0 sensors are real readings.
    // /temperature/1 is resolution; /temperature/2-5 are Low/High/CriticalLow/CriticalHigh limits.
    // The highest reading across all populated DIMM slots is returned.
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Team Group Inc - UD5-6000 (#1)", "Value": "",
        "Children": [{
          "Text": "Temperatures", "Value": "",
          "Children": [
            { "Text": "DIMM #1",                       "Value": "38 °C", "SensorId": "/memory/dimm/1/temperature/0", "Children": [] },
            { "Text": "Temperature Sensor Resolution",  "Value": "0,3 °C","SensorId": "/memory/dimm/1/temperature/1", "Children": [] },
            { "Text": "Thermal Sensor Low Limit",       "Value": "0 °C",  "SensorId": "/memory/dimm/1/temperature/2", "Children": [] },
            { "Text": "Thermal Sensor High Limit",      "Value": "55 °C", "SensorId": "/memory/dimm/1/temperature/3", "Children": [] },
            { "Text": "Thermal Sensor Critical Low",    "Value": "0 °C",  "SensorId": "/memory/dimm/1/temperature/4", "Children": [] },
            { "Text": "Thermal Sensor Critical High",   "Value": "85 °C", "SensorId": "/memory/dimm/1/temperature/5", "Children": [] }
          ]
        }]
      }, {
        "Text": "Team Group Inc - UD5-6000 (#3)", "Value": "",
        "Children": [{
          "Text": "Temperatures", "Value": "",
          "Children": [
            { "Text": "DIMM #3",                       "Value": "36 °C", "SensorId": "/memory/dimm/3/temperature/0", "Children": [] },
            { "Text": "Thermal Sensor Critical High",   "Value": "85 °C", "SensorId": "/memory/dimm/3/temperature/5", "Children": [] }
          ]
        }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(
      result.ram_temp,
      Some(38.0),
      "max of DIMM #1 (38) and DIMM #3 (36) should be 38"
    );
  }

  #[test]
  fn parse_lhm_ram_temperature_none_when_no_dimm_sensors() {
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Generic DDR4 (#0)", "Value": "",
        "Children": [{
          "Text": "Temperatures", "Value": "",
          "Children": [
            { "Text": "Thermal Sensor High Limit", "Value": "60 °C", "SensorId": "/memory/dimm/0/temperature/3", "Children": [] }
          ]
        }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(
      result.ram_temp, None,
      "only threshold sensors present — no real reading"
    );
  }

  #[test]
  fn parse_lhm_includes_ssd_sensor_id_prefix_in_disk_temps() {
    // SATA SSDs reported by LHM use /ssd/ SensorId prefix, not /nvme/ or /hdd/.
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "WDC WDS500G2B0A-00SM50", "Value": "",
        "Children": [{
          "Text": "Temperatures", "Value": "",
          "Children": [
            { "Text": "Temperature", "Value": "32 °C", "SensorId": "/ssd/0/temperature/0", "Children": [] }
          ]
        }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.disk_temps.len(), 1, "/ssd/ prefix must be included");
    assert_eq!(result.disk_temps[0].0, "WDC WDS500G2B0A-00SM50");
    assert_eq!(result.disk_temps[0].1, 32.0);
  }

  #[test]
  fn parse_lhm_gpu_block_uses_grandparent_not_window() {
    // An RTX 4090 reports 19 D3D load sensors between its temperature/clock/power
    // sensors and the GPU Memory Total anchor. A fixed ±25 window would miss them;
    // grandparent-based matching must capture all GPU sensors regardless of count.
    let mut load_children: Vec<serde_json::Value> = (0..19)
      .map(|i| json!({ "Text": format!("D3D Engine {i}"), "Value": "0 %", "Children": [] }))
      .collect();
    load_children.insert(0, json!({ "Text": "GPU Core", "Value": "10 %", "Children": [] }));

    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "NVIDIA GeForce RTX 4090", "Value": "",
        "Children": [
          { "Text": "Powers", "Value": "",
            "Children": [{ "Text": "GPU Package", "Value": "150 W", "Children": [] }] },
          { "Text": "Clocks", "Value": "",
            "Children": [{ "Text": "GPU Core", "Value": "2520 MHz", "Children": [] }] },
          { "Text": "Temperatures", "Value": "",
            "Children": [{ "Text": "GPU Core", "Value": "72 °C", "Children": [] }] },
          { "Text": "Load", "Value": "", "Children": load_children },
          { "Text": "Fans", "Value": "",
            "Children": [{ "Text": "GPU Fan 1", "Value": "1200 RPM", "Children": [] }] },
          { "Text": "Data", "Value": "",
            "Children": [
              { "Text": "GPU Memory Used",  "Value": "4096 MB", "Children": [] },
              { "Text": "GPU Memory Total", "Value": "24576 MB", "Children": [] }
            ]
          }
        ]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(
      result.gpu_temp,
      Some(72.0),
      "temp must be found despite many load sensors"
    );
    assert_eq!(result.gpu_freq, Some(2520.0), "clock must be found");
    assert_eq!(result.gpu_power, Some(150.0), "power must be found");
    assert_eq!(result.gpu_fan, Some(1200.0), "fan with suffix '1' must be found");
    assert_eq!(result.gpu_load, Some(10.0), "GPU Core load must still work");
  }

  #[test]
  fn parse_lhm_extracts_gpu_memory_clock() {
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "NVIDIA GeForce RTX 4090", "Value": "",
        "Children": [
          { "Text": "Clocks", "Value": "",
            "Children": [
              { "Text": "GPU Core",   "Value": "2520 MHz", "Children": [] },
              { "Text": "GPU Memory", "Value": "10501 MHz", "Children": [] }
            ]
          },
          { "Text": "Data", "Value": "",
            "Children": [
              { "Text": "GPU Memory Used",  "Value": "4096 MB", "Children": [] },
              { "Text": "GPU Memory Total", "Value": "24576 MB", "Children": [] }
            ]
          }
        ]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.gpu_freq, Some(2520.0));
    assert_eq!(result.gpu_mem_freq, Some(10501.0));
  }

  #[test]
  fn parse_lhm_extracts_gpu_d3d_sensors() {
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "NVIDIA GeForce RTX 4090", "Value": "",
        "Children": [
          { "Text": "Load", "Value": "",
            "Children": [
              { "Text": "GPU Core",          "Value": "75 %",  "Children": [] },
              { "Text": "D3D 3D",            "Value": "68 %",  "Children": [] },
              { "Text": "D3D Copy",          "Value": "2 %",   "Children": [] },
              { "Text": "D3D Video Decode",  "Value": "12 %",  "Children": [] }
            ]
          },
          { "Text": "Data", "Value": "",
            "Children": [
              { "Text": "GPU Memory Used",  "Value": "4096 MB", "Children": [] },
              { "Text": "GPU Memory Total", "Value": "24576 MB", "Children": [] }
            ]
          }
        ]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.gpu_load, Some(75.0));
    assert_eq!(result.gpu_d3d_3d, Some(68.0));
    assert_eq!(result.gpu_d3d_vdec, Some(12.0));
    // D3D Copy is intentionally not extracted — only 3D and Video Decode are surfaced.
    assert_eq!(result.gpu_mem_freq, None, "mem clock absent from tree → None");
  }

  #[test]
  fn parse_lhm_returns_zero_defaults_for_empty_tree() {
    let data = json!({ "Text": "Root", "Value": "", "Children": [] });
    let result = parse_lhm(&data);
    assert_eq!(result.cpu_temp, None);
    assert_eq!(result.gpu_load, None);
    assert_eq!(result.gpu_mem_freq, None);
    assert_eq!(result.gpu_d3d_3d, None);
    assert_eq!(result.gpu_d3d_vdec, None);
    assert_eq!(result.ram_temp, None);
    assert_eq!(result.disk_read, 0.0);
    assert_eq!(result.disk_write, 0.0);
    assert_eq!(result.net_up, 0.0);
    assert_eq!(result.net_down, 0.0);
    assert!(result.disk_temps.is_empty());
    assert!(result.mb_fans.is_empty());
    assert!(result.mb_temps.is_empty());
    assert!(result.mb_voltages.is_empty());
  }

  // --- Motherboard (LPC) sensor extraction -----------------------------------

  fn lpc_tree() -> serde_json::Value {
    // Mirrors the structure observed in real diagnostic dumps (NCT6799D / NCT6798D).
    json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Nuvoton NCT6799D", "Value": "",
        "Children": [
          {
            "Text": "Fans", "Value": "",
            "Children": [
              { "Text": "Fan #1", "Value": "882 RPM",  "SensorId": "/lpc/nct6799d/0/fan/0", "Children": [] },
              { "Text": "Fan #2", "Value": "968 RPM",  "SensorId": "/lpc/nct6799d/0/fan/1", "Children": [] },
              { "Text": "Fan #6", "Value": "0 RPM",    "SensorId": "/lpc/nct6799d/0/fan/5", "Children": [] },
              { "Text": "Fan #7", "Value": "2652 RPM", "SensorId": "/lpc/nct6799d/0/fan/6", "Children": [] }
            ]
          },
          {
            "Text": "Temperatures", "Value": "",
            "Children": [
              { "Text": "Temperature #1", "Value": "35,5 °C", "SensorId": "/lpc/nct6799d/0/temperature/1", "Children": [] },
              { "Text": "Temperature #2", "Value": "30 °C",   "SensorId": "/lpc/nct6799d/0/temperature/2", "Children": [] },
              { "Text": "Temperature #3", "Value": "2 °C",    "SensorId": "/lpc/nct6799d/0/temperature/3", "Children": [] }
            ]
          },
          {
            "Text": "Voltages", "Value": "",
            "Children": [
              { "Text": "Vcore",      "Value": "1,048 V", "SensorId": "/lpc/nct6799d/0/voltage/0", "Children": [] },
              { "Text": "AVCC",       "Value": "3,376 V", "SensorId": "/lpc/nct6799d/0/voltage/2", "Children": [] },
              { "Text": "+3.3V",      "Value": "3,328 V", "SensorId": "/lpc/nct6799d/0/voltage/3", "Children": [] },
              { "Text": "Voltage #5", "Value": "1,016 V", "SensorId": "/lpc/nct6799d/0/voltage/4", "Children": [] }
            ]
          }
        ]
      }]
    })
  }

  #[test]
  fn parse_lhm_extracts_mb_fans_sorted_descending_zero_excluded() {
    let result = parse_lhm(&lpc_tree());
    // Fan #6 (0 RPM) must be excluded; remainder sorted descending.
    assert_eq!(result.mb_fans.len(), 3);
    assert_eq!(result.mb_fans[0].0, "Fan #7");
    assert!((result.mb_fans[0].1 - 2652.0).abs() < 1e-9);
    assert_eq!(result.mb_fans[1].0, "Fan #2");
    assert_eq!(result.mb_fans[2].0, "Fan #1");
  }

  #[test]
  fn parse_lhm_mb_fans_all_active_returned_sorted_descending() {
    let fans: Vec<serde_json::Value> = (1..=7)
      .map(|i| {
        json!({
          "Text": format!("Fan #{i}"),
          "Value": format!("{} RPM", i * 100),
          "SensorId": format!("/lpc/nct6799d/0/fan/{}", i - 1),
          "Children": []
        })
      })
      .collect();
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Nuvoton NCT6799D", "Value": "",
        "Children": [{ "Text": "Fans", "Value": "", "Children": fans }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.mb_fans.len(), 7, "all active fan channels are returned");
    assert_eq!(result.mb_fans[0].0, "Fan #7", "highest RPM first");
    assert_eq!(result.mb_fans[6].0, "Fan #1", "lowest RPM last");
  }

  #[test]
  fn parse_lhm_extracts_mb_temps_filters_sentinel_below_5c() {
    let result = parse_lhm(&lpc_tree());
    // Temperature #3 = 2 °C must be filtered out.
    assert_eq!(result.mb_temps.len(), 2);
    assert_eq!(result.mb_temps[0].0, "Temperature #1");
    assert!((result.mb_temps[0].1 - 35.5).abs() < 0.01);
    assert_eq!(result.mb_temps[1].0, "Temperature #2");
    assert!((result.mb_temps[1].1 - 30.0).abs() < 0.01);
  }

  #[test]
  fn parse_lhm_extracts_mb_named_voltages_only() {
    let result = parse_lhm(&lpc_tree());
    // "Voltage #5" must be excluded; three named rails remain.
    assert_eq!(result.mb_voltages.len(), 3);
    let names: Vec<&str> = result.mb_voltages.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"Vcore"));
    assert!(names.contains(&"AVCC"));
    assert!(names.contains(&"+3.3V"));
    assert!(!names.contains(&"Voltage #5"), "generic slots must be excluded");
  }

  #[test]
  fn parse_lhm_mb_sensors_not_confused_with_disk_or_gpu_sensors() {
    // GPU and disk sensors must not bleed into motherboard extraction.
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [
        {
          "Text": "NVIDIA GeForce RTX 4090", "Value": "",
          "Children": [{
            "Text": "Fans", "Value": "",
            "Children": [
              { "Text": "GPU Fan 1", "Value": "1200 RPM", "SensorId": "/gpu-nvidia/0/fan/1", "Children": [] }
            ]
          }]
        },
        {
          "Text": "Nuvoton NCT6799D", "Value": "",
          "Children": [{
            "Text": "Fans", "Value": "",
            "Children": [
              { "Text": "Fan #7", "Value": "2652 RPM", "SensorId": "/lpc/nct6799d/0/fan/6", "Children": [] }
            ]
          }]
        }
      ]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.mb_fans.len(), 1, "only /lpc/ fan should be included");
    assert_eq!(result.mb_fans[0].0, "Fan #7");
  }

  // --- Active GPU selection (iGPU + dGPU) -----------------------------------

  fn igpu_dgpu_tree(igpu_load: &str, dgpu_load: &str) -> serde_json::Value {
    // AMD 890M iGPU (512 MB VRAM) + NVIDIA RTX 5070 Ti Laptop GPU (8 GB VRAM).
    json!({
      "Text": "Root", "Value": "",
      "Children": [
        {
          "Text": "AMD Radeon 890M", "Value": "",
          "Children": [
            { "Text": "Load", "Value": "",
              "Children": [{ "Text": "GPU Core", "Value": igpu_load, "Children": [] }] },
            { "Text": "Data", "Value": "",
              "Children": [
                { "Text": "GPU Memory Total", "Value": "512 MB", "Children": [] },
                { "Text": "GPU Memory Used",  "Value": "128 MB", "Children": [] }
              ]
            }
          ]
        },
        {
          "Text": "NVIDIA GeForce RTX 5070 Ti Laptop GPU", "Value": "",
          "Children": [
            { "Text": "Load", "Value": "",
              "Children": [{ "Text": "GPU Core", "Value": dgpu_load, "Children": [] }] },
            { "Text": "Data", "Value": "",
              "Children": [
                { "Text": "GPU Memory Total", "Value": "8192 MB", "Children": [] },
                { "Text": "GPU Memory Used",  "Value": "1024 MB", "Children": [] }
              ]
            }
          ]
        }
      ]
    })
  }

  #[test]
  fn extract_gpu_picks_igpu_when_dgpu_idle() {
    // dGPU at 0 %, iGPU at 11 % — iGPU must win.
    let result = parse_lhm(&igpu_dgpu_tree("11 %", "0 %"));
    assert_eq!(
      result.gpu_name.as_deref(),
      Some("AMD Radeon 890M"),
      "active iGPU must be selected when dGPU is idle"
    );
    assert_eq!(result.gpu_load, Some(11.0));
  }

  #[test]
  fn extract_gpu_amd_igpu_temp_and_power_sensors() {
    // AMD Radeon 890M exposes "GPU VR SoC" for temperature and "GPU Core" under
    // Powers — different names from discrete NVIDIA/AMD GPUs.
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "AMD Radeon(TM) 890M Graphics", "Value": "",
        "Children": [
          { "Text": "Temperatures", "Value": "",
            "Children": [{ "Text": "GPU VR SoC", "Value": "51 °C", "Children": [] }] },
          { "Text": "Powers", "Value": "",
            "Children": [{ "Text": "GPU Core", "Value": "2 W", "Children": [] }] },
          { "Text": "Load", "Value": "",
            "Children": [{ "Text": "GPU Core", "Value": "11 %", "Children": [] }] },
          { "Text": "Clocks", "Value": "",
            "Children": [
              { "Text": "GPU Core",   "Value": "1343 MHz", "Children": [] },
              { "Text": "GPU Memory", "Value": "1000 MHz", "Children": [] }
            ]
          },
          { "Text": "Data", "Value": "",
            "Children": [
              { "Text": "GPU Memory Used",  "Value": "319 MB", "Children": [] },
              { "Text": "GPU Memory Total", "Value": "512 MB", "Children": [] }
            ]
          }
        ]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.gpu_temp, Some(51.0), "GPU VR SoC must be used as temp");
    assert_eq!(result.gpu_power, Some(2.0), "GPU Core under Powers must be used");
    assert_eq!(result.gpu_load, Some(11.0));
    assert_eq!(result.gpu_freq, Some(1343.0));
    assert_eq!(result.gpu_mem_freq, Some(1000.0));
  }

  #[test]
  fn extract_gpu_picks_dgpu_when_active() {
    // dGPU at 60 %, iGPU at 5 % — dGPU must win.
    let result = parse_lhm(&igpu_dgpu_tree("5 %", "60 %"));
    assert_eq!(
      result.gpu_name.as_deref(),
      Some("NVIDIA GeForce RTX 5070 Ti Laptop GPU"),
      "active dGPU must be selected"
    );
    assert_eq!(result.gpu_load, Some(60.0));
  }

  #[test]
  fn extract_gpu_picks_dgpu_by_vram_when_both_idle() {
    // Both at 0 % — dGPU (most VRAM) must win.
    let result = parse_lhm(&igpu_dgpu_tree("0 %", "0 %"));
    assert_eq!(
      result.gpu_name.as_deref(),
      Some("NVIDIA GeForce RTX 5070 Ti Laptop GPU"),
      "dGPU (most VRAM) must win when both are idle"
    );
  }

  // --- AMD CPU voltage fallback for laptops without a Super I/O chip -----------

  #[test]
  fn parse_lhm_mb_voltages_fall_back_to_amd_svi2_when_no_lpc() {
    // Laptops with AMD CPUs expose Vcore/VSoC via AMD SMU SVI2 sensors.
    // When no /lpc/ sensors are present, those should populate mb_voltages.
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "AMD Ryzen AI 9 HX 370", "Value": "",
        "Children": [{
          "Text": "Voltages", "Value": "",
          "Children": [
            { "Text": "Core (SVI2 TFN)", "Value": "1,550 V", "SensorId": "/amdcpu/0/voltage/0", "Children": [] },
            { "Text": "SoC (SVI2 TFN)",  "Value": "0,950 V", "SensorId": "/amdcpu/0/voltage/1", "Children": [] },
            { "Text": "Core #1 VID",      "Value": "0,794 V", "SensorId": "/amdcpu/0/voltage/2", "Children": [] },
            { "Text": "Core #2 VID",      "Value": "0,794 V", "SensorId": "/amdcpu/0/voltage/3", "Children": [] }
          ]
        }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(
      result.mb_voltages.len(),
      2,
      "only named SVI2 rails, no per-core VID entries"
    );
    let names: Vec<&str> = result.mb_voltages.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"Core (SVI2 TFN)"));
    assert!(names.contains(&"SoC (SVI2 TFN)"));
    assert!(!names.iter().any(|n| n.contains("VID")), "VID entries must be excluded");
    // No fans or temps — EC-controlled on laptops.
    assert!(result.mb_fans.is_empty());
    assert!(result.mb_temps.is_empty());
    assert_eq!(result.mb_chip, None);
  }

  #[test]
  fn parse_lhm_mb_lpc_voltages_take_priority_over_amd_fallback() {
    // When both LPC and AMD CPU sensors are present, LPC must win (desktop case).
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [
        {
          "Text": "Nuvoton NCT6799D", "Value": "",
          "Children": [{
            "Text": "Voltages", "Value": "",
            "Children": [
              { "Text": "Vcore", "Value": "1,200 V", "SensorId": "/lpc/nct6799d/0/voltage/0", "Children": [] }
            ]
          }]
        },
        {
          "Text": "AMD Ryzen 9 7950X", "Value": "",
          "Children": [{
            "Text": "Voltages", "Value": "",
            "Children": [
              { "Text": "Core (SVI2 TFN)", "Value": "1,350 V", "SensorId": "/amdcpu/0/voltage/0", "Children": [] }
            ]
          }]
        }
      ]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.mb_voltages.len(), 1, "LPC voltage only");
    assert_eq!(result.mb_voltages[0].0, "Vcore", "LPC sensor must win");
  }
}

pub async fn fetch_lhm(client: &reqwest::Client) -> Option<LhmData> {
  // Keep timeout short so stats polling remains responsive even if LHM is down.
  let response = client.get("http://localhost:8085/data.json").send().await.ok()?;
  let json: Value = response.json().await.ok()?;
  Some(parse_lhm(&json))
}
