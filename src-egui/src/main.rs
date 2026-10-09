#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;

use app::state::{DialogStates, FontAtlasRefresh, GpuRecovery, OverlayState};
use eframe::egui;
use rigstats_backend::control;
use rigstats_backend::{debug, hardware, logging, settings};
use rigstats_egui::dashboard::{DashboardRuntime, DashboardView};
use rigstats_egui::dcomp_burst::DcompRevealBurst;
#[cfg(windows)]
use rigstats_egui::geometry::win_monitor;
use rigstats_egui::geometry::{
    compute_landscape_window_height, compute_window_height, dialog_center, guard_panel_position,
    is_position_on_screen, monitor_rect_at, overlay_anchor_position, pick_window_rect_for_profile,
    profile_is_landscape, profile_scale, profile_to_size, resolve_pinned_position,
};
use rigstats_egui::gpu_guard::install_gpu_loss_guard;
use rigstats_egui::hotkey;
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::overlay::{content_inset, draw_overlay, estimate_window_size};
use rigstats_egui::poll::{poll_loop, PollMode, PollModeHandle};
use rigstats_egui::tray::{build_tray, load_app_icon, panel_initial_h, panel_label, Tray, TrayCmd};
use rigstats_egui::wallpaper_supervisor::{self, ChildHost, WallpaperSupervisor};
use rigstats_egui::{alerts, panels, theme, update_flow, windows, PollStats};
#[cfg(windows)]
use rigstats_egui::{win32_behind, win32_dark_mode, win_opacity};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

fn app_data_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(appdata).join("se.codeby.rigstats")
}

// ── eframe application ────────────────────────────────────────────────────────

struct RigStatsApp {
    runtime: DashboardRuntime,
    receiver: mpsc::Receiver<PollStats>,
    tray_rx: mpsc::Receiver<TrayCmd>,
    /// Fires on the fixed global hotkeys (see `hotkey.rs`): Ctrl+Alt+O to
    /// flip `overlay_click_through` (same as the tray's "Lock/Unlock
    /// Overlay" row), Ctrl+Alt+P to cycle the Control Center profile.
    hotkey_rx: mpsc::Receiver<hotkey::HotkeyEvent>,
    /// Control Center (#187): events pushed by `control_task` (connect/
    /// disconnect, capabilities, profile changes, apply results), drained
    /// into `runtime.control` each frame — see `dashboard::DashboardRuntime::drain_control`.
    control_rx: mpsc::Receiver<control::ControlEvent>,
    /// Commands to `control_task` (apply a profile, refresh, release to
    /// firmware) — sent from the tray, hotkey, and Control Center window.
    control_cmd_tx: tokio::sync::mpsc::Sender<control::ControlCmd>,
    opacity: f32,
    /// Cached from settings — "on_top", "behind", or "normal".
    window_layer: String,
    tray: Tray,
    /// The dialog windows' open/focus flags and state.
    dialogs: DialogStates,
    /// `runtime.control.safety_trips` already notified about.
    seen_safety_trips: u32,
    // Shared settings (updated on save, applied each frame)
    current_settings: Arc<Mutex<settings::Settings>>,
    settings_reload: Arc<AtomicBool>,
    preferred_gpu: Arc<Mutex<Option<String>>>,
    /// Mirrors `runtime.control.active_profile` for `poll_loop`'s session-
    /// recording CSV column — `poll_loop` runs on its own tokio task with no
    /// access to `runtime`, so this is the cross-task handoff (see the
    /// `drain_control` call site in `update()`).
    active_profile_shared: Arc<Mutex<Option<String>>>,
    dir: Arc<PathBuf>,
    // Win32 HWND stored as isize for window-level opacity (SetLayeredWindowAttributes).
    // Found via FindWindowW on the first ui() frame; 0 until then.
    hwnd: isize,
    // True when a DX12 adapter was found at startup and the window was created
    // with a DComp-backed transparent swap chain (see `main()`). False on
    // hardware/drivers without DX12 (rare, but real — some VMs/RDP sessions,
    // very old GPUs): falls back to the pre-#101 opaque swap chain +
    // WS_EX_LAYERED opacity instead of crashing at startup.
    dcomp_available: bool,
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
    /// Per-panel DComp reveal-burst state for floating mode's per-pixel
    /// transparency (issue #169) — see `dcomp_burst::DcompRevealBurst`. Only
    /// a panel's creation frame triggers a burst; a later content-driven
    /// resize keeps the old, non-hiding behavior (see `render_floating_panels`).
    floating_dcomp: RefCell<HashMap<String, DcompRevealBurst>>,
    /// Shared with the heartbeat thread so it knows whether to drive parent repaints.
    floating_mode_arc: Arc<AtomicBool>,
    /// Colour palette for dialog windows — switches between dark/light based on OS theme.
    dialog_colors: theme::DialogColors,
    /// Cached OS dark-mode flag; checked each frame to detect live theme switches.
    os_dark_mode: bool,
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
    /// Shared with `poll_loop`: `Paused`/`Light` while in wallpaper mode, where
    /// the `rigstats-wallpaper` host is the dashboard's poller (see
    /// `update_wallpaper_mode`).
    poll_mode: PollModeHandle,
    /// Enter/leave decisions and the host's respawn/teardown policy.
    wallpaper: WallpaperSupervisor,
    /// The spawned `rigstats-wallpaper` host process, if any.
    wallpaper_host: ChildHost,
    /// The game overlay's window state (#183).
    overlay: OverlayState,
    // ── Tray recording indicator ───────────────────────────────────────────
    /// True while a session is being recorded — drives the blinking tray dot.
    recording_active: bool,
    /// Mirrors `recording_active` for the tray-polling background thread (see
    /// its use of `win_opacity::force_repaint` in `main()`, #177): while a
    /// context menu is open, winit's own event loop — and so `ui()` — doesn't
    /// run at all, and neither `request_repaint()`/`request_repaint_of()` nor a
    /// single pre-emptive `force_repaint()` reliably revives it once the menu
    /// closes (both empirically confirmed unreliable here). The poller instead
    /// keeps posting `force_repaint()` on every tick for as long as this is
    /// true, so the very next tick after the menu closes — whenever that is —
    /// lands a real repaint no matter how it closed.
    recording_active_shared: Arc<AtomicBool>,
    /// Current phase of the blink (dot shown vs. hidden).
    recording_blink_on: bool,
    /// When the blink last flipped.
    recording_blink_at: Instant,
    /// Font-atlas rebuilds after a minimize.
    font_atlas: FontAtlasRefresh,
    /// Shared with `SettingsWindow`; set by a startup background thread.
    battery_present: Arc<AtomicBool>,
    /// Relaunch after a lost GPU device.
    gpu_recovery: GpuRecovery,
    // ── Threshold alerts ─────────────────────────────────────────────────────
    /// Last time a notification fired for a given `"<component>_<level>"` key,
    /// e.g. `"cpu_warn"` — enforces `Settings.alert_cooldown_secs` per alert.
    alert_cooldowns: HashMap<String, Instant>,
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
        runtime: DashboardRuntime,
        receiver: mpsc::Receiver<PollStats>,
        tray_rx: mpsc::Receiver<TrayCmd>,
        hotkey_rx: mpsc::Receiver<hotkey::HotkeyEvent>,
        control_rx: mpsc::Receiver<control::ControlEvent>,
        control_cmd_tx: tokio::sync::mpsc::Sender<control::ControlCmd>,
        opacity: f32,
        dcomp_available: bool,
        tray: Tray,
        current_settings: Arc<Mutex<settings::Settings>>,
        settings_reload: Arc<AtomicBool>,
        dir: Arc<PathBuf>,
        preferred_gpu: Arc<Mutex<Option<String>>>,
        active_profile_shared: Arc<Mutex<Option<String>>>,
        floating_mode_arc: Arc<AtomicBool>,
        updater_win: Arc<Mutex<windows::updater::UpdaterState>>,
        updater_open: Arc<AtomicBool>,
        updater_focus: Arc<AtomicBool>,
        poll_mode: PollModeHandle,
        gpu_lost: Arc<AtomicBool>,
        gpu_started_at: Instant,
        gpu_retry_count: u32,
        recording_active_shared: Arc<AtomicBool>,
    ) -> Self {
        let init_settings = current_settings.lock_safe().clone();
        let init_positions: HashMap<String, [f32; 2]> = init_settings
            .panel_layouts
            .iter()
            .map(|(k, v)| (k.clone(), [v.x as f32, v.y as f32]))
            .collect();
        let gpu_names = tray.gpu_menu.names();
        // Battery presence for Settings (Battery panel / overlay metric).
        // Detected once in the background — it's a WMI query and used to run
        // (as PowerShell, ~1 s) on the UI thread every time Settings opened.
        // Starts `true` (option enabled) until the answer arrives.
        let battery_present = Arc::new(AtomicBool::new(true));
        {
            let flag = battery_present.clone();
            let dir = dir.clone();
            std::thread::spawn(move || {
                let present = hardware::detect_battery_present();
                debug::log_debug(&dir, &format!("hardware: battery_present={present}"));
                flag.store(present, Ordering::Relaxed);
            });
        }
        Self {
            runtime,
            receiver,
            tray_rx,
            hotkey_rx,
            control_rx,
            control_cmd_tx,
            opacity,
            window_layer: init_settings.window_layer.clone(),
            tray,
            dialogs: DialogStates::new(
                windows::settings::SettingsWindow::from_settings(
                    &init_settings,
                    gpu_names,
                    battery_present.clone(),
                ),
                updater_win,
                updater_open,
                updater_focus,
            ),
            seen_safety_trips: 0,
            current_settings,
            settings_reload,
            preferred_gpu,
            active_profile_shared,
            dir,
            hwnd: 0,
            dcomp_available,
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
            floating_dcomp: RefCell::new(HashMap::new()),
            floating_mode_arc,
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
            // Apply WindowLevel + opacity for the first few frames at startup when in
            // non-floating mode: the viewport builder only handles "on_top", so "behind"
            // must be sent via viewport command, and opacity needs a valid HWND which
            // may not be available until after the first paint.
            reapply_window_props_frames: if !init_settings.floating_mode { 4 } else { 0 },
            last_applied_window_size: None,
            last_fitted_height: None,
            startup_fit_frames: 12,
            poll_mode,
            wallpaper: WallpaperSupervisor::default(),
            wallpaper_host: ChildHost::default(),
            overlay: OverlayState::new(&init_settings),
            recording_active: false,
            recording_active_shared,
            recording_blink_on: true,
            font_atlas: FontAtlasRefresh {
                stale: false,
                rebuilt_at: Instant::now(),
                toggle: false,
            },
            battery_present,
            recording_blink_at: Instant::now(),
            gpu_recovery: GpuRecovery {
                lost: gpu_lost,
                relaunch_triggered: false,
                started_at: gpu_started_at,
                retry_count: gpu_retry_count,
            },
            alert_cooldowns: HashMap::new(),
        }
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
        #[cfg(windows)]
        if !self.dcomp_available {
            win_opacity::set_opacity(self.hwnd, self.opacity);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
            px, py,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(w, h)));
        self.last_fitted_height = None;
        self.reapply_window_props_frames = 4;
    }

    /// Fired once, from `ui()`, when `gpu_guard`'s device-error callbacks
    /// have flagged a fatal wgpu error (most commonly a hybrid iGPU/dGPU
    /// switch invalidating the D3D12 device mid-session). Spawns a fresh
    /// instance of the app — unless a persistent-failure budget is
    /// exhausted — and closes this one gracefully via
    /// `ViewportCommand::Close`. Deliberately never `process::exit`: an
    /// abrupt exit skips wgpu's teardown and can leak GPU/desktop-heap
    /// resources across the spawn, the same failure mode already worked
    /// around for the wallpaper host (see its `refresh_settings` doc comment).
    fn trigger_gpu_relaunch(&self, ctx: &egui::Context) {
        const MAX_RETRIES: u32 = 3;
        // Only count against the retry budget if THIS process itself died
        // within seconds of starting — a GPU error after a long healthy run
        // (the common case: hybrid-GPU switches happen at most a few times a
        // day) always gets a full, unthrottled retry.
        let fast_fail = self.gpu_recovery.started_at.elapsed() < Duration::from_secs(10);
        let attempt = if fast_fail {
            self.gpu_recovery.retry_count + 1
        } else {
            1
        };
        if attempt > MAX_RETRIES {
            debug::log_error(
                &self.dir,
                "gpu: device error repeated too quickly — giving up on relaunch \
                 (reopen RIGStats manually once the GPU state has settled)",
            );
        } else {
            match std::env::current_exe().and_then(|exe| {
                std::process::Command::new(exe)
                    .env("RIGSTATS_GPU_RETRY_COUNT", attempt.to_string())
                    .spawn()
            }) {
                Ok(_) => debug::log_warn(
                    &self.dir,
                    &format!("gpu: device error — relaunching (attempt {attempt}/{MAX_RETRIES})"),
                ),
                Err(e) => {
                    debug::log_error(&self.dir, &format!("gpu: relaunch failed — {e}"));
                }
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    #[cfg(windows)]
    /// Checks the just-arrived `self.runtime.latest` sample against
    /// `Settings.thresholds` and fires a Windows balloon-tip notification for
    /// any newly-breached, not-yet-cooled-down component. Pure comparison
    /// logic lives in `alerts::pending_alerts`; this just applies the
    /// notify-on-warn/crit gates and the per-alert cooldown, and sends.
    fn check_alerts(&mut self) {
        let (notify_warn, notify_crit, cooldown_secs, settings) = {
            let s = self.current_settings.lock_safe();
            (
                s.notify_on_warn,
                s.notify_on_crit,
                s.alert_cooldown_secs,
                s.clone(),
            )
        };
        if !notify_warn && !notify_crit {
            return;
        }
        for alert in alerts::pending_alerts(&self.runtime.latest, &settings) {
            let gated = match alert.level {
                alerts::AlertLevel::Warn => !notify_warn,
                alerts::AlertLevel::Crit => !notify_crit,
            };
            if gated {
                continue;
            }
            let level_key = match alert.level {
                alerts::AlertLevel::Warn => "warn",
                alerts::AlertLevel::Crit => "crit",
            };
            let key = format!("{}_{level_key}", alert.component);
            let due = self
                .alert_cooldowns
                .get(&key)
                .map_or(true, |last| last.elapsed().as_secs() >= cooldown_secs);
            if !due {
                continue;
            }
            self.alert_cooldowns.insert(key, Instant::now());

            let (title, icon) = match alert.level {
                alerts::AlertLevel::Warn => {
                    ("RIGStats — Warning", windows::settings::NotifyIcon::Warning)
                }
                alerts::AlertLevel::Crit => {
                    ("RIGStats — Critical", windows::settings::NotifyIcon::Error)
                }
            };
            let message = format!("{}: {:.0}", alert.label, alert.value);
            // Fire on a background thread — the notification script blocks
            // for several seconds (Start-Sleep before disposing the tray
            // icon) and must never stall a UI frame.
            std::thread::spawn(move || {
                windows::settings::send_notification(title, &message, icon);
            });
        }
    }

    /// Drive wallpaper mode each frame — the decisions are in
    /// `wallpaper_supervisor`; this carries out the window side. Wallpaper
    /// mode is on when `window_layer == "wallpaper"` and floating mode is off
    /// (floating wins if both are set).
    fn update_wallpaper_mode(&mut self, ctx: &egui::Context) {
        let want = self.window_layer == "wallpaper" && !self.floating_mode;
        let now = Instant::now();
        let effects = self.wallpaper.transition(
            want,
            || wallpaper_supervisor::host_path().map_or(true, |p| !p.exists()),
            &mut self.wallpaper_host,
            now,
        );
        // Before `supervise`: on entering, this saves the position the host
        // reads when it starts.
        self.apply_wallpaper_effects(ctx, effects);
        // Set every frame so an overlay toggle takes effect immediately.
        self.poll_mode
            .set(self.wallpaper.poll_mode(self.overlay.enabled));
        let effects = self.wallpaper.supervise(&mut self.wallpaper_host, now);
        self.apply_wallpaper_effects(ctx, effects);
    }

    fn apply_wallpaper_effects(
        &mut self,
        ctx: &egui::Context,
        effects: Vec<wallpaper_supervisor::Effect>,
    ) {
        use wallpaper_supervisor::{Effect, LogLevel};
        for effect in effects {
            match effect {
                Effect::AbortHostMissing => {
                    // Startup may already have parked the window.
                    self.window_layer = "normal".to_string();
                    {
                        let mut s = self.current_settings.lock_safe();
                        s.window_layer = "normal".to_string();
                        self.persist_settings_logged(&s);
                    }
                    self.restore_main_window(ctx);
                }
                Effect::Enter => {
                    // Hand where the user left the window in the previous
                    // (on-screen) layer to the host: position it in Normal
                    // mode, then switch to wallpaper.
                    //
                    // Guarded: on the very frame a prior mode transition's
                    // `OuterPosition` was issued, `ctx.input().outer_rect` can
                    // still report the previous position — after a quick
                    // off/on toggle that can be the off-screen parked
                    // coordinate, which would send the host to the wrong
                    // monitor (or off every monitor). Only trust a
                    // `last_fixed_pos` on a connected monitor; otherwise keep
                    // the saved `wallpaper_position`.
                    if let Some([x, y]) = self.last_fixed_pos {
                        if is_position_on_screen([x, y]) {
                            let mut s = self.current_settings.lock_safe();
                            s.wallpaper_position = Some([x.round() as i32, y.round() as i32]);
                            self.persist_settings_logged(&s);
                        }
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                        -32000.0, -32000.0,
                    )));
                }
                Effect::Leave => self.restore_main_window(ctx),
                Effect::Log(LogLevel::Debug, m) => debug::log_debug(&self.dir, &m),
                Effect::Log(LogLevel::Warn, m) => debug::log_warn(&self.dir, &m),
                Effect::Log(LogLevel::Error, m) => debug::log_error(&self.dir, &m),
            }
        }
    }

    /// Show/hide the overlay's own secondary viewport this frame, independent
    /// of `window_layer`/`floating_mode` — the whole point of the add-on
    /// design is that it coexists with whatever the main window is doing.
    /// Called unconditionally every frame; a no-op if `overlay_enabled` and
    /// `overlay_active` (last frame's shown-state) are both false.
    ///
    /// Mirrors `render_floating_panels`' per-viewport shape: size and anchor
    /// position are recomputed from current settings every shown frame (cheap
    /// pure functions) and simply passed into the `ViewportBuilder`, which
    /// egui diffs and only actually moves/resizes the OS window when the
    /// value changes — so a metric-list/layout/scale/anchor/margin edit is
    /// picked up live with no manual "did it change" bookkeeping needed.
    /// `"free"`-anchor position is the one exception: after the first shown
    /// frame of an activation it's OS-owned (via drag), matching floating
    /// panels' `panels_positioned` — re-sending it every frame would fight
    /// the OS and cause sub-pixel blur.
    fn render_overlay_viewport(&mut self, ctx: &egui::Context) {
        let want = self.overlay.enabled;
        if want && !self.overlay.active {
            self.overlay.active = true;
            self.overlay.positioned = false;
            // Fresh window each activation (the old one was destroyed on
            // hide) — force the resize-detection block below to treat this
            // activation's first size as "first ever" again, so it calls
            // `overlay_dcomp.start()` (which keeps the window hidden until
            // the reapply burst below settles — the user must never see the
            // transient DWM-redirection-bitmap white box, only the final,
            // correctly-composited frame) instead of being silently skipped
            // because a *previous* activation already recorded a size.
            self.overlay.last_size = None;
            // Same reasoning applies to the MousePassthrough guard below:
            // the new window starts out click-through-less (it's a brand
            // new HWND), but `overlay_click_through`'s logical value may be
            // unchanged from the last activation, so the "only send on
            // change" guard would otherwise skip resending it here — leaving
            // the new window stuck capturing mouse input.
            self.overlay.last_applied_click_through = None;
            debug::log_debug(&self.dir, "overlay: showing");
        } else if !want && self.overlay.active {
            self.overlay.active = false;
            debug::log_debug(&self.dir, "overlay: hiding");
        }
        if !want {
            return;
        }

        let (metrics, columns, anchor, margin, scale, opacity, background) = {
            let s = self.current_settings.lock_safe();
            (
                s.overlay_metrics.clone(),
                s.overlay_columns.min(6) as usize,
                s.overlay_anchor.clone(),
                s.overlay_margin as f32,
                s.overlay_scale.clamp(0.5, 2.0) as f32,
                s.overlay_opacity.clamp(0.05, 1.0) as f32,
                s.overlay_background,
            )
        };
        let measured = estimate_window_size(ctx, &metrics, columns, scale, background);
        let size = [measured.x, measured.y];
        if self.overlay.last_size != Some(size) {
            // Only the very first size (right after activation,
            // self.overlay.last_size still None) gets the full hide/burst
            // treatment. A later resize (e.g. dragging Scale) just resizes
            // silently, same as floating panels' later content-driven
            // resizes (#169) — confirmed by direct visual testing that the
            // DComp binding, once correctly established by the activation
            // burst, survives an ordinary resize without needing to be
            // re-applied every time. An earlier finding (from *before* this
            // session's hide/burst fix existed) had the white-box artifact
            // appearing right after changing Scale; that no longer holds now
            // that the underlying fix is actually correct. Scale is read
            // live here (no debounce) since a resize no longer re-triggers
            // any hide/reveal flicker for the debounce to guard against.
            let is_first_size = self.overlay.last_size.is_none();
            self.overlay.last_size = Some(size);
            if is_first_size {
                self.overlay.dcomp.start(self.dcomp_available);
            }
        }

        // Anchors to the primary monitor (origin at 0,0) — a game overlay's
        // whole purpose is sitting on the display the user is actually
        // looking at; `"free"` drag lets the user move it elsewhere.
        let monitor = monitor_rect_at([0.0, 0.0]).unwrap_or([0.0, 0.0, 1920.0, 1080.0]);
        let inset = content_inset(scale, background);
        let inset = [inset.x, inset.y];
        // A "free" position's content sits at window_origin + inset (top-left
        // aligned — see draw_overlay's h_align/content_layout for "free").
        // If the inset just changed (Background/Scale edited while already
        // dragged), shift the saved origin by the same delta and resend it
        // this frame so the content itself doesn't move — otherwise the
        // window would keep its old origin and the new padding would push
        // the content inward by the delta instead.
        let mut inset_compensated = false;
        if let (Some(prev), Some(pos)) = (self.overlay.last_content_inset, self.overlay.position) {
            if prev != inset {
                self.overlay.position =
                    Some([pos[0] - (inset[0] - prev[0]), pos[1] - (inset[1] - prev[1])]);
                inset_compensated = true;
            }
        }
        self.overlay.last_content_inset = Some(inset);
        let needs_position = !self.overlay.positioned;
        let pos: Option<[f32; 2]> = if anchor == "free" {
            if needs_position {
                let fallback = overlay_anchor_position("top-right", margin, size, monitor, inset);
                Some(
                    self.overlay
                        .position
                        .map(|p| guard_panel_position(p, fallback))
                        .unwrap_or(fallback),
                )
            } else if inset_compensated {
                self.overlay.position
            } else {
                None // OS/drag owns it now — don't fight it by resending.
            }
        } else {
            // Non-"free" anchors are never user-dragged (drag always claims
            // "free" immediately, see below), so it's always safe — and
            // necessary, since a resize changes where the anchored corner
            // sits — to keep recomputing and applying this every frame.
            Some(overlay_anchor_position(
                &anchor, margin, size, monitor, inset,
            ))
        };
        if needs_position {
            self.overlay.positioned = true;
        }

        // True per-pixel DComp transparency, without a CentralPanel wrapper.
        // Confirmed working (activation and live Scale-drag resize both stay
        // artifact-free) once paired with the hide/reapply-burst/reveal
        // sequence below — see `overlay_pending_reveal`'s doc for why that's
        // needed. If DComp is ever unavailable on a given system, the
        // `dcomp_available` branches throughout this function fall back to
        // whole-window opacity instead (see the `overlay-addon-wholewindow-fade`
        // tag for that fallback's history).
        let mut vp_builder = egui::ViewportBuilder::default()
            .with_title("RigStats \u{2014} Overlay")
            .with_inner_size(size)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_transparent(self.dcomp_available)
            // Held hidden until the reapply burst below settles, so the
            // very first frame the user ever sees is already correctly
            // per-pixel composited — see `DcompRevealBurst`'s doc.
            .with_visible(!self.overlay.dcomp.pending_reveal());
        if let Some([x, y]) = pos {
            vp_builder = vp_builder.with_position([x, y]);
        }

        let th = self.runtime.app_theme;
        let latest = &self.runtime.latest;
        let thresholds = &self.runtime.thresholds;
        let click_through = self.overlay.click_through;
        let dcomp_available = self.dcomp_available;
        // The overlay's own hwnd stays hidden (`with_visible(false)`) for
        // most of the reveal burst below — invalidating a hidden window
        // doesn't reliably queue a dispatched WM_PAINT the way it does for a
        // visible one, so `force_repaint(hwnd)` alone left the burst mostly
        // stalled on the app's ~1 fps idle heartbeat instead of running at
        // "as fast as possible" (observed: revealing the overlay took
        // 3-5 s instead of a fraction of a second). The main window is
        // always visible, so forcing its repaint too reliably drives the
        // next `ui()` call regardless of the overlay's own visibility state.
        let main_hwnd = self.hwnd;
        let mut applied_click_through = self.overlay.last_applied_click_through;
        let current_settings = self.current_settings.clone();
        let dir = self.dir.clone();
        let mut drag_pos: Option<[f32; 2]> = None;
        let dcomp = &mut self.overlay.dcomp;

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("overlay"),
            vp_builder,
            |child_ui, _class| {
                #[cfg(windows)]
                {
                    if dcomp_available {
                        let tick = dcomp.tick(
                            || win_opacity::find_hwnd("RigStats \u{2014} Overlay"),
                            dcomp_available,
                        );
                        if tick.just_started {
                            // Force a genuine size *change* (not just a style
                            // bit) — empirically, the swap chain only seems to
                            // pick up per-pixel-alpha support on an actual
                            // resize/reconfigure, not merely from
                            // WS_EX_NOREDIRECTIONBITMAP being set (which alone
                            // leaves a region of the surface stuck showing
                            // opaque white). Nudge to a different size now;
                            // the builder's real target `size` (sent every
                            // frame regardless) snaps it back next frame,
                            // triggering a second real reconfigure —
                            // mirroring exactly what manually dragging Scale
                            // down-then-up was observed to fix.
                            child_ui
                                .ctx()
                                .send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                    egui::Vec2::new(size[0] + 2.0, size[1]),
                                ));
                        }
                        if tick.reapply_style {
                            win_opacity::set_no_redirection_bitmap(dcomp.hwnd());
                        }
                        if tick.force_repaint {
                            // A style/frame change alone doesn't reliably
                            // force DWM to recomposite the DComp visual at its
                            // new size — nudge a real repaint too, and force
                            // the next frame to arrive quickly rather than
                            // waiting for the app's normal ~1 Hz tick, so the
                            // whole burst actually lands within a fraction of
                            // a second. `force_repaint` no-ops on a 0 hwnd, so
                            // this is safe even while still waiting to find it.
                            win_opacity::force_repaint(dcomp.hwnd());
                            win_opacity::force_repaint(main_hwnd);
                            child_ui.ctx().request_repaint();
                        }
                        if tick.reveal {
                            // Burst settled (or the safety timeout hit) —
                            // reveal now instead of waiting for the builder
                            // diff on the next frame.
                            child_ui
                                .ctx()
                                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        }
                    } else {
                        let hwnd = win_opacity::find_hwnd("RigStats \u{2014} Overlay");
                        win_opacity::set_opacity(hwnd, opacity);
                    }
                }

                // The one and only place MousePassthrough is sent for this
                // viewport: guarded so it's only dispatched on an actual change.
                if applied_click_through != Some(click_through) {
                    applied_click_through = Some(click_through);
                    child_ui
                        .ctx()
                        .send_viewport_cmd(egui::ViewportCommand::MousePassthrough(click_through));
                }

                let resp = draw_overlay(
                    child_ui, &th, latest, thresholds, &metrics, columns, &anchor, scale, opacity,
                    background,
                );

                // Always draggable when unlocked, regardless of the configured
                // anchor — matching every other draggable element in this app
                // (no separate "enable dragging" mode to find first). Starting
                // a drag claims free positioning immediately (persisted right
                // away) so the anchor logic above doesn't fight the drag by
                // recomputing the anchored position next frame.
                if !click_through {
                    if resp.response.drag_started() && anchor != "free" {
                        let mut s = current_settings.lock_safe();
                        s.overlay_anchor = "free".to_string();
                        if let Err(e) = settings::persist_settings(&dir, &s) {
                            debug::log_error(&dir, &format!("settings: persist failed — {e}"));
                        }
                    }
                    if resp.response.drag_started() {
                        child_ui
                            .ctx()
                            .send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    // Only track/persist position while an actual drag is in
                    // progress or just finished — reading the window's current
                    // position on every ordinary frame would otherwise overwrite
                    // a saved free-drag position whenever a non-"free" anchor
                    // moves the window on its own (a margin change, etc.).
                    if resp.response.dragged() || resp.response.drag_stopped() {
                        if let Some(outer) = child_ui.ctx().input(|i| i.viewport().outer_rect) {
                            drag_pos = Some([outer.left().round(), outer.top().round()]);
                        }
                    }
                }
            },
        );

        self.overlay.last_applied_click_through = applied_click_through;
        if let Some(pos) = drag_pos {
            if self.overlay.position != Some(pos) {
                self.overlay.position = Some(pos);
                self.overlay.position_dirty = true;
            }
        }
        if self.overlay.position_dirty {
            self.overlay.position_dirty = false;
            if let Some([x, y]) = self.overlay.position {
                let mut s = self.current_settings.lock_safe();
                s.overlay_position = Some([x.round() as i32, y.round() as i32]);
                self.persist_settings_logged(&s);
            }
        }
    }

    /// Flips `overlay_click_through`, persists it immediately, and updates
    /// the tray row's label/icon. Shared by the tray's "Lock/Unlock Overlay"
    /// row and the global hotkey (`hotkey.rs`) — both are just different
    /// triggers for the same action.
    fn toggle_overlay_lock(&mut self) {
        let mut s = self.current_settings.lock_safe();
        s.overlay_click_through = !s.overlay_click_through;
        self.overlay.click_through = s.overlay_click_through;
        self.persist_settings_logged(&s);
        drop(s);
        self.tray.set_overlay_lock(self.overlay.click_through);
    }

    /// Flips `window_layer` between `"overlay"` and `"normal"` — i.e. show/hide
    /// the overlay itself, the way a game-overlay hotkey is expected to work
    /// (distinct from `toggle_overlay_lock`, which only locks/unlocks
    /// click-through on an already-visible overlay). Shared by the tray's
    /// "Toggle Overlay Mode" row and the global hotkey.
    ///
    /// Overlay is mutually exclusive with floating/fullscreen/pinned
    /// dashboard, same as Desktop Wallpaper — force them off on entry
    /// (one-way, like the Settings dialog's own layer-switch handler).
    fn toggle_overlay_mode(&mut self) {
        let mut s = self.current_settings.lock_safe();
        s.overlay_enabled = !s.overlay_enabled;
        self.overlay.enabled = s.overlay_enabled;
        self.persist_settings_logged(&s);
        // `render_overlay_viewport` (called every frame) picks up the change
        // on the next frame and shows/hides the overlay's own viewport —
        // independent of window_layer/floating_mode, so nothing else here
        // needs to change.
    }

    /// Sends `ControlCmd::ApplyProfile` to `control_task` — shared by the
    /// tray's profile submenu and `cycle_profile`. Fire-and-forget: the
    /// result comes back later as a `ControlEvent::ApplyResult`/
    /// `ActiveProfile`, folded into `runtime.control` by `drain_control`.
    fn apply_profile(&mut self, id: String) {
        let _ = self
            .control_cmd_tx
            .try_send(control::ControlCmd::ApplyProfile(id));
    }

    /// Ctrl+Alt+P: advances to the next profile in `runtime.control.profiles`
    /// (wrapping), applies it, and fires a toast — same notification
    /// mechanism as temperature alerts (`check_alerts`) — so the switch is
    /// visible even when the dashboard itself isn't (floating/overlay/
    /// wallpaper modes).
    fn cycle_profile(&mut self) {
        let profiles = &self.runtime.control.profiles;
        if profiles.is_empty() {
            return;
        }
        let current_idx = self
            .runtime
            .control
            .active_profile
            .as_deref()
            .and_then(|active| profiles.iter().position(|p| p.id == active));
        let next = &profiles[current_idx.map_or(0, |i| (i + 1) % profiles.len())];
        let (id, name) = (next.id.clone(), next.name.clone());
        self.apply_profile(id);
        std::thread::spawn(move || {
            windows::settings::send_notification(
                "RIGStats — Profile",
                &name,
                windows::settings::NotifyIcon::Info,
            );
        });
    }

    /// One warning toast per critical-temperature fan override (#188), in the
    /// same notification style as temperature alerts. The service sends
    /// `safety_tripped` only on entering the critical state, so each counted
    /// event is a new trip.
    fn notify_fan_safety_trip(&mut self) {
        let control = &self.runtime.control;
        if control.safety_trips <= self.seen_safety_trips {
            return;
        }
        self.seen_safety_trips = control.safety_trips;
        let reason = control
            .safety_tripped
            .clone()
            .unwrap_or_else(|| "Critical temperature".to_owned());
        std::thread::spawn(move || {
            windows::settings::send_notification(
                "RIGStats — Fan safety",
                &format!("{reason}. All controlled fans set to 100 %."),
                windows::settings::NotifyIcon::Warning,
            );
        });
    }

    /// Works around lost font-atlas uploads, seen on a hybrid-GPU laptop at
    /// 175 % scaling after Win+D / minimize: random characters in the overlay
    /// rendered as blank gaps — a digit, `.`, `%` or `°` missing in one place
    /// but fine in another (egui caches glyphs per sub-pixel offset, so the
    /// same character at another x position is a separate atlas entry).
    ///
    /// egui keeps one font atlas shared by every viewport and uploads new
    /// glyphs as part of whichever viewport's pass ends next; a glyph whose
    /// upload is lost stays marked as cached but never reaches the GPU.
    /// egui-wgpu 0.34 drops the upload on a surface-less early return (and
    /// eframe skips it for a non-visible root) — fixed upstream in egui 0.35
    /// (emilk/egui#8250). This forces a full, fresh atlas upload after any
    /// observed minimize and periodically as a self-heal. Remove after the
    /// egui upgrade (#199).
    fn refresh_font_atlas_after_minimize(&mut self, ctx: &egui::Context) {
        let any_minimized =
            ctx.input(|i| i.raw.viewports.values().any(|v| v.minimized == Some(true)));
        if any_minimized {
            self.font_atlas.stale = true;
            return;
        }
        // Also rebuild periodically: a Win+D minimizes the root window too,
        // and a minimized root runs no frames at all, so the flag above never
        // gets a chance to see it — and uploads can also be lost with nothing
        // minimized (any surface-less viewport pass, e.g. the overlay while
        // it's hidden for its reveal burst). Cheap: re-rasterizes the ~100
        // glyphs in use.
        const PERIODIC: Duration = Duration::from_secs(30);
        if std::mem::take(&mut self.font_atlas.stale)
            || self.font_atlas.rebuilt_at.elapsed() >= PERIODIC
        {
            self.font_atlas.rebuilt_at = Instant::now();
            // The app uses egui's default fonts (text sizes are set via the
            // style in `theme::apply_dashboard_fonts`, not here) — but
            // `set_fonts` only schedules a rebuild if the passed-in
            // `FontDefinitions` differs from what's currently active (see
            // `Context::set_fonts`), so passing `default()` unchanged every
            // time is a silent no-op forever. Alternate a sub-pixel,
            // invisible nudge so it always compares unequal.
            self.font_atlas.toggle = !self.font_atlas.toggle;
            ctx.set_fonts(Self::font_definitions_for_rebuild(self.font_atlas.toggle));
        }
    }

    /// `FontDefinitions::default()` with an imperceptible nudge to the
    /// primary font's tweak, alternated by `toggle` — see the call site in
    /// `refresh_font_atlas_after_minimize` for why this is needed instead of
    /// just passing `default()` directly.
    fn font_definitions_for_rebuild(toggle: bool) -> egui::FontDefinitions {
        let mut defs = egui::FontDefinitions::default();
        if let Some(hack) = defs.font_data.get_mut("Hack") {
            let mut data = (**hack).clone();
            data.tweak.scale = if toggle { 1.0 + f32::EPSILON } else { 1.0 };
            *hack = std::sync::Arc::new(data);
        }
        defs
    }

    /// Shared tail of every dialog's per-frame render (Settings, About,
    /// Status, History, Updater), after its `show_viewport_immediate`:
    /// dark title bar, hidden-until-rendered reveal (see `dialog_reveal`),
    /// and bringing a newly opened dialog to the foreground.
    ///
    /// `visible` is what this frame's `ViewportBuilder` asked for. While
    /// still hidden, frames are forced so the reveal isn't left waiting on
    /// the ~1 fps idle heartbeat — `request_repaint` plus a real WM_PAINT to
    /// the main window, the same combination `render_overlay_viewport` uses
    /// for its hidden reveal burst. Focus waits until the dialog is visible
    /// (a hidden window can't take the foreground), so the flag is kept set
    /// until then.
    fn finish_dialog_frame(
        &mut self,
        ctx: &egui::Context,
        id: &'static str,
        found_hwnd: isize,
        wants_focus: bool,
        focus: &Arc<AtomicBool>,
        visible: bool,
    ) {
        self.dialogs.dialog_reveal.rendered(id);
        #[cfg(windows)]
        {
            win32_dark_mode::apply_titlebar_theme(found_hwnd, self.os_dark_mode);
            // Set while the dialog is open so it's in place before it closes.
            win_opacity::disable_dwm_transitions(found_hwnd);
        }
        if !visible {
            ctx.request_repaint();
            #[cfg(windows)]
            win_opacity::force_repaint(self.hwnd);
        }
        if wants_focus {
            // If found_hwnd == 0 the window wasn't ready yet; keep the flag
            // so we retry on the next frame.
            #[cfg(windows)]
            if found_hwnd != 0 && visible {
                win_opacity::bring_to_foreground(found_hwnd);
            } else {
                focus.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Sets and persists the displayed GPU (`None` = automatic, highest VRAM).
    /// Shared by the GPU panel's click dots, the tray "GPU" submenu, and the
    /// floating GPU panel. The wallpaper host picks the persisted value up on
    /// its next settings refresh.
    fn select_gpu(&mut self, pref: Option<String>) {
        let mut s = self.current_settings.lock_safe();
        s.preferred_gpu = pref;
        self.persist_settings_logged(&s);
        let pref = s.preferred_gpu.clone();
        drop(s);
        self.apply_preferred_gpu(pref);
        // Keep an open Settings dialog's draft in sync, otherwise its next
        // live-preview push (or Cancel) would revert this change.
        let mut win = self.dialogs.settings_win.lock_safe();
        win.draft.preferred_gpu = self.preferred_gpu.lock_safe().clone();
        win.original.preferred_gpu = win.draft.preferred_gpu.clone();
    }

    /// Hands `pref` to the poll loop and ticks the matching tray row.
    fn apply_preferred_gpu(&self, pref: Option<String>) {
        self.tray.gpu_menu.set_selected(pref.as_deref());
        *self.preferred_gpu.lock_safe() = pref;
    }
}

impl eframe::App for RigStatsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.dcomp_available {
            // Premultiplied, opacity-driven fill for the DComp-composited swap
            // chain (see `wgpu_options` in `main()`) — replaces the old
            // WS_EX_LAYERED-based window opacity with selective per-pixel
            // transparency: only the panel background fades (this fill, plus
            // theme::panel_frame's premultiply), while text/gauges/bars/graphs
            // stay fully opaque (issue #101).
            let c = theme::premul(theme::PANEL_FILL, self.opacity);
            [
                c.r() as f32 / 255.0,
                c.g() as f32 / 255.0,
                c.b() as f32 / 255.0,
                c.a() as f32 / 255.0,
            ]
        } else {
            // No DX12 adapter at startup — opaque swap chain; opacity is
            // applied at the window level via WS_EX_LAYERED instead (see
            // `win_opacity::set_opacity` call sites, gated the same way).
            let c = theme::PANEL_FILL;
            [
                c.r() as f32 / 255.0,
                c.g() as f32 / 255.0,
                c.b() as f32 / 255.0,
                1.0,
            ]
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // A fatal wgpu device error (see `gpu_guard`) was flagged from off the
        // UI thread. Don't touch the (likely dead) device any further this
        // frame — kick off the relaunch-and-close sequence once and bail.
        if self.gpu_recovery.lost.load(Ordering::Relaxed) {
            if !self.gpu_recovery.relaunch_triggered {
                self.gpu_recovery.relaunch_triggered = true;
                self.trigger_gpu_relaunch(ui.ctx());
            }
            return;
        }

        // On the first frame: locate the HWND. With a DComp swap chain
        // (dcomp_available), apply WS_EX_NOREDIRECTIONBITMAP once — required
        // for it to actually composite (see `set_no_redirection_bitmap`'s doc
        // comment and issue #101); opacity itself is then applied continuously
        // via `clear_color`/`draw_one_panel`. Otherwise (no DX12 adapter at
        // startup), fall back to the old WS_EX_LAYERED opacity mechanism.
        #[cfg(windows)]
        if self.hwnd == 0 {
            self.hwnd = win_opacity::find_hwnd("RigStats");
            if self.hwnd != 0 {
                if self.dcomp_available {
                    win_opacity::set_no_redirection_bitmap(self.hwnd);
                } else {
                    win_opacity::set_opacity(self.hwnd, self.opacity);
                }
            }
        }

        self.refresh_font_atlas_after_minimize(ui.ctx());

        // Drive the wallpaper-host lifecycle (spawn/supervise/teardown).
        #[cfg(windows)]
        self.update_wallpaper_mode(ui.ctx());

        // Global hotkeys — drained here, before `render_overlay_viewport`
        // below, so a keypress takes effect the same frame it's noticed
        // instead of one frame late.
        while let Ok(event) = self.hotkey_rx.try_recv() {
            match event {
                // Same action as the tray's "Toggle Overlay Mode" row:
                // show/hide the overlay itself, the conventional in-game-
                // overlay hotkey behavior (not the click-through lock, which
                // has no reason to be toggled as often and stays tray/
                // Settings-only).
                hotkey::HotkeyEvent::ToggleOverlay => self.toggle_overlay_mode(),
                hotkey::HotkeyEvent::CycleProfile => self.cycle_profile(),
            }
        }

        // Show/hide the overlay's own add-on viewport — independent of
        // window_layer/floating_mode, so it coexists with whatever the main
        // window below is doing.
        self.render_overlay_viewport(ui.ctx());

        // Re-apply WindowLevel for a few frames after a floating→non-floating
        // transition, because winit may reset the window level when it processes the move.
        if self.reapply_window_props_frames > 0 {
            self.reapply_window_props_frames -= 1;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    Self::window_level_from_layer(&self.window_layer),
                ));
            #[cfg(windows)]
            {
                if !self.dcomp_available {
                    win_opacity::set_opacity(self.hwnd, self.opacity);
                }
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
        let new_stats = self.runtime.drain(&self.receiver);
        if new_stats {
            self.check_alerts();
        }

        // Control Center (#187): fold any control-pipe events (connect/
        // disconnect, capabilities, profile list/active changes, apply
        // results) into `runtime.control`. Same drain shape as telemetry,
        // on its own channel/cadence — never blocks on the pipe.
        if self.runtime.drain_control(&self.control_rx) {
            self.tray.profile_menu.sync(
                &self.runtime.control.profiles,
                self.runtime.control.active_profile.as_deref(),
            );
            self.tray
                .set_lamp_available(self.runtime.control.has_lamp());
            // Hand off to poll_loop for the session-recording CSV column —
            // poll_loop runs on its own tokio task with no access to `runtime`.
            *self.active_profile_shared.lock_safe() = self.runtime.control.active_profile.clone();
            self.notify_fan_safety_trip();
        }

        // Apply settings saved from the settings window.
        if self.settings_reload.swap(false, Ordering::Relaxed) {
            let s = self.current_settings.lock_safe();
            let prev_visible = self.runtime.visible_panels.clone();
            let panels_changed = self.runtime.apply_settings(&s);
            // Any panel that was visible before but is now hidden must be removed
            // from panels_positioned so its saved position is re-applied when it
            // reappears (e.g. after cancel reverts a live preview toggle).
            if panels_changed {
                for key in &prev_visible {
                    if !self.runtime.visible_panels.contains(key) {
                        self.panels_positioned.remove(key);
                    }
                }
            }
            self.opacity = s.opacity.clamp(0.1, 1.0) as f32;
            self.window_layer = s.window_layer.clone();
            let was_floating = self.floating_mode;
            self.floating_mode = s.floating_mode;
            self.floating_mode_arc
                .store(self.floating_mode, Ordering::Relaxed);
            self.floating_panels_locked = s.floating_panels_locked;
            self.floating_panel_scale = s.floating_panel_scale.clamp(0.4, 1.0) as f32;
            self.overlay.click_through = s.overlay_click_through;
            self.overlay.enabled = s.overlay_enabled;
            let preferred_gpu = s.preferred_gpu.clone();
            let was_fullscreen = self.fullscreen_mode;
            self.fullscreen_mode = s.fullscreen_mode;
            self.fullscreen_align = s.fullscreen_align.clone();
            self.dashboard_pinned = s.dashboard_pinned;
            let profile = s.dashboard_profile.clone();
            drop(s);
            // Settings → Display → GPU (live preview, Save, or Cancel revert).
            self.apply_preferred_gpu(preferred_gpu);
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
            if !self.floating_mode && !self.wallpaper.is_active() {
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
            if !self.dcomp_available {
                win_opacity::set_opacity(self.hwnd, self.opacity);
            }
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

        // Fill the tray "GPU" submenu and Settings' GPU list in once the
        // background adapter detection reports back.
        let pref = self.preferred_gpu.lock_safe().clone();
        if let Some(names) = self.tray.gpu_menu.poll(pref.as_deref()) {
            self.dialogs.settings_win.lock_safe().set_gpu_names(names);
        }

        self.handle_tray_commands(ui.ctx());

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
        let any_dialog_open = self.dialogs.settings_open.load(Ordering::Relaxed)
            || self.dialogs.about_open.load(Ordering::Relaxed)
            || self.dialogs.status_open.load(Ordering::Relaxed)
            || self.dialogs.updater_open.load(Ordering::Relaxed)
            || self.dialogs.history_open.load(Ordering::Relaxed)
            || self.dialogs.control_open.load(Ordering::Relaxed);
        if !any_dialog_open && self.dialogs.any_dialog_open_prev {
            let mut vis = egui::Visuals::dark();
            vis.panel_fill = egui::Color32::TRANSPARENT;
            vis.window_fill = egui::Color32::from_gray(28);
            vis.override_text_color = Some(theme::C_TEXT);
            ui.ctx().set_visuals(vis);
        }
        self.dialogs.any_dialog_open_prev = any_dialog_open;

        for (id, open) in [
            ("settings", &self.dialogs.settings_open),
            ("about", &self.dialogs.about_open),
            ("status", &self.dialogs.status_open),
            ("history", &self.dialogs.history_open),
            ("updater", &self.dialogs.updater_open),
            ("control", &self.dialogs.control_open),
        ] {
            self.dialogs
                .dialog_reveal
                .track(id, open.load(Ordering::Relaxed));
        }
        // A dialog that closed this frame gets one last, content-less frame
        // that only hides its window, so eframe drops its GPU surface from an
        // already-hidden window next frame (no white flash on close — see
        // `DialogReveal::take_closing`). Only `visible` is set on the
        // builder: egui only applies fields that are set, so size/position
        // are left untouched.
        let closing = self.dialogs.dialog_reveal.take_closing();
        if !closing.is_empty() {
            for id in closing {
                ui.ctx().show_viewport_immediate(
                    egui::ViewportId::from_hash_of(id),
                    egui::ViewportBuilder::default().with_visible(false),
                    |_, _| {},
                );
            }
            // Make the teardown frame come promptly.
            ui.ctx().request_repaint();
            #[cfg(windows)]
            win_opacity::force_repaint(self.hwnd);
        }

        if self.dialogs.settings_open.load(Ordering::Relaxed) {
            let open = self.dialogs.settings_open.clone();
            let focus = self.dialogs.settings_focus.clone();
            let state = self.dialogs.settings_win.clone();
            let dir = self.dir.clone();
            let saved = self.current_settings.clone();
            let reload = self.settings_reload.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(560.0, 600.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let dcomp_available = self.dcomp_available;
            let visible = self.dialogs.dialog_reveal.visible("settings");
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("settings"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Settings")
                    .with_visible(visible)
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
                        dcomp_available,
                    );
                },
            );
            self.finish_dialog_frame(
                ui.ctx(),
                "settings",
                found_hwnd,
                wants_focus,
                &focus,
                visible,
            );
        }

        if self.dialogs.about_open.load(Ordering::Relaxed) {
            let open = self.dialogs.about_open.clone();
            let focus = self.dialogs.about_focus.clone();
            let dir = self.dir.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(360.0, 280.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let visible = self.dialogs.dialog_reveal.visible("about");
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("about"),
                egui::ViewportBuilder::default()
                    .with_title("About RigStats")
                    .with_visible(visible)
                    .with_inner_size([380.0, 460.0])
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
            self.finish_dialog_frame(ui.ctx(), "about", found_hwnd, wants_focus, &focus, visible);
        }

        if self.dialogs.control_open.load(Ordering::Relaxed) {
            let open = self.dialogs.control_open.clone();
            let focus = self.dialogs.control_focus.clone();
            let mctx = main_ctx.clone();
            let cmd_tx = self.control_cmd_tx.clone();
            let [px, py] = dialog_center(780.0, 640.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let visible = self.dialogs.dialog_reveal.visible("control");
            let control_state = self.runtime.control.clone();
            let control_ui = &mut self.dialogs.control_ui;
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("control"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Control Center")
                    .with_visible(visible)
                    .with_inner_size([780.0, 640.0])
                    .with_position([px, py])
                    .with_resizable(false)
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("RigStats — Control Center");
                    }
                    windows::control::show(
                        child_ui.ctx(),
                        &mctx,
                        &open,
                        &focus,
                        &control_state,
                        &cmd_tx,
                        &dc,
                        control_ui,
                    );
                },
            );
            self.finish_dialog_frame(
                ui.ctx(),
                "control",
                found_hwnd,
                wants_focus,
                &focus,
                visible,
            );
        }

        if self.dialogs.status_open.load(Ordering::Relaxed) {
            let open = self.dialogs.status_open.clone();
            let focus = self.dialogs.status_focus.clone();
            let state = self.dialogs.status_win.clone();
            let refreshing = self.dialogs.status_refreshing.clone();
            let collecting = self.dialogs.status_collecting.clone();
            let dir = self.dir.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(680.0, 820.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let lhm_connected = self.runtime.latest.lhm_connected;
            let wallpaper_active = self.wallpaper.is_active();
            let control_state = self.runtime.control.clone();
            let visible = self.dialogs.dialog_reveal.visible("status");
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("status"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Status")
                    .with_visible(visible)
                    .with_inner_size([680.0, 820.0])
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
                        &control_state,
                        &dc,
                    );
                },
            );
            self.finish_dialog_frame(ui.ctx(), "status", found_hwnd, wants_focus, &focus, visible);
        }

        if self.dialogs.history_open.load(Ordering::Relaxed) {
            let open = self.dialogs.history_open.clone();
            let focus = self.dialogs.history_focus.clone();
            let state = self.dialogs.history_win.clone();
            let refreshing = self.dialogs.history_refreshing.clone();
            let loading_rows = self.dialogs.history_loading_rows.clone();
            let dir = self.dir.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(820.0, 720.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let visible = self.dialogs.dialog_reveal.visible("history");
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("history"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats — Session History")
                    .with_visible(visible)
                    .with_inner_size([820.0, 720.0])
                    .with_position([px, py])
                    .with_taskbar(false)
                    .with_icon(load_app_icon())
                    .with_always_on_top(),
                |child_ui, _class| {
                    #[cfg(windows)]
                    {
                        found_hwnd = win_opacity::find_hwnd("RigStats \u{2014} Session History");
                    }
                    windows::history::show(
                        child_ui.ctx(),
                        &mctx,
                        &open,
                        &focus,
                        &state,
                        &refreshing,
                        &loading_rows,
                        &dir,
                        &dc,
                    );
                },
            );
            self.finish_dialog_frame(
                ui.ctx(),
                "history",
                found_hwnd,
                wants_focus,
                &focus,
                visible,
            );
        }

        if self.dialogs.updater_open.load(Ordering::Relaxed) {
            let open = self.dialogs.updater_open.clone();
            let focus = self.dialogs.updater_focus.clone();
            let state = self.dialogs.updater_win.clone();
            let mctx = main_ctx.clone();
            let [px, py] = dialog_center(490.0, 560.0);
            let wants_focus = focus.load(Ordering::Relaxed);
            let mut found_hwnd: isize = 0;
            let visible = self.dialogs.dialog_reveal.visible("updater");
            ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("updater"),
                egui::ViewportBuilder::default()
                    .with_title("RigStats Update")
                    .with_visible(visible)
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
            self.finish_dialog_frame(
                ui.ctx(),
                "updater",
                found_hwnd,
                wants_focus,
                &focus,
                visible,
            );

            // When the window sets status to Checking (manual button click),
            // run check+download on a plain OS thread — it blocks, and
            // tokio::spawn is not safe to call from the egui UI thread
            // (which is not inside a tokio async context).
            let start_check = {
                let s = self.dialogs.updater_win.lock_safe();
                matches!(s.status, windows::updater::UpdateStatus::Checking) && !s.busy
            };
            if start_check {
                let win = self.dialogs.updater_win.clone();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    update_flow::run_check_and_download(&win, &ctx, update_flow::Trigger::Manual);
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
            let float_pref = self.float_new_pref_gpu.lock_safe().take();
            if let Some(new_pref) = float_pref {
                self.select_gpu(Some(new_pref));
            }
        } else {
            // ── Fixed mode — all panels in one portrait/landscape window ──────

            // Track the window's current outer position so the padlock can pin
            // the exact spot it is at when clicked, and so a profile change can
            // carry the window over to its current spot. Skip while the window is
            // parked off-screen for wallpaper mode, which would otherwise overwrite
            // the real position with the parking coordinates.
            if !self.wallpaper.is_active() {
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
            let drag_resp = if over_padlock {
                drag_resp
            } else {
                drag_resp.on_hover_text("Drag to move")
            };
            let padlock_resp = ui
                .interact(
                    padlock_hit,
                    ui.id().with("fixed_padlock"),
                    egui::Sense::click(),
                )
                .on_hover_text(if pinned {
                    "Click to unlock"
                } else {
                    "Click to lock position here"
                });

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
                    self.runtime.app_theme.accent
                } else if padlock_resp.hovered() {
                    egui::Color32::from_gray(210)
                } else {
                    egui::Color32::from_gray(150)
                };
                draw_padlock(ui.painter(), padlock_center, pinned, color);
            }

            // Panels in the order defined by visible_panels (respects user reordering).
            let panels_to_draw = self.runtime.visible_panels.clone();
            let profile = self.current_settings.lock_safe().dashboard_profile.clone();
            // Extract update version once per frame (cheap lock read).
            let update_ver: Option<String> = {
                use windows::updater::UpdateStatus;
                let st = self.dialogs.updater_win.lock_safe();
                if let UpdateStatus::Ready { info, .. } = &st.status {
                    Some(info.version.clone())
                } else {
                    None
                }
            };
            let mut new_preferred_gpu: Option<String> = None;

            let is_landscape = profile_is_landscape(&profile);
            if is_landscape {
                // ── Landscape grid — panels packed into an even, adaptive grid ──
                // Fullscreen: the window is fixed to the profile/monitor size and
                // the grid stretches to fill it exactly. Otherwise the grid uses
                // natural cell sizes and the fit-to-content block below shrinks
                // the window to the rows actually used.
                new_preferred_gpu = self.render_landscape_grid(
                    ui,
                    &panels_to_draw,
                    update_ver.as_deref(),
                    &profile,
                );
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
            }

            // Fit window height to actual rendered content every frame so no black gap
            // appears regardless of panel set, spacing, or egui version. Applies to both
            // orientations — the landscape grid uses natural cell sizes outside fullscreen
            // so it shrinks/grows with the panel count just like the portrait stack.
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
                    // Landscape keeps the full profile width (no trim); the portrait
                    // stack trims 2 px to avoid a sub-pixel edge artifact.
                    let target_w = if is_landscape { w } else { w - 2.0 };
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                            target_w, used_h,
                        )));
                }
            }

            if let Some(new_pref) = new_preferred_gpu {
                self.select_gpu(Some(new_pref));
            }
        }

        // While recording, blink the tray dot on/off so an active session reads
        // as an ongoing event rather than a static indicator.
        if self.recording_active {
            const BLINK_PERIOD: Duration = Duration::from_millis(600);
            if self.recording_blink_at.elapsed() >= BLINK_PERIOD {
                self.recording_blink_on = !self.recording_blink_on;
                self.recording_blink_at = Instant::now();
                self.tray.set_recording_blink(self.recording_blink_on);
            }
            ui.ctx().request_repaint_after(BLINK_PERIOD);
        } else {
            // Idle repaint rate: 1 fps regardless of whether a dialog is open.
            // Interaction responsiveness (hover, click, drag) is driven by OS input
            // events which wake eframe immediately — request_repaint_after only
            // matters when idle. All dialogs call main_ctx.request_repaint_of(ROOT)
            // on close, so the show_viewport_immediate cleanup on the next frame is
            // already fast.
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
    }
}

/// Motherboard-panel fan click (#188): consumes the panel's `"open_fan_curve"`
/// flag and hands the fan's label on to the Control Center, which resolves
/// it to the header driving that fan. Returns whether the window should open.
fn take_fan_curve_request(ctx: &egui::Context) -> bool {
    let Some(label) = ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::new("open_fan_curve")))
    else {
        return false;
    };
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(windows::control::SELECT_FAN_ID), label));
    true
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
    let stroke = egui::Stroke::new(1.5_f32, color);

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

    fn view(&self) -> DashboardView<'_> {
        self.runtime.view()
    }

    /// Opacity to feed into per-panel rendering: the real setting when a DComp
    /// swap chain is active (issue #101 — only panel backgrounds/decorations
    /// fade via theme::panel_frame's premultiply; text/gauges/bars/logos stay
    /// fully opaque), or always `1.0` when falling back to the old opaque swap
    /// chain + WS_EX_LAYERED window opacity (no DX12 adapter at startup).
    fn effective_opacity(&self) -> f32 {
        if self.dcomp_available {
            self.opacity
        } else {
            1.0
        }
    }

    /// Draw a single panel via the shared [`DashboardView`], then consume the
    /// clock panel's `"open_updater"` badge-click flag (host has no updater)
    /// and the header panel's `"open_control_center"` chip-click flag (#187;
    /// host has no Control Center either — both flags are simply never
    /// consumed there, matching `bin/wallpaper.rs`'s plain `draw_one_panel`
    /// call with no flag-handling wrapper).
    fn draw_one_panel(
        &self,
        ui: &mut egui::Ui,
        panel: &str,
        sc: f32,
        update_ver: Option<&str>,
    ) -> Option<String> {
        let new_pref =
            self.view()
                .draw_one_panel(ui, panel, sc, self.effective_opacity(), update_ver);
        if panel == "clock"
            && ui
                .ctx()
                .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("open_updater")))
                .unwrap_or(false)
        {
            self.dialogs.updater_open.store(true, Ordering::Relaxed);
        }
        if panel == "header"
            && ui
                .ctx()
                .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("open_control_center")))
                .unwrap_or(false)
        {
            self.dialogs.control_open.store(true, Ordering::Relaxed);
            self.dialogs.control_focus.store(true, Ordering::Relaxed);
        }
        if panel == "motherboard" && take_fan_curve_request(ui.ctx()) {
            self.dialogs.control_open.store(true, Ordering::Relaxed);
            self.dialogs.control_focus.store(true, Ordering::Relaxed);
        }
        new_pref
    }

    /// Render the visible panels as an adaptive landscape grid via the shared view.
    /// Stretches to fill the fixed window only in Fill Screen mode; otherwise the
    /// grid uses natural cell sizes so the per-frame fit can shrink the window to
    /// the actual rows used.
    fn render_landscape_grid(
        &self,
        ui: &mut egui::Ui,
        panels: &[String],
        update_ver: Option<&str>,
        profile: &str,
    ) -> Option<String> {
        let ref_h = profile_to_size(profile)[1];
        // See `effective_opacity` above.
        self.view().render_landscape_grid(
            ui,
            panels,
            update_ver,
            self.fullscreen_mode,
            ref_h,
            self.effective_opacity(),
        )
    }

    /// Fixed-mode window geometry: returns `(top_left, [w, h])`.
    /// Width is always the profile width so panel proportions never stretch.
    /// In fullscreen mode the height fills the monitor (intended for a monitor
    /// whose resolution matches the profile), for both orientations; otherwise
    /// it is the content-fit estimate (the per-frame fit then refines it).
    /// Falls back to the content-fit size if no monitor is found.
    fn fixed_window_geometry(&self, profile: &str) -> ([f32; 2], [f32; 2]) {
        let [w, profile_h] = profile_to_size(profile);
        // Auto-target the monitor whose resolution matches the profile (a strip/
        // secondary screen only when it actually fits); otherwise the primary
        // monitor. Used for both orientations so portrait/side profiles land on a
        // matching screen or the main screen — never on an arbitrary small monitor.
        let [mx, my, _mw, mh] = pick_window_rect_for_profile(profile);
        let (auto_pos, size) = if self.fullscreen_mode {
            // Fill the height of the monitor the window currently sits on (where
            // the user placed it) — not the profile-matching monitor. Otherwise
            // enabling Fill Screen would teleport the dashboard to another screen.
            // Fall back to the auto-target monitor when the current position is
            // unknown or off every monitor. Applies to both orientations: the
            // landscape grid stretches to fill it, the portrait stack centers/
            // top-aligns within it.
            let m = self
                .last_fixed_pos
                .and_then(monitor_rect_at)
                .filter(|r| r[3] > 0.0)
                .unwrap_or([mx, my, _mw, mh]);
            ([m[0], m[1]], [w, m[3]])
        } else if profile_is_landscape(profile) {
            // Landscape, not fullscreen: content-fit estimate (the per-frame fit
            // then refines it), same as the portrait branch below.
            let h = compute_landscape_window_height(&self.runtime.visible_panels, w, profile_h);
            ([mx, my], [w, h])
        } else {
            let h = compute_window_height(&self.runtime.visible_panels, profile_scale(profile));
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
        // Per-pixel DComp transparency for floating panels (issue #169):
        // confirmed working by replaying the overlay's full fix, not just
        // the style bit — forced resize nudge + repeated reapply/
        // force_repaint burst + hidden until settled, both on panel
        // creation and on any later content-driven resize. A prior, lighter
        // attempt (style bit + relying on the natural one-shot creation
        // resize alone, no hide/burst) reproduced the same washed-out
        // white/gray result the original 2026-08-21 investigation on #169
        // found — the repeated reapply+force_repaint burst while hidden
        // turned out to be the missing piece, exactly as it was for the
        // overlay.
        let dcomp_available = self.dcomp_available;
        let content_opacity = if dcomp_available { opacity } else { 1.0 };
        let main_hwnd = self.hwnd;

        for idx in 0..self.runtime.visible_panels.len() {
            let key = self.runtime.visible_panels[idx].clone();

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

            // Pull this panel's DComp burst state out of the map for the
            // duration of this iteration (put back after show_viewport_immediate
            // returns) — same "extract, mutate as a local, write back" shape
            // the overlay uses for its own (single, not per-key) burst state.
            let mut dcomp = self
                .floating_dcomp
                .borrow_mut()
                .remove(&key)
                .unwrap_or_default();
            if needs_position {
                // Fresh panel: hide until the burst below settles, exactly
                // like the overlay's first show — the burst itself starts
                // once the closure below actually finds the HWND.
                dcomp.start(dcomp_available);
            }

            // Title shared between ViewportBuilder and Win32 FindWindowW lookup.
            let win_title = format!("RigStats \u{2014} {}", panel_label(&key));
            let mut vp_builder = egui::ViewportBuilder::default()
                .with_title(win_title.clone())
                .with_inner_size([panel_w, initial_h])
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_transparent(dcomp_available)
                .with_visible(!dcomp.pending_reveal())
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
            let stats = &self.runtime.latest;
            let cspark = &self.runtime.cpu_spark;
            let gspark = &self.runtime.gpu_spark;
            let nuspark = &self.runtime.net_up_spark;
            let ndspark = &self.runtime.net_dn_spark;
            let tex = &self.runtime.textures;
            let app_theme = self.runtime.app_theme;
            let control = &self.runtime.control;
            let float_update_ver: Option<String> = {
                use windows::updater::UpdateStatus;
                let st = self.dialogs.updater_win.lock_safe();
                if let UpdateStatus::Ready { info, .. } = &st.status {
                    Some(info.version.clone())
                } else {
                    None
                }
            };
            let updater_open_arc = &self.dialogs.updater_open;
            let control_open_arc = &self.dialogs.control_open;
            let control_focus_arc = &self.dialogs.control_focus;
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

                    // Transparent when DComp is driving real per-pixel alpha (each
                    // panel's own theme::panel_frame paints the visible background,
                    // premultiplied by content_opacity below) — opaque PANEL_FILL
                    // for the WS_EX_LAYERED fallback, which dims whatever's here
                    // uniformly and needs solid content behind it to look right.
                    let central_fill = if dcomp_available {
                        egui::Color32::TRANSPARENT
                    } else {
                        theme::PANEL_FILL
                    };
                    #[allow(deprecated)] // CentralPanel::show is correct in viewport callbacks
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none().fill(central_fill))
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
                                    let r = panels::header::draw(
                                        ui,
                                        stats,
                                        tex,
                                        content_opacity,
                                        &app_theme,
                                        scale,
                                        control,
                                    );
                                    if ui
                                        .ctx()
                                        .data_mut(|d| {
                                            d.remove_temp::<bool>(egui::Id::new(
                                                "open_control_center",
                                            ))
                                        })
                                        .unwrap_or(false)
                                    {
                                        control_open_arc.store(true, Ordering::Relaxed);
                                        control_focus_arc.store(true, Ordering::Relaxed);
                                    }
                                    r
                                }
                                "clock" => {
                                    let r = panels::clock::draw(
                                        ui,
                                        stats.uptime_secs,
                                        content_opacity,
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
                                    content_opacity,
                                    self.runtime.thresholds.cpu.0,
                                    self.runtime.thresholds.cpu.1,
                                    &app_theme,
                                    scale,
                                    self.runtime.control.active_ppt_limit(),
                                ),
                                "gpu" => {
                                    let r = panels::gpu::draw(
                                        ui,
                                        stats,
                                        gspark,
                                        tex,
                                        content_opacity,
                                        &app_theme,
                                        self.runtime.thresholds.gpu.0,
                                        self.runtime.thresholds.gpu.1,
                                        self.runtime.thresholds.gpu_hotspot,
                                        self.runtime.thresholds.gpu_mem,
                                        scale,
                                        self.runtime.control.active_gpu_limit(&stats.gpu_name),
                                    );
                                    new_pref = r.0;
                                    r.1
                                }
                                "ram" => panels::ram::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    self.runtime.thresholds.ram.0,
                                    self.runtime.thresholds.ram.1,
                                    &app_theme,
                                    scale,
                                ),
                                "net" => panels::net::draw(
                                    ui,
                                    stats,
                                    nuspark,
                                    ndspark,
                                    content_opacity,
                                    &app_theme,
                                    scale,
                                ),
                                "disk" => panels::disk::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    self.runtime.thresholds.disk.0,
                                    self.runtime.thresholds.disk.1,
                                    self.runtime.thresholds.disk_usage.0,
                                    self.runtime.thresholds.disk_usage.1,
                                    &app_theme,
                                    scale,
                                ),
                                "motherboard" => {
                                    let r = panels::motherboard::draw(
                                        ui,
                                        stats,
                                        content_opacity,
                                        self.runtime.thresholds.mb,
                                        &app_theme,
                                        scale,
                                        control.has_fan_control(),
                                    );
                                    if take_fan_curve_request(ui.ctx()) {
                                        control_open_arc.store(true, Ordering::Relaxed);
                                        control_focus_arc.store(true, Ordering::Relaxed);
                                    }
                                    r
                                }
                                "process" => panels::process::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    &app_theme,
                                    scale,
                                ),
                                "gpu_processes" => panels::gpu_processes::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    &app_theme,
                                    scale,
                                ),
                                "power" => panels::power::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    &app_theme,
                                    scale,
                                    self.runtime.psu_watts,
                                ),
                                "battery" => panels::battery::draw(
                                    ui,
                                    stats,
                                    content_opacity,
                                    &app_theme,
                                    scale,
                                    self.runtime.thresholds.battery.0,
                                    self.runtime.thresholds.battery.1,
                                    self.runtime.thresholds.battery_power.0,
                                    self.runtime.thresholds.battery_power.1,
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

                            // Hover-only response so a tooltip can be attached without
                            // interfering with the raw just_pressed drag trigger above.
                            if !locked && !in_padlock {
                                let drag_zone_resp = ui.interact(
                                    drag_zone,
                                    ui.id().with("panel_drag_zone"),
                                    egui::Sense::hover(),
                                );
                                let _ = drag_zone_resp.on_hover_text("Drag to move");
                            }

                            // Padlock interaction.
                            let padlock_resp = ui
                                .interact(
                                    padlock_hit,
                                    ui.id().with("padlock"),
                                    egui::Sense::click(),
                                )
                                .on_hover_text(if locked {
                                    "Click to unlock"
                                } else {
                                    "Click to lock position here"
                                });
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
                            // A later content-driven resize deliberately does NOT
                            // retrigger the hide/burst below (unlike the overlay,
                            // which only resizes on a deliberate user action like
                            // dragging Scale) — several of these panels' heights
                            // fluctuate with live data (e.g. GPU Apps'/Processes'
                            // row count), so retriggering on every such change
                            // caused near-constant hide/reveal flicker, worse than
                            // just leaving the resize non-hiding as before.

                            // Apply per-pixel DComp transparency — full burst
                            // treatment (forced resize nudge, repeated reapply +
                            // force_repaint, hidden until settled), same shape as
                            // the overlay's fix, not just the style bit — or fall
                            // back to whole-window WS_EX_LAYERED dimming.
                            #[cfg(windows)]
                            {
                                if dcomp_available {
                                    let tick = dcomp.tick(
                                        || win_opacity::find_hwnd(&win_title),
                                        dcomp_available,
                                    );
                                    if tick.just_started {
                                        // Force a genuine resize/reconfigure on
                                        // creation — same reasoning as the
                                        // overlay's own first-burst-frame nudge.
                                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                            egui::Vec2::new(panel_w + 2.0, initial_h),
                                        ));
                                    }
                                    if tick.reapply_style {
                                        win_opacity::set_no_redirection_bitmap(dcomp.hwnd());
                                    }
                                    if tick.force_repaint {
                                        win_opacity::force_repaint(dcomp.hwnd());
                                        win_opacity::force_repaint(main_hwnd);
                                        ctx.request_repaint();
                                    }
                                    if tick.reveal {
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                                    }
                                } else {
                                    let hwnd = win_opacity::find_hwnd(&win_title);
                                    win_opacity::set_opacity(hwnd, opacity);
                                }
                            }
                        });
                },
            );
            self.floating_dcomp.borrow_mut().insert(key.clone(), dcomp);
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
    // If another instance is already running, focus it and exit instead of
    // starting a second one.
    #[cfg(windows)]
    if rigstats_egui::single_instance::ensure_single_instance() {
        return;
    }

    // Opt the process into dark mode for OS-drawn UI elements (tray context menu).
    #[cfg(windows)]
    win32_dark_mode::enable();

    let dir = app_data_dir();
    app::startup::init_logging(&dir);

    let s = settings::load_settings(&dir);
    let visible_panels = s.visible_panels.clone();
    let opacity = s.opacity.clamp(0.1, 1.0) as f32;
    let ([inner_w, inner_h], [mut pos_x, mut pos_y]) = app::startup::initial_window(&s);
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

    // Close any session left open by an unclean shutdown and best-effort import
    // legacy daily-rolling logs, before the poll loop (or the tray/UI) can see
    // the session index.
    logging::reconcile_sessions_on_startup(&dir);

    let preferred_gpu_arc: Arc<Mutex<Option<String>>> =
        Arc::new(Mutex::new(s.preferred_gpu.clone()));
    let current_settings_shared = Arc::new(Mutex::new(settings::load_settings(&dir)));
    let (tx, rx) = mpsc::sync_channel::<PollStats>(4);
    let dir_clone = dir.clone();
    let pref_poll = preferred_gpu_arc.clone();
    let settings_poll = current_settings_shared.clone();
    // Poll mode: switched by `update_wallpaper_mode` while in wallpaper mode,
    // where the `rigstats-wallpaper` host becomes the dashboard's poller.
    let poll_mode = PollModeHandle::new(PollMode::Full);
    let poll_mode_loop = poll_mode.clone();
    // Control Center (#187) active profile id, for the session-recording CSV
    // column — updated by the UI thread whenever `ControlState.active_profile`
    // changes (see the `drain_control` call site below).
    let active_profile_arc: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let active_profile_poll = active_profile_arc.clone();
    // Restarted after a panic (#219) — the dashboard would freeze otherwise.
    runtime.spawn(debug::supervise(
        dir_clone.clone(),
        "Poll loop",
        move || {
            poll_loop(
                tx.clone(),
                dir_clone.clone(),
                pref_poll.clone(),
                settings_poll.clone(),
                poll_mode_loop.clone(),
                active_profile_poll.clone(),
            )
        },
    ));

    // Wallpaper mode at startup: the host owns the on-screen dashboard, so place
    // our own window off-screen from the start to avoid a one-frame flash.
    let start_wallpaper = s.window_layer == "wallpaper" && !s.floating_mode;
    if start_wallpaper {
        pos_x = -32000.0;
        pos_y = -32000.0;
    }

    let dcomp_available = app::startup::probe_dcomp(&runtime);
    debug::log_debug(&dir, &format!("startup: dcomp_available={dcomp_available}"));

    let options =
        app::startup::native_options(&s, [inner_w, inner_h], [pos_x, pos_y], dcomp_available);

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

            // A fresh launch never has an active recording session — any session left
            // open by an unclean shutdown was already closed by reconcile_sessions_on_startup.
            let overlay_locked_init = current_settings_shared.lock_safe().overlay_click_through;
            let gpu_names_rx = app::background::spawn_gpu_name_detection(&dir, &cc.egui_ctx);
            let tray = build_tray(false, overlay_locked_init, gpu_names_rx);

            // Global hotkey (fixed Ctrl+Alt+O in v1) to toggle overlay
            // click-through without leaving the game. The listener thread
            // outlives this closure (runs for the process lifetime), so the
            // JoinHandle is intentionally dropped rather than joined.
            let (hotkey_tx, hotkey_rx) = mpsc::channel::<hotkey::HotkeyEvent>();
            #[cfg(windows)]
            let _ = rigstats_egui::hotkey::spawn(dir.clone(), hotkey_tx, cc.egui_ctx.clone());

            let (control_cmd_tx, control_rx) = app::background::spawn_control_task(&runtime, &dir);

            // Mirrors `RigStatsApp.recording_active` for the tray thread (#177).
            let recording_active_shared = Arc::new(AtomicBool::new(false));
            let tray_rx = app::background::spawn_tray_event_thread(
                &tray,
                &dir,
                &cc.egui_ctx,
                recording_active_shared.clone(),
            );

            let current_settings = current_settings_shared;
            let settings_reload = Arc::new(AtomicBool::new(false));
            let dir_arc = Arc::new(dir.clone());
            let dashboard_runtime = DashboardRuntime::new(&cc.egui_ctx, &s);

            // Survive a hybrid iGPU/dGPU switch: without this, a fatal wgpu
            // device error (e.g. the D3D12 adapter going away) falls through
            // to wgpu's default handler and panics the whole process — see
            // `gpu_guard`. `update()` checks this flag every frame.
            let gpu_lost = Arc::new(AtomicBool::new(false));
            if let Some(render_state) = cc.wgpu_render_state.as_ref() {
                install_gpu_loss_guard(
                    render_state,
                    cc.egui_ctx.clone(),
                    dir_arc.clone(),
                    gpu_lost.clone(),
                );
            }
            let gpu_started_at = Instant::now();
            let gpu_retry_count: u32 = std::env::var("RIGSTATS_GPU_RETRY_COUNT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            let fm_arc_hb = Arc::new(AtomicBool::new(current_settings.lock_safe().floating_mode));
            app::background::spawn_floating_heartbeat(&cc.egui_ctx, fm_arc_hb.clone());

            let (updater_win_bg, updater_open_bg, updater_focus_bg) =
                app::background::start_updater(&runtime, &dir, &cc.egui_ctx);

            Ok(Box::new(RigStatsApp::new(
                dashboard_runtime,
                rx,
                tray_rx,
                hotkey_rx,
                control_rx,
                control_cmd_tx,
                opacity,
                dcomp_available,
                tray,
                current_settings,
                settings_reload,
                dir_arc,
                preferred_gpu_arc,
                active_profile_arc,
                fm_arc_hb,
                updater_win_bg,
                updater_open_bg,
                updater_focus_bg,
                poll_mode,
                gpu_lost,
                gpu_started_at,
                gpu_retry_count,
                recording_active_shared,
            )))
        }),
    )
    .expect("eframe");

    debug::append_debug_log(&dir, "shutdown: clean");
    runtime.shutdown_background();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for #199: `egui::Context::set_fonts` silently no-ops
    /// if the passed-in `FontDefinitions` compares equal to what's already
    /// active. `refresh_font_atlas_after_minimize` depends on
    /// `font_definitions_for_rebuild(true)` and `(false)` always differing —
    /// if a future edit collapses this back to a plain `default()` call (or
    /// otherwise makes the two toggle states identical), the periodic/
    /// minimize-triggered atlas rebuild becomes a dead no-op again, exactly
    /// as it silently was before this fix, with no test failure to catch it.
    #[test]
    fn font_definitions_for_rebuild_toggle_states_differ() {
        let off = RigStatsApp::font_definitions_for_rebuild(false);
        let on = RigStatsApp::font_definitions_for_rebuild(true);
        let scale = |defs: &egui::FontDefinitions| defs.font_data["Hack"].tweak.scale;
        assert_ne!(
            scale(&off),
            scale(&on),
            "the two toggle states must differ, or set_fonts's equality check \
             will silently suppress every rebuild"
        );
    }
}
