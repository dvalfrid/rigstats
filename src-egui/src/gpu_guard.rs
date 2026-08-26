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

use eframe::egui;
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
