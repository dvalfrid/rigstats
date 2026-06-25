#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use eframe::egui;
use rigstats_backend::{debug, logging, settings};
use rigstats_egui::dashboard::{DashboardView, PanelThresholds};
#[cfg(windows)]
use rigstats_egui::geometry::win_monitor;
use rigstats_egui::geometry::{
    compute_window_height, dialog_center, guard_panel_position, monitor_rect_at,
    pick_window_rect_for_profile, profile_is_landscape, profile_scale, profile_to_size,
    resolve_pinned_position,
};
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::poll::poll_loop;
use rigstats_egui::spark::Sparkline;
use rigstats_egui::tray::{build_tray, load_app_icon, panel_initial_h, panel_label, Tray, TrayCmd};
use rigstats_egui::{brand, panels, theme, update_check, windows, PollStats};
#[cfg(windows)]
use rigstats_egui::{win32_behind, win32_dark_mode, win_opacity};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tray_icon::menu::MenuEvent;

fn app_data_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(appdata).join("se.codeby.rigstats")
}

// ── eframe application ────────────────────────────────────────────────────────

struct RigStatsApp {
    receiver: mpsc::Receiver<PollStats>,
    tray_rx: mpsc::Receiver<TrayCmd>,
    latest: PollStats,
    visible_panels: Vec<String>,
    opacity: f32,
    /// Cached from settings — "on_top", "behind", or "normal".
    window_layer: String,
    tray: Tray,
    // Sparklines
    cpu_spark: Sparkline,
    gpu_spark: Sparkline,
    net_up_spark: Sparkline,
    net_dn_spark: Sparkline,
    // Brand textures (loaded once at startup)
    textures: brand::Textures,
    // Secondary windows
    settings_open: Arc<AtomicBool>,
    about_open: Arc<AtomicBool>,
    status_open: Arc<AtomicBool>,
    updater_open: Arc<AtomicBool>,
    // Set to true when a dialog is opened; cleared on first callback frame to send Focus.
    settings_focus: Arc<AtomicBool>,
    about_focus: Arc<AtomicBool>,
    status_focus: Arc<AtomicBool>,
    updater_focus: Arc<AtomicBool>,
    settings_win: Arc<Mutex<windows::settings::SettingsWindow>>,
    status_win: Arc<Mutex<windows::status::StatusState>>,
    status_refreshing: Arc<AtomicBool>,
    status_collecting: Arc<AtomicBool>,
    updater_win: Arc<Mutex<windows::updater::UpdaterState>>,
    // true while a manual check/download is in flight (prevents double-trigger)
    updater_busy: Arc<AtomicBool>,
    // Shared settings (updated on save, applied each frame)
    current_settings: Arc<Mutex<settings::Settings>>,
    settings_reload: Arc<AtomicBool>,
    preferred_gpu: Arc<Mutex<Option<String>>>,
    dir: Arc<PathBuf>,
    // Win32 HWND stored as isize for window-level opacity (SetLayeredWindowAttributes).
    // Found via FindWindowW on the first ui() frame; 0 until then.
    hwnd: isize,
    // ── Fullscreen (fill-screen) mode ──────────────────────────────────────
    /// When true (and not floating), the fixed window fills the whole monitor
    /// instead of fitting panel content; the dashboard background fills the rest.
    fullscreen_mode: bool,
    /// Vertical placement of the panel stack when fullscreen: `"top"` | `"center"`.
    fullscreen_align: String,
    /// Measured panel-stack content height (excluding drag handle + centering pad),
    /// cached from the previous frame so fullscreen centering is exact. `None`
    /// until the first fullscreen frame; `compute_window_height` is the fallback.
    fullscreen_content_h: Option<f32>,
    // ── Pinned (non-floating) dashboard ────────────────────────────────────
    /// When true, the fixed-mode dashboard window is pinned: it cannot be dragged
    /// and its position is restored from `Settings::pinned_positions` across
    /// restarts instead of auto-targeting the matching monitor.
    dashboard_pinned: bool,
    /// Last outer position observed for the fixed-mode window this session, used
    /// to capture the spot to pin when the padlock is clicked. `None` until the
    /// first fixed-mode frame reports a position.
    last_fixed_pos: Option<[f32; 2]>,
    // ── Floating mode ──────────────────────────────────────────────────────
    floating_mode: bool,
    floating_panels_locked: bool,
    floating_panel_scale: f32,
    /// Last-known screen positions for each panel key, keyed by panel key.
    /// Loaded from settings at startup; updated on drag; persisted on change.
    floating_positions: Arc<Mutex<HashMap<String, [f32; 2]>>>,
    /// Set true inside a floating panel viewport when its position changes.
    /// Consumed in `ui()` to debounce settings writes to once per tick.
    positions_dirty: Arc<AtomicBool>,
    /// Receives a new preferred-GPU name when the user clicks a GPU dot
    /// inside the floating GPU panel viewport.
    float_new_pref_gpu: Arc<Mutex<Option<String>>>,
    /// Live lock state toggled from the padlock icon in the drag handle.
    /// Propagated back to `floating_panels_locked` and persisted in `update()`.
    floating_lock_arc: Arc<AtomicBool>,
    /// Guards the one-time initial hide of the main window when the app
    /// starts with floating_mode already enabled.
    initial_floating_applied: bool,
    /// Tracks which floating panel viewports have already had their initial
    /// position applied.  Once a panel is in this set, `with_position` is
    /// NOT included in the ViewportBuilder — the OS owns the position from
    /// that point on, which prevents the builder diff from continuously
    /// sending SetOuterPosition and causing sub-pixel blur.
    /// Cleared whenever floating mode transitions from off → on so positions
    /// are restored from the saved layout on next activation.
    panels_positioned: HashSet<String>,
    /// Per-panel "always behind" enforcement state (key → last enforce time +
    /// previous primary-button state). Used to throttle the Win32 Z-order
    /// re-push so floating "behind" panels don't re-assert every frame — which
    /// would create a SetWindowPos → repaint → SetWindowPos spin loop and burn
    /// CPU. Enforcement happens on creation, in a short burst after a drag, and
    /// then ~1/s as an idle safety net.
    behind_enforce: RefCell<HashMap<String, BehindEnforce>>,
    /// Shared with the heartbeat thread so it knows whether to drive parent repaints.
    floating_mode_arc: Arc<AtomicBool>,
    /// Live thresholds for temperature colour coding — updated on settings reload.
    thresholds: PanelThresholds,
    /// User-specified PSU wattage for the System Power panel bar scale.
    psu_watts: Option<u16>,
    /// Active panel theme derived from `Settings.theme`.
    app_theme: theme::AppTheme,
    /// Colour palette for dialog windows — switches between dark/light based on OS theme.
    dialog_colors: theme::DialogColors,
    /// Cached OS dark-mode flag; checked each frame to detect live theme switches.
    os_dark_mode: bool,
    /// Whether any dialog was open last frame; used to restore dark visuals when the
    /// last dialog closes (avoids calling set_visuals every frame).
    any_dialog_open_prev: bool,
    /// When > 0, re-applies WindowLevel + opacity for this many more frames.
    /// Used after floating→non-floating transitions where winit may reset the
    /// window level when the window is moved back on-screen.
    reapply_window_props_frames: u8,
    /// Last [w, h] sent via InnerSize — avoids spurious resize events when
    /// only opacity or theme changed (which would cause a visible jump).
    last_applied_window_size: Option<[f32; 2]>,
    /// Last content height fitted in fixed mode — avoids dispatching an
    /// InnerSize viewport command every frame when the height is unchanged
    /// (which during interaction runs at display refresh rate, causing
    /// needless WM_SIZE churn and sub-pixel jitter).
    last_fitted_height: Option<f32>,
    /// Counts down over the first docked frames after launch. Early `InnerSize`
    /// viewport commands can be dropped before the window is fully realized,
    /// which leaves the bottom panel clipped until the user toggles floating
    /// mode. While this is > 0 we force the fit-to-content path to re-snap (and
    /// drive fast repaints) so the true content height reliably sticks.
    startup_fit_frames: u8,
    // ── Wallpaper (WorkerW) mode ───────────────────────────────────────────
    /// Shared with `poll_loop`: set while in wallpaper mode so this app stops
    /// polling and releases the sensor pipe to the `rigstats-wallpaper` host.
    poll_paused: Arc<AtomicBool>,
    /// Handle to the spawned `rigstats-wallpaper` host process, if running.
    wallpaper_child: Option<std::process::Child>,
    /// True while wallpaper mode is active; used to detect enter/leave transitions.
    wallpaper_active: bool,
    /// When the current host was last spawned. Used to tell a fast-failing host
    /// (dies within seconds → back off) from a long-lived one that died for an
    /// external reason (e.g. an Explorer restart → respawn promptly).
    wallpaper_last_spawn: Option<Instant>,
    /// Consecutive fast spawn failures. Drives exponential respawn backoff so a
    /// host that cannot start (e.g. transient GPU/DLL-init failure) never storms
    /// the supervisor into respawning every frame. Reset on entering the mode.
    wallpaper_spawn_fails: u32,
    /// Set when leaving wallpaper mode while the host is still alive: the host
    /// self-exits cleanly from disk state (so wgpu tears down properly instead of
    /// being TerminateProcess-d), and this deadline triggers a `kill()` fallback
    /// if it has not exited in time.
    wallpaper_teardown_at: Option<Instant>,
}

/// Per-panel state for throttling "always behind" Z-order enforcement.
struct BehindEnforce {
    /// When the panel was last pushed to the bottom of the Z-order.
    last_enforce: Instant,
    /// Primary-button state on the previous frame — used to detect drag release.
    prev_primary_down: bool,
    /// While `Instant::now() < force_until`, enforce every frame (short burst
    /// after a drag finishes so the panel snaps back behind promptly).
    force_until: Instant,
}

impl RigStatsApp {
    #[allow(clippy::too_many_arguments)]
    fn new(
        receiver: mpsc::Receiver<PollStats>,
        tray_rx: mpsc::Receiver<TrayCmd>,
        visible_panels: Vec<String>,
        opacity: f32,
        tray: Tray,
        current_settings: Arc<Mutex<settings::Settings>>,
        settings_reload: Arc<AtomicBool>,
        dir: Arc<PathBuf>,
        textures: brand::Textures,
        preferred_gpu: Arc<Mutex<Option<String>>>,
        floating_mode_arc: Arc<AtomicBool>,
        updater_win: Arc<Mutex<windows::updater::UpdaterState>>,
        updater_open: Arc<AtomicBool>,
        updater_focus: Arc<AtomicBool>,
        poll_paused: Arc<AtomicBool>,
    ) -> Self {
        let init_settings = current_settings.lock_safe().clone();
        let init_positions: HashMap<String, [f32; 2]> = init_settings
            .panel_layouts
            .iter()
            .map(|(k, v)| (k.clone(), [v.x as f32, v.y as f32]))
            .collect();
        Self {
            receiver,
            tray_rx,
            latest: PollStats::default(),
            visible_panels,
            opacity,
            window_layer: init_settings.window_layer.clone(),
            tray,
            cpu_spark: Sparkline::new(60),
            gpu_spark: Sparkline::new(60),
            net_up_spark: Sparkline::new(60),
            net_dn_spark: Sparkline::new(60),
            textures,
            settings_open: Arc::new(AtomicBool::new(false)),
            about_open: Arc::new(AtomicBool::new(false)),
            status_open: Arc::new(AtomicBool::new(false)),
            updater_open,
            settings_focus: Arc::new(AtomicBool::new(false)),
            about_focus: Arc::new(AtomicBool::new(false)),
            status_focus: Arc::new(AtomicBool::new(false)),
            updater_focus,
            settings_win: Arc::new(Mutex::new(
                windows::settings::SettingsWindow::from_settings(&init_settings),
            )),
            status_win: Arc::new(Mutex::new(windows::status::StatusState::placeholder())),
            status_refreshing: Arc::new(AtomicBool::new(false)),
            status_collecting: Arc::new(AtomicBool::new(false)),
            updater_win,
            updater_busy: Arc::new(AtomicBool::new(false)),
            current_settings,
            settings_reload,
            preferred_gpu,
            dir,
            hwnd: 0,
            fullscreen_mode: init_settings.fullscreen_mode,
            fullscreen_align: init_settings.fullscreen_align.clone(),
            fullscreen_content_h: None,
            dashboard_pinned: init_settings.dashboard_pinned,
            last_fixed_pos: None,
            floating_mode: init_settings.floating_mode,
            floating_panels_locked: init_settings.floating_panels_locked,
            floating_panel_scale: init_settings.floating_panel_scale.clamp(0.4, 1.0) as f32,
            floating_positions: Arc::new(Mutex::new(init_positions)),
            positions_dirty: Arc::new(AtomicBool::new(false)),
            float_new_pref_gpu: Arc::new(Mutex::new(None)),
            floating_lock_arc: Arc::new(AtomicBool::new(init_settings.floating_panels_locked)),
            initial_floating_applied: false,
            panels_positioned: HashSet::new(),
            behind_enforce: RefCell::new(HashMap::new()),
            floating_mode_arc,
            thresholds: PanelThresholds::from_settings(&init_settings),
            psu_watts: init_settings.psu_watts,
            app_theme: theme::AppTheme::from_key(&init_settings.theme),
            os_dark_mode: {
                #[cfg(windows)]
                {
                    win32_dark_mode::is_system_dark_mode()
                }
                #[cfg(not(windows))]
                {
                    true
                }
            },
            dialog_colors: {
                #[cfg(windows)]
                {
                    if win32_dark_mode::is_system_dark_mode() {
                        theme::DialogColors::dark()
                    } else {
                        theme::DialogColors::light()
                    }
                }
                #[cfg(not(windows))]
                {
                    theme::DialogColors::dark()
                }
            },
            any_dialog_open_prev: false,
            // Apply WindowLevel + opacity for the first few frames at startup when in
            // non-floating mode: the viewport builder only handles "on_top", so "behind"
            // must be sent via viewport command, and opacity needs a valid HWND which
            // may not be available until after the first paint.
            reapply_window_props_frames: if !init_settings.floating_mode { 4 } else { 0 },
            last_applied_window_size: None,
            last_fitted_height: None,
            startup_fit_frames: 12,
            poll_paused,
            wallpaper_child: None,
            wallpaper_active: false,
            wallpaper_last_spawn: None,
            wallpaper_spawn_fails: 0,
            wallpaper_teardown_at: None,
        }
    }

    /// Absolute path to the sibling `rigstats-wallpaper.exe`, or `None` if the
    /// running exe's own path can't be resolved.
    #[cfg(windows)]
    fn wallpaper_host_path() -> Option<std::path::PathBuf> {
        std::env::current_exe()
            .ok()
            .map(|exe| exe.with_file_name("rigstats-wallpaper.exe"))
    }

    /// Spawn the `rigstats-wallpaper` host process (sibling exe), passing this
    /// process's PID so the host exits if the main app goes away.
    #[cfg(windows)]
    fn spawn_wallpaper_host() -> std::io::Result<std::process::Child> {
        let host = Self::wallpaper_host_path().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "could not resolve current exe path",
            )
        })?;
        std::process::Command::new(host)
            .env("RIGSTATS_PARENT_PID", std::process::id().to_string())
            .spawn()
    }

    /// Restore the main dashboard window on-screen after wallpaper mode (either
    /// on leaving the mode or when entering was aborted because the host binary
    /// is missing). Places it at the saved `wallpaper_position` when that is
    /// still on a connected monitor, else the profile-matching monitor.
    #[cfg(windows)]
    fn restore_main_window(&mut self, ctx: &egui::Context) {
        let profile = self.current_settings.lock_safe().dashboard_profile.clone();
        let ([mut px, mut py], [w, h]) = self.fixed_window_geometry(&profile);
        if let Some(p) = self.current_settings.lock_safe().wallpaper_position {
            let [gx, gy] = guard_panel_position([p[0] as f32, p[1] as f32], [px, py]);
            px = gx;
            py = gy;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            Self::window_level_from_layer(&self.window_layer),
        ));
        win_opacity::set_opacity(self.hwnd, self.opacity);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
            px, py,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
        self.last_fitted_height = None;
        self.reapply_window_props_frames = 4;
    }

    /// Drive the wallpaper-host lifecycle each frame.
    ///
    /// Wallpaper mode is active when `window_layer == "wallpaper"` and floating
    /// mode is off (floating wins if both are set). On entering: park our own
    /// window off-screen and pause polling so the host owns the dashboard and the
    /// sensor pipe. While active: (re)spawn the host if it has exited (e.g. an
    /// Explorer restart destroyed its WorkerW-child window). On leaving: kill the
    /// host, resume polling, and restore our window.
    #[cfg(windows)]
    fn update_wallpaper_mode(&mut self, ctx: &egui::Context) {
        let want = self.window_layer == "wallpaper" && !self.floating_mode;
        if want && !self.wallpaper_active {
            // Guard: the host is a sibling exe (`rigstats-wallpaper.exe`). If it is
            // missing (a packaging regression — e.g. the installer not bundling it),
            // do NOT enter wallpaper mode: parking the main window off-screen while
            // no host appears would hide the dashboard entirely. Revert to the
            // normal layer, restore the window on-screen (startup may have already
            // parked it), and log clearly so a diagnostic shows the cause.
            let host_missing = Self::wallpaper_host_path()
                .map(|p| !p.exists())
                .unwrap_or(true);
            if host_missing {
                debug::log_error(
                    &self.dir,
                    "wallpaper: rigstats-wallpaper.exe not found next to the app — \
                     staying in normal mode (reinstall/update to restore wallpaper mode)",
                );
                self.window_layer = "normal".to_string();
                {
                    let mut s = self.current_settings.lock_safe();
                    s.window_layer = "normal".to_string();
                    self.persist_settings_logged(&s);
                }
                self.restore_main_window(ctx);
                return;
            }
            self.wallpaper_active = true;
            self.poll_paused.store(true, Ordering::Relaxed);
            // Reset the respawn throttle for a fresh session, and force-reap any
            // host still draining from a just-left session so we never end up with
            // two hosts when the user toggles back in quickly.
            self.wallpaper_spawn_fails = 0;
            self.wallpaper_last_spawn = None;
            self.wallpaper_teardown_at = None;
            if let Some(mut child) = self.wallpaper_child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            // Capture where the user left the window in the previous (on-screen)
            // layer and hand it to the host, so the workflow is: position the
            // window where you want it in Normal mode, then switch to wallpaper.
            // Persisted before the host is spawned below so it reads the value.
            if let Some([x, y]) = self.last_fixed_pos {
                let mut s = self.current_settings.lock_safe();
                s.wallpaper_position = Some([x.round() as i32, y.round() as i32]);
                self.persist_settings_logged(&s);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                -32000.0, -32000.0,
            )));
            debug::log_debug(&self.dir, "wallpaper: entering — parking main window");
        } else if !want && self.wallpaper_active {
            self.wallpaper_active = false;
            self.poll_paused.store(false, Ordering::Relaxed);
            // Graceful teardown: the floating/layer flag was already persisted to
            // disk by the caller, so the host self-exits cleanly within ~1 s (its
            // wgpu device tears down properly). We DO NOT TerminateProcess it here
            // — repeatedly killing the GPU host leaked graphics/desktop-heap
            // resources until a later spawn failed at DLL-init (0xc0000142) and the
            // supervisor respawned it every frame. Hand the child to the drain
            // block below, which reaps it and only kill()s as a timed fallback.
            if self.wallpaper_child.is_some() {
                self.wallpaper_teardown_at = Some(Instant::now());
            }
            self.restore_main_window(ctx);
            debug::log_debug(&self.dir, "wallpaper: leaving — restoring main window");
        }

        // Drain a host that is shutting down after leaving wallpaper mode: reap it
        // once it has self-exited; kill() only as a fallback if it overruns the
        // grace period. Runs while not active so a re-entered mode starts clean.
        if !self.wallpaper_active {
            if let Some(deadline) = self.wallpaper_teardown_at {
                let exited = match self.wallpaper_child.as_mut() {
                    Some(child) => matches!(child.try_wait(), Ok(Some(_)) | Err(_)),
                    None => true,
                };
                if exited {
                    self.wallpaper_child = None;
                    self.wallpaper_teardown_at = None;
                    self.wallpaper_last_spawn = None;
                    debug::log_debug(&self.dir, "wallpaper: host exited cleanly");
                } else if deadline.elapsed() >= Duration::from_secs(3) {
                    if let Some(mut child) = self.wallpaper_child.take() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    self.wallpaper_teardown_at = None;
                    self.wallpaper_last_spawn = None;
                    debug::log_warn(&self.dir, "wallpaper: host did not exit in time, killed");
                }
            }
        }

        if self.wallpaper_active {
            // Detect whether the host needs (re)spawning, and whether the previous
            // instance died quickly (a failure to back off from) or after running
            // for a while (a legitimate loss, e.g. an Explorer restart). Capture the
            // exit code too: a loader failure such as 0xC0000142
            // (STATUS_DLL_INIT_FAILED) kills the host before its main() runs, so the
            // host can never log the cause itself — the supervisor's record of the
            // exit code is the only diagnostic trace that reaches a submitted log.
            let (exited, exit_code): (bool, Option<i32>) = match self.wallpaper_child.as_mut() {
                None => (true, None),
                Some(child) => match child.try_wait() {
                    Ok(Some(status)) => (true, status.code()),
                    Ok(None) => (false, None),
                    Err(_) => (true, None),
                },
            };
            if exited {
                if self.wallpaper_child.take().is_some() {
                    // The host was running and has now exited. If it died within a
                    // few seconds of spawning, treat it as a fast failure and back
                    // off; if it lived longer, reset the failure counter.
                    let fast_fail = self
                        .wallpaper_last_spawn
                        .map(|t| t.elapsed() < Duration::from_secs(4))
                        .unwrap_or(true);
                    let code_desc = match exit_code {
                        Some(c) => format!("exit code {c} (0x{:08X})", c as u32),
                        None => "no exit code".to_string(),
                    };
                    if fast_fail {
                        self.wallpaper_spawn_fails = self.wallpaper_spawn_fails.saturating_add(1);
                        debug::log_warn(
                            &self.dir,
                            &format!(
                                "wallpaper: host exited within 4s of spawn ({code_desc}); \
                                 consecutive fast failures: {}",
                                self.wallpaper_spawn_fails
                            ),
                        );
                    } else {
                        self.wallpaper_spawn_fails = 0;
                        debug::log_debug(
                            &self.dir,
                            &format!(
                                "wallpaper: host exited after running ({code_desc}); respawning"
                            ),
                        );
                    }
                }
                // Exponential backoff after a fast failure: 0 s, 2 s, 4 s, 8 s …
                // capped at 30 s. Stop entirely after 5 consecutive failures so a
                // host that simply cannot start does not spawn error dialogs
                // forever; re-toggling the mode resets the counter.
                const MAX_FAILS: u32 = 5;
                if self.wallpaper_spawn_fails >= MAX_FAILS {
                    if self.wallpaper_spawn_fails == MAX_FAILS {
                        self.wallpaper_spawn_fails += 1; // log the giving-up once
                        debug::log_error(
                            &self.dir,
                            "wallpaper: host failed to start repeatedly — giving up \
                             (toggle wallpaper mode off and on to retry)",
                        );
                    }
                } else {
                    let backoff = if self.wallpaper_spawn_fails == 0 {
                        Duration::ZERO
                    } else {
                        Duration::from_secs(
                            (2u64.saturating_pow(self.wallpaper_spawn_fails)).min(30),
                        )
                    };
                    let ready = self
                        .wallpaper_last_spawn
                        .map(|t| t.elapsed() >= backoff)
                        .unwrap_or(true);
                    if ready {
                        match Self::spawn_wallpaper_host() {
                            Ok(child) => {
                                debug::log_debug(&self.dir, "wallpaper: spawned host process");
                                self.wallpaper_child = Some(child);
                                self.wallpaper_last_spawn = Some(Instant::now());
                            }
                            Err(e) => {
                                self.wallpaper_spawn_fails =
                                    self.wallpaper_spawn_fails.saturating_add(1);
                                self.wallpaper_last_spawn = Some(Instant::now());
                                debug::log_error(
                                    &self.dir,
                                    &format!("wallpaper: spawn host failed — {e}"),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

impl eframe::App for RigStatsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Solid dark background matching PANEL_FILL. Window-level opacity is
        // applied by SetLayeredWindowAttributes so the swap chain stays opaque.
        let c = theme::PANEL_FILL;
        [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            1.0,
        ]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // On the first frame: locate the HWND and apply the initial window opacity.
        #[cfg(windows)]
        if self.hwnd == 0 {
            self.hwnd = win_opacity::find_hwnd("RigStats");
            if self.hwnd != 0 {
                win_opacity::set_opacity(self.hwnd, self.opacity);
            }
        }

        // Drive the wallpaper-host lifecycle (spawn/supervise/teardown).
        #[cfg(windows)]
        self.update_wallpaper_mode(ui.ctx());

        // Re-apply WindowLevel + opacity for a few frames after a floating→non-floating
        // transition, because winit may reset the window level when it processes the move.
        if self.reapply_window_props_frames > 0 {
            self.reapply_window_props_frames -= 1;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    Self::window_level_from_layer(&self.window_layer),
                ));
            #[cfg(windows)]
            {
                win_opacity::set_opacity(self.hwnd, self.opacity);
                // For "behind" mode, also enforce HWND_BOTTOM directly — ViewportCommand
                // alone is not reliable on Windows. Only needed at startup/transition;
                // normal operation won't bring the window back to front on its own.
                if self.window_layer == "behind" && !self.floating_mode {
                    win32_behind::keep_behind("RigStats");
                }
            }
        }

        // Detect OS dark/light mode changes and refresh dialog colours accordingly.
        #[cfg(windows)]
        {
            let dark = win32_dark_mode::is_system_dark_mode();
            if dark != self.os_dark_mode {
                self.os_dark_mode = dark;
                self.dialog_colors = if dark {
                    theme::DialogColors::dark()
                } else {
                    theme::DialogColors::light()
                };
            }
        }

        // Pull latest stats from poll thread; push to sparklines only on new data.
        while let Ok(stats) = self.receiver.try_recv() {
            let cpu_v = stats.cpu_load as f32;
            let gpu_v = stats.gpu_load.unwrap_or(0.0) as f32;
            let nu_v = stats.net_up_mbps as f32;
            let nd_v = stats.net_down_mbps as f32;
            self.cpu_spark.push(cpu_v);
            self.gpu_spark.push(gpu_v);
            self.net_up_spark.push(nu_v);
            self.net_dn_spark.push(nd_v);
            self.latest = stats;
        }

        // Apply settings saved from the settings window.
        if self.settings_reload.swap(false, Ordering::Relaxed) {
            let s = self.current_settings.lock_safe();
            let prev_visible = self.visible_panels.clone();
            self.visible_panels = s.visible_panels.clone();
            // Any panel that was visible before but is now hidden must be removed
            // from panels_positioned so its saved position is re-applied when it
            // reappears (e.g. after cancel reverts a live preview toggle).
            for key in &prev_visible {
                if !self.visible_panels.contains(key) {
                    self.panels_positioned.remove(key);
                }
            }
            self.opacity = s.opacity.clamp(0.1, 1.0) as f32;
            self.thresholds = PanelThresholds::from_settings(&s);
            self.psu_watts = s.psu_watts;
            self.app_theme = theme::AppTheme::from_key(&s.theme);
            self.window_layer = s.window_layer.clone();
            let was_floating = self.floating_mode;
            self.floating_mode = s.floating_mode;
            self.floating_mode_arc
                .store(self.floating_mode, Ordering::Relaxed);
            self.floating_panels_locked = s.floating_panels_locked;
            self.floating_panel_scale = s.floating_panel_scale.clamp(0.4, 1.0) as f32;
            let was_fullscreen = self.fullscreen_mode;
            self.fullscreen_mode = s.fullscreen_mode;
            self.fullscreen_align = s.fullscreen_align.clone();
            self.dashboard_pinned = s.dashboard_pinned;
            let profile = s.dashboard_profile.clone();
            drop(s);
            // Toggling fullscreen changes how the window is sized; clear the
            // fit-to-content guard so the next fixed frame re-snaps correctly, and
            // drop the cached centering height so it is re-measured.
            if was_fullscreen != self.fullscreen_mode {
                self.last_fitted_height = None;
                self.fullscreen_content_h = None;
            }
            let (pos, [w, h]) = self.fixed_window_geometry(&profile);
            // Pinned, but this profile has no saved position yet (e.g. the user
            // switched profiles while pinned): persist the auto-targeted position
            // now so the profile is properly pinned and stays put across restarts
            // instead of lingering in a locked-but-unsaved state.
            if !self.floating_mode && self.dashboard_pinned {
                let mut s = self.current_settings.lock_safe();
                if !s.pinned_positions.contains_key(&profile) {
                    s.pinned_positions.insert(
                        profile.clone(),
                        [pos[0].round() as i32, pos[1].round() as i32],
                    );
                    self.persist_settings_logged(&s);
                }
            }
            // Only resize the main window when in fixed mode AND the size actually changed.
            // Sending InnerSize every settings-reload (e.g. opacity slider drag) causes a
            // visible jump even when the dimensions are identical.
            //
            // In wallpaper mode our window is parked off-screen and the host owns the
            // visible dashboard — repositioning here would yank the (frozen, polling-
            // paused) main window back on-screen over the wallpaper, so skip it.
            if !self.floating_mode && !self.wallpaper_active {
                let new_size = [w, h];
                if self.last_applied_window_size != Some(new_size) {
                    self.last_applied_window_size = Some(new_size);
                    // Resize and snap to the monitor matching the profile orientation.
                    // (A size change implies a profile/fullscreen/panel change, not an
                    // opacity drag, so repositioning here does not cause jitter.)
                    let [px, py] = pos;
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                            px, py,
                        )));
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
                    self.last_fitted_height = None;
                }
            }
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    Self::window_level_from_layer(&self.window_layer),
                ));
            #[cfg(windows)]
            win_opacity::set_opacity(self.hwnd, self.opacity);
            // Toggle main window position when floating mode changes.
            // We move it off-screen instead of hiding it — a hidden window is not
            // ticked by eframe, so the floating panels would not update.
            if was_floating != self.floating_mode {
                if self.floating_mode {
                    // Clear positioned-set so restored positions are re-applied
                    // from the saved layout the first time each panel is shown.
                    self.panels_positioned.clear();
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                            -32000.0, -32000.0,
                        )));
                } else {
                    // Restore to the correct portrait monitor position (and full
                    // monitor size if fullscreen is enabled).
                    let [px, py] = pos;
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                            px, py,
                        )));
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
                    // Force the fit-to-content path to re-dispatch InnerSize next
                    // fixed frame: compute_window_height is only an estimate and may
                    // understate the real content height, which would otherwise clip
                    // the bottom panel. Clearing the guard makes the next frame snap
                    // the window to the true min_rect height.
                    self.last_fitted_height = None;
                }
            }
        }

        // Handle tray commands forwarded by the background polling thread.
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCmd::OpenSettings => {
                    // Re-initialise draft from current settings each time the window opens.
                    let s = self.current_settings.lock_safe().clone();
                    *self.settings_win.lock_safe() =
                        windows::settings::SettingsWindow::from_settings(&s);
                    self.settings_open.store(true, Ordering::Relaxed);
                    self.settings_focus.store(true, Ordering::Relaxed);
                }
                TrayCmd::OpenAbout => {
                    self.about_open.store(true, Ordering::Relaxed);
                    self.about_focus.store(true, Ordering::Relaxed);
                }
                TrayCmd::OpenStatus => {
                    self.status_open.store(true, Ordering::Relaxed);
                    self.status_focus.store(true, Ordering::Relaxed);
                    windows::status::spawn_load(
                        self.status_win.clone(),
                        self.status_refreshing.clone(),
                        self.dir.as_ref().clone(),
                        self.latest.lhm_connected,
                        self.wallpaper_active,
                        ui.ctx().clone(),
                    );
                }
                TrayCmd::OpenUpdater => {
                    self.updater_open.store(true, Ordering::Relaxed);
                    self.updater_focus.store(true, Ordering::Relaxed);
                }
                TrayCmd::ToggleFloating => {
                    let new_mode = {
                        let mut s = self.current_settings.lock_safe();
                        s.floating_mode = !s.floating_mode;
                        self.persist_settings_logged(&s);
                        s.floating_mode
                    };
                    let was_floating = self.floating_mode;
                    self.floating_mode = new_mode;
                    self.floating_mode_arc.store(new_mode, Ordering::Relaxed);
                    if was_floating != new_mode {
                        if new_mode {
                            self.panels_positioned.clear();
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::Pos2::new(-32000.0, -32000.0),
                                ));
                        } else {
                            let profile =
                                self.current_settings.lock_safe().dashboard_profile.clone();
                            let ([px, py], [w, h]) = self.fixed_window_geometry(&profile);
                            // Apply level + opacity BEFORE moving on-screen so they are
                            // already in effect when the window becomes visible.
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                    Self::window_level_from_layer(&self.window_layer),
                                ));
                            #[cfg(windows)]
                            win_opacity::set_opacity(self.hwnd, self.opacity);
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::Pos2::new(px, py),
                                ));
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                egui::Vec2::new(w, h),
                            ));
                            // See note in the settings-reload transition: clear the
                            // fit-to-content guard so the next fixed frame snaps the
                            // window to the true content height instead of the
                            // compute_window_height estimate (which can clip the
                            // bottom panel).
                            self.last_fitted_height = None;
                            // Re-apply for the next few frames as winit may reset the
                            // window level when it processes the move event.
                            self.reapply_window_props_frames = 4;
                        }
                    }
                }
                TrayCmd::ToggleRecording => {
                    let (new_enabled, retention_days) = {
                        let mut s = self.current_settings.lock_safe();
                        s.logging_enabled = !s.logging_enabled;
                        self.persist_settings_logged(&s);
                        (s.logging_enabled, s.log_retention_days)
                    };
                    if new_enabled {
                        logging::prune_old_logs(&self.dir, retention_days);
                    }
                    self.tray.set_recording(new_enabled);
                }
            }
        }

        // Sync floating lock state toggled by padlock icon in drag handle.
        let arc_locked = self.floating_lock_arc.load(Ordering::Relaxed);
        if arc_locked != self.floating_panels_locked {
            self.floating_panels_locked = arc_locked;
            let mut s = self.current_settings.lock_safe();
            s.floating_panels_locked = arc_locked;
            self.persist_settings_logged(&s);
        }

        // ── Secondary windows ─────────────────────────────────────────────────

        let main_ctx = ui.ctx().clone();
        let dc = self.dialog_colors;

        // When the last dialog closes, restore the main-window dark visuals.
        // We only call set_visuals on the transition frame (not every frame) to
        // avoid the repaint loop that causes jerky window dragging in light mode.
        let any_dialog_open = self.settings_open.load(Ordering::Relaxed)
            || self.about_open.load(Ordering::Relaxed)
            || self.status_open.load(Ordering::Relaxed)
            || self.updater_open.load(Ordering::Relaxed);
        if !any_dialog_open && self.any_dialog_open_prev {
            let mut vis = egui::Visuals::dark();
            vis.panel_fill = egui::Color32::TRANSPARENT;
            vis.window_fill = egui::Color32::from_gray(28);
            vis.override_text_color = Some(theme::C_TEXT);
            ui.ctx().set_visuals(vis);
        }
        self.any_dialog_open_prev = any_dialog_open;

        if self.settings_open.load(Ordering::Relaxed) {
            let open = self.settings_open.clone();
            let focus = self.settings_focus.clone();
            let state = self.settings_win.clone();
            let dir = self.dir.clone();
            let saved = self.current_settings.clone();
            let reload = self.settings_reload.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(560.0, 600.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("settings"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Settings")
                    .with_inner_size([560.0, 600.0])
                    .with_position([px, py])
                    .with_resizable(false)
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    // Capture HWND while we're inside the callback — window definitely exists here.
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("RigStats \u{2014} Settings");
                    }
                    windows::settings::show(
                        child_ui.ctx(),
                        &mctx,
                        &open,
                        &focus,
                        &state,
                        &dir,
                        &saved,
                        &reload,
                        &dc,
                    );
                },
            );
            #[cfg(windows)]
            win32_dark_mode::apply_titlebar_theme(found_hwnd, self.os_dark_mode);
            // Bring dialog to foreground now that we have the HWND.
            // If found_hwnd == 0 the window wasn't ready yet; restore the focus flag
            // so we retry on the next frame (which fires within 100 ms since a dialog is open).
            #[cfg(windows)]
            if wants_focus {
                if found_hwnd != 0 {
                    win_opacity::bring_to_foreground(found_hwnd);
                } else {
                    focus.store(true, Ordering::Relaxed);
                }
            }
        }

        if self.about_open.load(Ordering::Relaxed) {
            let open = self.about_open.clone();
            let focus = self.about_focus.clone();
            let dir = self.dir.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(360.0, 280.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("about"),
                egui::ViewportBuilder::default()
                    .with_title("About RigStats")
                    .with_inner_size([380.0, 420.0])
                    .with_position([px, py])
                    .with_resizable(false)
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("About RigStats");
                    }
                    windows::about::show(child_ui.ctx(), &mctx, &open, &focus, &dir, &dc);
                },
            );
            #[cfg(windows)]
            win32_dark_mode::apply_titlebar_theme(found_hwnd, self.os_dark_mode);
            #[cfg(windows)]
            if wants_focus {
                if found_hwnd != 0 {
                    win_opacity::bring_to_foreground(found_hwnd);
                } else {
                    focus.store(true, Ordering::Relaxed);
                }
            }
        }

        if self.status_open.load(Ordering::Relaxed) {
            let open = self.status_open.clone();
            let focus = self.status_focus.clone();
            let state = self.status_win.clone();
            let refreshing = self.status_refreshing.clone();
            let collecting = self.status_collecting.clone();
            let dir = self.dir.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(680.0, 720.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let lhm_connected = self.latest.lhm_connected;
            let wallpaper_active = self.wallpaper_active;
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("status"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Status")
                    .with_inner_size([680.0, 720.0])
                    .with_position([px, py])
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("RigStats \u{2014} Status");
                    }
                    windows::status::show(
                        child_ui.ctx(),
                        &mctx,
                        &open,
                        &focus,
                        &state,
                        &refreshing,
                        &collecting,
                        &dir,
                        lhm_connected,
                        wallpaper_active,
                        &dc,
                    );
                },
            );
            #[cfg(windows)]
            win32_dark_mode::apply_titlebar_theme(found_hwnd, self.os_dark_mode);
            #[cfg(windows)]
            if wants_focus {
                if found_hwnd != 0 {
                    win_opacity::bring_to_foreground(found_hwnd);
                } else {
                    focus.store(true, Ordering::Relaxed);
                }
            }
        }

        if self.updater_open.load(Ordering::Relaxed) {
            let open = self.updater_open.clone();
            let focus = self.updater_focus.clone();
            let state = self.updater_win.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(490.0, 560.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("updater"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats Update")
                    .with_inner_size([490.0, 560.0])
                    .with_position([px, py])
                    .with_resizable(false)
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("RigStats Update");
                    }
                    windows::updater::show(child_ui.ctx(), &mctx, &open, &focus, &state, &dc);
                },
            );
            #[cfg(windows)]
            win32_dark_mode::apply_titlebar_theme(found_hwnd, self.os_dark_mode);
            #[cfg(windows)]
            if wants_focus {
                if found_hwnd != 0 {
                    win_opacity::bring_to_foreground(found_hwnd);
                } else {
                    focus.store(true, Ordering::Relaxed);
                }
            }

            // When the window sets status to Checking (manual button click),
            // kick off check+download on a plain OS thread — update_check is
            // synchronous and tokio::spawn is not safe to call from the egui
            // UI thread (which is not inside a tokio async context).
            let is_checking = matches!(
                self.updater_win.lock_safe().status,
                windows::updater::UpdateStatus::Checking
            );
            if is_checking && !self.updater_busy.swap(true, Ordering::Relaxed) {
                let win = self.updater_win.clone();
                let busy = self.updater_busy.clone();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let thread_result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let result = update_check::check();
                            match result {
                                Ok(update_check::CheckResult::UpdateAvailable(info)) => {
                                    let version = info.version.clone();
                                    let url = info.url.clone();
                                    let dest = update_check::installer_temp_path(&version);
                                    {
                                        let mut s = win.lock_safe();
                                        s.status = windows::updater::UpdateStatus::Downloading {
                                            downloaded: 0,
                                            total: 0,
                                        };
                                    }
                                    ctx.request_repaint();
                                    let win2 = win.clone();
                                    let ctx2 = ctx.clone();
                                    let dl_result =
                                        update_check::download(&url, &dest, |downloaded, total| {
                                            let mut s = win2.lock_safe();
                                            s.status =
                                                windows::updater::UpdateStatus::Downloading {
                                                    downloaded,
                                                    total,
                                                };
                                            ctx2.request_repaint();
                                        });
                                    let mut s = win.lock_safe();
                                    s.status = match dl_result {
                                        Ok(()) => windows::updater::UpdateStatus::Ready {
                                            info,
                                            installer_path: dest,
                                        },
                                        Err(e) => windows::updater::UpdateStatus::Error(e),
                                    };
                                }
                                Ok(update_check::CheckResult::UpToDate) => {
                                    win.lock_safe().status =
                                        windows::updater::UpdateStatus::UpToDate;
                                }
                                Err(e) => {
                                    win.lock_safe().status =
                                        windows::updater::UpdateStatus::Error(e);
                                }
                            }
                        }));
                    if let Err(_panic) = thread_result {
                        if let Ok(mut s) = win.lock() {
                            s.status = windows::updater::UpdateStatus::Error(
                                "Update check failed unexpectedly".to_string(),
                            );
                        }
                    }
                    ctx.request_repaint();
                    busy.store(false, Ordering::Relaxed);
                });
            }
        }

        // On the first frame: move main window off-screen when floating mode is active.
        // We NEVER use Visible(false) in floating mode because a hidden window is not
        // ticked by eframe — show_viewport_immediate would stop being called and all
        // floating panels would freeze.
        if !self.initial_floating_applied {
            self.initial_floating_applied = true;
            if self.floating_mode {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                        -32000.0, -32000.0,
                    )));
            }
        }

        if self.floating_mode {
            // ── Floating mode — each panel in its own borderless viewport ─────
            self.render_floating_panels(ui);

            // Persist positions when any panel was dragged.
            if self.positions_dirty.swap(false, Ordering::Relaxed) {
                self.persist_floating_positions();
            }

            // Apply GPU preference change made from the floating GPU panel.
            if let Some(new_pref) = self.float_new_pref_gpu.lock_safe().take() {
                *self.preferred_gpu.lock_safe() = Some(new_pref.clone());
                let mut s = self.current_settings.lock_safe();
                s.preferred_gpu = Some(new_pref);
                self.persist_settings_logged(&s);
            }
        } else {
            // ── Fixed mode — all panels in one portrait/landscape window ──────

            // Track the window's current outer position so the padlock can pin
            // the exact spot it is at when clicked, and so a profile change can
            // carry the window over to its current spot. Skip while the window is
            // parked off-screen for wallpaper mode, which would otherwise overwrite
            // the real position with the parking coordinates.
            if !self.wallpaper_active {
                if let Some(outer) = ui.ctx().input(|i| i.viewport().outer_rect) {
                    self.last_fixed_pos = Some([outer.left().round(), outer.top().round()]);
                }
            }

            // Drag handle — thin invisible strip at top for moving the borderless window.
            let drag_w = {
                let w = ui.available_width();
                if w.is_finite() && w > 0.0 {
                    w
                } else {
                    ui.ctx().content_rect().width().max(1.0)
                }
            };
            let (drag_rect, drag_resp) = ui.allocate_exact_size(
                egui::Vec2::new(drag_w, theme::DRAG_HANDLE_H),
                egui::Sense::drag(),
            );

            // Padlock at the right of the drag strip: pins the whole dashboard.
            let pinned = self.dashboard_pinned;
            let padlock_center = egui::pos2(drag_rect.right() - 12.0, drag_rect.center().y);
            let padlock_hit = egui::Rect::from_center_size(
                padlock_center,
                egui::Vec2::new(24.0, drag_rect.height().max(14.0)),
            );
            let hover_pos = ui.ctx().input(|i| i.pointer.hover_pos());
            let over_padlock = hover_pos.map(|p| padlock_hit.contains(p)).unwrap_or(false);
            let padlock_resp = ui.interact(
                padlock_hit,
                ui.id().with("fixed_padlock"),
                egui::Sense::click(),
            );

            // Drag only when not pinned and not starting on the padlock.
            if drag_resp.dragged() && !pinned && !over_padlock {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            // After drag ends in "behind" mode, the window was activated for SC_MOVE
            // and is now in front. Push it back behind on the next few frames.
            if drag_resp.drag_stopped() && self.window_layer == "behind" {
                self.reapply_window_props_frames = 4;
            }

            if padlock_resp.clicked() {
                self.dashboard_pinned = !self.dashboard_pinned;
                let profile = self.current_settings.lock_safe().dashboard_profile.clone();
                let mut s = self.current_settings.lock_safe();
                s.dashboard_pinned = self.dashboard_pinned;
                if self.dashboard_pinned {
                    // Pin: remember the current window position for this profile.
                    if let Some([x, y]) = self.last_fixed_pos {
                        s.pinned_positions
                            .insert(profile, [x.round() as i32, y.round() as i32]);
                    }
                }
                self.persist_settings_logged(&s);
            }
            if padlock_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            // Drag dots — only when movable (unpinned) and hovering the strip.
            if drag_resp.hovered() && !pinned && !over_padlock {
                let painter = ui.painter();
                let cy = drag_rect.center().y;
                let cx = drag_rect.center().x;
                for i in [-5.0f32, 0.0, 5.0] {
                    painter.circle_filled(
                        egui::Pos2::new(cx + i, cy),
                        1.5,
                        egui::Color32::from_gray(160),
                    );
                }
            }
            // Padlock icon — shown while pinned (so it can be released) or when
            // hovering the strip (so it is discoverable).
            if pinned || drag_resp.hovered() || padlock_resp.hovered() {
                let color = if pinned {
                    self.app_theme.accent
                } else if padlock_resp.hovered() {
                    egui::Color32::from_gray(210)
                } else {
                    egui::Color32::from_gray(150)
                };
                draw_padlock(ui.painter(), padlock_center, pinned, color);
            }

            // Panels in the order defined by visible_panels (respects user reordering).
            let panels_to_draw = self.visible_panels.clone();
            let profile = self.current_settings.lock_safe().dashboard_profile.clone();
            // Extract update version once per frame (cheap lock read).
            let update_ver: Option<String> = {
                use windows::updater::UpdateStatus;
                let st = self.updater_win.lock_safe();
                if let UpdateStatus::Ready { info, .. } = &st.status {
                    Some(info.version.clone())
                } else {
                    None
                }
            };
            let mut new_preferred_gpu: Option<String> = None;

            if profile_is_landscape(&profile) {
                // ── Landscape grid — panels packed into an even, adaptive grid ──
                // The window is fixed to the profile size, so there is no per-frame
                // content-fit; the grid fills the available area exactly.
                new_preferred_gpu =
                    self.render_landscape_grid(ui, &panels_to_draw, update_ver.as_deref());
            } else {
                // ── Portrait vertical stack ─────────────────────────────────────
                let sc = profile_scale(&profile);
                // Fullscreen + centered: pad the top so the panel stack is vertically
                // centered in the filled window. Panel proportions are untouched —
                // only the surrounding background grows. Use the measured content
                // height from the previous frame (cached) for an exact center; fall
                // back to the compute_window_height estimate on the first frame.
                let center_fullscreen = self.fullscreen_mode && self.fullscreen_align == "center";
                let center_pad = if center_fullscreen {
                    // Pure panel-stack height (excludes drag handle and pad).
                    let content = self.fullscreen_content_h.unwrap_or_else(|| {
                        compute_window_height(&panels_to_draw, sc) - theme::DRAG_HANDLE_H
                    });
                    // Center the visible content with equal background above and below.
                    // The drag handle occupies the first DRAG_HANDLE_H px (invisible),
                    // so discount it from the top so the content itself is centered.
                    let pad =
                        ((ui.available_height() - content - theme::DRAG_HANDLE_H) * 0.5).max(0.0);
                    ui.add_space(pad);
                    pad
                } else {
                    0.0
                };
                for panel in &panels_to_draw {
                    if let Some(p) = self.draw_one_panel(ui, panel, sc, update_ver.as_deref()) {
                        new_preferred_gpu = Some(p);
                    }
                    ui.add_space((6.0 * sc).round());
                }
                // Cache the true panel-stack content height for exact centering next
                // frame: min_rect = drag handle + center pad + content, so content =
                // min_rect − DRAG_HANDLE_H − pad. Re-center on the next frame if the
                // measurement drifts from what we used this frame.
                if center_fullscreen {
                    let content = ui.min_rect().height() - theme::DRAG_HANDLE_H - center_pad;
                    if content > 10.0 {
                        let changed = self
                            .fullscreen_content_h
                            .map(|h| (h - content).abs() > 0.5)
                            .unwrap_or(true);
                        self.fullscreen_content_h = Some(content);
                        if changed {
                            ui.ctx().request_repaint();
                        }
                    }
                }
                // Fit window height to actual rendered content every frame so no black gap
                // appears regardless of panel set, spacing, or egui version.
                // Skipped in fullscreen mode — there the window stays at the full monitor
                // size and must NOT shrink to content.
                if !self.fullscreen_mode {
                    let used_h = ui.min_rect().height();
                    // During the first frames after launch the window may not be fully
                    // realized, so early InnerSize commands can be dropped — which would
                    // leave the bottom panel clipped until the user toggles floating mode.
                    // Force a re-fit (and a fast repaint) for a handful of frames so the
                    // true content height always sticks at startup.
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
                        let [w, _] = profile_to_size(&profile);
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
                            egui::Vec2::new(w - 2.0, used_h),
                        ));
                    }
                }
            }

            if let Some(new_pref) = new_preferred_gpu {
                *self.preferred_gpu.lock_safe() = Some(new_pref.clone());
                let mut s = self.current_settings.lock_safe();
                s.preferred_gpu = Some(new_pref);
                self.persist_settings_logged(&s);
            }
        }

        // Idle repaint rate: 1 fps regardless of whether a dialog is open.
        // Interaction responsiveness (hover, click, drag) is driven by OS input events
        // which wake eframe immediately — request_repaint_after only matters when idle.
        // All dialogs call main_ctx.request_repaint_of(ROOT) on close, so the
        // show_viewport_immediate cleanup on the next frame is already fast.
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

// ── Floating panel helpers ────────────────────────────────────────────────────

/// Returns the accent colour for a given floating panel key.
/// Draw a padlock icon centred on `center` (fits inside a ~10 × 12 px area).
///
/// * **locked**: symmetric arch, both shackle arms enter the body.
/// * **unlocked**: arch is lifted with a clear gap; only the right arm reaches
///   the body — the left end hangs free, clearly showing the lock is open.
fn draw_padlock(painter: &egui::Painter, center: egui::Pos2, locked: bool, color: egui::Color32) {
    let body_w = 8.0_f32;
    let body_h = 4.5_f32;
    let sr = 2.6_f32; // shackle half-width = radius of semicircle
    let stroke = egui::Stroke::new(1.5, color);

    // Body — filled rect in the lower portion.
    let body_cy = center.y + 2.2;
    painter.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(center.x, body_cy),
            egui::Vec2::new(body_w, body_h),
        ),
        1.0,
        color,
    );

    let body_top = body_cy - body_h / 2.0;
    let left_x = center.x - sr;
    let right_x = center.x + sr;

    if locked {
        // Arch sits tightly on body; both arms just touch body_top.
        let arc_cy = body_top - sr;
        painter.line_segment(
            [egui::pos2(left_x, body_top), egui::pos2(left_x, arc_cy)],
            stroke,
        );
        painter.line_segment(
            [egui::pos2(right_x, body_top), egui::pos2(right_x, arc_cy)],
            stroke,
        );
        let pts: Vec<egui::Pos2> = (0..=10)
            .map(|i| {
                let a = std::f32::consts::PI * i as f32 / 10.0;
                egui::pos2(center.x - sr * a.sin(), arc_cy - sr * a.cos())
            })
            .collect();
        painter.add(egui::Shape::line(pts, stroke));
    } else {
        // Arch is raised well above body — clear gap shows it is open.
        // Only the right arm extends down to body_top; left end hangs free.
        let arc_cy = body_top - sr * 2.6; // raised ~2.5× compared to locked
        painter.line_segment(
            [egui::pos2(right_x, body_top), egui::pos2(right_x, arc_cy)],
            stroke,
        );
        // Left arm: short stub at arc end (makes the opening obvious).
        painter.line_segment(
            [
                egui::pos2(left_x, arc_cy),
                egui::pos2(left_x, arc_cy + sr * 0.7),
            ],
            stroke,
        );
        let pts: Vec<egui::Pos2> = (0..=10)
            .map(|i| {
                let a = std::f32::consts::PI * i as f32 / 10.0;
                egui::pos2(center.x - sr * a.sin(), arc_cy - sr * a.cos())
            })
            .collect();
        painter.add(egui::Shape::line(pts, stroke));
    }
}

impl RigStatsApp {
    fn window_level_from_layer(layer: &str) -> egui::WindowLevel {
        match layer {
            "on_top" => egui::WindowLevel::AlwaysOnTop,
            "behind" => egui::WindowLevel::AlwaysOnBottom,
            _ => egui::WindowLevel::Normal,
        }
    }

    /// Build a borrowed [`DashboardView`] over this frame's render state, shared
    /// with the `rigstats-wallpaper` host so the panel layout lives in one place.
    fn view(&self) -> DashboardView<'_> {
        DashboardView {
            latest: &self.latest,
            cpu_spark: &self.cpu_spark,
            gpu_spark: &self.gpu_spark,
            net_up_spark: &self.net_up_spark,
            net_dn_spark: &self.net_dn_spark,
            textures: &self.textures,
            app_theme: &self.app_theme,
            thresholds: &self.thresholds,
            psu_watts: self.psu_watts,
        }
    }

    /// Draw a single panel via the shared [`DashboardView`], then consume the
    /// clock panel's `"open_updater"` badge-click flag (host has no updater).
    fn draw_one_panel(
        &self,
        ui: &mut egui::Ui,
        panel: &str,
        sc: f32,
        update_ver: Option<&str>,
    ) -> Option<String> {
        let new_pref = self.view().draw_one_panel(ui, panel, sc, update_ver);
        if panel == "clock"
            && ui
                .ctx()
                .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("open_updater")))
                .unwrap_or(false)
        {
            self.updater_open.store(true, Ordering::Relaxed);
        }
        new_pref
    }

    /// Render the visible panels as an adaptive landscape grid via the shared view.
    fn render_landscape_grid(
        &self,
        ui: &mut egui::Ui,
        panels: &[String],
        update_ver: Option<&str>,
    ) -> Option<String> {
        self.view().render_landscape_grid(ui, panels, update_ver)
    }

    /// Fixed-mode window geometry: returns `(top_left, [w, h])`.
    /// Width is always the profile width so panel proportions never stretch.
    /// In fullscreen mode the height fills the portrait monitor (intended for a
    /// monitor whose resolution matches the profile); otherwise it is the
    /// content-fit estimate (the per-frame fit then refines it). Falls back to the
    /// content-fit size if no monitor is found.
    fn fixed_window_geometry(&self, profile: &str) -> ([f32; 2], [f32; 2]) {
        let [w, h] = profile_to_size(profile);
        // Auto-target the monitor whose resolution matches the profile (a strip/
        // secondary screen only when it actually fits); otherwise the primary
        // monitor. Used for both orientations so portrait/side profiles land on a
        // matching screen or the main screen — never on an arbitrary small monitor.
        let [mx, my, _mw, mh] = pick_window_rect_for_profile(profile);
        let (auto_pos, size) = if profile_is_landscape(profile) {
            // Landscape: the grid fills a fixed window the size of the profile.
            ([mx, my], [w, h])
        } else if self.fullscreen_mode {
            // Fill the height of the monitor the window currently sits on (where
            // the user placed it) — not the profile-matching monitor. Otherwise
            // enabling Fill Screen would teleport the dashboard to another screen.
            // Fall back to the auto-target monitor when the current position is
            // unknown or off every monitor.
            let m = self
                .last_fixed_pos
                .and_then(monitor_rect_at)
                .filter(|r| r[3] > 0.0)
                .unwrap_or([mx, my, _mw, mh]);
            ([m[0], m[1]], [w, m[3]])
        } else {
            let h = compute_window_height(&self.visible_panels, profile_scale(profile));
            ([mx, my], [w, h])
        };
        // Pinned override: keep the computed size but restore the saved position
        // for this profile instead of auto-targeting a monitor.
        if let Some(pp) = self.pinned_position(profile) {
            return (pp, size);
        }
        // Carry the window's current position across a profile change, so switching
        // profiles keeps the dashboard where the user left it. Fall back to the
        // auto-target only when that spot is off every connected monitor. Skipped in
        // fullscreen, which must snap to the monitor it fills.
        if !self.fullscreen_mode {
            if let Some(last) = self.last_fixed_pos {
                return (guard_panel_position(last, auto_pos), size);
            }
        }
        (auto_pos, size)
    }

    /// Returns the saved pinned position for `profile` when the dashboard is
    /// pinned and the stored position is still on a connected monitor; otherwise
    /// `None` so the caller auto-targets the matching monitor.
    fn pinned_position(&self, profile: &str) -> Option<[f32; 2]> {
        if !self.dashboard_pinned {
            return None;
        }
        let saved = {
            let s = self.current_settings.lock_safe();
            s.pinned_positions.get(profile).copied()
        };
        #[cfg(windows)]
        let monitors = win_monitor::list();
        #[cfg(not(windows))]
        let monitors: Vec<(i32, i32, i32, i32)> = Vec::new();
        resolve_pinned_position(self.dashboard_pinned, saved, &monitors)
    }

    /// Render every visible panel as its own borderless OS window using
    /// `show_viewport_immediate`.  Called each frame when `floating_mode` is true.
    ///
    /// `show_viewport_immediate` renders each child synchronously as part of the
    /// parent frame — no deferred callbacks, no separate event loops.  The parent
    /// ticks at ~1 fps (via `request_repaint_after(1 s)` in `update()`), so all
    /// panels naturally update at ~1 fps without any Win32 tricks.
    fn render_floating_panels(&mut self, ui: &mut egui::Ui) {
        let s = self.current_settings.lock_safe();
        let window_level = Self::window_level_from_layer(&s.window_layer);
        let scale = self.floating_panel_scale;
        drop(s);

        let opacity = self.opacity;

        for idx in 0..self.visible_panels.len() {
            let key = self.visible_panels[idx].clone();

            let init_pos: [f32; 2] = {
                let default_pos = [100.0 + idx as f32 * 20.0, 80.0 + idx as f32 * 30.0];
                let positions = self.floating_positions.lock_safe();
                let saved = positions.get(&key).copied().unwrap_or(default_pos);
                guard_panel_position(saved, default_pos)
            };

            let panel_w = 450.0 * scale;
            let initial_h = panel_initial_h(&key) * scale;

            // Only set window position on first creation.  After that the OS
            // owns the position (via drag); re-sending with_position every frame
            // causes egui to diff-and-dispatch SetOuterPosition continuously,
            // which fights the OS and produces sub-pixel blur.
            let needs_position = !self.panels_positioned.contains(&key);
            if needs_position {
                self.panels_positioned.insert(key.clone());
            }

            // Title shared between ViewportBuilder and Win32 FindWindowW lookup.
            let win_title = format!("RigStats \u{2014} {}", panel_label(&key));
            let mut vp_builder = egui::ViewportBuilder::default()
                .with_title(win_title.clone())
                .with_inner_size([panel_w, initial_h])
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_window_level(window_level);

            let is_behind = window_level == egui::WindowLevel::AlwaysOnBottom;

            if needs_position {
                vp_builder = vp_builder.with_position(init_pos);
            }

            // `show_viewport_immediate` is FnMut with no Send/'static bound —
            // we can borrow self fields directly instead of going through Arc.
            let positions_arc = &self.floating_positions;
            let dirty = &self.positions_dirty;
            let behind_enforce = &self.behind_enforce;
            let new_pref_arc = &self.float_new_pref_gpu;
            let lock_arc = &self.floating_lock_arc;
            let stats = &self.latest;
            let cspark = &self.cpu_spark;
            let gspark = &self.gpu_spark;
            let nuspark = &self.net_up_spark;
            let ndspark = &self.net_dn_spark;
            let tex = &self.textures;
            let app_theme = self.app_theme;
            let float_update_ver: Option<String> = {
                use windows::updater::UpdateStatus;
                let st = self.updater_win.lock_safe();
                if let UpdateStatus::Ready { info, .. } = &st.status {
                    Some(info.version.clone())
                } else {
                    None
                }
            };
            let updater_open_arc = &self.updater_open;
            // On the very first frame a viewport is shown, `outer_rect` reports the
            // egui-default position (before the OS has honoured `with_position`).
            // Saving that would overwrite the loaded position, so we skip tracking
            // on the first frame — `needs_position` was true iff this is that frame.
            let skip_pos_tracking = needs_position;

            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of(format!("float_{key}")),
                vp_builder,
                |child_ui, _class| {
                    let ctx = child_ui.ctx();

                    // ── Track window position for persistence ─────────────────
                    if !skip_pos_tracking {
                        if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                            let new_pos = [outer.left().round(), outer.top().round()];
                            let mut pos = positions_arc.lock_safe();
                            let stored = pos.entry(key.clone()).or_insert([f32::NAN, f32::NAN]);
                            if stored[0].is_nan()
                                || stored[1].is_nan()
                                || ((*stored)[0] - new_pos[0]).abs() > 0.5
                                || ((*stored)[1] - new_pos[1]).abs() > 0.5
                            {
                                *stored = new_pos;
                                dirty.store(true, Ordering::Relaxed);
                            }
                        }
                    }

                    // ── "Always Behind" enforcement ───────────────────────────
                    // SC_MOVE (StartDrag) requires the window to be active.
                    // WS_EX_NOACTIVATE prevents activation, so we only enforce
                    // "behind" when the primary button is NOT pressed on this
                    // panel.  prepare_for_drag() strips WS_EX_NOACTIVATE and
                    // activates the window just before sending StartDrag, then
                    // apply_behind() re-arms once the drag finishes.
                    //
                    // Crucially we DON'T re-push every frame: send_viewport_cmd +
                    // SetWindowPos each generate a fresh repaint, so re-asserting
                    // unconditionally creates a spin loop that runs at the full
                    // refresh rate (the cause of the floating-mode CPU spike).
                    // Instead we enforce on creation, in a short burst after a
                    // drag, and otherwise ~1/s as an idle safety net.
                    let primary_down = ctx.input(|i| i.pointer.primary_down());
                    if is_behind {
                        let now = Instant::now();
                        let mut map = behind_enforce.borrow_mut();
                        let st = map.entry(key.clone()).or_insert(BehindEnforce {
                            last_enforce: now.checked_sub(Duration::from_secs(10)).unwrap_or(now),
                            prev_primary_down: false,
                            force_until: now,
                        });
                        let released = st.prev_primary_down && !primary_down;
                        st.prev_primary_down = primary_down;
                        if released {
                            // Window was activated for the drag; snap it back for
                            // a short burst so it settles behind reliably.
                            st.force_until = now + Duration::from_millis(400);
                        }
                        let should_enforce = !primary_down
                            && (needs_position
                                || now < st.force_until
                                || now.duration_since(st.last_enforce)
                                    >= Duration::from_millis(750));
                        if should_enforce {
                            st.last_enforce = now;
                            drop(map);
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                egui::WindowLevel::AlwaysOnBottom,
                            ));
                            #[cfg(windows)]
                            win32_behind::apply_behind(&win_title);
                        }
                    }

                    #[allow(deprecated)] // CentralPanel::show is correct in viewport callbacks
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none().fill(theme::PANEL_FILL))
                        .show(ctx, |ui| {
                            // ── Drag & lock state ─────────────────────────────
                            let locked = lock_arc.load(Ordering::Relaxed);
                            let hover_pos = ctx.input(|i| i.pointer.hover_pos());
                            let just_pressed = ctx.input(|i| i.pointer.primary_pressed());

                            // ── Panel content ─────────────────────────────────
                            // Each draw() returns the panel's outer Rect so we can
                            // overlay the drag dots and padlock without extra height.
                            let mut new_pref: Option<String> = None;
                            let panel_rect = match key.as_str() {
                                "header" => {
                                    panels::header::draw(ui, stats, tex, 1.0, &app_theme, scale)
                                }
                                "clock" => {
                                    let r = panels::clock::draw(
                                        ui,
                                        stats.uptime_secs,
                                        1.0,
                                        &app_theme,
                                        float_update_ver.as_deref(),
                                        scale,
                                    );
                                    if ui
                                        .ctx()
                                        .data_mut(|d| {
                                            d.remove_temp::<bool>(egui::Id::new("open_updater"))
                                        })
                                        .unwrap_or(false)
                                    {
                                        updater_open_arc.store(true, Ordering::Relaxed);
                                    }
                                    r
                                }
                                "cpu" => panels::cpu::draw(
                                    ui,
                                    stats,
                                    cspark,
                                    tex,
                                    1.0,
                                    self.thresholds.cpu.0,
                                    self.thresholds.cpu.1,
                                    &app_theme,
                                    scale,
                                ),
                                "gpu" => {
                                    let r = panels::gpu::draw(
                                        ui,
                                        stats,
                                        gspark,
                                        tex,
                                        1.0,
                                        &app_theme,
                                        self.thresholds.gpu.0,
                                        self.thresholds.gpu.1,
                                        self.thresholds.gpu_hotspot.0,
                                        self.thresholds.gpu_hotspot.1,
                                        scale,
                                    );
                                    new_pref = r.0;
                                    r.1
                                }
                                "ram" => panels::ram::draw(
                                    ui,
                                    stats,
                                    1.0,
                                    self.thresholds.ram.0,
                                    self.thresholds.ram.1,
                                    &app_theme,
                                    scale,
                                ),
                                "net" => panels::net::draw(
                                    ui, stats, nuspark, ndspark, 1.0, &app_theme, scale,
                                ),
                                "disk" => panels::disk::draw(
                                    ui,
                                    stats,
                                    1.0,
                                    self.thresholds.disk.0,
                                    self.thresholds.disk.1,
                                    &app_theme,
                                    scale,
                                ),
                                "motherboard" => panels::motherboard::draw(
                                    ui,
                                    stats,
                                    1.0,
                                    self.thresholds.mb.0,
                                    self.thresholds.mb.1,
                                    &app_theme,
                                    scale,
                                ),
                                "process" => {
                                    panels::process::draw(ui, stats, 1.0, &app_theme, scale)
                                }
                                "power" => panels::power::draw(
                                    ui,
                                    stats,
                                    1.0,
                                    &app_theme,
                                    scale,
                                    self.psu_watts,
                                ),
                                "battery" => panels::battery::draw(
                                    ui,
                                    stats,
                                    1.0,
                                    &app_theme,
                                    scale,
                                    self.thresholds.battery.0,
                                    self.thresholds.battery.1,
                                    self.thresholds.battery_power.0,
                                    self.thresholds.battery_power.1,
                                ),
                                _ => egui::Rect::NOTHING,
                            };

                            if let Some(p) = new_pref {
                                *new_pref_arc.lock_safe() = Some(p);
                            }

                            // ── Drag zone: top 24 px of the panel inner area ──
                            // inner_margin top = 8 px; title row is ~20 px tall,
                            // so top+24 covers the title row comfortably.
                            let drag_zone = egui::Rect::from_min_max(
                                panel_rect.min,
                                egui::pos2(panel_rect.right(), panel_rect.top() + 24.0),
                            );
                            // Padlock hit area: right 20 px of the drag zone.
                            let padlock_cx = drag_zone.right() - 14.0;
                            let padlock_cy = drag_zone.center().y;
                            let padlock_center = egui::pos2(padlock_cx, padlock_cy);
                            let padlock_hit = egui::Rect::from_center_size(
                                padlock_center,
                                egui::Vec2::new(22.0, drag_zone.height()),
                            );

                            let in_drag_zone =
                                hover_pos.map(|p| drag_zone.contains(p)).unwrap_or(false);
                            let in_padlock =
                                hover_pos.map(|p| padlock_hit.contains(p)).unwrap_or(false);

                            // Drag trigger (whole drag zone minus padlock area).
                            if !locked && just_pressed && in_drag_zone && !in_padlock {
                                #[cfg(windows)]
                                if is_behind {
                                    win32_behind::prepare_for_drag(&win_title);
                                }
                                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                            }

                            // Padlock interaction.
                            let padlock_resp = ui.interact(
                                padlock_hit,
                                ui.id().with("padlock"),
                                egui::Sense::click(),
                            );
                            if padlock_resp.clicked() {
                                lock_arc.store(!locked, Ordering::Relaxed);
                            }
                            if padlock_resp.hovered() {
                                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                            }

                            // ── Overlay: dots + padlock painted on the panel ──
                            let painter = ui.painter();

                            // Three dots — shown when unlocked and hovering the drag zone.
                            if in_drag_zone && !locked {
                                let cy = drag_zone.center().y;
                                let cx = drag_zone.center().x;
                                for i in [-5.0f32, 0.0, 5.0] {
                                    painter.circle_filled(
                                        egui::pos2(cx + i, cy),
                                        1.5,
                                        egui::Color32::from_gray(160),
                                    );
                                }
                            }

                            // Padlock: full icon on hover; tiny dot when locked + not hovering.
                            if in_drag_zone {
                                let padlock_color = if locked {
                                    app_theme.accent
                                } else if padlock_resp.hovered() {
                                    egui::Color32::from_gray(130)
                                } else {
                                    egui::Color32::from_gray(100)
                                };
                                draw_padlock(painter, padlock_center, locked, padlock_color);
                            } else if locked {
                                // Subtle locked indicator when not hovering — just a small dot.
                                painter.circle_filled(
                                    padlock_center,
                                    2.5,
                                    egui::Color32::from_rgba_unmultiplied(
                                        app_theme.accent.r(),
                                        app_theme.accent.g(),
                                        app_theme.accent.b(),
                                        120,
                                    ),
                                );
                            }

                            // Auto-resize height to content, but only when it actually
                            // changes — sending InnerSize every frame causes sub-pixel
                            // jitter that makes the panel blurry after dragging.
                            let used_h = ui.min_rect().height().round();
                            if used_h > 10.0 {
                                let current_h = ctx
                                    .input(|i| i.viewport().inner_rect)
                                    .map_or(0.0, |r| r.height().round());
                                if (used_h - current_h).abs() > 0.5 {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                        egui::Vec2::new(panel_w, used_h),
                                    ));
                                }
                            }

                            // Apply opacity to this panel's OS window.
                            #[cfg(windows)]
                            {
                                let title = format!("RigStats \u{2014} {}", panel_label(&key));
                                let hwnd = win_opacity::find_hwnd(&title);
                                win_opacity::set_opacity(hwnd, opacity);
                            }
                        });
                },
            );
        }
    }

    /// Persist settings to disk, logging any failure instead of swallowing it
    /// silently — a failed write means the user loses settings/layout changes,
    /// which should never pass unnoticed.
    fn persist_settings_logged(&self, s: &settings::Settings) {
        if let Err(e) = settings::persist_settings(&self.dir, s) {
            debug::log_error(&self.dir, &format!("settings: persist failed — {e}"));
        }
    }

    /// Flush `floating_positions` → `panel_layouts` in settings and persist to disk.
    fn persist_floating_positions(&self) {
        let positions = self.floating_positions.lock_safe();
        let mut s = self.current_settings.lock_safe();
        for (key, &[x, y]) in positions.iter() {
            s.panel_layouts.insert(
                key.clone(),
                settings::PanelLayout {
                    x: x as i32,
                    y: y as i32,
                },
            );
        }
        drop(positions);
        self.persist_settings_logged(&s);
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    // Opt the process into dark mode for OS-drawn UI elements (tray context menu).
    #[cfg(windows)]
    win32_dark_mode::enable();

    let dir = app_data_dir();
    debug::reset_debug_log(&dir);
    debug::append_debug_log(&dir, "rigstats starting");
    debug::append_debug_log(&dir, &format!("settings dir: {}", dir.display()));

    #[cfg(windows)]
    {
        let dark = win32_dark_mode::is_system_dark_mode();
        debug::log_debug(&dir, &format!("os_dark_mode: {dark}"));
    }

    let s = settings::load_settings(&dir);
    let visible_panels = s.visible_panels.clone();
    let opacity = s.opacity.clamp(0.1, 1.0) as f32;
    let always_on_top = s.window_layer == "on_top";
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
    let (inner_w, inner_h, mut pos_x, mut pos_y) = if landscape {
        // Landscape: the grid fills a fixed window the size of the profile.
        (win_w, win_h, mx, my)
    } else if fullscreen && mh > 0.0 {
        (win_w, mh, mx, my)
    } else {
        let h = compute_window_height(&visible_panels, profile_scale(&s.dashboard_profile));
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
    debug::log_debug(
        &dir,
        &format!(
            "settings: profile={} panels={} opacity={opacity:.2} floating_mode={}",
            s.dashboard_profile,
            visible_panels.join(","),
            s.floating_mode
        ),
    );
    debug::log_debug(
        &dir,
        &format!("window: initial_position=({pos_x:.1}, {pos_y:.1})"),
    );

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let preferred_gpu_arc: Arc<Mutex<Option<String>>> =
        Arc::new(Mutex::new(s.preferred_gpu.clone()));
    let current_settings_shared = Arc::new(Mutex::new(settings::load_settings(&dir)));
    let (tx, rx) = mpsc::sync_channel::<PollStats>(4);
    let dir_clone = dir.clone();
    let pref_poll = preferred_gpu_arc.clone();
    let settings_poll = current_settings_shared.clone();
    // Pause flag: set while in wallpaper mode so this app releases the sensor
    // pipe to the `rigstats-wallpaper` host (which becomes the active poller).
    let poll_paused = Arc::new(AtomicBool::new(false));
    let poll_paused_loop = poll_paused.clone();
    runtime.spawn(async move {
        poll_loop(tx, dir_clone, pref_poll, settings_poll, poll_paused_loop).await
    });

    // Wallpaper mode at startup: the host owns the on-screen dashboard, so place
    // our own window off-screen from the start to avoid a one-frame flash.
    let start_wallpaper = s.window_layer == "wallpaper" && !s.floating_mode;
    if start_wallpaper {
        pos_x = -32000.0;
        pos_y = -32000.0;
    }

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

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "RigStats",
        options,
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            // Suppress egui's built-in panel backgrounds; clear_color provides the base fill.
            visuals.panel_fill = egui::Color32::TRANSPARENT;
            // Popups (ComboBox, tooltips) use window_fill — keep it solid so they're readable.
            visuals.window_fill = egui::Color32::from_gray(28);
            // Default body text colour: #b8cce8
            visuals.override_text_color = Some(theme::C_TEXT);
            cc.egui_ctx.set_visuals(visuals);

            // Larger font sizes for readability on a portrait monitor.
            theme::apply_dashboard_fonts(&cc.egui_ctx);

            let tray = build_tray(s.logging_enabled);
            let (tray_tx, tray_rx) = mpsc::channel::<TrayCmd>();

            // Spawn a thread that polls tray events at 50 ms intervals and wakes the
            // egui event loop via request_repaint().  Quit is handled here directly
            // with process::exit so it is never delayed by a missed repaint.
            let ctx = cc.egui_ctx.clone();
            let quit_id = tray.quit_id.clone();
            let settings_id = tray.settings_id.clone();
            let about_id = tray.about_id.clone();
            let status_id = tray.status_id.clone();
            let updater_id = tray.updater_id.clone();
            let floating_id = tray.floating_id.clone();
            let recording_id = tray.recording_id.clone();
            let dir_tray = dir.clone();
            std::thread::spawn(move || loop {
                // Guard each iteration: a panic in a tray/Win32 call must not
                // silently kill this thread, or tray clicks would stop working
                // for the rest of the session. Recover and log instead.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut repaint = false;
                    if let Ok(ev) = MenuEvent::receiver().try_recv() {
                        // Use the foreground rights that come with the tray-menu interaction.
                        // We immediately bring the (off-screen) parent window to the foreground
                        // so that our process owns foreground when the dialog is created a few
                        // milliseconds later.  Without this the dialog window would be created
                        // as a background window and SetForegroundWindow would be refused.
                        #[cfg(windows)]
                        #[allow(unsafe_code)]
                        unsafe {
                            winapi::um::winuser::AllowSetForegroundWindow(0xFFFF_FFFFu32); // ASFW_ANY
                            let parent_hwnd = win_opacity::find_hwnd("RigStats");
                            if parent_hwnd != 0 {
                                winapi::um::winuser::SetForegroundWindow(parent_hwnd as _);
                            }
                        }

                        let cmd = if ev.id == quit_id {
                            debug::append_debug_log(&dir_tray, "shutdown: clean (tray quit)");
                            std::process::exit(0);
                        } else if ev.id == floating_id {
                            Some(TrayCmd::ToggleFloating)
                        } else if ev.id == recording_id {
                            Some(TrayCmd::ToggleRecording)
                        } else if ev.id == settings_id {
                            Some(TrayCmd::OpenSettings)
                        } else if ev.id == about_id {
                            Some(TrayCmd::OpenAbout)
                        } else if ev.id == status_id {
                            Some(TrayCmd::OpenStatus)
                        } else if ev.id == updater_id {
                            Some(TrayCmd::OpenUpdater)
                        } else {
                            None
                        };
                        if let Some(c) = cmd {
                            let _ = tray_tx.send(c);
                            repaint = true;
                        }
                    }
                    // Drain tray-icon click events so they don't accumulate. We no
                    // longer act on left-click — the dashboard is always visible.
                    let _ = tray_icon::TrayIconEvent::receiver().try_recv();
                    if repaint {
                        ctx.request_repaint();
                    }
                }));
                if outcome.is_err() {
                    debug::log_warn(
                        &dir_tray,
                        "tray: event handler panicked — thread recovered, continuing",
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            });

            let current_settings = current_settings_shared;
            let settings_reload = Arc::new(AtomicBool::new(false));
            let dir_arc = Arc::new(dir.clone());
            let textures = brand::Textures::load(&cc.egui_ctx);

            // Heartbeat: wakes the parent eframe loop at ~1 fps when floating mode is
            // active.  With `show_viewport_immediate`, all panels are rendered
            // synchronously as part of the parent frame, so one `request_repaint()`
            // per second is enough to drive all panel updates.
            let fm_arc_hb = Arc::new(AtomicBool::new(current_settings.lock_safe().floating_mode));
            let ctx_hb = cc.egui_ctx.clone();
            let fm_arc_hb2 = fm_arc_hb.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(950));
                if fm_arc_hb2.load(Ordering::Relaxed) {
                    ctx_hb.request_repaint();
                }
            });

            // Background auto-update check: fires 10 s after startup, then every 6 h.
            // When a newer version is found and downloaded, opens the updater window.
            let updater_win_bg: Arc<Mutex<windows::updater::UpdaterState>> =
                Arc::new(Mutex::new(windows::updater::UpdaterState::default()));
            let updater_open_bg = Arc::new(AtomicBool::new(false));
            let updater_focus_bg = Arc::new(AtomicBool::new(false));

            // Check for --just-updated=VERSION argument passed by the NSIS /autoupdate installer.
            if let Some(version) = std::env::args()
                .find(|a| a.starts_with("--just-updated="))
                .and_then(|a| a.split_once('=').map(|x| x.1.to_owned()))
            {
                if !version.is_empty() {
                    updater_win_bg.lock_safe().status =
                        windows::updater::UpdateStatus::JustUpdated { version };
                    updater_open_bg.store(true, Ordering::Relaxed);
                    updater_focus_bg.store(true, Ordering::Relaxed);
                }
            }

            {
                let win = updater_win_bg.clone();
                let open = updater_open_bg.clone();
                let focus = updater_focus_bg.clone();
                let ctx = cc.egui_ctx.clone();
                let dir_upd = dir.clone();
                runtime.spawn(async move {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    loop {
                        let result = tokio::task::spawn_blocking(update_check::check)
                            .await
                            .unwrap_or_else(|_| Err("task panic".to_string()));
                        match result {
                            Ok(update_check::CheckResult::UpdateAvailable(info)) => {
                                let version = info.version.clone();
                                let url = info.url.clone();
                                let dest = update_check::installer_temp_path(&version);
                                {
                                    let mut s = win.lock_safe();
                                    s.status = windows::updater::UpdateStatus::Downloading {
                                        downloaded: 0,
                                        total: 0,
                                    };
                                }
                                ctx.request_repaint();
                                let win2 = win.clone();
                                let ctx2 = ctx.clone();
                                let dest2 = dest.clone();
                                let dl_result = tokio::task::spawn_blocking(move || {
                                    update_check::download(&url, &dest2, |downloaded, total| {
                                        let mut s = win2.lock_safe();
                                        s.status = windows::updater::UpdateStatus::Downloading {
                                            downloaded,
                                            total,
                                        };
                                        ctx2.request_repaint();
                                    })
                                })
                                .await
                                .unwrap_or_else(|_| Err("download task panic".to_string()));
                                match dl_result {
                                    Ok(()) => {
                                        let mut s = win.lock_safe();
                                        s.status = windows::updater::UpdateStatus::Ready {
                                            info,
                                            installer_path: dest,
                                        };
                                        drop(s);
                                        open.store(true, Ordering::Relaxed);
                                        focus.store(true, Ordering::Relaxed);
                                        ctx.request_repaint();
                                    }
                                    Err(e) => {
                                        win.lock_safe().status =
                                            windows::updater::UpdateStatus::Error(e);
                                    }
                                }
                            }
                            Ok(update_check::CheckResult::UpToDate) => {}
                            Err(e) => {
                                debug::log_warn(
                                    &dir_upd,
                                    &format!("update-check: background check failed — {e}"),
                                );
                            }
                        }
                        tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
                    }
                });
            }

            Ok(Box::new(RigStatsApp::new(
                rx,
                tray_rx,
                visible_panels,
                opacity,
                tray,
                current_settings,
                settings_reload,
                dir_arc,
                textures,
                preferred_gpu_arc,
                fm_arc_hb,
                updater_win_bg,
                updater_open_bg,
                updater_focus_bg,
                poll_paused,
            )))
        }),
    )
    .expect("eframe");

    debug::append_debug_log(&dir, "shutdown: clean");
    runtime.shutdown_background();
}
