use crate::theme;
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
    pub wmi_ok: bool,
    pub log_path: String,
    pub last_refresh: String,
}

impl StatusState {
    pub fn load(dir: &std::path::Path, pipe_connected: bool) -> Self {
        let log_path = dir.join("rigstats-debug.log");
        let log = std::fs::read_to_string(&log_path)
            .unwrap_or_else(|_| "(log file not found)".to_string());
        Self {
            log,
            service_running: query_service_running(),
            pipe_connected,
            wmi_ok: hardware::probe_wmi_status().is_ok(),
            log_path: log_path.display().to_string(),
            last_refresh: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }
}

fn query_service_running() -> bool {
    std::process::Command::new("sc.exe")
        .args(["query", "rigstats-sensor"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("RUNNING"))
        .unwrap_or(false)
}

// ── Colour tokens ─────────────────────────────────────────────────────────────

const C_BG: egui::Color32 = egui::Color32::from_gray(38);
const C_CARD: egui::Color32 = egui::Color32::from_gray(30);
const C_CARD_BORDER: egui::Color32 = egui::Color32::from_gray(55);
const C_LOG_FILL: egui::Color32 = egui::Color32::from_gray(22);
const C_LABEL: egui::Color32 = egui::Color32::from_gray(140);
const C_MUTED: egui::Color32 = egui::Color32::from_gray(115);
const C_TEXT: egui::Color32 = egui::Color32::from_rgb(155, 180, 210);
const C_GOOD: egui::Color32 = egui::Color32::from_rgb(80, 190, 90);
const C_BAD: egui::Color32 = egui::Color32::from_rgb(200, 70, 60);

// ── Widget helpers ────────────────────────────────────────────────────────────

fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(C_CARD)
        .stroke(egui::Stroke::new(1.0, C_CARD_BORDER))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(12, 10))
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).strong().color(C_LABEL));
    ui.add_space(4.0);
}

fn meta_row(ui: &mut egui::Ui, label: &str, value: &str, value_color: egui::Color32) {
    ui.label(egui::RichText::new(label).size(11.0).color(C_MUTED));
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
        .inner_margin(egui::Margin { left: 6, right: 6, top: 2, bottom: 2 })
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

fn render_diagnostics(ui: &mut egui::Ui, state: &StatusState) {
    section_label(ui, "Diagnostics");
    card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());

        // Service + Pipe side by side
        egui::Grid::new("diag_status_grid")
            .num_columns(2)
            .min_col_width(180.0)
            .spacing([8.0, 2.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Service").size(11.0).color(C_MUTED));
                ui.label(egui::RichText::new("Pipe").size(11.0).color(C_MUTED));
                ui.end_row();
                let svc_color = if state.service_running { C_GOOD } else { C_BAD };
                let svc_text = if state.service_running { "RUNNING" } else { "STOPPED" };
                ui.label(egui::RichText::new(svc_text).size(13.0).strong().color(svc_color));
                let pipe_color = if state.pipe_connected { C_GOOD } else { C_BAD };
                let pipe_text = if state.pipe_connected { "Connected" } else { "Disconnected" };
                ui.label(egui::RichText::new(pipe_text).size(13.0).strong().color(pipe_color));
                ui.end_row();
            });

        ui.add_space(6.0);
        meta_row(ui, "Debug Log Path", &state.log_path, C_TEXT);
        ui.add_space(6.0);
        meta_row(ui, "Last Successful Refresh", &state.last_refresh, C_TEXT);
    });
}

fn render_dependencies(ui: &mut egui::Ui, state: &StatusState) {
    ui.add_space(8.0);
    section_label(ui, "Dependencies");
    card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        let sensor_ver = format!("LHM {DEP_LHM_VER}");
        let deps: [(&str, &str, &str, bool); 3] = [
            (
                "rigstats-sensor",
                "Hardware sensor feed (Windows Service)",
                sensor_ver.as_str(),
                state.service_running,
            ),
            ("sysinfo", "CPU, RAM, disk, network stats", DEP_SYSINFO_VER, true),
            ("wmi", "Windows hardware metadata", DEP_WMI_VER, state.wmi_ok),
        ];
        for (i, (name, desc, ver, ok)) in deps.iter().enumerate() {
            if i > 0 {
                ui.add(egui::Separator::default().spacing(6.0));
            }
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(*name).size(13.0).strong().color(C_TEXT));
                    ui.label(egui::RichText::new(*desc).size(11.0).color(C_MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_badge(ui, *ok);
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(*ver).size(12.0).color(C_MUTED));
                });
            });
        }
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

    let manifest = format!(
        "{{\n  \"collected_at_unix\": {ts},\n  \"rigstats_version\": \"{VERSION}\"\n}}"
    );

    let debug_log = std::fs::read(dir.join("rigstats-debug.log"))
        .unwrap_or_else(|_| b"(log file not found)".to_vec());

    let settings_json = std::fs::read_to_string(dir.join("rigstats-settings.json"))
        .map(|s| {
            serde_json::from_str::<serde_json::Value>(&s)
                .and_then(|v| serde_json::to_string_pretty(&v))
                .unwrap_or(s)
        })
        .unwrap_or_else(|_| "(settings file not found)".to_string());

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

    // ── Write ZIP ─────────────────────────────────────────────────────────────

    let zip_file = std::fs::File::create(&out_path)?;
    let mut writer = zip::ZipWriter::new(zip_file);
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let entries: &[(&str, &[u8])] = &[
        ("manifest.json", manifest.as_bytes()),
        ("debug.log", &debug_log),
        ("install.log", &install_log),
        ("settings.json", settings_json.as_bytes()),
        ("sidecar-log.txt", &sidecar_log),
        ("sensor-tree.txt", &sensor_tree),
        ("sidecar-service.txt", service_txt.as_bytes()),
        ("hardware.json", hardware_json.as_bytes()),
        ("environment.txt", env_txt.as_bytes()),
        ("sysinfo.json", sysinfo_json.as_bytes()),
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
        Err(err) => debug::append_debug_log(
            dir,
            &format!("status: diagnostics collection failed: {err}"),
        ),
    }
}

fn render_debug_log(ui: &mut egui::Ui, log: &str, log_h: f32) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        section_label(ui, "Debug Log");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::dialog_btn_secondary(ui, "Copy Log").clicked() {
                ui.ctx().copy_text(log.to_string());
            }
        });
    });

    card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Frame::new()
            .fill(C_LOG_FILL)
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
                .color(C_MUTED),
        );
    });
}

// ── Window ────────────────────────────────────────────────────────────────────

#[allow(deprecated)]
pub fn show(
    ctx: &egui::Context,
    main_ctx: &egui::Context,
    open: &Arc<AtomicBool>,
    needs_focus: &Arc<AtomicBool>,
    state: &Arc<Mutex<StatusState>>,
    dir: &Arc<PathBuf>,
    pipe_connected: bool,
) {
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    let mut action_refresh = false;
    let mut action_collect_diag = false;
    let mut action_close = false;

    let st = state.lock().unwrap().clone();

    // ── Hero ──────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::top("status_hero")
        .frame(
            egui::Frame::new()
                .fill(C_BG)
                .inner_margin(egui::Margin { left: 14, right: 14, top: 14, bottom: 12 }),
        )
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("Status")
                    .size(22.0)
                    .strong()
                    .color(C_TEXT),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Diagnostics, debug log and dependency health.")
                    .size(12.0)
                    .color(C_MUTED),
            );
        });

    // ── Footer ────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("status_footer")
        .frame(
            egui::Frame::new()
                .fill(C_BG)
                .inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 10 }),
        )
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if theme::dialog_btn_secondary(ui, "Collect Diagnostics…").clicked() {
                    action_collect_diag = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::dialog_btn_secondary(ui, "Close").clicked() {
                        action_close = true;
                    }
                    ui.add_space(6.0);
                    if theme::dialog_btn_primary(ui, "Refresh").clicked() {
                        action_refresh = true;
                    }
                });
            });
        });

    // ── Central content ───────────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(C_BG)
                .inner_margin(egui::Margin::same(10)),
        )
        .show(ctx, |ui| {
            // Reserve space for the debug log scroll — takes what's left after
            // Diagnostics (~130 px) + Dependencies (~130 px) + Debug Log header + note.
            const STATIC_H: f32 = 300.0;
            let log_h = (ui.available_height() - STATIC_H).max(80.0);

            render_diagnostics(ui, &st);
            render_dependencies(ui, &st);
            render_debug_log(ui, &st.log, log_h);
        });

    if action_refresh {
        *state.lock().unwrap() = StatusState::load(dir.as_ref(), pipe_connected);
    }
    if action_collect_diag {
        collect_and_open_diagnostics(dir.as_ref());
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
