//! Guards the wgpu render device against fatal errors — most notably a
//! hybrid iGPU/dGPU switch invalidating the D3D12 device out from under a
//! running frame — that would otherwise panic the process.
//!
//! Neither `eframe` nor `egui-wgpu` install a `Device::on_uncaptured_error`
//! or `set_device_lost_callback` handler. Without one, any fatal device
//! error falls through to wgpu-core's `default_error_handler`, which
//! unconditionally panics on the calling thread — see
//! `wgpu_core::backend::wgpu_core::default_error_handler`. Surface-level
//! errors (`Outdated`/`Lost`/`Occluded`) are already handled gracefully by
//! egui-wgpu itself; this only covers the device going away entirely.
//!
//! Also picks which adapter renders ([`render_power_preference`]): on a
//! laptop the power-saving one, so the app isn't a "dGPU app" that Eco mode
//! (G-Helper/Armoury Crate "kill GPU apps") terminates before powering the
//! dGPU off — a kill no in-process guard can survive.

use eframe::egui;
use eframe::wgpu::PowerPreference;
use rigstats_backend::debug;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Installs handlers that flip `lost` to `true` and request a repaint
/// instead of letting wgpu panic the process. The callbacks only log and
/// set state — no GPU work — so they're safe to run on whichever thread
/// wgpu happens to call them from.
///
/// The caller must check `lost` each frame and tear the app down via
/// `ViewportCommand::Close`, never `process::exit`: an abrupt exit skips
/// wgpu's teardown and can leak GPU/desktop-heap resources across process
/// spawns (see the teardown comment in `bin/wallpaper.rs`).
pub fn install_gpu_loss_guard(
    render_state: &eframe::egui_wgpu::RenderState,
    ctx: egui::Context,
    dir: Arc<PathBuf>,
    lost: Arc<AtomicBool>,
) {
    let dir_err = dir.clone();
    let lost_err = lost.clone();
    let ctx_err = ctx.clone();
    render_state
        .device
        .on_uncaptured_error(Arc::new(move |err| {
            debug::log_error(&dir_err, &format!("gpu: uncaptured wgpu error — {err}"));
            lost_err.store(true, Ordering::Relaxed);
            ctx_err.request_repaint();
        }));
    render_state
        .device
        .set_device_lost_callback(move |reason, msg| {
            debug::log_error(&dir, &format!("gpu: device lost ({reason:?}) — {msg}"));
            lost.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        });
}

/// Adapter preference for every wgpu setup (main app and wallpaper host).
/// Laptops (a system battery) render on the power-saving GPU: a hybrid
/// iGPU/dGPU switch can't take the app down and the dGPU may sleep on
/// battery. Desktops — or an unknown battery state — keep egui-wgpu's
/// default high-performance choice. `WGPU_POWER_PREF` (`low`/`high`)
/// overrides both, as does the per-app choice in Windows' graphics settings.
pub fn render_power_preference() -> PowerPreference {
    PowerPreference::from_env().unwrap_or_else(|| {
        if has_system_battery() {
            PowerPreference::LowPower
        } else {
            PowerPreference::HighPerformance
        }
    })
}

/// Logs which adapter the render device ended up on, once at start-up.
pub fn log_render_adapter(render_state: &eframe::egui_wgpu::RenderState, dir: &std::path::Path) {
    let info = render_state.adapter.get_info();
    debug::log_debug(
        dir,
        &format!(
            "gpu: render adapter={} ({:?}) power_preference={:?}",
            info.name,
            info.backend,
            render_power_preference()
        ),
    );
}

/// `GetSystemPowerStatus` is instant (no WMI), so it can run before the
/// window — and its wgpu device — is created.
#[cfg(windows)]
#[allow(unsafe_code)] // one FFI call filling a stack struct
fn has_system_battery() -> bool {
    use winapi::um::winbase::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return false;
    }
    battery_flag_means_battery(status.BatteryFlag)
}

#[cfg(not(windows))]
fn has_system_battery() -> bool {
    false
}

/// `SYSTEM_POWER_STATUS.BatteryFlag`: 128 = no system battery, 255 = unknown.
/// Unknown counts as "no battery" so it keeps today's behaviour.
fn battery_flag_means_battery(flag: u8) -> bool {
    flag != 128 && flag != 255
}

#[cfg(test)]
mod tests {
    use super::battery_flag_means_battery;

    #[test]
    fn battery_flag_maps_to_presence() {
        assert!(!battery_flag_means_battery(128), "no system battery");
        assert!(!battery_flag_means_battery(255), "unknown status");
        assert!(battery_flag_means_battery(1), "high");
        assert!(battery_flag_means_battery(8), "charging");
        assert!(battery_flag_means_battery(0), "between low and high");
    }
}
