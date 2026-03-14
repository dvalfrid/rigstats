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
  pub disk_read: f64,
  pub disk_write: f64,
  pub net_up: f64,
  pub net_down: f64,
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

fn flatten_lhm(value: &Value, results: &mut Vec<FlatNode>, parent: &str) {
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

  if !node_val.is_empty() && node_val != "Value" {
    results.push(FlatNode {
      text: text.clone(),
      value: node_val,
      parent: parent.to_string(),
    });
  }

  if let Some(children) = value
    .get("Children")
    .or_else(|| value.get("children"))
    .and_then(Value::as_array)
  {
    let next_parent = if text.is_empty() { parent } else { &text };
    for child in children {
      flatten_lhm(child, results, next_parent);
    }
  }
}

fn parse_lhm(data: &Value) -> LhmData {
  // Parse once, then derive all dashboard fields from the flattened sensor list.
  let mut nodes = Vec::new();
  flatten_lhm(data, &mut nodes, "");

  let vram_total_idx = nodes.iter().position(|n| {
    n.text == "GPU Memory Total" && parse_val(&n.value).map(|v| v > 10000.0).unwrap_or(false)
  });

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

  let disk1_read = read_nodes.get(0).map(|n| to_mbs(&n.value)).unwrap_or(0.0);
  let disk1_write = write_nodes.get(0).map(|n| to_mbs(&n.value)).unwrap_or(0.0);
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
    let down = download_nodes
      .get(i)
      .map(|n| to_mbs(&n.value) * 8.0)
      .unwrap_or(0.0);
    if up + down > best_up + best_down {
      best_up = up;
      best_down = down;
    }
  }

  let cpu_temp = nodes
    .iter()
    .find(|n| n.text == "Core (Tctl/Tdie)")
    .and_then(|n| parse_val(&n.value));
  let cpu_power = nodes
    .iter()
    .find(|n| n.parent == "Powers" && n.text == "Package")
    .and_then(|n| parse_val(&n.value));

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
    disk_read: disk1_read + disk2_read,
    disk_write: disk1_write + disk2_write,
    net_up: best_up,
    net_down: best_down,
  }
}

pub async fn fetch_lhm(client: &reqwest::Client) -> Option<LhmData> {
  // Keep timeout short so stats polling remains responsive even if LHM is down.
  let response = client.get("http://localhost:8085/data.json").send().await.ok()?;
  let json: Value = response.json().await.ok()?;
  Some(parse_lhm(&json))
}
