//! Start-up steps of `main()` that only compute or configure — no threads
//! or tasks are started here.

use eframe::egui;
use rigstats_backend::{debug, settings::Settings};
#[cfg(windows)]
use rigstats_egui::geometry::win_monitor;
use rigstats_egui::geometry::{
    compute_landscape_window_height, compute_window_height, pick_window_rect_for_profile,
    profile_is_landscape, profile_scale, profile_to_size, resolve_pinned_position,
};
use rigstats_egui::tray::load_app_icon;
#[cfg(windows)]
use rigstats_egui::win32_dark_mode;
use std::path::Path;

/// Starts this session's debug log (the previous one is kept as
/// `rigstats-debug-prev.log`) and logs every panic to it.
pub(crate) fn init_logging(dir: &Path) {
    debug::reset_debug_log(dir);
    debug::install_panic_logger(dir);
    debug::append_debug_log(dir, "rigstats starting");
    debug::append_debug_log(dir, &format!("settings dir: {}", dir.display()));

    #[cfg(windows)]
    {
        let dark = win32_dark_mode::is_system_dark_mode();
        debug::log_debug(dir, &format!("os_dark_mode: {dark}"));
    }
}

/// The main window's initial inner size and position: the profile width,
/// a content-fit height (or the monitor height in fullscreen), on the
/// monitor matching the profile — or at the pinned position when that is
/// still on-screen.
pub(crate) fn initial_window(s: &Settings) -> ([f32; 2], [f32; 2]) {
    let visible_panels = &s.visible_panels;
    let [win_w, win_h] = profile_to_size(&s.dashboard_profile);
    let landscape = profile_is_landscape(&s.dashboard_profile);
    // Width is always the profile width (panels never stretch). In fullscreen the
    // height fills the monitor and the window pins to the monitor top-left; the
    // 2 px width trim used in normal mode is dropped so a matching screen fills.
    let fullscreen = s.fullscreen_mode && !s.floating_mode;
    // Auto-target the monitor matching the profile resolution (or the primary
    // monitor when none matches) for both orientations, so portrait/side profiles
    // land on a matching screen or the main screen rather than an arbitrary one.
    let [mx, my, _mw, mh] = pick_window_rect_for_profile(&s.dashboard_profile);
    let (inner_w, inner_h, mut pos_x, mut pos_y) = if fullscreen && mh > 0.0 {
        (win_w, mh, mx, my)
    } else if landscape {
        // Landscape, not fullscreen: content-fit estimate (the per-frame fit
        // then refines it), same as the portrait branch below.
        let h = compute_landscape_window_height(visible_panels, win_w, win_h);
        (win_w, h, mx, my)
    } else {
        let h = compute_window_height(visible_panels, profile_scale(&s.dashboard_profile));
        (win_w - 2.0, h, mx, my)
    };
    // Pinned dashboard: restore the saved position for this profile (keeping the
    // computed size) instead of auto-targeting a monitor, when still on-screen.
    if !s.floating_mode && s.dashboard_pinned {
        let saved = s.pinned_positions.get(&s.dashboard_profile).copied();
        #[cfg(windows)]
        let monitors = win_monitor::list();
        #[cfg(not(windows))]
        let monitors: Vec<(i32, i32, i32, i32)> = Vec::new();
        if let Some([x, y]) = resolve_pinned_position(true, saved, &monitors) {
            pos_x = x;
            pos_y = y;
        }
    }
    ([inner_w, inner_h], [pos_x, pos_y])
}

/// Probe for a DX12 adapter before committing to a DComp-backed swap chain.
/// On hardware/drivers without DX12 (rare, but real — some VMs/RDP sessions
/// without GPU passthrough, very old GPUs), forcing `Backends::DX12` would
/// otherwise make eframe::run_native fail and the app would refuse to start
/// at all. `enumerate_adapters` is a cheap, synchronous-enough capability
/// check (blocked on via `runtime`) run once at startup — no
/// adapter/device/surface is actually created by it.
pub(crate) fn probe_dcomp(runtime: &tokio::runtime::Runtime) -> bool {
    runtime.block_on(async {
        let probe_instance = eframe::wgpu::Instance::new(eframe::wgpu::InstanceDescriptor {
            backends: eframe::wgpu::Backends::DX12,
            ..eframe::wgpu::InstanceDescriptor::new_without_display_handle()
        });
        !probe_instance
            .enumerate_adapters(eframe::wgpu::Backends::DX12)
            .await
            .is_empty()
    })
}

/// The main window's viewport and wgpu setup: a DirectComposition swap
/// chain with per-pixel alpha when a DX12 adapter exists, otherwise the
/// default opaque one.
pub(crate) fn native_options(
    s: &Settings,
    [inner_w, inner_h]: [f32; 2],
    [pos_x, pos_y]: [f32; 2],
    dcomp_available: bool,
) -> eframe::NativeOptions {
    let always_on_top = s.window_layer == "on_top";
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("RigStats")
        .with_icon(load_app_icon())
        .with_inner_size([inner_w, inner_h])
        .with_position([pos_x, pos_y])
        .with_decorations(false)
        .with_taskbar(false); // app is tray-only, never show in taskbar
    if always_on_top {
        viewport = viewport.with_always_on_top();
    }
    if s.floating_mode {
        viewport = viewport.with_taskbar(false);
    }

    let wgpu_options = if dcomp_available {
        // Needed for egui-wgpu's alpha-mode picker to select a transparent
        // CompositeAlphaMode; the actual per-pixel compositing is provided by
        // the DComp swap chain configured below plus
        // `win_opacity::set_no_redirection_bitmap` (issue #101, using the
        // technique proven in #131/#168). Floating panels and secondary
        // dialog viewports (Settings/About/Status/Updater) deliberately do
        // NOT set this, so they stay fully opaque.
        viewport = viewport.with_transparent(true);
        // Force the DX12 backend with a DirectComposition-backed swap chain
        // (`DxgiFromVisual`), giving the main window real per-pixel alpha
        // instead of DWM's normal opaque flip-model swap chain — see issue
        // #101 and `docs/architecture.md`.
        eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(
                eframe::egui_wgpu::WgpuSetupCreateNew {
                    instance_descriptor: eframe::wgpu::InstanceDescriptor {
                        backends: eframe::wgpu::Backends::DX12,
                        backend_options: eframe::wgpu::BackendOptions {
                            dx12: eframe::wgpu::Dx12BackendOptions {
                                presentation_system:
                                    eframe::wgpu::Dx12SwapchainKind::DxgiFromVisual,
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                        ..eframe::wgpu::InstanceDescriptor::new_without_display_handle()
                    },
                    ..eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle()
                },
            ),
            ..Default::default()
        }
    } else {
        // No DX12 adapter — default backend selection, opaque swap chain.
        // Opacity falls back to WS_EX_LAYERED (see `RigStatsApp::clear_color`
        // and the `win_opacity::set_opacity` call sites gated on this flag).
        eframe::egui_wgpu::WgpuConfiguration::default()
    };

    eframe::NativeOptions {
        viewport,
        wgpu_options,
        ..Default::default()
    }
}
