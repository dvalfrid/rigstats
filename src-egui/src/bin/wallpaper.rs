//! `rigstats-wallpaper` — the WorkerW desktop-wallpaper host process.
//!
//! Spawned and supervised by the main `rigstats` app when the user selects the
//! "Desktop Wallpaper" window layer. It is a minimal eframe app: a single
//! borderless window that fills the target monitor, renders the dashboard via
//! [`rigstats_egui::dashboard::DashboardView`], and reparents itself into the
//! desktop `WorkerW` layer. Kept as a separate process so that an Explorer
//! restart (which destroys the WorkerW hierarchy and any child window) cannot
//! take down the main app — the supervisor relaunches the host instead. While
//! active the host is the telemetry owner: it runs `poll_loop`, owns the sensor
//! pipe, and does CSV logging (the main app stops polling in wallpaper mode, so
//! the single-client pipe is never contended). See `docs/architecture.md`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use rigstats_backend::{debug, settings};
use rigstats_egui::dashboard::DashboardRuntime;
use rigstats_egui::geometry::{
    compute_landscape_window_height, compute_window_height, guard_panel_position,
    pick_window_rect_for_profile, profile_is_landscape, profile_scale, profile_to_size,
};
use rigstats_egui::poll::poll_loop;
use rigstats_egui::{theme, PollStats};
#[cfg(windows)]
use rigstats_egui::{win32_wallpaper, win_opacity};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// Distinct window title so the Win32 lookups target the host, never the main
/// `RigStats` window.
const TITLE: &str = "RIGStats Wallpaper";

fn app_data_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(appdata).join("se.codeby.rigstats")
}

struct WallpaperHost {
    runtime: DashboardRuntime,
    receiver: mpsc::Receiver<PollStats>,
    profile: String,
    opacity: f32,
    dir: PathBuf,
    /// Win32 HWND (isize) for opacity; 0 until found on the first frame.
    hwnd: isize,
    /// PID of the supervising main app; the host exits if it disappears so it
    /// never lingers as an orphan. `None` if launched standalone (dev/testing).
    parent_pid: Option<u32>,
    last_settings_check: Instant,
    last_attach_check: Instant,
    last_parent_check: Instant,
    /// Last window height we fit to the portrait content, so we only re-dispatch
    /// `InnerSize` when the rendered content height actually changes. `None`
    /// forces a fit on the next frame (startup, or after a panel-set change).
    last_fitted_height: Option<f32>,
    /// Re-fit for a few frames after launch: early `InnerSize` commands can be
    /// dropped before the window is fully realized, which would leave a black gap
    /// or a clipped bottom panel until something else triggers a resize.
    startup_fit_frames: u8,
}

impl WallpaperHost {
    /// Reload settings from disk and apply live changes. Returns `true` when the
    /// host should shut down (the profile changed → relaunch for the new size, or
    /// the user left wallpaper mode). The caller closes the window via
    /// `ViewportCommand::Close` so eframe/wgpu tears down cleanly — a plain
    /// `process::exit` would skip the GPU device teardown and leak graphics
    /// resources across spawn cycles (the cause of later DLL-init failures).
    #[must_use]
    fn refresh_settings(&mut self) -> bool {
        let s = settings::load_settings(&self.dir);
        // Left wallpaper mode (toggled floating on, or switched window layer):
        // the supervisor expects us to exit so it can take the dashboard back.
        if s.floating_mode || s.window_layer != "wallpaper" {
            debug::append_debug_log(
                &self.dir,
                "rigstats-wallpaper: wallpaper mode left, closing",
            );
            return true;
        }
        if s.dashboard_profile != self.profile {
            debug::log_debug(
                &self.dir,
                &format!(
                    "rigstats-wallpaper: profile changed {} → {}, closing for relaunch",
                    self.profile, s.dashboard_profile
                ),
            );
            return true;
        }
        if self.runtime.apply_settings(&s) {
            // Panel set changed → content height changed; force a re-fit.
            self.last_fitted_height = None;
        }
        let new_op = s.opacity.clamp(0.1, 1.0) as f32;
        if (new_op - self.opacity).abs() > f32::EPSILON {
            self.opacity = new_op;
            #[cfg(windows)]
            win_opacity::set_opacity(self.hwnd, self.opacity);
        }
        false
    }
}

impl eframe::App for WallpaperHost {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Solid dashboard background; window-level opacity is applied separately
        // via SetLayeredWindowAttributes so the swap chain stays opaque.
        let c = theme::PANEL_FILL;
        [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            1.0,
        ]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Exit if the supervising main app has gone (covers the case where it was
        // killed without cleanly stopping us).
        #[cfg(windows)]
        if let Some(pid) = self.parent_pid {
            if self.last_parent_check.elapsed() >= Duration::from_secs(2) {
                self.last_parent_check = Instant::now();
                if !win32_wallpaper::process_alive(pid) {
                    debug::append_debug_log(&self.dir, "rigstats-wallpaper: parent gone, exiting");
                    std::process::exit(0);
                }
            }
        }

        // First frame: locate the HWND (while still a top-level window — once
        // reparented, FindWindow can no longer find it), cache it, apply opacity
        // (WS_EX_LAYERED must be added while the window is still top-level; a child
        // window rejects the layered style), then attach to WorkerW.
        #[cfg(windows)]
        if self.hwnd == 0 {
            self.hwnd = win_opacity::find_hwnd(TITLE);
            if self.hwnd != 0 {
                win_opacity::set_opacity(self.hwnd, self.opacity);
                if win32_wallpaper::attach(self.hwnd) {
                    debug::append_debug_log(&self.dir, "rigstats-wallpaper: attached to WorkerW");
                } else {
                    debug::log_warn(&self.dir, "rigstats-wallpaper: WorkerW attach failed");
                }
            }
        }

        // Drain new stats into the sparklines.
        self.runtime.drain(&self.receiver);

        // Live settings refresh (~1 Hz). A true return means we should shut down
        // (profile changed, or wallpaper mode was left) — close cleanly so wgpu
        // tears down properly instead of leaking via an abrupt process exit.
        if self.last_settings_check.elapsed() >= Duration::from_millis(900) {
            self.last_settings_check = Instant::now();
            if self.refresh_settings() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }

        // Re-attach safety net (~1 Hz): if Explorer rebuilt the WorkerW hierarchy
        // underneath us, the window survives but is detached — snap it back.
        #[cfg(windows)]
        if self.last_attach_check.elapsed() >= Duration::from_secs(1) {
            self.last_attach_check = Instant::now();
            if self.hwnd != 0 && !win32_wallpaper::is_attached(self.hwnd) {
                win32_wallpaper::attach(self.hwnd);
            }
        }

        // Render the dashboard filling the window (no drag handle in wallpaper
        // mode — it is display-only). There is no Fill Screen toggle in wallpaper
        // mode (the whole point is a small widget over the real desktop), so the
        // landscape grid always uses natural, non-stretched cell sizes just like
        // the portrait stack — both fit the window to their actual content below.
        let panels = self.runtime.visible_panels.clone();
        if profile_is_landscape(&self.profile) {
            let ref_h = profile_to_size(&self.profile)[1];
            self.runtime
                .view()
                .render_landscape_grid(ui, &panels, None, false, ref_h);
        } else {
            let sc = profile_scale(&self.profile);
            for panel in &panels {
                self.runtime.view().draw_one_panel(ui, panel, sc, None);
                ui.add_space((6.0 * sc).round());
            }
        }

        // Fit the window height to the rendered content so no black gap appears
        // below it (the profile height is the full monitor span; the actual
        // content is usually shorter). Mirrors the main app's per-frame fit.
        let used_h = ui.min_rect().height();
        if self.startup_fit_frames > 0 {
            self.startup_fit_frames -= 1;
            self.last_fitted_height = None;
            ui.ctx().request_repaint();
        }
        let changed = self
            .last_fitted_height
            .map(|h| (h - used_h).abs() > 0.5)
            .unwrap_or(true);
        if used_h > 10.0 && changed {
            self.last_fitted_height = Some(used_h);
            let [w, _] = profile_to_size(&self.profile);
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, used_h)));
        }

        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

fn main() {
    let dir = app_data_dir();
    debug::append_debug_log(&dir, "rigstats-wallpaper starting");

    let parent_pid: Option<u32> = std::env::var("RIGSTATS_PARENT_PID")
        .ok()
        .and_then(|v| v.parse().ok());

    let s = settings::load_settings(&dir);
    let profile = s.dashboard_profile.clone();
    let opacity = s.opacity.clamp(0.1, 1.0) as f32;

    // Size the window to fit the dashboard content (the per-frame fit in `ui()`
    // then refines this estimate): the landscape grid uses natural cell sizes,
    // the portrait stack its panel-stack height. Never stretch to the full
    // profile height — that leaves a black gap below the content. Width is
    // always the profile width so panel proportions are preserved. Use the
    // position the user set in a normal layer before switching
    // (s.wallpaper_position) when it is still on a connected monitor; otherwise
    // centre on the matching monitor. The wallpaper shows around it.
    let [pw, ph] = profile_to_size(&profile);
    let size = if profile_is_landscape(&profile) {
        [
            pw,
            compute_landscape_window_height(&s.visible_panels, pw, ph),
        ]
    } else {
        [
            pw,
            compute_window_height(&s.visible_panels, profile_scale(&profile)),
        ]
    };
    let [mx, my, mw, mh] = pick_window_rect_for_profile(&profile);
    let centered = if mw > 0.0 && mh > 0.0 {
        [mx + (mw - size[0]) * 0.5, my + (mh - size[1]) * 0.5]
    } else {
        [0.0, 0.0]
    };
    let pos = match s.wallpaper_position {
        Some(p) => guard_panel_position([p[0] as f32, p[1] as f32], centered),
        None => centered,
    };

    debug::log_debug(
        &dir,
        &format!(
            "wallpaper host: profile={profile} pos=({:.0},{:.0}) size=({:.0}x{:.0})",
            pos[0], pos[1], size[0], size[1]
        ),
    );

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let preferred_gpu_arc: Arc<Mutex<Option<String>>> =
        Arc::new(Mutex::new(s.preferred_gpu.clone()));
    let settings_shared = Arc::new(Mutex::new(settings::load_settings(&dir)));
    let (tx, rx) = mpsc::sync_channel::<PollStats>(4);
    let dir_poll = dir.clone();
    let pref_poll = preferred_gpu_arc.clone();
    let settings_poll = settings_shared.clone();
    // The host is the active poller — never paused.
    let never_paused = Arc::new(AtomicBool::new(false));
    runtime.spawn(
        async move { poll_loop(tx, dir_poll, pref_poll, settings_poll, never_paused).await },
    );

    let viewport = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_inner_size(size)
        .with_position(pos)
        .with_decorations(false)
        .with_taskbar(false)
        .with_window_level(egui::WindowLevel::Normal);

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    // `dir` is moved into the eframe closure (the host owns it); keep a copy for
    // logging the run result after run_native returns.
    let log_dir = dir.clone();

    let run_result = eframe::run_native(
        TITLE,
        options,
        Box::new(move |cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = egui::Color32::TRANSPARENT;
            visuals.window_fill = egui::Color32::from_gray(28);
            visuals.override_text_color = Some(theme::C_TEXT);
            cc.egui_ctx.set_visuals(visuals);
            theme::apply_dashboard_fonts(&cc.egui_ctx);

            // Drive ~1 Hz repaints from a heartbeat thread. `request_repaint_after`
            // alone does not reliably wake the window once it has been reparented
            // into WorkerW (a child window receives no OS-driven paint/timer
            // events), so without this the dashboard freezes on its first frame.
            // A cross-thread `request_repaint` wakes the event loop regardless.
            let hb_ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(1000));
                hb_ctx.request_repaint();
            });

            let host = WallpaperHost {
                runtime: DashboardRuntime::new(&cc.egui_ctx, &s),
                receiver: rx,
                profile,
                opacity,
                dir,
                hwnd: 0,
                parent_pid,
                last_settings_check: Instant::now(),
                last_attach_check: Instant::now(),
                last_parent_check: Instant::now(),
                last_fitted_height: None,
                startup_fit_frames: 8,
            };
            Ok(Box::new(host))
        }),
    );

    // Log an eframe/wgpu init or run failure so a host that reached main() but
    // could not create its window leaves a trace in a submitted diagnostic (a
    // loader-level failure like 0xC0000142 never gets here — that is recorded by
    // the supervisor's exit-code log instead).
    if let Err(e) = run_result {
        debug::log_error(
            &log_dir,
            &format!("rigstats-wallpaper: run_native failed — {e}"),
        );
    } else {
        debug::append_debug_log(&log_dir, "rigstats-wallpaper exiting");
    }

    // Hold the runtime until run_native returns (window closed).
    drop(runtime);
}
