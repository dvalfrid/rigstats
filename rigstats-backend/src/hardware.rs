//! Windows hardware detection via WMI and PowerShell fallbacks.
//!
//! All public functions here are called once at startup; their results are
//! stored in `AppState` so the per-tick hot path pays no WMI/process cost.
//! Each function tries WMI first and falls back to a PowerShell CIM call on
//! any COM/WMI failure, keeping the app functional even on locked-down systems.

use crate::debug::run_hidden_command;
use crate::stats::DiskKind;
use serde::Deserialize;

// --- WMI row structs -------------------------------------------------------
// Field names must match the WMI property names exactly (PascalCase).
// Structs used with typed `conn.query::<T>()` MUST carry a container
// `#[serde(rename = "Win32_…")]`: the `wmi` crate builds `SELECT … FROM <name>`
// from the serde container name, so without it the query targets a class that
// doesn't exist, fails, and the caller silently falls back to a ~1 s PowerShell
// process (#198). `wmi_classes_tests` below guards this.

#[cfg(windows)]
#[derive(Deserialize, Debug)]
#[serde(rename = "Win32_VideoController")]
struct VideoControllerName {
    #[serde(rename = "Name")]
    name: Option<String>,
}

#[cfg(windows)]
#[derive(Deserialize, Debug)]
#[serde(rename = "Win32_ComputerSystem")]
struct ComputerSystem {
    #[serde(rename = "Manufacturer")]
    manufacturer: Option<String>,
    #[serde(rename = "Model")]
    model: Option<String>,
}

#[cfg(windows)]
#[derive(Deserialize, Debug)]
#[serde(rename = "Win32_ComputerSystemProduct")]
struct ComputerSystemProduct {
    #[serde(rename = "Version")]
    version: Option<String>,
    #[serde(rename = "Name")]
    name: Option<String>,
}

#[cfg(windows)]
#[derive(Deserialize, Debug)]
#[serde(rename = "Win32_BaseBoard")]
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
#[serde(rename = "Win32_PhysicalMemory")]
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
            let conn = wmi::WMIConnection::new(com)
                .map_err(|e| format!("WMI connection failed: {}", e))?;

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
    if lower.contains("radeon rx")
        || lower.contains("geforce")
        || lower.contains("rtx")
        || lower.contains("arc")
    {
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
fn gpu_names_from_shell() -> Vec<String> {
    let Ok(output) = run_hidden_command(
    "powershell",
    &[
      "-NoProfile",
      "-Command",
      "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name | Out-String",
    ],
  ) else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

#[cfg(windows)]
fn get_gpu_name_from_shell() -> Option<String> {
    pick_best_gpu_name(gpu_names_from_shell())
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

/// Trims, drops empty and virtual/basic display adapters, and removes
/// duplicates (by normalized name — two identical cards list once), keeping
/// first-seen order.
#[cfg(windows)]
fn clean_gpu_names(raw: Vec<String>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in raw {
        let name = name.trim().to_string();
        if name.is_empty() || is_ignored_adapter_name(&name) {
            continue;
        }
        let norm = crate::lhm::normalize_gpu_name(&name);
        if !names
            .iter()
            .any(|n| crate::lhm::normalize_gpu_name(n) == norm)
        {
            names.push(name);
        }
    }
    names
}

/// Lists every physical GPU adapter name (virtual/basic display adapters
/// filtered out, duplicates removed) — the choices for the "displayed GPU"
/// selector. WMI names match the sidecar's LHM device names.
/// Prefers WMI; falls back to PowerShell `Get-CimInstance`.
pub fn detect_gpu_names() -> Vec<String> {
    #[cfg(windows)]
    {
        let wmi_names: Vec<String> = wmi::COMLibrary::new()
            .ok()
            .and_then(|com| wmi::WMIConnection::new(com).ok())
            .and_then(|conn| conn.query::<VideoControllerName>().ok())
            .map(|rows| rows.into_iter().filter_map(|r| r.name).collect())
            .unwrap_or_default();
        clean_gpu_names(if wmi_names.is_empty() {
            gpu_names_from_shell()
        } else {
            wmi_names
        })
    }

    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

// --- GPU driver detection --------------------------------------------------

/// Driver metadata for a single GPU adapter, surfaced in the Status dialog so
/// users can spot an outdated driver (a common cause of missing GPU sensors).
#[derive(Debug, Clone)]
pub struct GpuDriverInfo {
    /// Adapter name, e.g. "AMD Radeon RX 9070 XT".
    pub name: String,
    /// Driver package version (WMI `DriverVersion`), e.g. "32.0.31019.2002".
    pub version: Option<String>,
    /// Driver date formatted as "YYYY-MM-DD".
    pub date: Option<String>,
    /// Age of the driver in whole days, derived from `date`. Negative values are
    /// clamped to `None` to guard against machines with a skewed clock.
    pub age_days: Option<i64>,
}

#[cfg(windows)]
#[derive(Deserialize, Debug)]
#[serde(rename = "Win32_VideoController")]
struct VideoControllerDriver {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "DriverVersion")]
    driver_version: Option<String>,
    #[serde(rename = "DriverDate")]
    driver_date: Option<wmi::WMIDateTime>,
}

#[cfg(windows)]
#[derive(Deserialize, Debug)]
struct GpuDriverShellRow {
    name: Option<String>,
    version: Option<String>,
    date: Option<String>,
}

/// Computes the age in whole days of a driver dated `YYYY-MM-DD`.
/// Returns `None` for unparseable dates or future dates (clock skew).
#[cfg(windows)]
fn driver_age_days(date: &str) -> Option<i64> {
    let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let today = chrono::Local::now().date_naive();
    let days = (today - d).num_days();
    if days >= 0 {
        Some(days)
    } else {
        None
    }
}

#[cfg(windows)]
fn gpu_drivers_from_shell() -> Vec<GpuDriverInfo> {
    let output = match run_hidden_command(
    "powershell",
    &[
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      "@(Get-CimInstance Win32_VideoController | ForEach-Object { @{ name = $_.Name; version = $_.DriverVersion; date = $(if ($_.DriverDate) { $_.DriverDate.ToString('yyyy-MM-dd') }) } }) | ConvertTo-Json -Compress",
    ],
  ) {
    Ok(o) if o.status.success() => o,
    _ => return Vec::new(),
  };

    let text = String::from_utf8_lossy(&output.stdout);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // ConvertTo-Json emits a bare object for a single adapter and an array for many.
    let rows: Vec<GpuDriverShellRow> = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(serde_json::Value::Array(_)) => serde_json::from_str(trimmed).unwrap_or_default(),
        Ok(obj @ serde_json::Value::Object(_)) => serde_json::from_value(obj)
            .map(|r| vec![r])
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    rows.into_iter()
        .filter_map(|r| {
            let name = r.name?;
            if is_ignored_adapter_name(&name) {
                return None;
            }
            let age_days = r.date.as_deref().and_then(driver_age_days);
            Some(GpuDriverInfo {
                name,
                version: r.version,
                date: r.date,
                age_days,
            })
        })
        .collect()
}

/// Detects driver version and date for every real GPU adapter.
/// Prefers WMI; falls back to PowerShell `Get-CimInstance`. Returns an empty
/// vector when no adapter could be read or on non-Windows targets.
pub fn detect_gpu_drivers() -> Vec<GpuDriverInfo> {
    #[cfg(windows)]
    {
        if let Ok(com) = wmi::COMLibrary::new() {
            if let Ok(conn) = wmi::WMIConnection::new(com) {
                if let Ok(rows) = conn.query::<VideoControllerDriver>() {
                    let list: Vec<GpuDriverInfo> = rows
                        .into_iter()
                        .filter_map(|r| {
                            let name = r.name?;
                            if is_ignored_adapter_name(&name) {
                                return None;
                            }
                            let date = r.driver_date.map(|d| d.0.format("%Y-%m-%d").to_string());
                            let age_days = date.as_deref().and_then(driver_age_days);
                            Some(GpuDriverInfo {
                                name,
                                version: r.driver_version,
                                date,
                                age_days,
                            })
                        })
                        .collect();
                    if !list.is_empty() {
                        return list;
                    }
                }
            }
        }

        gpu_drivers_from_shell()
    }

    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Maps OEM/product strings to a canonical brand slug used for logo selection.
#[cfg(windows)]
pub fn classify_system_brand(fields: &[&str]) -> &'static str {
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
    // Reject bare version numbers like "1.05", "2.0", "10.1" — these are
    // firmware/BIOS version strings, not meaningful model names (e.g. Razer Blade).
    if trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
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
pub fn is_placeholder_model_name(name: &str) -> bool {
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
        18 => Some("DDR"),     // MemoryType=18 (WMI), SMBIOSMemoryType overlaps are rare
        20 => Some("DDR2"),    // MemoryType=20 (WMI DDR2 FB-DIMM, close enough)
        24 => Some("DDR3"),    // MemoryType=24 / SMBIOSMemoryType=24
        26 => Some("DDR4"),    // MemoryType=26 / SMBIOSMemoryType=26
        27 => Some("LPDDR"),   // SMBIOSMemoryType=27
        28 => Some("LPDDR2"),  // SMBIOSMemoryType=28
        29 => Some("LPDDR3"),  // SMBIOSMemoryType=29
        30 => Some("LPDDR4"),  // SMBIOSMemoryType=30
        34 => Some("DDR5"),    // MemoryType=34 / SMBIOSMemoryType=34
        35 => Some("LPDDR5"),  // SMBIOSMemoryType=35
        36 => Some("LPDDR5X"), // SMBIOSMemoryType=36 (matches the PowerShell fallback)
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
        "$m = Get-CimInstance Win32_PhysicalMemory; if(-not $m){ return }; $dimms = $m.Count; $speed = ($m | ForEach-Object { if($_.ConfiguredClockSpeed){ $_.ConfiguredClockSpeed } else { $_.Speed } } | Measure-Object -Maximum).Maximum; $typeCode = ($m | Select-Object -First 1 -ExpandProperty SMBIOSMemoryType); if(-not $typeCode){ $typeCode = ($m | Select-Object -First 1 -ExpandProperty MemoryType) }; $type = switch([int]$typeCode){ 18 {'DDR'} 20 {'DDR2'} 24 {'DDR3'} 26 {'DDR4'} 27 {'LPDDR'} 28 {'LPDDR2'} 29 {'LPDDR3'} 30 {'LPDDR4'} 34 {'DDR5'} 35 {'LPDDR5'} 36 {'LPDDR5X'} default {''} }; $r = if($type -and $speed){ \"$type $speed MT/s ($dimms DIMMs)\" } elseif($type){ \"$type ($dimms DIMMs)\" } elseif($speed){ \"$speed MT/s ($dimms DIMMs)\" } else { \"RAM ($dimms DIMMs)\" }; $r",
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

#[cfg(windows)]
#[derive(Deserialize, Debug)]
struct NetAdapterGatewayRow {
    #[serde(rename = "DefaultIPGateway")]
    default_ip_gateway: Option<Vec<String>>,
}

/// First IPv4 default gateway across adapters, in adapter order (gateway
/// lists can also hold IPv6 addresses, which the ping sampler doesn't use).
fn first_ipv4_gateway<'a>(gateways: impl IntoIterator<Item = &'a str>) -> Option<String> {
    gateways
        .into_iter()
        .map(str::trim)
        .find(|g| g.parse::<std::net::Ipv4Addr>().is_ok())
        .map(str::to_string)
}

/// Detects the default network gateway to use as the ping target.
/// Prefers WMI; falls back to PowerShell `Get-CimInstance`, then `1.1.1.1`.
/// Blocking (WMI/process) — call via `spawn_blocking` from async code.
pub fn detect_ping_target() -> String {
    #[cfg(windows)]
    {
        let wmi_rows: Option<Vec<NetAdapterGatewayRow>> = wmi::COMLibrary::new()
            .ok()
            .and_then(|com| wmi::WMIConnection::new(com).ok())
            .and_then(|conn| {
                conn.raw_query(
                    "SELECT DefaultIPGateway FROM Win32_NetworkAdapterConfiguration \
                     WHERE IPEnabled = TRUE",
                )
                .ok()
            });
        // WMI answered: trust it, even with no gateway (offline) — the
        // PowerShell fallback would only find the same nothing, slower.
        if let Some(rows) = wmi_rows {
            return first_ipv4_gateway(
                rows.iter()
                    .filter_map(|r| r.default_ip_gateway.as_ref())
                    .flatten()
                    .map(String::as_str),
            )
            .unwrap_or_else(|| "1.1.1.1".to_string());
        }

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

pub fn sample_ping_ms(target: &str) -> Option<f64> {
    use std::io::ErrorKind;
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::{Duration, Instant};

    let addr = format!("{target}:80").to_socket_addrs().ok()?.next()?;

    let start = Instant::now();
    let result = TcpStream::connect_timeout(&addr, Duration::from_millis(500));
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    match result {
        Ok(_) => Some(elapsed),
        Err(e) if e.kind() == ErrorKind::ConnectionRefused => Some(elapsed),
        _ => None,
    }
}

// --- Battery detection -----------------------------------------------------

#[cfg(windows)]
#[derive(serde::Deserialize, Debug)]
struct Win32BatteryRow {
    #[serde(rename = "EstimatedChargeRemaining")]
    charge_remaining: Option<u16>,
    #[serde(rename = "BatteryStatus")]
    battery_status: Option<u16>,
    #[serde(rename = "EstimatedRunTime")]
    estimated_run_time: Option<u32>,
}

#[cfg(windows)]
#[derive(serde::Deserialize, Debug)]
struct WmiBatteryStatusRow {
    /// Charge rate in mW (valid when charging; 0 when discharging).
    #[serde(rename = "ChargeRate")]
    charge_rate: Option<u32>,
    /// Discharge rate in mW (valid when discharging; 0 when charging).
    #[serde(rename = "DischargeRate")]
    discharge_rate: Option<u32>,
}

/// Whether the system has a battery (gates the Battery panel/overlay metric
/// in Settings). Unlike [`sample_battery_wmi`], a WMI *failure* returns `true`
/// — erring toward leaving the option enabled rather than hiding it on a
/// laptop — and only a successful, empty `Win32_Battery` query means "no".
/// Blocking (WMI) — call off the UI thread.
pub fn detect_battery_present() -> bool {
    #[cfg(windows)]
    {
        #[derive(Deserialize)]
        struct BatteryIdRow {
            #[serde(rename = "DeviceID")]
            _device_id: Option<String>,
        }
        wmi::COMLibrary::new()
            .ok()
            .and_then(|com| wmi::WMIConnection::new(com).ok())
            .and_then(|conn| {
                conn.raw_query::<BatteryIdRow>("SELECT DeviceID FROM Win32_Battery")
                    .ok()
            })
            .map_or(true, |rows| !rows.is_empty())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Queries the first battery detected by WMI.
///
/// Returns `(charge_pct, is_charging, time_remaining_mins, power_w)` or `None`
/// when no battery is present (desktop systems, WMI failure, or empty result set).
/// `power_w` is positive watts — charge rate when on AC, discharge rate when on battery.
pub fn sample_battery_wmi() -> Option<(u8, bool, Option<u32>, Option<f64>)> {
    #[cfg(windows)]
    {
        let com = wmi::COMLibrary::new().ok()?;
        let conn = wmi::WMIConnection::new(com).ok()?;
        let rows: Vec<Win32BatteryRow> = conn
      .raw_query("SELECT EstimatedChargeRemaining, BatteryStatus, EstimatedRunTime FROM Win32_Battery")
      .ok()?;
        let row = rows.into_iter().next()?;
        let charge_pct = row.charge_remaining.map(|v| v.min(100) as u8).unwrap_or(0);
        // BatteryStatus values: 1=discharging, 2=AC/no discharge, 3=fully charged,
        //                       6=charging, 7=charging+high, 8=charging+low, 9=charging+critical
        let status = row.battery_status.unwrap_or(1);
        let is_charging = matches!(status, 2 | 3 | 6 | 7 | 8 | 9);
        // EstimatedRunTime of 71582788 (0x44AAAAA4) means "unknown / on AC".
        let time_remaining_mins = row.estimated_run_time.filter(|&t| t > 0 && t < 71_582_788);

        // root\wmi BatteryStatus gives ChargeRate / DischargeRate in mW.
        let power_w = (|| -> Option<f64> {
            let wmi_conn = wmi::WMIConnection::with_namespace_path("ROOT\\WMI", com).ok()?;
            let status_rows: Vec<WmiBatteryStatusRow> = wmi_conn
                .raw_query("SELECT ChargeRate, DischargeRate FROM BatteryStatus")
                .ok()?;
            let sr = status_rows.into_iter().next()?;
            let mw = if is_charging {
                sr.charge_rate.filter(|&v| v > 0)?
            } else {
                sr.discharge_rate.filter(|&v| v > 0)?
            };
            Some(mw as f64 / 1000.0)
        })();

        Some((charge_pct, is_charging, time_remaining_mins, power_w))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

// --- Tests -----------------------------------------------------------------

#[cfg(test)]
mod cross_platform_tests {
    use super::{is_placeholder_model_name, normalize_model_name};

    #[test]
    fn normalize_model_name_accepts_real_names() {
        assert_eq!(
            normalize_model_name("ROG GM700TZ"),
            Some("ROG GM700TZ".to_string())
        );
        assert_eq!(
            normalize_model_name("PRIME B650M-A AX6 II"),
            Some("PRIME B650M-A AX6 II".to_string())
        );
    }

    #[test]
    fn normalize_model_name_trims_whitespace() {
        assert_eq!(
            normalize_model_name("  ROG GM700TZ  "),
            Some("ROG GM700TZ".to_string())
        );
    }

    #[test]
    fn normalize_model_name_rejects_empty_and_whitespace() {
        assert_eq!(normalize_model_name(""), None);
        assert_eq!(normalize_model_name("   "), None);
    }

    #[test]
    fn normalize_model_name_rejects_bare_version_numbers() {
        assert_eq!(normalize_model_name("1.05"), None);
        assert_eq!(normalize_model_name("2.0"), None);
        assert_eq!(normalize_model_name("10.1"), None);
        assert_eq!(normalize_model_name("1.2.3"), None);
        // Real model names that contain digits must not be rejected
        assert!(normalize_model_name("Blade 15").is_some());
        assert!(normalize_model_name("ROG GM700TZ").is_some());
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
            assert_eq!(
                normalize_model_name(p),
                None,
                "expected None for placeholder: {p}"
            );
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
        assert_eq!(
            normalize_manufacturer("Micro-Star International Co., Ltd."),
            "MSI"
        );
        assert_eq!(normalize_manufacturer("Micro Star International"), "MSI");
        assert_eq!(normalize_manufacturer("MSI"), "MSI");
    }

    #[test]
    fn normalize_manufacturer_maps_gigabyte() {
        assert_eq!(
            normalize_manufacturer("Gigabyte Technology Co., Ltd."),
            "Gigabyte"
        );
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

    let drives: Vec<DiskDriveRow> = conn
        .raw_query("SELECT DeviceID, Model FROM Win32_DiskDrive")
        .ok()?;
    let disk_id_to_model: std::collections::HashMap<String, String> = drives
        .into_iter()
        .filter_map(|r| Some((r.device_id?.trim().to_string(), r.model?.trim().to_string())))
        .collect();

    let disk_to_part: Vec<DiskToPartRow> = conn
        .raw_query("SELECT Antecedent, Dependent FROM Win32_DiskDriveToDiskPartition")
        .ok()?;
    // partition_id → disk_model
    // Use .get().cloned() (not .remove()) so drives with multiple partitions
    // (e.g. EFI + Recovery + C:) all get the model, not just the first partition.
    let mut part_to_model: std::collections::HashMap<String, String> = disk_to_part
        .into_iter()
        .filter_map(|r| {
            let disk_path = r.antecedent?;
            let part_path = r.dependent?;
            let disk_id = extract_device_id(&disk_path)?;
            let part_id = extract_device_id(&part_path)?;
            let model = disk_id_to_model.get(&disk_id)?.clone();
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

/// Returns a map from physical disk model name to `DiskKind` (NVMe / SSD / HDD).
/// Keyed by the same model string that `detect_disk_model_map` stores as values,
/// so callers can join the two maps: drive-letter → model → kind.
pub fn detect_disk_type_map() -> std::collections::HashMap<String, DiskKind> {
    #[cfg(windows)]
    {
        if let Some(map) = try_disk_type_map_via_wmi() {
            if !map.is_empty() {
                return map;
            }
        }
        try_disk_type_map_via_shell().unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        std::collections::HashMap::new()
    }
}

#[cfg(windows)]
fn classify_disk(bus_type: Option<u16>, media_type: Option<u16>) -> DiskKind {
    match bus_type {
        Some(17) => DiskKind::NVMe, // MSFT_PhysicalDisk BusType 17 = NVMe
        _ => match media_type {
            Some(4) => DiskKind::Ssd, // MediaType 4 = SSD
            Some(3) => DiskKind::Hdd, // MediaType 3 = HDD
            _ => DiskKind::Unknown,
        },
    }
}

#[cfg(windows)]
fn try_disk_type_map_via_wmi() -> Option<std::collections::HashMap<String, DiskKind>> {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct PhysicalDiskRow {
        #[serde(rename = "FriendlyName")]
        friendly_name: Option<String>,
        #[serde(rename = "MediaType")]
        media_type: Option<u16>,
        #[serde(rename = "BusType")]
        bus_type: Option<u16>,
    }

    let com = wmi::COMLibrary::new().ok()?;
    let conn =
        wmi::WMIConnection::with_namespace_path("ROOT\\microsoft\\windows\\storage", com).ok()?;
    let disks: Vec<PhysicalDiskRow> = conn
        .raw_query("SELECT FriendlyName, MediaType, BusType FROM MSFT_PhysicalDisk")
        .ok()?;

    Some(
        disks
            .into_iter()
            .filter_map(|d| {
                Some((
                    d.friendly_name?.trim().to_string(),
                    classify_disk(d.bus_type, d.media_type),
                ))
            })
            .collect(),
    )
}

#[cfg(windows)]
fn try_disk_type_map_via_shell() -> Option<std::collections::HashMap<String, DiskKind>> {
    #[derive(serde::Deserialize)]
    struct Entry {
        #[serde(rename = "FriendlyName")]
        name: Option<String>,
        #[serde(rename = "MediaType")]
        media_type: Option<String>,
        #[serde(rename = "BusType")]
        bus_type: Option<String>,
    }

    let output = run_hidden_command(
    "powershell",
    &[
      "-NoProfile",
      "-Command",
      "@(Get-PhysicalDisk|Select-Object FriendlyName,MediaType,BusType)|ConvertTo-Json -Compress",
    ],
  )
  .ok()?;

    if !output.status.success() {
        return None;
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
            .filter_map(|e| {
                let name = e.name?.trim().to_string();
                let kind = match (e.bus_type.as_deref(), e.media_type.as_deref()) {
                    (Some("NVMe"), _) => DiskKind::NVMe,
                    (_, Some("SSD")) => DiskKind::Ssd,
                    (_, Some("HDD")) => DiskKind::Hdd,
                    _ => DiskKind::Unknown,
                };
                Some((name, kind))
            })
            .collect(),
    )
}

#[cfg(all(test, windows))]
mod tests {
    use super::{
        classify_system_brand, clean_gpu_names, driver_age_days, first_ipv4_gateway,
        gpu_name_score, map_memory_type, pick_best_gpu_name,
    };

    // map_memory_type

    #[test]
    fn driver_age_days_past_date_is_positive() {
        // A date well in the past must yield a large positive age.
        let age = driver_age_days("2020-01-01").expect("past date should parse");
        assert!(age > 1000, "expected age > 1000 days, got {age}");
    }

    #[test]
    fn driver_age_days_future_date_is_none() {
        // Clock-skew guard: future dates return None rather than a negative age.
        assert_eq!(driver_age_days("2999-12-31"), None);
    }

    #[test]
    fn driver_age_days_unparseable_is_none() {
        assert_eq!(driver_age_days("not-a-date"), None);
        assert_eq!(driver_age_days(""), None);
        assert_eq!(driver_age_days("06/16/2026"), None);
    }

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
        assert_eq!(map_memory_type(36), Some("LPDDR5X"));
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
        assert_eq!(
            classify_system_brand(&["LENOVO", "Legion T7 34IRZ8"]),
            "legion"
        );
        assert_eq!(
            classify_system_brand(&["HP", "OMEN 45L Desktop GT22"]),
            "omen"
        );
        assert_eq!(
            classify_system_brand(&["Acer", "Predator Orion 7000"]),
            "predator"
        );
        assert_eq!(
            classify_system_brand(&["Gigabyte Technology Co., Ltd.", "AORUS MODEL X"]),
            "aorus"
        );
    }

    #[test]
    fn classify_system_brand_recognizes_oem_brands() {
        assert_eq!(
            classify_system_brand(&["Micro-Star International Co., Ltd"]),
            "msi"
        );
        assert_eq!(
            classify_system_brand(&["Gigabyte Technology Co., Ltd."]),
            "gigabyte"
        );
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
        assert!(
            gpu_name_score("NVIDIA GeForce RTX 4090") > gpu_name_score("Intel UHD Graphics 770")
        );
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
        assert_eq!(
            pick_best_gpu_name(names),
            Some("NVIDIA GeForce RTX 4090".to_string())
        );
    }

    #[test]
    fn pick_best_gpu_name_skips_empty_strings() {
        let names = vec![
            "".to_string(),
            "  ".to_string(),
            "AMD Radeon RX 7900 XTX".to_string(),
        ];
        assert_eq!(
            pick_best_gpu_name(names),
            Some("AMD Radeon RX 7900 XTX".to_string())
        );
    }

    #[test]
    fn pick_best_gpu_name_returns_none_for_empty_list() {
        let names: Vec<String> = vec![];
        assert_eq!(pick_best_gpu_name(names), None);
    }

    #[test]
    fn clean_gpu_names_keeps_physical_adapters_in_order() {
        let raw = vec![
            "AMD Radeon(TM) 890M Graphics".to_string(),
            "NVIDIA GeForce RTX 5070 Ti Laptop GPU".to_string(),
        ];
        assert_eq!(clean_gpu_names(raw.clone()), raw);
    }

    #[test]
    fn clean_gpu_names_drops_virtual_empty_and_duplicates() {
        let raw = vec![
            "  ".to_string(),
            "Microsoft Basic Display Adapter".to_string(),
            " NVIDIA GeForce RTX 4090 ".to_string(),
            "Parsec Virtual Display Adapter".to_string(),
            "NVIDIA GeForce RTX 4090".to_string(),
            "nvidia geforce rtx(tm) 4090".to_string(),
            "Intel(R) UHD Graphics 770".to_string(),
        ];
        assert_eq!(
            clean_gpu_names(raw),
            vec![
                "NVIDIA GeForce RTX 4090".to_string(),
                "Intel(R) UHD Graphics 770".to_string(),
            ]
        );
    }

    #[test]
    fn clean_gpu_names_empty_input() {
        assert!(clean_gpu_names(Vec::new()).is_empty());
    }

    #[test]
    fn first_ipv4_gateway_skips_ipv6_and_keeps_adapter_order() {
        let gws = ["fe80::1", "192.168.1.1", "10.0.0.1"];
        assert_eq!(first_ipv4_gateway(gws), Some("192.168.1.1".to_string()));
    }

    #[test]
    fn first_ipv4_gateway_trims_and_rejects_garbage() {
        assert_eq!(
            first_ipv4_gateway([" 10.0.0.138 "]),
            Some("10.0.0.138".to_string())
        );
        assert_eq!(first_ipv4_gateway(["", "not-an-ip", "999.1.1.1"]), None);
    }

    #[test]
    fn first_ipv4_gateway_none_when_offline() {
        assert_eq!(first_ipv4_gateway(std::iter::empty::<&str>()), None);
    }
}

/// Every struct used with a typed `conn.query::<T>()` must query a real
/// `Win32_*` class — see the note above the WMI row structs (#198).
/// `wmi::build_query` is pure (no COM), so this runs anywhere tests do.
#[cfg(all(test, windows))]
mod wmi_classes_tests {
    use super::{
        BaseBoardInfo, ComputerSystem, ComputerSystemProduct, PhysicalMemory,
        VideoControllerDriver, VideoControllerName,
    };

    fn from_class<'de, T: serde::Deserialize<'de>>() -> String {
        let query = wmi::build_query::<T>(None).expect("query must build");
        query
            .split(" FROM ")
            .nth(1)
            .expect("query must have a FROM clause")
            .trim()
            .to_string()
    }

    #[test]
    fn typed_wmi_structs_query_real_win32_classes() {
        assert_eq!(from_class::<VideoControllerName>(), "Win32_VideoController");
        assert_eq!(
            from_class::<VideoControllerDriver>(),
            "Win32_VideoController"
        );
        assert_eq!(from_class::<ComputerSystem>(), "Win32_ComputerSystem");
        assert_eq!(
            from_class::<ComputerSystemProduct>(),
            "Win32_ComputerSystemProduct"
        );
        assert_eq!(from_class::<BaseBoardInfo>(), "Win32_BaseBoard");
        assert_eq!(from_class::<PhysicalMemory>(), "Win32_PhysicalMemory");
    }

    #[test]
    fn base_board_still_parses_powershell_json() {
        // `detect_motherboard_name`'s PowerShell fallback deserializes the
        // same struct from JSON; the container rename must not break that.
        let b: BaseBoardInfo =
            serde_json::from_str(r#"{"Manufacturer":"ASUSTeK COMPUTER INC.","Product":"GA403WR"}"#)
                .expect("JSON must deserialize");
        assert_eq!(b.manufacturer.as_deref(), Some("ASUSTeK COMPUTER INC."));
        assert_eq!(b.product.as_deref(), Some("GA403WR"));
    }
}
