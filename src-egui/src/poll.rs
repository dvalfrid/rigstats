//! Background poll loop and the data types exchanged with the UI thread.
//!
//! `poll_loop` runs on the tokio runtime, samples hardware once per second, and
//! sends a [`PollStats`] snapshot to the egui UI thread. Extracted from `main.rs`.

use crate::lock_ext::LockSafe;
use rigstats_backend::{debug, hardware, lhm, lhm_process, logging, settings};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

// ── Data types exchanged between poll thread and UI thread ────────────────────

#[derive(Clone, Debug, Default)]
pub struct DriveInfo {
    pub fs: String,
    pub used: u64,
    pub total: u64,
    pub pct: u8,
    pub temp: Option<f64>,
    pub model: String,
    pub kind: rigstats_backend::stats::DiskKind,
}

#[derive(Clone, Debug, Default)]
pub struct ProcessInfo {
    pub name: String,
    pub cpu: f32,
    pub mem_mb: u64,
}

#[derive(Clone, Debug, Default)]
pub struct PollStats {
    // CPU
    pub cpu_load: u8,
    pub cpu_temp: Option<f64>,
    pub cpu_freq_mhz: f64,
    pub cpu_power: Option<f64>,
    pub cpu_cores: Vec<u8>,
    // GPU
    pub gpu_load: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub gpu_hotspot: Option<f64>,
    pub gpu_freq_mhz: Option<f64>,
    pub gpu_mem_freq_mhz: Option<f64>,
    pub gpu_vram_used_mb: Option<f64>,
    pub gpu_vram_total_mb: Option<f64>,
    pub gpu_power: Option<f64>,
    pub total_gpu_power: Option<f64>,
    pub gpu_fan: Option<f64>,
    pub gpu_d3d_3d: Option<f64>,
    pub gpu_d3d_vdec: Option<f64>,
    // RAM
    pub ram_used: u64,
    pub ram_total: u64,
    pub ram_spec: String,
    pub ram_details: String,
    // Network
    pub net_up_mbps: f64,
    pub net_down_mbps: f64,
    pub net_iface: String,
    pub net_ping_ms: Option<f64>,
    pub net_ping_wan_ms: Option<f64>,
    // Disk
    pub disk_read_mbps: f64,
    pub disk_write_mbps: f64,
    pub disk_drives: Vec<DriveInfo>,
    // Motherboard (LHM)
    pub mb_fans: Vec<(String, f64)>,
    pub mb_temps: Vec<(String, f64)>,
    pub mb_voltages: Vec<(String, f64)>,
    pub mb_chip: Option<String>,
    pub mb_board: Option<String>,
    // Battery
    pub battery_present: bool,
    pub battery_charge_pct: Option<u8>,
    pub battery_charging: Option<bool>,
    pub battery_time_mins: Option<u32>,
    pub battery_power_w: Option<f64>,
    // Processes
    pub processes: Vec<ProcessInfo>,
    // System
    pub uptime_secs: u64,
    pub hostname: String,
    pub cpu_model: String,
    pub model_name: String,   // product model, e.g. "ROG GM700TZ"
    pub system_brand: String, // brand key, e.g. "asus-rog"
    pub gpu_name: String,     // GPU display name, e.g. "AMD Radeon RX 9070 XT"
    pub ram_temp: Option<f64>,
    pub gpu_devices: Vec<(String, f64)>,
    // Meta
    pub lhm_connected: bool,
}

fn lhm_temp_for_model(wmi_model: &str, disk_temps: &[(String, f64)]) -> Option<f64> {
    let needle = wmi_model.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    disk_temps.iter().find_map(|(name, temp)| {
        let hay = name.trim().to_lowercase();
        if hay == needle || hay.contains(&needle) || needle.contains(&hay) {
            Some(*temp)
        } else {
            None
        }
    })
}

// ── Logging helpers ───────────────────────────────────────────────────────────

fn poll_stats_to_log_payload(s: &PollStats) -> rigstats_backend::stats::StatsPayload {
    use rigstats_backend::stats::{
        BatteryStats, CpuStats, DiskDrive, DiskStats, GpuStats, MotherboardStats, NetStats,
        ProcessEntry, RamStats, StatsPayload,
    };
    StatsPayload {
        cpu: CpuStats {
            load: s.cpu_load,
            cores: s.cpu_cores.clone(),
            temp: s.cpu_temp,
            freq: s.cpu_freq_mhz,
            power: s.cpu_power,
        },
        gpu: GpuStats {
            name: Some(s.gpu_name.clone()),
            load: s.gpu_load,
            temp: s.gpu_temp,
            hotspot: s.gpu_hotspot,
            freq: s.gpu_freq_mhz,
            mem_freq: s.gpu_mem_freq_mhz,
            vram_used: s.gpu_vram_used_mb,
            vram_total: s.gpu_vram_total_mb,
            fan_speed: s.gpu_fan,
            power: s.gpu_power,
            d3d_3d: s.gpu_d3d_3d,
            d3d_vdec: s.gpu_d3d_vdec,
            available_gpus: s.gpu_devices.clone(),
        },
        ram: RamStats {
            total: s.ram_total,
            used: s.ram_used,
            free: s.ram_total.saturating_sub(s.ram_used),
            spec: s.ram_spec.clone(),
            details: String::new(),
            temp: s.ram_temp,
        },
        net: NetStats {
            up: s.net_up_mbps,
            down: s.net_down_mbps,
            iface: s.net_iface.clone(),
            ping_ms: s.net_ping_ms,
        },
        disk: DiskStats {
            read: s.disk_read_mbps,
            write: s.disk_write_mbps,
            drives: s
                .disk_drives
                .iter()
                .map(|d| DiskDrive {
                    fs: d.fs.clone(),
                    size: d.total,
                    used: d.used,
                    pct: d.pct,
                    temp: d.temp,
                    model: d.model.clone(),
                    kind: d.kind,
                })
                .collect(),
        },
        motherboard: MotherboardStats {
            fans: s.mb_fans.clone(),
            temps: s.mb_temps.clone(),
            voltages: s.mb_voltages.clone(),
            chip: s.mb_chip.clone(),
            board: s.mb_board.clone(),
        },
        battery: BatteryStats {
            present: s.battery_present,
            charge_pct: s.battery_charge_pct,
            charging: s.battery_charging,
            time_remaining_mins: s.battery_time_mins,
            power_w: s.battery_power_w,
        },
        top_processes: s
            .processes
            .iter()
            .map(|p| ProcessEntry {
                name: p.name.clone(),
                cpu: p.cpu,
                mem_mb: p.mem_mb,
            })
            .collect(),
        system_uptime_secs: s.uptime_secs,
        lhm_connected: s.lhm_connected,
    }
}

// ── Poll loop (tokio runtime) ─────────────────────────────────────────────────

pub(crate) async fn poll_loop(
    tx: mpsc::SyncSender<PollStats>,
    dir: PathBuf,
    preferred_gpu: Arc<Mutex<Option<String>>>,
    settings_arc: Arc<Mutex<settings::Settings>>,
) {
    let ram_spec = tokio::task::spawn_blocking(hardware::detect_ram_spec)
        .await
        .unwrap_or_default();
    debug::log_debug(&dir, &format!("hardware: ram_spec={ram_spec}"));
    let ram_details = tokio::task::spawn_blocking(hardware::detect_ram_details)
        .await
        .unwrap_or_default();
    debug::log_debug(&dir, &format!("hardware: ram_details={ram_details}"));
    let disk_model_map: HashMap<String, String> =
        tokio::task::spawn_blocking(hardware::detect_disk_model_map)
            .await
            .unwrap_or_default();
    debug::log_debug(
        &dir,
        &format!("hardware: disk_model_map entries={}", disk_model_map.len()),
    );
    let disk_type_map: HashMap<String, rigstats_backend::stats::DiskKind> =
        tokio::task::spawn_blocking(hardware::detect_disk_type_map)
            .await
            .unwrap_or_default();
    debug::log_debug(
        &dir,
        &format!("hardware: disk_type_map entries={}", disk_type_map.len()),
    );
    let ping_target = hardware::detect_ping_target();
    debug::log_debug(&dir, &format!("hardware: ping_target={ping_target}"));
    let mb_board: Option<String> = tokio::task::spawn_blocking(hardware::detect_motherboard_name)
        .await
        .ok()
        .flatten();
    debug::log_debug(&dir, &format!("hardware: mb_board={mb_board:?}"));
    let model_name: String = tokio::task::spawn_blocking(hardware::detect_model_name)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    debug::log_debug(&dir, &format!("hardware: model_name={model_name}"));
    let system_brand: String = tokio::task::spawn_blocking(hardware::detect_system_brand)
        .await
        .unwrap_or_default();
    debug::log_debug(&dir, &format!("hardware: system_brand={system_brand}"));
    let gpu_name: String = tokio::task::spawn_blocking(hardware::detect_gpu_name)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    debug::log_debug(&dir, &format!("hardware: gpu_name={gpu_name}"));

    let mut sys = System::new();
    sys.refresh_cpu();
    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_default();
    let hostname = System::host_name().unwrap_or_else(|| "—".to_string());
    debug::log_debug(&dir, &format!("hardware: cpu_model={cpu_model}"));
    debug::log_debug(&dir, &format!("hardware: hostname={hostname}"));

    let mut disks = Disks::new_with_refreshed_list();
    let mut networks = Networks::new_with_refreshed_list();
    let pipe = tokio::sync::Mutex::new(None::<lhm::LhmPipeReader>);

    let mut last_net_instant = Instant::now();
    let mut last_ping: Option<(Instant, Option<f64>)> = None;
    let mut last_ping_wan: Option<(Instant, Option<f64>)> = None;
    type BatteryCache = (u8, bool, Option<u32>, Option<f64>);
    let mut last_battery: Option<(Instant, BatteryCache)> = None;
    let mut last_prune_day: Option<u64> = None;
    let mut first_tick_logged = false;

    loop {
        sys.refresh_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::new().with_cpu_usage().with_frequency())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_processes_specifics(
            sysinfo::ProcessRefreshKind::new()
                .with_cpu()
                .with_memory()
                .with_disk_usage(),
        );

        let cpu_load = sys.global_cpu_info().cpu_usage() as u8;
        // global_cpu_info().frequency() returns 0 on Windows in sysinfo 0.30; use per-core average.
        let cpu_freq_mhz = {
            let cpus = sys.cpus();
            if cpus.is_empty() {
                0.0
            } else {
                cpus.iter().map(|c| c.frequency() as f64).sum::<f64>() / cpus.len() as f64
            }
        };
        let cpu_cores: Vec<u8> = sys.cpus().iter().map(|c| c.cpu_usage() as u8).collect();
        let uptime_secs = System::uptime();

        let num_cpus = sys.cpus().len().max(1) as f32;
        let mut processes: Vec<ProcessInfo> = sys
            .processes()
            .values()
            .map(|p| ProcessInfo {
                name: p.name().to_string(),
                cpu: p.cpu_usage() / num_cpus,
                mem_mb: p.memory() / 1_048_576,
            })
            .collect();
        processes.sort_by(|a, b| {
            b.cpu
                .partial_cmp(&a.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        processes.truncate(8);

        disks.refresh();
        let mut disk_drives: Vec<DriveInfo> = disks
            .iter()
            .filter(|d| d.total_space() > 1_000_000_000)
            .map(|d| {
                let total = d.total_space();
                let used = total.saturating_sub(d.available_space());
                let pct = if total > 0 {
                    ((used as f64 / total as f64) * 100.0) as u8
                } else {
                    0
                };
                DriveInfo {
                    fs: d.mount_point().to_string_lossy().to_string(),
                    used,
                    total,
                    pct,
                    temp: None,
                    model: String::new(),
                    kind: rigstats_backend::stats::DiskKind::Unknown,
                }
            })
            .collect();

        let now = Instant::now();
        let elapsed = now.duration_since(last_net_instant).as_secs_f64().max(0.5);
        last_net_instant = now;

        // disk_usage().read_bytes / written_bytes are per-tick deltas from sysinfo processes.
        let total_read_bytes: u64 = sys
            .processes()
            .values()
            .map(|p| p.disk_usage().read_bytes)
            .sum();
        let total_write_bytes: u64 = sys
            .processes()
            .values()
            .map(|p| p.disk_usage().written_bytes)
            .sum();
        let disk_read_mbps = total_read_bytes as f64 / elapsed / 1_048_576.0;
        let disk_write_mbps = total_write_bytes as f64 / elapsed / 1_048_576.0;
        networks.refresh();
        let mut best_iface = "--".to_string();
        let mut best_up = 0.0f64;
        let mut best_down = 0.0f64;
        for (name, data) in networks.iter() {
            let up = data.transmitted() as f64 * 8.0 / 1_000_000.0 / elapsed;
            let down = data.received() as f64 * 8.0 / 1_000_000.0 / elapsed;
            if up + down > best_up + best_down {
                best_up = up;
                best_down = down;
                best_iface = name.clone();
            }
        }

        let ping_ms = {
            let stale = last_ping
                .as_ref()
                .map(|(t, _)| t.elapsed().as_secs_f64() >= 5.0)
                .unwrap_or(true);
            if stale {
                let target = ping_target.clone();
                let measured =
                    tokio::task::spawn_blocking(move || hardware::sample_ping_ms(&target))
                        .await
                        .unwrap_or(None);
                last_ping = Some((Instant::now(), measured));
                measured
            } else {
                last_ping.as_ref().and_then(|(_, v)| *v)
            }
        };
        let ping_wan_ms = {
            let stale = last_ping_wan
                .as_ref()
                .map(|(t, _)| t.elapsed().as_secs_f64() >= 5.0)
                .unwrap_or(true);
            if stale {
                let measured = tokio::task::spawn_blocking(|| hardware::sample_ping_ms("1.1.1.1"))
                    .await
                    .unwrap_or(None);
                last_ping_wan = Some((Instant::now(), measured));
                measured
            } else {
                last_ping_wan.as_ref().and_then(|(_, v)| *v)
            }
        };

        let (
            battery_present,
            battery_charge_pct,
            battery_charging,
            battery_time_mins,
            battery_power_w,
        ) = {
            let stale = last_battery
                .as_ref()
                .map(|(t, _)| t.elapsed().as_secs_f64() >= 10.0)
                .unwrap_or(true);
            if stale {
                let result = match tokio::task::spawn_blocking(hardware::sample_battery_wmi).await {
                    Ok(result) => result,
                    Err(err) => {
                        debug::log_warn(
                            &dir,
                            &format!("battery: sample_battery_wmi join error: {err}"),
                        );
                        None
                    }
                };
                if result.is_none() {
                    match tokio::task::spawn_blocking(hardware::probe_wmi_status).await {
                        Ok(Err(err)) => debug::log_warn(
                            &dir,
                            &format!("battery: WMI probe failed after battery read miss: {err}"),
                        ),
                        Err(err) => debug::log_warn(
                            &dir,
                            &format!("battery: probe_wmi_status join error: {err}"),
                        ),
                        Ok(Ok(())) => {}
                    }
                }
                last_battery = result.as_ref().map(|d| (Instant::now(), *d));
                match result {
                    Some((pct, charging, mins, w)) => (true, Some(pct), Some(charging), mins, w),
                    None => (false, None, None, None, None),
                }
            } else {
                match last_battery.as_ref().map(|(_, d)| *d) {
                    Some((pct, charging, mins, w)) => (true, Some(pct), Some(charging), mins, w),
                    None => (false, None, None, None, None),
                }
            }
        };

        let pref = preferred_gpu.lock_safe().clone();
        let lhm_data = lhm::fetch_lhm_pipe(&pipe, pref.as_deref(), &dir).await;
        let lhm_connected = lhm_data.is_some();
        lhm_process::track_lhm_connection_state(&dir, lhm_connected);

        for drive in disk_drives.iter_mut() {
            let key = drive.fs.trim_end_matches(['\\', '/']).to_string();
            if let Some(model) = disk_model_map.get(&key) {
                drive.model = model.clone();
                drive.kind = disk_type_map
                    .get(model.as_str())
                    .copied()
                    .unwrap_or_default();
            }
        }

        if let Some(ref lhm) = lhm_data {
            for (i, drive) in disk_drives.iter_mut().enumerate() {
                let key = drive.fs.trim_end_matches(['\\', '/']).to_string();
                if let Some(model) = disk_model_map.get(&key) {
                    drive.temp = lhm_temp_for_model(model, &lhm.disk_temps);
                }
                if drive.temp.is_none() && !disk_model_map.contains_key(&key) {
                    drive.temp = lhm.disk_temps.get(i).map(|(_, t)| *t);
                }
            }
        }

        let display_model_name = {
            let s = settings_arc.lock_safe();
            if s.model_name.is_empty() {
                model_name.clone()
            } else {
                s.model_name.clone()
            }
        };

        let stats = PollStats {
            cpu_load,
            cpu_temp: lhm_data.as_ref().and_then(|l| l.cpu_temp),
            cpu_freq_mhz,
            cpu_power: lhm_data.as_ref().and_then(|l| l.cpu_power),
            cpu_cores,
            gpu_load: lhm_data.as_ref().and_then(|l| l.gpu_load),
            gpu_temp: lhm_data.as_ref().and_then(|l| l.gpu_temp),
            gpu_hotspot: lhm_data.as_ref().and_then(|l| l.gpu_hotspot),
            gpu_freq_mhz: lhm_data.as_ref().and_then(|l| l.gpu_freq),
            gpu_mem_freq_mhz: lhm_data.as_ref().and_then(|l| l.gpu_mem_freq),
            gpu_vram_used_mb: lhm_data.as_ref().and_then(|l| l.vram_used),
            gpu_vram_total_mb: lhm_data.as_ref().and_then(|l| l.vram_total),
            gpu_power: lhm_data.as_ref().and_then(|l| l.gpu_power),
            total_gpu_power: lhm_data.as_ref().and_then(|l| l.total_gpu_power),
            gpu_fan: lhm_data.as_ref().and_then(|l| l.gpu_fan),
            gpu_d3d_3d: lhm_data.as_ref().and_then(|l| l.gpu_d3d_3d),
            gpu_d3d_vdec: lhm_data.as_ref().and_then(|l| l.gpu_d3d_vdec),
            ram_used: sys.used_memory(),
            ram_total: sys.total_memory(),
            ram_spec: ram_spec.clone(),
            ram_details: ram_details.clone(),
            net_up_mbps: best_up,
            net_down_mbps: best_down,
            net_iface: best_iface,
            net_ping_ms: ping_ms,
            net_ping_wan_ms: ping_wan_ms,
            disk_read_mbps,
            disk_write_mbps,
            disk_drives,
            mb_fans: lhm_data
                .as_ref()
                .map(|l| l.mb_fans.clone())
                .unwrap_or_default(),
            mb_temps: lhm_data
                .as_ref()
                .map(|l| l.mb_temps.clone())
                .unwrap_or_default(),
            mb_voltages: lhm_data
                .as_ref()
                .map(|l| l.mb_voltages.clone())
                .unwrap_or_default(),
            mb_chip: lhm_data.as_ref().and_then(|l| l.mb_chip.clone()),
            mb_board: mb_board.clone(),
            battery_present,
            battery_charge_pct,
            battery_charging,
            battery_time_mins,
            battery_power_w,
            processes,
            uptime_secs,
            hostname: hostname.clone(),
            cpu_model: cpu_model.clone(),
            model_name: display_model_name,
            system_brand: system_brand.clone(),
            gpu_name: lhm_data
                .as_ref()
                .and_then(|l| l.gpu_name.clone())
                .unwrap_or_else(|| gpu_name.clone()),
            ram_temp: lhm_data.as_ref().and_then(|l| l.ram_temp),
            gpu_devices: lhm_data
                .as_ref()
                .map(|l| l.gpu_devices.clone())
                .unwrap_or_default(),
            lhm_connected,
        };

        let _ = tx.send(stats.clone());
        if !first_tick_logged {
            debug::append_debug_log(
                &dir,
                &format!("poll: first tick complete lhm_connected={lhm_connected}"),
            );
            first_tick_logged = true;
        }

        // CSV logging — reads settings each tick so changes take effect immediately.
        let (log_enabled, retention_days) = {
            let s = settings_arc.lock_safe();
            (s.logging_enabled, s.log_retention_days)
        };
        if log_enabled {
            let payload = poll_stats_to_log_payload(&stats);
            if let Err(e) = logging::append_stats_row(&payload, &dir) {
                debug::log_error(&dir, &format!("logging: csv write error — {e}"));
            }
            let today = logging::unix_now_secs() / 86400;
            if last_prune_day != Some(today) {
                logging::prune_old_logs(&dir, retention_days);
                last_prune_day = Some(today);
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
