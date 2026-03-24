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

#[derive(Debug, Clone, Default)]
pub struct LhmData {
  pub gpu_load: Option<f64>,
  pub gpu_temp: Option<f64>,
  pub gpu_hotspot: Option<f64>,
  pub gpu_freq: Option<f64>,
  pub gpu_power: Option<f64>,
  pub gpu_fan: Option<f64>,
  pub vram_used: Option<f64>,
  pub vram_total: Option<f64>,
  pub cpu_temp: Option<f64>,
  pub cpu_power: Option<f64>,
  pub ram_temp: Option<f64>,
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

fn parse_lhm(data: &Value) -> LhmData {
  // Parse once, then derive all dashboard fields from the flattened sensor list.
  let mut nodes = Vec::new();
  flatten_lhm(data, &mut nodes, "", "");

  let vram_total_idx = nodes
    .iter()
    .position(|n| n.text == "GPU Memory Total" && parse_val(&n.value).map(|v| v > 10000.0).unwrap_or(false));

  let gpu_block: Vec<FlatNode> = if let Some(idx) = vram_total_idx {
    let start = idx.saturating_sub(25);
    let end = (idx + 5).min(nodes.len());
    nodes[start..end].to_vec()
  } else {
    vec![]
  };

  let gpu_find = |parent: &str, text: &str| {
    gpu_block
      .iter()
      .find(|n| n.parent == parent && n.text == text)
      .and_then(|n| parse_val(&n.value))
  };

  let to_mbs = |raw: &str| -> f64 {
    let v = parse_val(raw).unwrap_or(0.0);
    if raw.contains("KB") {
      v / 1024.0
    } else if raw.contains("GB") {
      v * 1024.0
    } else {
      v
    }
  };

  let read_nodes: Vec<&FlatNode> = nodes
    .iter()
    .filter(|n| n.parent == "Throughput" && n.text == "Read Rate")
    .collect();
  let write_nodes: Vec<&FlatNode> = nodes
    .iter()
    .filter(|n| n.parent == "Throughput" && n.text == "Write Rate")
    .collect();

  let disk1_read = read_nodes.first().map(|n| to_mbs(&n.value)).unwrap_or(0.0);
  let disk1_write = write_nodes.first().map(|n| to_mbs(&n.value)).unwrap_or(0.0);
  let disk2_read = read_nodes.get(1).map(|n| to_mbs(&n.value)).unwrap_or(0.0);
  let disk2_write = write_nodes.get(1).map(|n| to_mbs(&n.value)).unwrap_or(0.0);

  let upload_nodes: Vec<&FlatNode> = nodes
    .iter()
    .filter(|n| n.parent == "Throughput" && n.text == "Upload Speed")
    .collect();
  let download_nodes: Vec<&FlatNode> = nodes
    .iter()
    .filter(|n| n.parent == "Throughput" && n.text == "Download Speed")
    .collect();

  let mut best_up = 0.0;
  let mut best_down = 0.0;
  for (i, up_node) in upload_nodes.iter().enumerate() {
    let up = to_mbs(&up_node.value) * 8.0;
    let down = download_nodes.get(i).map(|n| to_mbs(&n.value) * 8.0).unwrap_or(0.0);
    if up + down > best_up + best_down {
      best_up = up;
      best_down = down;
    }
  }

  // Disk temperature sensors: identified by SensorId prefix (/nvme/, /hdd/, /ata/).
  // This avoids false positives from motherboard chips and RAM modules.
  // "Warning Composite" and "Critical Composite" are NVMe thresholds, not readings — excluded.
  // With those removed, max() naturally selects "Composite" (the authoritative NVMe reading).
  // LHM reports 0 as a sentinel for unsupported sensors — skip those too.
  let mut disk_temps: Vec<(String, f64)> = Vec::new();
  for n in nodes.iter().filter(|n| {
    n.parent == "Temperatures"
      && (n.sensor_id.starts_with("/nvme/")
        || n.sensor_id.starts_with("/hdd/")
        || n.sensor_id.starts_with("/ata/")
        || n.sensor_id.starts_with("/scsi/"))
      && !n.text.contains("Warning")
      && !n.text.contains("Critical")
  }) {
    if let Some(t) = parse_val(&n.value).filter(|&v| v > 0.0) {
      if let Some(existing) = disk_temps.iter_mut().find(|(name, _)| name == &n.grandparent) {
        if t > existing.1 {
          existing.1 = t;
        }
      } else if !n.grandparent.is_empty() {
        disk_temps.push((n.grandparent.clone(), t));
      }
    }
  }

  // AMD Ryzen reports "Core (Tctl/Tdie)"; Intel reports "CPU Package" or "Core Average".
  let cpu_temp = ["Core (Tctl/Tdie)", "CPU Package", "Core Average"]
    .iter()
    .find_map(|&name| nodes.iter().find(|n| n.text == name).and_then(|n| parse_val(&n.value)));
  let cpu_power = nodes
    .iter()
    .find(|n| n.parent == "Powers" && n.text == "Package")
    .and_then(|n| parse_val(&n.value));

  // DDR5 (and some DDR4) DIMM temperature sensors: the real reading is always
  // /temperature/0 per slot. /temperature/1-5 are resolution and threshold values
  // (Low Limit, High Limit, Critical Low, Critical High) — those are excluded.
  // Take the highest reading across all populated slots as the representative RAM temperature.
  let ram_temp = nodes
    .iter()
    .filter(|n| {
      n.parent == "Temperatures" && n.sensor_id.ends_with("/temperature/0") && n.sensor_id.starts_with("/memory/dimm/")
    })
    .filter_map(|n| parse_val(&n.value).filter(|&v| v > 0.0))
    .reduce(f64::max);

  LhmData {
    gpu_load: gpu_find("Load", "GPU Core"),
    gpu_temp: gpu_find("Temperatures", "GPU Core"),
    gpu_hotspot: gpu_find("Temperatures", "GPU Hot Spot"),
    gpu_freq: gpu_find("Clocks", "GPU Core"),
    gpu_power: gpu_find("Powers", "GPU Package"),
    gpu_fan: gpu_find("Fans", "GPU Fan"),
    vram_used: gpu_find("Data", "GPU Memory Used"),
    vram_total: gpu_find("Data", "GPU Memory Total"),
    cpu_temp,
    cpu_power,
    ram_temp,
    disk_read: disk1_read + disk2_read,
    disk_write: disk1_write + disk2_write,
    net_up: best_up,
    net_down: best_down,
    disk_temps,
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
        "Text": "CPU", "Value": "",
        "Children": [{
          "Text": "Core (Tctl/Tdie)",
          "Value": "72 °C",
          "Children": []
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
        "Text": "CPU", "Value": "",
        "Children": [{
          "Text": "CPU Package",
          "Value": "68 °C",
          "Children": []
        }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.cpu_temp, Some(68.0));
  }

  #[test]
  fn parse_lhm_prefers_amd_sensor_over_intel_when_both_present() {
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [
        { "Text": "Core (Tctl/Tdie)", "Value": "72 °C", "Children": [] },
        { "Text": "CPU Package",       "Value": "68 °C", "Children": [] }
      ]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.cpu_temp, Some(72.0));
  }

  #[test]
  fn parse_lhm_extracts_cpu_power() {
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Powers", "Value": "",
        "Children": [{
          "Text": "Package",
          "Value": "125 W",
          "Children": []
        }]
      }]
    });
    let result = parse_lhm(&data);
    assert_eq!(result.cpu_power, Some(125.0));
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
  fn parse_lhm_sums_two_disks() {
    let data = json!({
      "Text": "Root", "Value": "",
      "Children": [{
        "Text": "Throughput", "Value": "",
        "Children": [
          { "Text": "Read Rate",  "Value": "10",   "Children": [] },
          { "Text": "Write Rate", "Value": "5",    "Children": [] },
          { "Text": "Read Rate",  "Value": "20",   "Children": [] },
          { "Text": "Write Rate", "Value": "15",   "Children": [] }
        ]
      }]
    });
    let result = parse_lhm(&data);
    assert!(
      (result.disk_read - 30.0).abs() < 1e-9,
      "disk read should sum both drives: {}",
      result.disk_read
    );
    assert!(
      (result.disk_write - 20.0).abs() < 1e-9,
      "disk write should sum both drives: {}",
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
    // Only /nvme/, /hdd/, /ata/ SensorIds are included.
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
  fn parse_lhm_returns_zero_defaults_for_empty_tree() {
    let data = json!({ "Text": "Root", "Value": "", "Children": [] });
    let result = parse_lhm(&data);
    assert_eq!(result.cpu_temp, None);
    assert_eq!(result.gpu_load, None);
    assert_eq!(result.ram_temp, None);
    assert_eq!(result.disk_read, 0.0);
    assert_eq!(result.disk_write, 0.0);
    assert_eq!(result.net_up, 0.0);
    assert_eq!(result.net_down, 0.0);
    assert!(result.disk_temps.is_empty());
  }
}

pub async fn fetch_lhm(client: &reqwest::Client) -> Option<LhmData> {
  // Keep timeout short so stats polling remains responsive even if LHM is down.
  let response = client.get("http://localhost:8085/data.json").send().await.ok()?;
  let json: Value = response.json().await.ok()?;
  Some(parse_lhm(&json))
}
