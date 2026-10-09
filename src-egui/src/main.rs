#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;

use app::state::{
    DialogStates, FloatingState, FontAtlasRefresh, GpuRecovery, OverlayState, RecordingIndicator,
    WindowFit,
};
use eframe::egui;
use rigstats_backend::control;
use rigstats_backend::{debug, hardware, logging, settings};
use rigstats_egui::dashboard::{DashboardRuntime, DashboardView};
#[cfg(windows)]
use rigstats_egui::geometry::win_monitor;
use rigstats_egui::geometry::{
    compute_landscape_window_height, compute_window_height, guard_panel_position,
    is_position_on_screen, monitor_rect_at, pick_window_rect_for_profile, profile_is_landscape,
    profile_scale, profile_to_size, resolve_pinned_position,
};
use rigstats_egui::gpu_guard::install_gpu_loss_guard;
use rigstats_egui::hotkey;
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::poll::{poll_loop, PollMode, PollModeHandle};
use rigstats_egui::tray::{build_tray, Tray, TrayCmd};
use rigstats_egui::wallpaper_supervisor::{self, ChildHost, WallpaperSupervisor};
use rigstats_egui::{alerts, theme, windows, PollStats};
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
    /// The fixed-mode window: fullscreen, pinned position, fit to content.
    window: WindowFit,
    /// Floating mode's panels and their windows.
    floating: FloatingState,
    /// Colour palette for dialog windows — switches between dark/light based on OS theme.
    dialog_colors: theme::DialogColors,
    /// Cached OS dark-mode flag; checked each frame to detect live theme switches.
    os_dark_mode: bool,
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
    /// The tray's recording indicator.
    recording: RecordingIndicator,
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
            window: WindowFit::new(&init_settings),
            floating: FloatingState {
                mode: init_settings.floating_mode,
                panels_locked: init_settings.floating_panels_locked,
                panel_scale: init_settings.floating_panel_scale.clamp(0.4, 1.0) as f32,
                positions: Arc::new(Mutex::new(init_positions)),
                positions_dirty: Arc::new(AtomicBool::new(false)),
                new_pref_gpu: Arc::new(Mutex::new(None)),
                lock_arc: Arc::new(AtomicBool::new(init_settings.floating_panels_locked)),
                initial_applied: false,
                panels_positioned: HashSet::new(),
                behind_enforce: RefCell::new(HashMap::new()),
                dcomp: RefCell::new(HashMap::new()),
                mode_arc: floating_mode_arc,
            },
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
            poll_mode,
            wallpaper: WallpaperSupervisor::default(),
            wallpaper_host: ChildHost::default(),
            overlay: OverlayState::new(&init_settings),
            recording: RecordingIndicator {
                active: false,
                active_shared: recording_active_shared,
                blink_on: true,
                blink_at: Instant::now(),
            },
            font_atlas: FontAtlasRefresh {
                stale: false,
                rebuilt_at: Instant::now(),
                toggle: false,
            },
            battery_present,
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
        self.window.last_fitted_height = None;
        self.window.reapply_window_props_frames = 4;
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
        let want = self.window_layer == "wallpaper" && !self.floating.mode;
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
                    if let Some([x, y]) = self.window.last_fixed_pos {
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
        if self.window.reapply_window_props_frames > 0 {
            self.window.reapply_window_props_frames -= 1;
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
                if self.window_layer == "behind" && !self.floating.mode {
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
        self.apply_reloaded_settings(ui.ctx());

        // Fill the tray "GPU" submenu and Settings' GPU list in once the
        // background adapter detection reports back.
        let pref = self.preferred_gpu.lock_safe().clone();
        if let Some(names) = self.tray.gpu_menu.poll(pref.as_deref()) {
            self.dialogs.settings_win.lock_safe().set_gpu_names(names);
        }

        self.handle_tray_commands(ui.ctx());

        // Sync floating lock state toggled by padlock icon in drag handle.
        let arc_locked = self.floating.lock_arc.load(Ordering::Relaxed);
        if arc_locked != self.floating.panels_locked {
            self.floating.panels_locked = arc_locked;
            let mut s = self.current_settings.lock_safe();
            s.floating_panels_locked = arc_locked;
            self.persist_settings_logged(&s);
        }

        self.render_dialogs(ui);

        // On the first frame: move main window off-screen when floating mode is active.
        // We NEVER use Visible(false) in floating mode because a hidden window is not
        // ticked by eframe — show_viewport_immediate would stop being called and all
        // floating panels would freeze.
        if !self.floating.initial_applied {
            self.floating.initial_applied = true;
            if self.floating.mode {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::Pos2::new(
                        -32000.0, -32000.0,
                    )));
            }
        }

        if self.floating.mode {
            // ── Floating mode — each panel in its own borderless viewport ─────
            self.render_floating_panels(ui);

            // Persist positions when any panel was dragged.
            if self.floating.positions_dirty.swap(false, Ordering::Relaxed) {
                self.persist_floating_positions();
            }

            // Apply GPU preference change made from the floating GPU panel.
            let float_pref = self.floating.new_pref_gpu.lock_safe().take();
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
                    self.window.last_fixed_pos = Some([outer.left().round(), outer.top().round()]);
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
            let pinned = self.window.dashboard_pinned;
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
                self.window.reapply_window_props_frames = 4;
            }

            if padlock_resp.clicked() {
                self.window.dashboard_pinned = !self.window.dashboard_pinned;
                let profile = self.current_settings.lock_safe().dashboard_profile.clone();
                let mut s = self.current_settings.lock_safe();
                s.dashboard_pinned = self.window.dashboard_pinned;
                if self.window.dashboard_pinned {
                    // Pin: remember the current window position for this profile.
                    if let Some([x, y]) = self.window.last_fixed_pos {
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
                let center_fullscreen =
                    self.window.fullscreen_mode && self.window.fullscreen_align == "center";
                let center_pad = if center_fullscreen {
                    // Pure panel-stack height (excludes drag handle and pad).
                    let content = self.window.fullscreen_content_h.unwrap_or_else(|| {
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
                            .window
                            .fullscreen_content_h
                            .map(|h| (h - content).abs() > 0.5)
                            .unwrap_or(true);
                        self.window.fullscreen_content_h = Some(content);
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
            if !self.window.fullscreen_mode {
                let used_h = ui.min_rect().height();
                // During the first frames after launch the window may not be fully
                // realized, so early InnerSize commands can be dropped — which would
                // leave the bottom panel clipped until the user toggles floating mode.
                // Force a re-fit (and a fast repaint) for a handful of frames so the
                // true content height always sticks at startup.
                if self.window.startup_fit_frames > 0 {
                    self.window.startup_fit_frames -= 1;
                    self.window.last_fitted_height = None;
                    ui.ctx().request_repaint();
                }
                let changed = self
                    .window
                    .last_fitted_height
                    .map(|h| (h - used_h).abs() > 0.5)
                    .unwrap_or(true);
                if used_h > 10.0 && changed {
                    self.window.last_fitted_height = Some(used_h);
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
        if self.recording.active {
            const BLINK_PERIOD: Duration = Duration::from_millis(600);
            if self.recording.blink_at.elapsed() >= BLINK_PERIOD {
                self.recording.blink_on = !self.recording.blink_on;
                self.recording.blink_at = Instant::now();
                self.tray.set_recording_blink(self.recording.blink_on);
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
            self.window.fullscreen_mode,
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
        let (auto_pos, size) = if self.window.fullscreen_mode {
            // Fill the height of the monitor the window currently sits on (where
            // the user placed it) — not the profile-matching monitor. Otherwise
            // enabling Fill Screen would teleport the dashboard to another screen.
            // Fall back to the auto-target monitor when the current position is
            // unknown or off every monitor. Applies to both orientations: the
            // landscape grid stretches to fill it, the portrait stack centers/
            // top-aligns within it.
            let m = self
                .window
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
        if !self.window.fullscreen_mode {
            if let Some(last) = self.window.last_fixed_pos {
                return (guard_panel_position(last, auto_pos), size);
            }
        }
        (auto_pos, size)
    }

    /// Returns the saved pinned position for `profile` when the dashboard is
    /// pinned and the stored position is still on a connected monitor; otherwise
    /// `None` so the caller auto-targets the matching monitor.
    fn pinned_position(&self, profile: &str) -> Option<[f32; 2]> {
        if !self.window.dashboard_pinned {
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
        resolve_pinned_position(self.window.dashboard_pinned, saved, &monitors)
    }

    /// Persist settings to disk, logging any failure instead of swallowing it
    /// silently — a failed write means the user loses settings/layout changes,
    /// which should never pass unnoticed.
    fn persist_settings_logged(&self, s: &settings::Settings) {
        if let Err(e) = settings::persist_settings(&self.dir, s) {
            debug::log_error(&self.dir, &format!("settings: persist failed — {e}"));
        }
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
