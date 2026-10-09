//! Sensor pipe client: reads the JSON lines `rigstats-sensor.exe` writes to
//! `\\.\pipe\rigstats-sensors` (LibreHardwareMonitor values the sidecar has
//! already extracted) into `LhmData`, and picks the GPU to display.

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LhmData {
    /// Device name of the GPU currently selected for display (grandparent in LHM tree).
    pub gpu_name: Option<String>,
    pub gpu_load: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub gpu_hotspot: Option<f64>,
    /// VRAM temperature: NVIDIA's memory junction, AMD's "GPU Memory".
    pub gpu_mem_temp: Option<f64>,
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
    /// Wireless devices' batteries (headset, keyboard, mouse — #290), read by
    /// the sidecar about once a minute. Empty from sidecars without it.
    pub peripherals: Vec<Peripheral>,
}

/// One wireless device's battery, as the sidecar last read it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Peripheral {
    pub id: String,
    pub name: String,
    /// "keyboard", "mouse", "headset".
    pub kind: String,
    pub battery: u8,
    pub charging: bool,
}

// --- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        gpu_names_match, normalize_gpu_name, select_gpu_idx, Peripheral, SidecarGpuDevice,
        SidecarPayload,
    };

    /// The sensor pipe contract, checked against the sidecar's own golden
    /// files: each `sensor-sidecar.Tests/fixtures/*/expected.json` is the
    /// payload `HardwareHost` sends (same snake_case options, only indented).
    /// With unknown fields denied in tests, a field the sidecar adds or
    /// renames fails here instead of silently reading as `None`.
    /// The `peripherals` field against the example the sidecar's
    /// `TelemetryContractTests` serializes to — so neither side can rename a
    /// field alone.
    #[test]
    fn sidecar_contract_example_with_peripherals_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../sensor-sidecar.Tests/contract/telemetry-peripherals.json");
        let text = std::fs::read_to_string(&path).expect("read the contract example");
        let payload = serde_json::from_str::<SidecarPayload>(&text)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let data = payload.into_lhm_data(None);
        assert_eq!(
            data.peripherals,
            vec![
                Peripheral {
                    id: "asus-keyboard-1ace-1".into(),
                    name: "ROG Azoth X".into(),
                    kind: "keyboard".into(),
                    battery: 82,
                    charging: false,
                },
                Peripheral {
                    id: "asus-headset-1afa-1".into(),
                    name: "ROG Delta II".into(),
                    kind: "headset".into(),
                    battery: 24,
                    charging: true,
                },
            ]
        );
    }

    #[test]
    fn sidecar_golden_fixtures_match_the_pipe_payload() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../sensor-sidecar.Tests/fixtures");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("sidecar fixtures folder") {
            let path = entry.expect("fixture entry").path().join("expected.json");
            if !path.exists() {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read expected.json");
            let payload = serde_json::from_str::<SidecarPayload>(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let data = payload.into_lhm_data(None);
            assert!(
                data.gpu_devices.iter().all(|(name, _)| !name.is_empty()),
                "{}: GPU without a name",
                path.display()
            );
            checked += 1;
        }
        assert!(
            checked >= 5,
            "only {checked} golden fixtures found in {}",
            dir.display()
        );
    }

    // --- Sidecar pipe transport -----------------------------------------------

    fn make_gpu(name: &str, vram_mb: f32, load: f32) -> SidecarGpuDevice {
        SidecarGpuDevice {
            name: name.to_string(),
            _sensor_family: None,
            load: Some(load),
            temp: None,
            hotspot_temp: None,
            mem_temp: None,
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
    fn normalize_gpu_name_strips_marks_case_and_spacing() {
        assert_eq!(
            normalize_gpu_name("  AMD Radeon(TM)  890M Graphics "),
            "amd radeon 890m graphics"
        );
        assert_eq!(
            normalize_gpu_name("Intel(R) Arc(TM) A770 Graphics"),
            "intel arc a770 graphics"
        );
        assert_eq!(normalize_gpu_name("Intel® Arc™ A770"), "intel arc a770");
    }

    #[test]
    fn gpu_names_match_across_wmi_and_lhm_spellings() {
        assert!(gpu_names_match(
            "AMD Radeon(TM) 890M Graphics",
            "AMD Radeon 890M Graphics"
        ));
        assert!(gpu_names_match(
            "NVIDIA GeForce RTX 5070 Ti Laptop GPU",
            "nvidia geforce rtx 5070 ti laptop gpu"
        ));
        assert!(!gpu_names_match(
            "AMD Radeon(TM) 890M Graphics",
            "NVIDIA GeForce RTX 5070 Ti Laptop GPU"
        ));
        assert!(!gpu_names_match("", "AMD Radeon 890M"));
    }

    #[test]
    fn select_gpu_idx_matches_preferred_despite_trademark_marks() {
        let devices = vec![
            make_gpu("AMD Radeon 890M Graphics", 512.0, 0.0),
            make_gpu("NVIDIA GeForce RTX 5070 Ti Laptop GPU", 12288.0, 0.0),
        ];
        assert_eq!(
            select_gpu_idx(&devices, Some("AMD Radeon(TM) 890M Graphics")),
            Some(0)
        );
    }

    #[test]
    fn select_gpu_idx_prefers_exact_over_substring_match() {
        let devices = vec![
            make_gpu("NVIDIA GeForce RTX 4090 D", 24576.0, 0.0),
            make_gpu("NVIDIA GeForce RTX 4090", 24576.0, 0.0),
        ];
        assert_eq!(
            select_gpu_idx(&devices, Some("NVIDIA GeForce RTX 4090")),
            Some(1)
        );
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
        "load": 60.0, "temp": 72.0, "hotspot_temp": 80.0, "mem_temp": 88.0,
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
        assert!((data.gpu_mem_temp.unwrap() - 88.0).abs() < 0.01);
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
}

// --- Named pipe transport (replaces LHM HTTP client) -----------------------

/// Deserialization structs matching the JSON emitted by `rigstats-sensor.exe`.
///
/// Tests reject unknown fields (`deny_unknown_fields` under `cfg(test)`) so
/// `sidecar_golden_fixtures_match_the_pipe_payload` catches a field the
/// sidecar adds or renames; release builds stay tolerant of a newer sidecar.
#[derive(serde::Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
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
    // Left out by the sidecar until its first battery round, and by older ones.
    #[serde(default)]
    peripherals: Vec<SidecarPeripheral>,
}

#[derive(serde::Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
struct SidecarPeripheral {
    id: String,
    name: String,
    kind: String,
    battery: u8,
    charging: bool,
}

#[derive(serde::Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
struct SidecarGpuDevice {
    name: String,
    // Sent by the sidecar (which sensor names it matched), not used by the app.
    #[serde(default, rename = "sensor_family")]
    _sensor_family: Option<serde::de::IgnoredAny>,
    load: Option<f32>,
    temp: Option<f32>,
    hotspot_temp: Option<f32>,
    // Absent from sidecars before it was added: serde reads it as None.
    mem_temp: Option<f32>,
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
#[cfg_attr(test, serde(deny_unknown_fields))]
struct SidecarMbFan {
    label: String,
    rpm: f32,
}
#[derive(serde::Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
struct SidecarMbTemp {
    label: String,
    celsius: f32,
}
#[derive(serde::Deserialize)]
#[cfg_attr(test, serde(deny_unknown_fields))]
struct SidecarMbVoltage {
    label: String,
    volts: f32,
}

/// Canonical form of a GPU adapter name for comparisons: lower-case, `(TM)`,
/// `(R)`, `™` and `®` removed, whitespace collapsed. WMI and LHM don't always
/// agree on these marks (e.g. "AMD Radeon(TM) 890M" vs "AMD Radeon 890M").
pub fn normalize_gpu_name(name: &str) -> String {
    let lower = name.to_lowercase();
    let stripped = lower
        .replace("(tm)", " ")
        .replace("(r)", " ")
        .replace(['™', '®'], " ");
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether two GPU adapter names refer to the same device — normalized
/// equality, or one containing the other (a user preference saved from one
/// source must still match the other's slightly different name).
pub fn gpu_names_match(a: &str, b: &str) -> bool {
    let (a, b) = (normalize_gpu_name(a), normalize_gpu_name(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

/// Picks the GPU to display: preferred match → highest VRAM → tiebreak by load.
fn select_gpu_idx(devices: &[SidecarGpuDevice], preferred_gpu: Option<&str>) -> Option<usize> {
    if devices.is_empty() {
        return None;
    }
    if let Some(pref) = preferred_gpu {
        // Exact (normalized) match first, so a preference can't be captured by
        // another adapter whose name merely contains it.
        let pref_norm = normalize_gpu_name(pref);
        let pos = devices
            .iter()
            .position(|d| normalize_gpu_name(&d.name) == pref_norm)
            .or_else(|| devices.iter().position(|d| gpu_names_match(&d.name, pref)));
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
            gpu_mem_temp: gpu.and_then(|g| g.mem_temp).map(|v| v as f64),
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
            peripherals: self
                .peripherals
                .into_iter()
                .map(|p| Peripheral {
                    id: p.id,
                    name: p.name,
                    kind: p.kind,
                    battery: p.battery.min(100),
                    charging: p.charging,
                })
                .collect(),
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

/// Persistent pipe reader, held by each `poll_loop` (`src-egui/src/poll.rs`)
/// across ticks and reconnected by `fetch_lhm_pipe` after a disconnect.
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
    if !crate::pipe_server::is_service_pipe(&client) {
        log_pipe_trouble_throttled(
            dir,
            "pipe: rigstats-sensors is not served by the RIGStats service — ignoring it",
        );
        return None;
    }
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
