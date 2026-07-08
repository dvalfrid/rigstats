use crate::lock_ext::LockSafe;
use crate::theme::{self, DialogColors};
use chrono::Local;
use rigstats_backend::{debug, hardware};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use sysinfo::System;

// ── Dependency metadata (compile-time constants from Cargo.toml) ──────────────

const DEP_LHM_VER: &str = "0.9.6";
const DEP_SYSINFO_VER: &str = "0.30";
const DEP_WMI_VER: &str = "0.13";
const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct StatusState {
    pub log: String,
    pub service_running: bool,
    pub pipe_connected: bool,
    /// True when the app is in Desktop Wallpaper mode: the main app pauses its
    /// poll loop and releases the sensor pipe so the wallpaper host owns it, so
    /// `pipe_connected` is legitimately false here. The Pipe row reflects that
    /// the host owns the pipe instead of showing a misleading red "Disconnected".
    pub wallpaper_active: bool,
    pub wmi_ok: bool,
    pub log_path: String,
    pub last_refresh: String,
    pub gpu_drivers: Vec<hardware::GpuDriverInfo>,
}

impl StatusState {
    pub fn load(dir: &std::path::Path, pipe_connected: bool, wallpaper_active: bool) -> Self {
        let log_path = dir.join("rigstats-debug.log");
        let log = std::fs::read_to_string(&log_path)
            .unwrap_or_else(|_| "(log file not found)".to_string());
        Self {
            log,
            service_running: query_service_running(),
            pipe_connected,
            wallpaper_active,
            wmi_ok: hardware::probe_wmi_status().is_ok(),
            log_path: log_path.display().to_string(),
            last_refresh: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            gpu_drivers: hardware::detect_gpu_drivers(),
        }
    }

    /// Cheap, non-blocking initial state shown until the first background
    /// `load()` completes. `load()` spawns subprocesses (`sc.exe`) and runs
    /// WMI/COM queries that can block for seconds, so it must never run on the
    /// egui UI thread.
    pub fn placeholder() -> Self {
        Self {
            log: "(loading…)".to_string(),
            service_running: false,
            pipe_connected: false,
            wallpaper_active: false,
            wmi_ok: false,
            log_path: String::new(),
            last_refresh: String::new(),
            gpu_drivers: Vec::new(),
        }
    }
}

/// Run `StatusState::load` on a background thread so the UI thread never blocks
/// on `sc.exe` / WMI / PowerShell. Writes the result into `state` and requests a
/// repaint when done. No-op if a load is already in flight.
pub fn spawn_load(
    state: Arc<Mutex<StatusState>>,
    refreshing: Arc<AtomicBool>,
    dir: PathBuf,
    pipe_connected: bool,
    wallpaper_active: bool,
    ctx: egui::Context,
) {
    if refreshing.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let loaded = StatusState::load(&dir, pipe_connected, wallpaper_active);
        *state.lock_safe() = loaded;
        refreshing.store(false, Ordering::Relaxed);
        ctx.request_repaint();
    });
}

fn query_service_running() -> bool {
    rigstats_backend::debug::run_hidden_command("sc.exe", &["query", "rigstats-sensor"])
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("RUNNING"))
        .unwrap_or(false)
}

// Semantic status colours — independent of light/dark mode.
const C_GOOD: egui::Color32 = egui::Color32::from_rgb(80, 190, 90);
const C_BAD: egui::Color32 = egui::Color32::from_rgb(200, 70, 60);
const C_WARN: egui::Color32 = egui::Color32::from_rgb(220, 170, 60);

// A GPU driver older than this is flagged as potentially outdated. Outdated AMD
// drivers are a known cause of missing GPU sensors (see docs/troubleshooting.md).
const DRIVER_STALE_DAYS: i64 = 270;

/// Returns a short freshness label and colour for a driver of the given age.
fn driver_age_label(age_days: i64) -> (String, egui::Color32) {
    let months = age_days / 30;
    if age_days >= DRIVER_STALE_DAYS {
        (format!("⚠ {months} mo old"), C_WARN)
    } else if months >= 1 {
        (format!("{months} mo old"), C_GOOD)
    } else {
        ("Up to date".to_string(), C_GOOD)
    }
}

/// Maps a GPU adapter name to its vendor's driver download page.
/// Returns `(vendor_label, url)` or `None` for unknown vendors.
fn gpu_driver_support(name: &str) -> Option<(&'static str, &'static str)> {
    let n = name.to_ascii_lowercase();
    if n.contains("nvidia") || n.contains("geforce") || n.contains("rtx") || n.contains("quadro") {
        Some(("NVIDIA", "https://www.nvidia.com/Download/index.aspx"))
    } else if n.contains("radeon") || n.contains("amd") || n.contains("ryzen") {
        Some(("AMD", "https://www.amd.com/en/support"))
    } else if n.contains("intel") || n.contains("arc") {
        Some((
            "Intel",
            "https://www.intel.com/content/www/us/en/download-center/home.html",
        ))
    } else {
        None
    }
}

// ── Widget helpers ────────────────────────────────────────────────────────────

fn card_frame(dc: &DialogColors) -> egui::Frame {
    egui::Frame::new()
        .fill(dc.card)
        .stroke(egui::Stroke::new(1.0, dc.card_border))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(12, 10))
}

fn section_label(ui: &mut egui::Ui, dc: &DialogColors, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(11.0)
            .strong()
            .color(dc.label),
    );
    ui.add_space(4.0);
}

fn meta_row(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    label: &str,
    value: &str,
    value_color: egui::Color32,
) {
    ui.label(egui::RichText::new(label).size(11.0).color(dc.muted));
    ui.label(egui::RichText::new(value).size(13.0).color(value_color));
}

fn status_badge(ui: &mut egui::Ui, ok: bool) {
    let (text, fill, text_color) = if ok {
        (
            "SUCCESS",
            egui::Color32::from_rgb(28, 100, 42),
            egui::Color32::WHITE,
        )
    } else {
        (
            "FAILED",
            egui::Color32::from_rgb(120, 30, 30),
            egui::Color32::WHITE,
        )
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin {
            left: 6,
            right: 6,
            top: 2,
            bottom: 2,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(10.0)
                    .strong()
                    .color(text_color),
            );
        });
}

// ── Section renderers ─────────────────────────────────────────────────────────

fn render_diagnostics(ui: &mut egui::Ui, dc: &DialogColors, state: &StatusState) {
    section_label(ui, dc, "Diagnostics");
    card_frame(dc).show(ui, |ui| {
        ui.set_width(ui.available_width());

        // Service + Pipe side by side
        egui::Grid::new("diag_status_grid")
            .num_columns(2)
            .min_col_width(180.0)
            .spacing([8.0, 2.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Service").size(11.0).color(dc.muted));
                ui.label(egui::RichText::new("Pipe").size(11.0).color(dc.muted));
                ui.end_row();
                let svc_color = if state.service_running { C_GOOD } else { C_BAD };
                let svc_text = if state.service_running {
                    "RUNNING"
                } else {
                    "STOPPED"
                };
                ui.label(
                    egui::RichText::new(svc_text)
                        .size(13.0)
                        .strong()
                        .color(svc_color),
                );
                let (pipe_text, pipe_color) = if state.wallpaper_active {
                    // Desktop Wallpaper mode: the main app released the pipe to the
                    // wallpaper host on purpose — not an error.
                    ("Wallpaper host", C_GOOD)
                } else if state.pipe_connected {
                    ("Connected", C_GOOD)
                } else {
                    ("Disconnected", C_BAD)
                };
                ui.label(
                    egui::RichText::new(pipe_text)
                        .size(13.0)
                        .strong()
                        .color(pipe_color),
                );
                ui.end_row();
            });

        ui.add_space(6.0);
        meta_row(ui, dc, "Debug Log Path", &state.log_path, dc.text);
        ui.add_space(6.0);
        meta_row(
            ui,
            dc,
            "Last Successful Refresh",
            &state.last_refresh,
            dc.text,
        );
    });
}

fn render_components(ui: &mut egui::Ui, dc: &DialogColors, state: &StatusState) {
    ui.add_space(8.0);
    ui.columns(2, |cols| {
        section_label(&mut cols[0], dc, "Dependencies");
        let before = cols[0].min_rect().height();
        dependencies_card(&mut cols[0], dc, state);
        let dep_card_h = cols[0].min_rect().height() - before;

        section_label(&mut cols[1], dc, "GPU Drivers");
        drivers_card(&mut cols[1], dc, state, dep_card_h);
    });
}

fn dependencies_card(ui: &mut egui::Ui, dc: &DialogColors, state: &StatusState) {
    card_frame(dc).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let sensor_ver = format!("LHM {DEP_LHM_VER}");
        let deps: [(&str, &str, &str, bool); 3] = [
            (
                "rigstats-sensor",
                "Hardware sensor feed\n(Windows Service)",
                sensor_ver.as_str(),
                state.service_running,
            ),
            (
                "sysinfo",
                "CPU, RAM, disk, network stats",
                DEP_SYSINFO_VER,
                true,
            ),
            (
                "wmi",
                "Windows hardware metadata",
                DEP_WMI_VER,
                state.wmi_ok,
            ),
        ];
        for (i, (name, desc, ver, ok)) in deps.iter().enumerate() {
            if i > 0 {
                ui.add(egui::Separator::default().spacing(6.0));
            }
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(*name)
                            .size(13.0)
                            .strong()
                            .color(dc.text),
                    );
                    ui.label(egui::RichText::new(*desc).size(11.0).color(dc.muted));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_badge(ui, *ok);
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(*ver).size(12.0).color(dc.muted));
                });
            });
        }
    });
}

fn drivers_card(ui: &mut egui::Ui, dc: &DialogColors, state: &StatusState, target_h: f32) {
    // Inner height available for content = target minus the frame's vertical
    // chrome (inner_margin 10+10 plus the 1 px stroke top/bottom).
    let inner_h = (target_h - 22.0).max(0.0);
    card_frame(dc).show(ui, |ui| {
        ui.set_width(ui.available_width());

        if state.gpu_drivers.is_empty() {
            ui.set_min_height(inner_h);
            ui.label(
                egui::RichText::new("No GPU driver information available.")
                    .size(11.0)
                    .color(dc.muted),
            );
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt("gpu_drivers_scroll")
            .max_height(inner_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_height(inner_h);
                for (i, d) in state.gpu_drivers.iter().enumerate() {
                    if i > 0 {
                        ui.add(egui::Separator::default().spacing(6.0));
                    }
                    ui.label(
                        egui::RichText::new(&d.name)
                            .size(13.0)
                            .strong()
                            .color(dc.text),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "Driver {}",
                            d.version.as_deref().unwrap_or("unknown")
                        ))
                        .size(11.0)
                        .color(dc.muted),
                    );
                    ui.horizontal(|ui| {
                        if let Some(date) = &d.date {
                            ui.label(egui::RichText::new(date).size(11.0).color(dc.muted));
                        }
                        if let Some(age) = d.age_days {
                            let (txt, col) = driver_age_label(age);
                            ui.label(egui::RichText::new(txt).size(11.0).strong().color(col));
                        }
                        if let Some((_, url)) = gpu_driver_support(&d.name) {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add(egui::Hyperlink::from_label_and_url(
                                        egui::RichText::new("↗ Latest driver")
                                            .size(11.0)
                                            .color(dc.link),
                                        url,
                                    ));
                                },
                            );
                        }
                    });
                }
            });
    });
}

/// Run a PowerShell snippet and return stdout as a String.
fn run_ps_capture(script: &str) -> String {
    match rigstats_backend::debug::run_hidden_command(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    ) {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(e) => format!("(error: {e})"),
    }
}

fn collect_and_open_diagnostics_impl(dir: &Path) -> std::io::Result<PathBuf> {
    use std::io::Write as IoWrite;
    use zip::write::SimpleFileOptions;

    let ts = rigstats_backend::debug::unix_now_secs();
    let default_name = format!("rigstats-diag-{ts}.zip");

    // Show native save-file dialog.
    let out_path = rfd::FileDialog::new()
        .set_file_name(&default_name)
        .add_filter("ZIP Archive", &["zip"])
        .save_file()
        .ok_or_else(|| std::io::Error::other("cancelled"))?;

    let manifest =
        format!("{{\n  \"collected_at_unix\": {ts},\n  \"rigstats_version\": \"{VERSION}\"\n}}");

    let debug_log = std::fs::read(dir.join("rigstats-debug.log"))
        .unwrap_or_else(|_| b"(log file not found)".to_vec());

    let debug_log_prev = std::fs::read(dir.join("rigstats-debug-prev.log"))
        .unwrap_or_else(|_| b"(no previous session log)".to_vec());

    let settings_json = std::fs::read_to_string(dir.join("rigstats-settings.json"))
        .map(|s| {
            serde_json::from_str::<serde_json::Value>(&s)
                .and_then(|v| serde_json::to_string_pretty(&v))
                .unwrap_or(s)
        })
        .unwrap_or_else(|_| "(settings file not found)".to_string());

    let dashboard_profile = std::fs::read_to_string(dir.join("rigstats-settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("dashboardProfile")
                .and_then(|p| p.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();

    let displays_json = {
        let monitors = crate::geometry::win_monitor::list();
        let selected_rect = (!dashboard_profile.is_empty())
            .then(|| crate::geometry::pick_window_rect_for_profile(&dashboard_profile));

        let monitor_entries: Vec<serde_json::Value> = monitors
            .iter()
            .map(|&(l, t, r, b)| {
                let is_selected = selected_rect.is_some_and(|[x, y, w, h]| {
                    x.round() as i32 == l
                        && y.round() as i32 == t
                        && w.round() as i32 == r - l
                        && h.round() as i32 == b - t
                });
                serde_json::json!({
                    "left": l,
                    "top": t,
                    "right": r,
                    "bottom": b,
                    "width": r - l,
                    "height": b - t,
                    "is_primary": l == 0 && t == 0,
                    "is_selected": is_selected,
                })
            })
            .collect();

        serde_json::to_string_pretty(&serde_json::json!({
            "dashboard_profile": dashboard_profile,
            "monitors": monitor_entries,
        }))
        .unwrap_or_else(|_| "(failed to enumerate displays)".to_string())
    };

    let sidecar_log = std::fs::read(
        PathBuf::from(std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into()))
            .join("se.codeby.rigstats")
            .join("rigstats-sensor.log"),
    )
    .unwrap_or_else(|_| b"(sidecar log not found)".to_vec());

    let sensor_tree = std::fs::read(
        PathBuf::from(std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into()))
            .join("se.codeby.rigstats")
            .join("sensor-tree.txt"),
    )
    .unwrap_or_else(|_| b"(sensor-tree.txt not found)".to_vec());

    let install_log = std::fs::read(
        PathBuf::from(std::env::var_os("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".into()))
            .join("se.codeby.rigstats")
            .join("rigstats-install.log"),
    )
    .unwrap_or_else(|_| b"(install log not found)".to_vec());

    let hardware_json = run_ps_capture(concat!(
        "try{",
        "$os=Get-CimInstance Win32_OperatingSystem -EA Stop;",
        "$cpu=Get-CimInstance Win32_Processor -EA Stop;",
        "$gpu=Get-CimInstance Win32_VideoController -EA Stop;",
        "$cs=Get-CimInstance Win32_ComputerSystem -EA Stop;",
        "$bb=Get-CimInstance Win32_BaseBoard -EA Stop;",
        "$mem=Get-CimInstance Win32_PhysicalMemory -EA Stop;",
        "$disk=Get-CimInstance Win32_DiskDrive -EA Stop;",
        "@{",
        "os=@{caption=$os.Caption;version=$os.Version;build=$os.BuildNumber};",
        "cpu=@($cpu|%{@{name=$_.Name;cores=$_.NumberOfCores;threads=$_.NumberOfLogicalProcessors}});",
        "gpu=@($gpu|%{@{name=$_.Name;ramBytes=$_.AdapterRAM;driver=$_.DriverVersion}});",
        "board=@{csMfr=$cs.Manufacturer;csModel=$cs.Model;bbProd=$bb.Product};",
        "ram=@($mem|%{@{capBytes=$_.Capacity;speed=$_.Speed;configured=$_.ConfiguredClockSpeed}});",
        "disk=@($disk|%{@{model=$_.Model;sizeBytes=$_.Size;mediaType=$_.MediaType}})",
        "}|ConvertTo-Json -Depth 4",
        "}catch{'{ \"error\": \"collection failed\" }'}"
    ));

    let service_txt = {
        let mut out = String::new();
        for (label, args) in &[
            ("sc query rigstats-sensor", vec!["query", "rigstats-sensor"]),
            ("sc qc rigstats-sensor", vec!["qc", "rigstats-sensor"]),
        ] {
            let _ = writeln!(out, "=== {label} ===");
            match rigstats_backend::debug::run_hidden_command("sc", args) {
                Ok(r) => {
                    out.push_str(&String::from_utf8_lossy(&r.stdout));
                    if !r.stderr.is_empty() {
                        out.push_str(&String::from_utf8_lossy(&r.stderr));
                    }
                }
                Err(e) => {
                    let _ = writeln!(out, "Error: {e}");
                }
            }
            out.push('\n');
        }
        out
    };

    let env_txt = {
        let vars = [
            "OS",
            "PROCESSOR_ARCHITECTURE",
            "PROCESSOR_IDENTIFIER",
            "NUMBER_OF_PROCESSORS",
            "COMPUTERNAME",
            "SystemRoot",
            "USERNAME",
            "USERDOMAIN",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
        ];
        vars.iter()
            .map(|v| {
                format!(
                    "{}={}",
                    v,
                    std::env::var(v).unwrap_or_else(|_| "(not set)".to_string())
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let ram_spec_probe = run_ps_capture(
        "$m=Get-CimInstance Win32_PhysicalMemory; \
         $dimms=$m.Count; \
         $speed=($m|%{if($_.ConfiguredClockSpeed){$_.ConfiguredClockSpeed}else{$_.Speed}}|Measure-Object -Maximum).Maximum; \
         $t=switch([int]($m|Select -First 1 -Exp SMBIOSMemoryType)){34{'DDR5'}26{'DDR4'}24{'DDR3'}default{''}}; \
         \"$t $speed MT/s ($dimms DIMMs)\"",
    );

    let sysinfo_json = {
        let mut sys = System::new();
        sys.refresh_cpu();
        sys.refresh_memory();
        let cpu_brand = sys
            .cpus()
            .first()
            .map(|c| c.brand().to_string())
            .unwrap_or_default();
        let cpu_count = sys.cpus().len();
        let total_mb = sys.total_memory() / 1_048_576;
        let used_mb = sys.used_memory() / 1_048_576;
        let wmi_ok = hardware::probe_wmi_status().is_ok();
        serde_json::to_string_pretty(&serde_json::json!({
            "cpu_brand": cpu_brand,
            "cpu_count": cpu_count,
            "total_memory_mb": total_mb,
            "used_memory_mb": used_mb,
            "wmi_available": wmi_ok,
            "sysinfo_available": true,
            "ram_spec_probe": ram_spec_probe,
        }))
        .unwrap_or_default()
    };

    // Recent Application Event Log entries for rigstats (crashes, errors).
    let event_log_txt = run_ps_capture(
        "try { \
          $evts = Get-WinEvent -FilterHashtable @{LogName='Application';ProviderName='rigstats*';Level=1,2} \
            -MaxEvents 50 -EA Stop | \
            Select-Object TimeCreated,Id,LevelDisplayName,Message; \
          if ($evts) { $evts | Format-List | Out-String } else { 'No rigstats error events found.' } \
        } catch { \
          try { \
            $evts2 = Get-WinEvent -FilterHashtable @{LogName='Application';Level=1,2} \
              -MaxEvents 100 -EA Stop | \
              Where-Object { $_.Message -match 'rigstats' } | \
              Select-Object TimeCreated,Id,LevelDisplayName,Message; \
            if ($evts2) { $evts2 | Format-List | Out-String } else { 'No rigstats-related Application errors found.' } \
          } catch { \"Event log query failed: $_\" } \
        }",
    );

    // ── Write ZIP ─────────────────────────────────────────────────────────────

    let zip_file = std::fs::File::create(&out_path)?;
    let mut writer = zip::ZipWriter::new(zip_file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let entries: &[(&str, &[u8])] = &[
        ("manifest.json", manifest.as_bytes()),
        ("debug.log", &debug_log),
        ("debug-prev.log", &debug_log_prev),
        ("install.log", &install_log),
        ("settings.json", settings_json.as_bytes()),
        ("sidecar-log.txt", &sidecar_log),
        ("sensor-tree.txt", &sensor_tree),
        ("sidecar-service.txt", service_txt.as_bytes()),
        ("hardware.json", hardware_json.as_bytes()),
        ("environment.txt", env_txt.as_bytes()),
        ("sysinfo.json", sysinfo_json.as_bytes()),
        ("event-log.txt", event_log_txt.as_bytes()),
        ("displays.json", displays_json.as_bytes()),
    ];

    for (name, data) in entries {
        writer
            .start_file(*name, opts)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        writer.write_all(data)?;
    }
    writer
        .finish()
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    // Open the containing folder with the zip selected.
    let _ = Command::new("explorer.exe")
        .args(["/select,", &out_path.display().to_string()])
        .spawn();

    Ok(out_path)
}

pub fn collect_and_open_diagnostics(dir: &Path) {
    match collect_and_open_diagnostics_impl(dir) {
        Ok(path) => debug::append_debug_log(
            dir,
            &format!("status: diagnostics collected at {}", path.display()),
        ),
        Err(err) => debug::log_error(
            dir,
            &format!("status: diagnostics collection failed: {err}"),
        ),
    }
}

/// Run diagnostics collection on a background thread. The save dialog, multiple
/// PowerShell/WMI/`sc.exe` probes and zip writing easily block for several
/// seconds, so this must never run on the egui UI thread. No-op if a collection
/// is already in flight.
pub fn spawn_collect(collecting: Arc<AtomicBool>, dir: PathBuf, ctx: egui::Context) {
    if collecting.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        collect_and_open_diagnostics(&dir);
        collecting.store(false, Ordering::Relaxed);
        ctx.request_repaint();
    });
}

fn render_debug_log(ui: &mut egui::Ui, dc: &DialogColors, log: &str, log_h: f32) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        section_label(ui, dc, "Debug Log");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::dialog_btn_secondary(ui, "Copy Log", dc).clicked() {
                ui.ctx().copy_text(log.to_string());
            }
        });
    });

    card_frame(dc).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Frame::new()
            .fill(dc.inset)
            .corner_radius(egui::CornerRadius::same(4))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(log_h)
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut text = log;
                        ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .interactive(false),
                        );
                    });
            });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Showing latest log lines from RigStats backend diagnostics.")
                .size(11.0)
                .color(dc.muted),
        );
    });
}

// ── Window ────────────────────────────────────────────────────────────────────

#[allow(deprecated)]
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    main_ctx: &egui::Context,
    open: &Arc<AtomicBool>,
    needs_focus: &Arc<AtomicBool>,
    state: &Arc<Mutex<StatusState>>,
    refreshing: &Arc<AtomicBool>,
    collecting: &Arc<AtomicBool>,
    dir: &Arc<PathBuf>,
    pipe_connected: bool,
    wallpaper_active: bool,
    dc: &DialogColors,
) {
    dc.apply_to_ctx(ctx);
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    let mut action_refresh = false;
    let mut action_collect_diag = false;
    let mut action_open_folder = false;
    let mut action_close = false;

    let st = state.lock_safe().clone();

    // ── Hero ──────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::top("status_hero")
        .frame(egui::Frame::new().fill(dc.bg).inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 14,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("Status")
                    .size(22.0)
                    .strong()
                    .color(dc.text),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Diagnostics, debug log and dependency health.")
                    .size(12.0)
                    .color(dc.muted),
            );
        });

    // ── Footer ────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("status_footer")
        .frame(egui::Frame::new().fill(dc.bg).inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 8,
            bottom: 10,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let collect_busy = collecting.load(Ordering::Relaxed);
                let collect_label = if collect_busy {
                    "Collecting…"
                } else {
                    "Collect Diagnostics…"
                };
                if theme::dialog_btn_secondary(ui, collect_label, dc).clicked() && !collect_busy {
                    action_collect_diag = true;
                }
                if collect_busy {
                    ui.add_space(4.0);
                    ui.spinner();
                }
                ui.add_space(4.0);
                if theme::dialog_btn_secondary(ui, "Log Folder", dc).clicked() {
                    action_open_folder = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::dialog_btn_secondary(ui, "Close", dc).clicked() {
                        action_close = true;
                    }
                    ui.add_space(6.0);
                    if theme::dialog_btn_primary(ui, "Refresh").clicked() {
                        action_refresh = true;
                    }
                    if refreshing.load(Ordering::Relaxed) {
                        ui.add_space(6.0);
                        ui.spinner();
                        ui.label(
                            egui::RichText::new("Refreshing…")
                                .size(11.0)
                                .color(dc.muted),
                        );
                    }
                });
            });
        });

    // ── Central content ───────────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(dc.bg)
                .inner_margin(egui::Margin::same(10)),
        )
        .show(ctx, |ui| {
            // Reserve space for the debug log scroll — takes what's left after
            // Diagnostics (~130 px) + Dependencies (~130 px) + Debug Log header + note.
            const STATIC_H: f32 = 300.0;
            let log_h = (ui.available_height() - STATIC_H).max(80.0);

            render_diagnostics(ui, dc, &st);
            render_components(ui, dc, &st);
            render_debug_log(ui, dc, &st.log, log_h);
        });

    if action_refresh {
        spawn_load(
            state.clone(),
            refreshing.clone(),
            dir.as_ref().clone(),
            pipe_connected,
            wallpaper_active,
            main_ctx.clone(),
        );
    }
    if action_open_folder {
        let _ = std::process::Command::new("explorer")
            .arg(dir.as_ref())
            .spawn();
    }
    if action_collect_diag {
        spawn_collect(collecting.clone(), dir.as_ref().clone(), main_ctx.clone());
    }
    if action_close {
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    if ctx.input(|i| i.viewport().close_requested()) {
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

#[cfg(test)]
mod tests {
    use super::{driver_age_label, gpu_driver_support, C_GOOD, C_WARN, DRIVER_STALE_DAYS};

    #[test]
    fn fresh_driver_is_up_to_date() {
        let (txt, col) = driver_age_label(5);
        assert_eq!(txt, "Up to date");
        assert_eq!(col, C_GOOD);
    }

    #[test]
    fn months_old_but_not_stale_is_good() {
        let (txt, col) = driver_age_label(60);
        assert_eq!(txt, "2 mo old");
        assert_eq!(col, C_GOOD);
    }

    #[test]
    fn stale_driver_is_warned() {
        let (txt, col) = driver_age_label(DRIVER_STALE_DAYS);
        assert!(
            txt.starts_with('\u{26a0}'),
            "expected warning prefix, got {txt}"
        );
        assert_eq!(col, C_WARN);
    }

    #[test]
    fn vendor_support_matches_known_gpus() {
        assert_eq!(
            gpu_driver_support("NVIDIA GeForce RTX 4090").map(|v| v.0),
            Some("NVIDIA")
        );
        assert_eq!(
            gpu_driver_support("AMD Radeon RX 9070 XT").map(|v| v.0),
            Some("AMD")
        );
        assert_eq!(
            gpu_driver_support("Intel Arc A770").map(|v| v.0),
            Some("Intel")
        );
        assert_eq!(gpu_driver_support("Some Virtual Display"), None);
    }
}
