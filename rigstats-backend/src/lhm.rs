//! LibreHardwareMonitor JSON parsing and transport helpers.
//!
//! LHM publishes a nested tree structure. We flatten it into simple nodes, then
//! extract metrics by parent/text pairs for stable lookup.

#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
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
    /// Sum of power across all detected GPU devices. Use this for system-wide
    /// power estimates; `gpu_power` is the selected GPU only.
    pub total_gpu_power: Option<f64>,
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
    /// All detected GPU devices: `(device_name, vram_total_mb)`.
    /// Used by the frontend to display GPU selector; the backend selects which GPU data to return
    /// in `gpu_*` fields based on user preference and load heuristics.
    pub gpu_devices: Vec<(String, f64)>,
}

#[cfg(test)]
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

#[cfg(test)]
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
#[cfg(test)]
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

#[cfg(test)]
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
/// Collects all GPU candidates from multiple GPU sensor families,
/// then picks the one that is currently active:
///   • If `preferred_gpu` is Some and exists in the candidates, that GPU is selected.
///   • Otherwise, Primary: highest VRAM (stable default, avoids per-tick switching).
///   • Tiebreak: highest load.
///
/// Returns (GpuData, Vec<(device_name, vram_total_mb)>).
#[cfg(test)]
fn extract_gpu(nodes: &[FlatNode], preferred_gpu: Option<&str>) -> (GpuData, Vec<(String, f64)>) {
    // Collect all unique GPU device names from a broad GPU sensor set so iGPU+dGPU
    // systems are represented even when one device lacks "GPU Memory Total".
    let mut seen_devices: Vec<String> = Vec::new();
    let is_gpu_candidate = |n: &FlatNode| {
        if n.grandparent.is_empty() {
            return false;
        }
        // Prefer SensorId family detection — this is stable across vendor naming variants.
        if n.sensor_id.starts_with("/gpu-") {
            return true;
        }
        // Fallback for snapshots where SensorId is missing/incomplete.
        matches!(
            (n.parent.as_str(), n.text.as_str()),
            ("Load", "GPU Core")
                | ("Load", "D3D 3D")
                | ("Data", "GPU Memory Total")
                | ("Data", "D3D Shared Memory Total")
                | ("Temperatures", "GPU Core")
                | ("Temperatures", "GPU VR SoC")
                | ("Clocks", "GPU Core")
        )
    };
    for n in nodes.iter().filter(|n| is_gpu_candidate(n)) {
        if !n.grandparent.is_empty() && !seen_devices.contains(&n.grandparent) {
            seen_devices.push(n.grandparent.clone());
        }
    }

    let load_for = |dev: &str| -> f64 {
        nodes
            .iter()
            .find(|n| n.grandparent == dev && n.parent == "Load" && n.text == "GPU Core")
            .or_else(|| {
                nodes
                    .iter()
                    .find(|n| n.grandparent == dev && n.parent == "Load" && n.text == "D3D 3D")
            })
            .and_then(|n| parse_val(&n.value))
            .unwrap_or(0.0)
    };
    let vram_for = |dev: &str| -> f64 {
        nodes
            .iter()
            .find(|n| n.grandparent == dev && n.parent == "Data" && n.text == "GPU Memory Total")
            .or_else(|| {
                nodes.iter().find(|n| {
                    n.grandparent == dev
                        && n.parent == "Data"
                        && n.text == "D3D Shared Memory Total"
                })
            })
            .and_then(|n| parse_val(&n.value))
            .unwrap_or(0.0)
    };

    // Build list of all GPU devices with their VRAM.
    let gpu_devices: Vec<(String, f64)> = seen_devices
        .iter()
        .map(|dev| (dev.clone(), vram_for(dev)))
        .collect();

    // Match preferred GPU robustly to survive minor naming differences across samples.
    let preferred_match = preferred_gpu.and_then(|pref| {
        let pref_norm = pref.trim().to_ascii_lowercase();
        seen_devices
            .iter()
            .find(|d| {
                let d_norm = d.trim().to_ascii_lowercase();
                d_norm == pref_norm || d_norm.contains(&pref_norm) || pref_norm.contains(&d_norm)
            })
            .cloned()
    });

    // Pick the GPU: prefer user-selected, otherwise use load-based selection.
    let gpu_device: Option<String> = if let Some(pref) = preferred_match {
        Some(pref)
    } else {
        // No user preference: choose the most capable GPU for stable display.
        seen_devices.iter().cloned().max_by(|a, b| {
            let va = vram_for(a);
            let vb = vram_for(b);
            match va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal) {
                std::cmp::Ordering::Equal => load_for(a)
                    .partial_cmp(&load_for(b))
                    .unwrap_or(std::cmp::Ordering::Equal),
                other => other,
            }
        })
    };

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

    let gpu_data = GpuData {
        name: gpu_device.clone(),
        load: find("Load", "GPU Core"),
        // AMD iGPUs (e.g. Radeon 890M) report "GPU VR SoC" instead of "GPU Core" temperature.
        temp: find("Temperatures", "GPU Core").or_else(|| find("Temperatures", "GPU VR SoC")),
        // "GPU Hot Spot" is present on desktop NVIDIA GPUs; laptop GPUs (e.g. RTX
        // 5070 Ti Laptop) expose "GPU Memory Junction" instead — use it as fallback.
        hotspot: find("Temperatures", "GPU Hot Spot")
            .or_else(|| find("Temperatures", "GPU Memory Junction")),
        freq: find("Clocks", "GPU Core"),
        mem_freq: find("Clocks", "GPU Memory"),
        // AMD iGPUs expose the total GPU power as "GPU Core" under Powers rather than "GPU Package".
        power: find("Powers", "GPU Package").or_else(|| find("Powers", "GPU Core")),
        fan: gpu_block
            .iter()
            .find(|n| n.parent == "Fans" && n.text.starts_with("GPU Fan"))
            .and_then(|n| parse_val(&n.value)),
        vram_used: find("Data", "GPU Memory Used"),
        vram_total: find("Data", "GPU Memory Total")
            .or_else(|| find("Data", "D3D Shared Memory Total")),
        d3d_3d: find("Load", "D3D 3D"),
        d3d_vdec: find("Load", "D3D Video Decode"),
    };

    (gpu_data, gpu_devices)
}

/// Returns total disk read and write throughput in MB/s across all drives.
#[cfg(test)]
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
#[cfg(test)]
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
        let down = downloads
            .get(i)
            .map(|n| to_mbs(&n.value) * 8.0)
            .unwrap_or(0.0);
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
#[cfg(test)]
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
#[cfg(test)]
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
#[cfg(test)]
fn extract_ram_temp(nodes: &[FlatNode]) -> Option<f64> {
    nodes
        .iter()
        .filter(|n| {
            n.parent == "Temperatures"
                && n.sensor_id.starts_with("/memory/dimm/")
                && n.sensor_id.ends_with("/temperature/0")
        })
        .filter_map(|n| parse_val(&n.value).filter(|&v| v > 0.0))
        .reduce(f64::max)
}

#[cfg(test)]
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
#[cfg(test)]
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
            n.sensor_id.starts_with("/lpc/")
                && n.sensor_id.contains("/voltage/")
                && !n.text.starts_with("Voltage #")
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

#[cfg(test)]
fn parse_lhm(data: &Value, preferred_gpu: Option<&str>) -> LhmData {
    let mut nodes = Vec::new();
    flatten_lhm(data, &mut nodes, "", "");

    let (gpu, gpu_devices) = extract_gpu(&nodes, preferred_gpu);
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
        total_gpu_power: gpu.power,
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
        gpu_devices,
    }
}

// --- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        extract_disk_temps, extract_motherboard, extract_network, extract_ram_temp, flatten_lhm,
        parse_lhm, parse_val, select_gpu_idx, FlatNode, SidecarGpuDevice, SidecarPayload,
    };
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
        assert_eq!(
            result.cpu_temp,
            Some(68.0),
            "temp must come from Temperatures section"
        );
        assert_eq!(
            result.cpu_power,
            Some(95.0),
            "power must come from Powers section"
        );
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
        assert_eq!(
            result.disk_temps.len(),
            2,
            "motherboard sensor must be excluded"
        );
        assert_eq!(result.disk_temps[0].0, "Samsung SSD 980 PRO");
        assert_eq!(
            result.disk_temps[0].1, 44.0,
            "Composite wins (highest real sensor)"
        );
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
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
        load_children.insert(
            0,
            json!({ "Text": "GPU Core", "Value": "10 %", "Children": [] }),
        );

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
        let result = parse_lhm(&data, None);
        assert_eq!(
            result.gpu_temp,
            Some(72.0),
            "temp must be found despite many load sensors"
        );
        assert_eq!(result.gpu_freq, Some(2520.0), "clock must be found");
        assert_eq!(result.gpu_power, Some(150.0), "power must be found");
        assert_eq!(
            result.gpu_fan,
            Some(1200.0),
            "fan with suffix '1' must be found"
        );
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
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&data, None);
        assert_eq!(result.gpu_load, Some(75.0));
        assert_eq!(result.gpu_d3d_3d, Some(68.0));
        assert_eq!(result.gpu_d3d_vdec, Some(12.0));
        // D3D Copy is intentionally not extracted — only 3D and Video Decode are surfaced.
        assert_eq!(
            result.gpu_mem_freq, None,
            "mem clock absent from tree → None"
        );
    }

    #[test]
    fn parse_lhm_returns_zero_defaults_for_empty_tree() {
        let data = json!({ "Text": "Root", "Value": "", "Children": [] });
        let result = parse_lhm(&data, None);
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
        let result = parse_lhm(&lpc_tree(), None);
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
        let result = parse_lhm(&data, None);
        assert_eq!(
            result.mb_fans.len(),
            7,
            "all active fan channels are returned"
        );
        assert_eq!(result.mb_fans[0].0, "Fan #7", "highest RPM first");
        assert_eq!(result.mb_fans[6].0, "Fan #1", "lowest RPM last");
    }

    #[test]
    fn parse_lhm_extracts_mb_temps_filters_sentinel_below_5c() {
        let result = parse_lhm(&lpc_tree(), None);
        // Temperature #3 = 2 °C must be filtered out.
        assert_eq!(result.mb_temps.len(), 2);
        assert_eq!(result.mb_temps[0].0, "Temperature #1");
        assert!((result.mb_temps[0].1 - 35.5).abs() < 0.01);
        assert_eq!(result.mb_temps[1].0, "Temperature #2");
        assert!((result.mb_temps[1].1 - 30.0).abs() < 0.01);
    }

    #[test]
    fn parse_lhm_extracts_mb_named_voltages_only() {
        let result = parse_lhm(&lpc_tree(), None);
        // "Voltage #5" must be excluded; three named rails remain.
        assert_eq!(result.mb_voltages.len(), 3);
        let names: Vec<&str> = result.mb_voltages.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Vcore"));
        assert!(names.contains(&"AVCC"));
        assert!(names.contains(&"+3.3V"));
        assert!(
            !names.contains(&"Voltage #5"),
            "generic slots must be excluded"
        );
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
        let result = parse_lhm(&data, None);
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
    fn extract_gpu_defaults_to_dgpu_when_igpu_is_more_active() {
        // Default policy is stable: dGPU (most VRAM) wins even when iGPU load is higher.
        let result = parse_lhm(&igpu_dgpu_tree("11 %", "0 %"), None);
        assert_eq!(
            result.gpu_name.as_deref(),
            Some("NVIDIA GeForce RTX 5070 Ti Laptop GPU"),
            "default selection must stay on dGPU for stability"
        );
        assert_eq!(result.gpu_load, Some(0.0));
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
        let result = parse_lhm(&data, None);
        assert_eq!(
            result.gpu_temp,
            Some(51.0),
            "GPU VR SoC must be used as temp"
        );
        assert_eq!(
            result.gpu_power,
            Some(2.0),
            "GPU Core under Powers must be used"
        );
        assert_eq!(result.gpu_load, Some(11.0));
        assert_eq!(result.gpu_freq, Some(1343.0));
        assert_eq!(result.gpu_mem_freq, Some(1000.0));
    }

    #[test]
    fn extract_gpu_picks_dgpu_when_active() {
        // dGPU at 60 %, iGPU at 5 % — dGPU must win.
        let result = parse_lhm(&igpu_dgpu_tree("5 %", "60 %"), None);
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
        let result = parse_lhm(&igpu_dgpu_tree("0 %", "0 %"), None);
        assert_eq!(
            result.gpu_name.as_deref(),
            Some("NVIDIA GeForce RTX 5070 Ti Laptop GPU"),
            "dGPU (most VRAM) must win when both are idle"
        );
    }

    #[test]
    fn extract_gpu_prefers_explicit_gpu_exact_match() {
        let result = parse_lhm(&igpu_dgpu_tree("11 %", "0 %"), Some("AMD Radeon 890M"));
        assert_eq!(
            result.gpu_name.as_deref(),
            Some("AMD Radeon 890M"),
            "explicit preferred GPU must override default stable selection"
        );
        assert_eq!(result.gpu_load, Some(11.0));
    }

    #[test]
    fn extract_gpu_prefers_explicit_gpu_fuzzy_match() {
        let result = parse_lhm(&igpu_dgpu_tree("11 %", "0 %"), Some("radeon 890m"));
        assert_eq!(
            result.gpu_name.as_deref(),
            Some("AMD Radeon 890M"),
            "case-insensitive fuzzy preferred name must match"
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
        let result = parse_lhm(&data, None);
        assert_eq!(
            result.mb_voltages.len(),
            2,
            "only named SVI2 rails, no per-core VID entries"
        );
        let names: Vec<&str> = result.mb_voltages.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Core (SVI2 TFN)"));
        assert!(names.contains(&"SoC (SVI2 TFN)"));
        assert!(
            !names.iter().any(|n| n.contains("VID")),
            "VID entries must be excluded"
        );
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
        let result = parse_lhm(&data, None);
        assert_eq!(result.mb_voltages.len(), 1, "LPC voltage only");
        assert_eq!(result.mb_voltages[0].0, "Vcore", "LPC sensor must win");
    }

    // --- Sidecar pipe transport -----------------------------------------------

    fn make_gpu(name: &str, vram_mb: f32, load: f32) -> SidecarGpuDevice {
        SidecarGpuDevice {
            name: name.to_string(),
            load: Some(load),
            temp: None,
            hotspot_temp: None,
            core_clock: None,
            mem_clock: None,
            power: None,
            fan: None,
            vram_used_mb: None,
            vram_total_mb: Some(vram_mb),
            d3d_3d: None,
            d3d_vdec: None,
        }
    }

    #[test]
    fn select_gpu_idx_returns_none_for_empty() {
        assert_eq!(select_gpu_idx(&[], None), None);
    }

    #[test]
    fn select_gpu_idx_single_device_returns_zero() {
        let devices = vec![make_gpu("RTX 4090", 24576.0, 0.0)];
        assert_eq!(select_gpu_idx(&devices, None), Some(0));
    }

    #[test]
    fn select_gpu_idx_picks_highest_vram_by_default() {
        let devices = vec![
            make_gpu("Radeon 890M", 512.0, 11.0),
            make_gpu("RTX 5070 Ti", 8192.0, 0.0),
        ];
        assert_eq!(
            select_gpu_idx(&devices, None),
            Some(1),
            "dGPU (more VRAM) must win even when iGPU load is higher"
        );
    }

    #[test]
    fn select_gpu_idx_tiebreaks_by_load() {
        let devices = vec![
            make_gpu("GPU A", 8192.0, 5.0),
            make_gpu("GPU B", 8192.0, 60.0),
        ];
        assert_eq!(
            select_gpu_idx(&devices, None),
            Some(1),
            "higher load must win on VRAM tie"
        );
    }

    #[test]
    fn select_gpu_idx_respects_preferred_exact_match() {
        let devices = vec![
            make_gpu("Radeon 890M", 512.0, 11.0),
            make_gpu("RTX 5070 Ti", 8192.0, 0.0),
        ];
        assert_eq!(select_gpu_idx(&devices, Some("Radeon 890M")), Some(0));
    }

    #[test]
    fn select_gpu_idx_respects_preferred_case_insensitive() {
        let devices = vec![
            make_gpu("Radeon 890M", 512.0, 11.0),
            make_gpu("RTX 5070 Ti", 8192.0, 0.0),
        ];
        assert_eq!(select_gpu_idx(&devices, Some("radeon 890m")), Some(0));
    }

    #[test]
    fn select_gpu_idx_falls_back_when_preferred_not_found() {
        let devices = vec![
            make_gpu("Radeon 890M", 512.0, 0.0),
            make_gpu("RTX 5070 Ti", 8192.0, 0.0),
        ];
        // Unknown preference → fall back to highest VRAM
        assert_eq!(select_gpu_idx(&devices, Some("GTX 1080")), Some(1));
    }

    #[test]
    fn sidecar_payload_full_round_trip() {
        let json = r#"{
      "cpu_temp": 72.0,
      "cpu_power": 95.0,
      "gpu_devices": [{
        "name": "NVIDIA GeForce RTX 4090",
        "load": 60.0, "temp": 72.0, "hotspot_temp": 80.0,
        "core_clock": 2520.0, "mem_clock": 10501.0,
        "power": 150.0, "fan": 1200.0,
        "vram_used_mb": 4096.0, "vram_total_mb": 24576.0,
        "d3d_3d": 55.0, "d3d_vdec": 12.0
      }],
      "disk_temps": {"Samsung SSD 980 PRO": 44.0, "WD Blue": 35.0},
      "ram_temp": 38.0,
      "mb_fans": [{"label": "Fan #1", "rpm": 882.0}],
      "mb_temps": [{"label": "Temperature #1", "celsius": 35.5}],
      "mb_voltages": [{"label": "Vcore", "volts": 1.048}],
      "mb_chip": "Nuvoton NCT6799D"
    }"#;

        let data = serde_json::from_str::<SidecarPayload>(json)
            .expect("JSON must deserialize")
            .into_lhm_data(None);

        assert_eq!(data.cpu_temp, Some(72.0));
        assert_eq!(data.cpu_power, Some(95.0));
        assert_eq!(data.ram_temp, Some(38.0));
        assert_eq!(data.gpu_name.as_deref(), Some("NVIDIA GeForce RTX 4090"));
        assert!((data.gpu_load.unwrap() - 60.0).abs() < 0.01);
        assert!((data.gpu_temp.unwrap() - 72.0).abs() < 0.01);
        assert!((data.gpu_hotspot.unwrap() - 80.0).abs() < 0.01);
        assert!((data.gpu_freq.unwrap() - 2520.0).abs() < 0.01);
        assert!((data.gpu_mem_freq.unwrap() - 10501.0).abs() < 0.01);
        assert!((data.gpu_power.unwrap() - 150.0).abs() < 0.01);
        assert!((data.gpu_fan.unwrap() - 1200.0).abs() < 0.01);
        assert!((data.vram_used.unwrap() - 4096.0).abs() < 0.01);
        assert!((data.vram_total.unwrap() - 24576.0).abs() < 0.01);
        assert!((data.gpu_d3d_3d.unwrap() - 55.0).abs() < 0.01);
        assert!((data.gpu_d3d_vdec.unwrap() - 12.0).abs() < 0.01);
        assert_eq!(data.disk_temps.len(), 2);
        assert_eq!(data.mb_fans.len(), 1);
        assert_eq!(data.mb_fans[0].0, "Fan #1");
        assert!((data.mb_fans[0].1 - 882.0).abs() < 0.01);
        assert_eq!(data.mb_temps.len(), 1);
        assert!((data.mb_temps[0].1 - 35.5).abs() < 0.01);
        assert_eq!(data.mb_voltages.len(), 1);
        assert!((data.mb_voltages[0].1 - 1.048).abs() < 0.001);
        assert_eq!(data.mb_chip.as_deref(), Some("Nuvoton NCT6799D"));
        assert_eq!(data.gpu_devices.len(), 1);
        assert_eq!(data.gpu_devices[0].0, "NVIDIA GeForce RTX 4090");
        assert!((data.gpu_devices[0].1 - 24576.0).abs() < 0.01);
        // Placeholders until sidecar emits throughput
        assert_eq!(data.disk_read, 0.0);
        assert_eq!(data.disk_write, 0.0);
        assert_eq!(data.net_up, 0.0);
        assert_eq!(data.net_down, 0.0);
    }

    #[test]
    fn sidecar_payload_no_gpus_yields_none_fields() {
        let json = r#"{
      "cpu_temp": 65.0, "cpu_power": null,
      "gpu_devices": [],
      "disk_temps": {}, "ram_temp": null,
      "mb_fans": [], "mb_temps": [], "mb_voltages": [], "mb_chip": null
    }"#;
        let data = serde_json::from_str::<SidecarPayload>(json)
            .unwrap()
            .into_lhm_data(None);
        assert_eq!(data.gpu_name, None);
        assert_eq!(data.gpu_load, None);
        assert_eq!(data.gpu_temp, None);
        assert!(data.gpu_devices.is_empty());
    }

    #[test]
    fn sidecar_payload_gpu_preference_overrides_vram_heuristic() {
        let json = r#"{
      "cpu_temp": null, "cpu_power": null,
      "gpu_devices": [
        {"name": "AMD Radeon 890M",   "load": 11.0, "temp": null, "hotspot_temp": null,
         "core_clock": null, "mem_clock": null, "power": null, "fan": null,
         "vram_used_mb": null, "vram_total_mb": 512.0, "d3d_3d": null, "d3d_vdec": null},
        {"name": "RTX 5070 Ti Laptop", "load": 0.0,  "temp": null, "hotspot_temp": null,
         "core_clock": null, "mem_clock": null, "power": null, "fan": null,
         "vram_used_mb": null, "vram_total_mb": 8192.0, "d3d_3d": null, "d3d_vdec": null}
      ],
      "disk_temps": {}, "ram_temp": null,
      "mb_fans": [], "mb_temps": [], "mb_voltages": [], "mb_chip": null
    }"#;

        // Default: dGPU wins on VRAM
        let data = serde_json::from_str::<SidecarPayload>(json)
            .unwrap()
            .into_lhm_data(None);
        assert_eq!(data.gpu_name.as_deref(), Some("RTX 5070 Ti Laptop"));

        // Preference: iGPU selected despite lower VRAM
        let data = serde_json::from_str::<SidecarPayload>(json)
            .unwrap()
            .into_lhm_data(Some("AMD Radeon 890M"));
        assert_eq!(data.gpu_name.as_deref(), Some("AMD Radeon 890M"));
        assert!((data.gpu_load.unwrap() - 11.0).abs() < 0.01);
    }

    // --- Direct extract_* edge cases ------------------------------------------
    //
    // These call the extractor functions on hand-built FlatNode vectors (bypassing
    // the JSON flatten step) to pin specific filtering rules as readable regression
    // tests. They cover edge cases not already exercised by the parse_lhm_* suite.

    fn node(parent: &str, text: &str, value: &str, sensor_id: &str) -> FlatNode {
        FlatNode {
            text: text.to_string(),
            value: value.to_string(),
            parent: parent.to_string(),
            grandparent: String::new(),
            sensor_id: sensor_id.to_string(),
        }
    }

    fn node_gp(
        parent: &str,
        grandparent: &str,
        text: &str,
        value: &str,
        sensor_id: &str,
    ) -> FlatNode {
        FlatNode {
            grandparent: grandparent.to_string(),
            ..node(parent, text, value, sensor_id)
        }
    }

    #[test]
    fn extract_motherboard_excludes_zero_rpm_fan() {
        let nodes = vec![
            node("Fans", "CPU Fan", "0 RPM", "/lpc/nct6799d/0/fan/0"),
            node("Fans", "Chassis Fan", "1200 RPM", "/lpc/nct6799d/0/fan/1"),
        ];
        let mb = extract_motherboard(&nodes);
        assert_eq!(mb.fans.len(), 1, "0 RPM fan must be dropped");
        assert_eq!(mb.fans[0].0, "Chassis Fan");
    }

    #[test]
    fn extract_motherboard_excludes_temp_below_5c_sentinel() {
        let nodes = vec![
            node(
                "Temperatures",
                "System",
                "34 °C",
                "/lpc/nct6799d/0/temperature/0",
            ),
            node(
                "Temperatures",
                "Unused",
                "1 °C",
                "/lpc/nct6799d/0/temperature/1",
            ),
        ];
        let mb = extract_motherboard(&nodes);
        assert_eq!(mb.temps.len(), 1, "sub-5 °C sentinel must be dropped");
        assert_eq!(mb.temps[0].0, "System");
    }

    #[test]
    fn extract_motherboard_amd_svi2_fallback_excludes_vid_rails() {
        // No LPC chip (laptop EC) → AMD SVI2 fallback path. Per-core "… VID" readouts
        // are switching targets, not supply rails, and must be excluded.
        let nodes = vec![
            node(
                "Voltages",
                "Core (SVI2 TFN)",
                "1,350 V",
                "/amdcpu/0/voltage/0",
            ),
            node("Voltages", "Core #1 VID", "1,400 V", "/amdcpu/0/voltage/1"),
        ];
        let mb = extract_motherboard(&nodes);
        assert_eq!(mb.voltages.len(), 1, "VID rail must be excluded");
        assert_eq!(mb.voltages[0].0, "Core (SVI2 TFN)");
    }

    #[test]
    fn extract_network_mismatched_upload_download_counts_default_to_zero() {
        // More uploads than downloads: the upload without a matching download index
        // must pair with a 0 download rather than panicking or reusing another index.
        let nodes = vec![
            node(
                "Throughput",
                "Upload Speed",
                "5 MB/s",
                "/nic/0/throughput/0",
            ),
            node(
                "Throughput",
                "Download Speed",
                "1 MB/s",
                "/nic/0/throughput/1",
            ),
            node(
                "Throughput",
                "Upload Speed",
                "20 MB/s",
                "/nic/1/throughput/0",
            ),
        ];
        let (up, down) = extract_network(&nodes);
        // Interface 1 has the largest combined throughput (20 MB/s up, 0 down).
        assert!((up - 20.0 * 8.0).abs() < 0.01, "busiest upload in Mbit/s");
        assert_eq!(down, 0.0, "missing download index defaults to 0");
    }

    #[test]
    fn extract_disk_temps_excludes_empty_sensor_id() {
        // A temperature with no storage SensorId prefix (e.g. blank id) must be ignored.
        let nodes = vec![
            node_gp("Temperatures", "Ghost Drive", "Temperature", "40 °C", ""),
            node_gp(
                "Temperatures",
                "Samsung 990",
                "Temperature",
                "42 °C",
                "/nvme/0/temperature/0",
            ),
        ];
        let temps = extract_disk_temps(&nodes);
        assert_eq!(temps.len(), 1);
        assert_eq!(temps[0].0, "Samsung 990");
    }

    #[test]
    fn extract_disk_temps_excludes_warning_and_critical_and_zero() {
        let nodes = vec![
            node_gp(
                "Temperatures",
                "NVMe",
                "Temperature",
                "45 °C",
                "/nvme/0/temperature/0",
            ),
            node_gp(
                "Temperatures",
                "NVMe",
                "Warning Composite",
                "80 °C",
                "/nvme/0/temperature/1",
            ),
            node_gp(
                "Temperatures",
                "NVMe",
                "Critical Composite",
                "90 °C",
                "/nvme/0/temperature/2",
            ),
            node_gp(
                "Temperatures",
                "NVMe",
                "Temperature 4",
                "0 °C",
                "/nvme/0/temperature/3",
            ),
        ];
        let temps = extract_disk_temps(&nodes);
        assert_eq!(temps.len(), 1);
        assert!(
            (temps[0].1 - 45.0).abs() < 0.01,
            "threshold/zero sensors excluded"
        );
    }

    #[test]
    fn extract_ram_temp_uses_temperature0_index_only() {
        // Only the per-DIMM /temperature/0 sensor is the real reading; indices 1–5 are
        // resolution/threshold values and must be ignored even if numerically higher.
        let nodes = vec![
            node(
                "Temperatures",
                "DIMM #1",
                "38 °C",
                "/memory/dimm/0/temperature/0",
            ),
            node(
                "Temperatures",
                "DIMM #1 Max",
                "99 °C",
                "/memory/dimm/0/temperature/1",
            ),
        ];
        assert_eq!(extract_ram_temp(&nodes), Some(38.0));
    }
}

// --- Named pipe transport (replaces LHM HTTP client) -----------------------

/// Deserialization structs matching the JSON emitted by `rigstats-sensor.exe`.
#[derive(serde::Deserialize)]
struct SidecarPayload {
    cpu_temp: Option<f32>,
    cpu_power: Option<f32>,
    gpu_devices: Vec<SidecarGpuDevice>,
    disk_temps: std::collections::HashMap<String, f32>,
    ram_temp: Option<f32>,
    mb_fans: Vec<SidecarMbFan>,
    mb_temps: Vec<SidecarMbTemp>,
    mb_voltages: Vec<SidecarMbVoltage>,
    mb_chip: Option<String>,
}

#[derive(serde::Deserialize)]
struct SidecarGpuDevice {
    name: String,
    load: Option<f32>,
    temp: Option<f32>,
    hotspot_temp: Option<f32>,
    core_clock: Option<f32>,
    mem_clock: Option<f32>,
    power: Option<f32>,
    fan: Option<f32>,
    vram_used_mb: Option<f32>,
    vram_total_mb: Option<f32>,
    d3d_3d: Option<f32>,
    d3d_vdec: Option<f32>,
}

#[derive(serde::Deserialize)]
struct SidecarMbFan {
    label: String,
    rpm: f32,
}
#[derive(serde::Deserialize)]
struct SidecarMbTemp {
    label: String,
    celsius: f32,
}
#[derive(serde::Deserialize)]
struct SidecarMbVoltage {
    label: String,
    volts: f32,
}

/// Picks the GPU to display: preferred match → highest VRAM → tiebreak by load.
/// Mirrors the selection logic in `extract_gpu` for the HTTP path.
fn select_gpu_idx(devices: &[SidecarGpuDevice], preferred_gpu: Option<&str>) -> Option<usize> {
    if devices.is_empty() {
        return None;
    }
    if let Some(pref) = preferred_gpu {
        let pref_norm = pref.trim().to_ascii_lowercase();
        let pos = devices.iter().position(|d| {
            let dn = d.name.trim().to_ascii_lowercase();
            dn == pref_norm || dn.contains(&pref_norm) || pref_norm.contains(&dn)
        });
        if pos.is_some() {
            return pos;
        }
    }
    devices
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            let va = a.vram_total_mb.unwrap_or(0.0);
            let vb = b.vram_total_mb.unwrap_or(0.0);
            match va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal) {
                std::cmp::Ordering::Equal => a
                    .load
                    .unwrap_or(0.0)
                    .partial_cmp(&b.load.unwrap_or(0.0))
                    .unwrap_or(std::cmp::Ordering::Equal),
                other => other,
            }
        })
        .map(|(i, _)| i)
}

impl SidecarPayload {
    fn into_lhm_data(self, preferred_gpu: Option<&str>) -> LhmData {
        let gpu_devices: Vec<(String, f64)> = self
            .gpu_devices
            .iter()
            .map(|g| (g.name.clone(), g.vram_total_mb.unwrap_or(0.0) as f64))
            .collect();

        let gpu = select_gpu_idx(&self.gpu_devices, preferred_gpu).map(|i| &self.gpu_devices[i]);

        let total_gpu_power: Option<f64> = {
            let powers: Vec<f64> = self
                .gpu_devices
                .iter()
                .filter_map(|g| g.power.map(|v| v as f64))
                .collect();
            if powers.is_empty() {
                None
            } else {
                Some(powers.iter().sum())
            }
        };

        LhmData {
            gpu_name: gpu.map(|g| g.name.clone()),
            gpu_load: gpu.and_then(|g| g.load).map(|v| v as f64),
            gpu_temp: gpu.and_then(|g| g.temp).map(|v| v as f64),
            gpu_hotspot: gpu.and_then(|g| g.hotspot_temp).map(|v| v as f64),
            gpu_freq: gpu.and_then(|g| g.core_clock).map(|v| v as f64),
            gpu_mem_freq: gpu.and_then(|g| g.mem_clock).map(|v| v as f64),
            gpu_power: gpu.and_then(|g| g.power).map(|v| v as f64),
            total_gpu_power,
            gpu_fan: gpu.and_then(|g| g.fan).map(|v| v as f64),
            vram_used: gpu.and_then(|g| g.vram_used_mb).map(|v| v as f64),
            vram_total: gpu.and_then(|g| g.vram_total_mb).map(|v| v as f64),
            gpu_d3d_3d: gpu.and_then(|g| g.d3d_3d).map(|v| v as f64),
            gpu_d3d_vdec: gpu.and_then(|g| g.d3d_vdec).map(|v| v as f64),
            cpu_temp: self.cpu_temp.map(|v| v as f64),
            cpu_power: self.cpu_power.map(|v| v as f64),
            ram_temp: self.ram_temp.map(|v| v as f64),
            // Disk throughput not yet extracted by sidecar — will be added in follow-up.
            disk_read: 0.0,
            disk_write: 0.0,
            // Network is sourced from sysinfo in commands.rs, not from LHM.
            net_up: 0.0,
            net_down: 0.0,
            disk_temps: self
                .disk_temps
                .into_iter()
                .map(|(k, v)| (k, v as f64))
                .collect(),
            mb_fans: self
                .mb_fans
                .into_iter()
                .map(|f| (f.label, f.rpm as f64))
                .collect(),
            mb_temps: self
                .mb_temps
                .into_iter()
                .map(|t| (t.label, t.celsius as f64))
                .collect(),
            mb_voltages: self
                .mb_voltages
                .into_iter()
                .map(|v| (v.label, v.volts as f64))
                .collect(),
            mb_chip: self.mb_chip,
            gpu_devices,
        }
    }
}

/// Unix timestamp of the last pipe-trouble log message.
/// Throttles to one entry per 30-second window so the log stays readable.
static LAST_PIPE_FAIL_LOG_SECS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Logs a pipe-trouble message at most once per 30-second window. Connect
/// failures and established-connection read errors/timeouts share this window,
/// so a persistently broken pipe (e.g. a hung sidecar holding the pipe open)
/// writes at most one debug line per 30 s instead of one every tick.
fn log_pipe_trouble_throttled(dir: &std::path::Path, msg: &str) {
    use crate::debug::{log_warn, unix_now_secs};
    use std::sync::atomic::Ordering;
    let now = unix_now_secs();
    let last = LAST_PIPE_FAIL_LOG_SECS.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= 30 {
        LAST_PIPE_FAIL_LOG_SECS.store(now, Ordering::Relaxed);
        log_warn(dir, msg);
    }
}

/// Persistent pipe reader stored in `AppState`.
pub type LhmPipeReader = tokio::io::BufReader<tokio::net::windows::named_pipe::NamedPipeClient>;

/// Upper bound on a single newline-delimited sidecar frame. A healthy sidecar
/// emits a few KB of JSON per tick; anything past this indicates a buggy or
/// runaway sidecar, so we drop the connection (and log) rather than keep
/// buffering an unbounded line into memory.
const MAX_PIPE_LINE_BYTES: usize = 256 * 1024;

/// Reads one sensor sample from the sidecar named pipe.
///
/// Reuses the existing connection when healthy; reconnects transparently on
/// disconnect or timeout so the stats tick never blocks longer than 1200 ms.
pub async fn fetch_lhm_pipe(
    pipe: &tokio::sync::Mutex<Option<LhmPipeReader>>,
    preferred_gpu: Option<&str>,
    dir: &std::path::Path,
) -> Option<LhmData> {
    use crate::debug::{append_debug_log, log_error, log_warn};
    use tokio::io::AsyncBufReadExt;

    let mut guard = pipe.lock().await;

    // Try reading from the established connection first.
    if let Some(ref mut reader) = *guard {
        let mut line = String::new();
        let res = tokio::time::timeout(
            std::time::Duration::from_millis(1200),
            reader.read_line(&mut line),
        )
        .await;
        match res {
            Ok(Ok(n)) if n > 0 => {
                if line.len() > MAX_PIPE_LINE_BYTES {
                    log_warn(
            dir,
            &format!(
              "pipe: oversized frame ({} bytes, cap {MAX_PIPE_LINE_BYTES}) — dropping connection to resync",
              line.len()
            ),
          );
                    *guard = None;
                    return None;
                }
                return match serde_json::from_str::<SidecarPayload>(line.trim()) {
                    Ok(p) => Some(p.into_lhm_data(preferred_gpu)),
                    Err(e) => {
                        let preview = line.trim().chars().take(120).collect::<String>();
                        log_error(
                            dir,
                            &format!("pipe: JSON parse error: {e} — raw: {preview}"),
                        );
                        None
                    }
                };
            }
            Ok(Err(e)) => {
                log_pipe_trouble_throttled(dir, &format!("pipe: read error (established): {e}"));
                *guard = None;
            }
            Err(_) => {
                log_pipe_trouble_throttled(dir, "pipe: read timed out (established connection)");
                *guard = None;
            }
            Ok(Ok(_)) => {
                // n == 0: EOF — server closed its end.
                *guard = None;
            }
        }
    }

    // Connect (first call or after disconnect).
    // The sidecar pipe is PipeDirection.Out (server writes, client reads only).
    // Windows denies GENERIC_WRITE access on an outbound-only pipe, so we must
    // explicitly request read-only access to avoid ERROR_ACCESS_DENIED (os=5).
    let client = match tokio::net::windows::named_pipe::ClientOptions::new()
        .write(false)
        .open(r"\\.\pipe\rigstats-sensors")
    {
        Ok(c) => c,
        Err(e) => {
            log_pipe_trouble_throttled(
                dir,
                &format!("pipe: connect failed: {e} (os={:?})", e.raw_os_error()),
            );
            return None;
        }
    };
    append_debug_log(dir, "pipe: connected to rigstats-sensors");
    let mut reader = tokio::io::BufReader::new(client);

    let mut line = String::new();
    let res = tokio::time::timeout(
        std::time::Duration::from_millis(1200),
        reader.read_line(&mut line),
    )
    .await;

    match res {
        Ok(Ok(n)) if n > 0 => {
            if line.len() > MAX_PIPE_LINE_BYTES {
                log_warn(
          dir,
          &format!(
            "pipe: oversized frame ({} bytes, cap {MAX_PIPE_LINE_BYTES}) on first read — discarding connection",
            line.len()
          ),
        );
                return None;
            }
            let data = match serde_json::from_str::<SidecarPayload>(line.trim()) {
                Ok(p) => Some(p.into_lhm_data(preferred_gpu)),
                Err(e) => {
                    let preview = line.trim().chars().take(120).collect::<String>();
                    log_error(
                        dir,
                        &format!("pipe: JSON parse error (first read): {e} — raw: {preview}"),
                    );
                    None
                }
            };
            // Store the live connection even if parsing failed — sidecar is up.
            *guard = Some(reader);
            data
        }
        Ok(Err(e)) => {
            log_warn(dir, &format!("pipe: read error (first connect): {e}"));
            None
        }
        Err(_) => {
            log_warn(dir, "pipe: timed out waiting for first line after connect");
            None
        }
        Ok(Ok(_)) => None, // n == 0: EOF immediately after connect
    }
}
