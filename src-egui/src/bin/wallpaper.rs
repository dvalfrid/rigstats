//! `rigstats-wallpaper` — the WorkerW desktop-wallpaper host process.
//!
//! Spawned and supervised by the main `rigstats` app when the user selects the
//! "Desktop Wallpaper" window layer. It is a minimal eframe app: its own
//! window is a transparent 1x1 anchor, and the dashboard (or each floating
//! panel) is a borderless viewport drawn via
//! [`rigstats_egui::dashboard::DashboardView`]; all of them are reparented
//! into the desktop `WorkerW` layer. Kept as a separate process so that an
//! Explorer restart (which destroys the WorkerW hierarchy and any child
//! window) cannot take down the main app — the supervisor relaunches the host
//! instead. While
//! active the host is the telemetry owner: it runs `poll_loop`, owns the sensor
//! pipe, and does CSV logging (the main app stops polling in wallpaper mode, or
//! only polls in light mode to feed the game overlay). See `docs/architecture.md`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use rigstats_backend::{debug, settings};
use rigstats_egui::dashboard::{DashboardRuntime, DashboardView};
use rigstats_egui::geometry::{
    compute_landscape_window_height, compute_window_height, floating_panel_w, guard_panel_position,
    pick_window_rect_for_profile, profile_is_landscape, profile_scale, profile_to_size,
};
use rigstats_egui::gpu_guard::{
    install_gpu_loss_guard, log_render_adapter, render_power_preference,
};
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::poll::{poll_loop, PollMode, PollModeHandle};
use rigstats_egui::tray::panel_initial_h;
use rigstats_egui::{theme, PollStats};
#[cfg(windows)]
use rigstats_egui::{win32_wallpaper, win_opacity};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Since when the window has been waiting for a WorkerW to attach to
    /// (#579): the first try can come while Explorer is replacing it after a
    /// new desktop background, and the 1 Hz check below attaches it a moment
    /// later. `None` while attached.
    unattached_since: Option<Instant>,
    /// Whether "still not attached" was logged for the current wait.
    attach_warned: bool,
    last_parent_check: Instant,
    /// Set by `gpu_guard::install_gpu_loss_guard`'s callbacks when wgpu
    /// reports a fatal device error (e.g. a hybrid iGPU/dGPU switch). The
    /// main app's `update_wallpaper_mode` supervisor already respawns this
    /// host on any exit, so on error we just need to close cleanly — no
    /// self-relaunch logic needed here.
    gpu_lost: Arc<AtomicBool>,
    /// Shared with `poll_loop`; refreshed from settings so a GPU picked in the
    /// main app applies here without a relaunch.
    preferred_gpu: Arc<Mutex<Option<String>>>,
    /// The windows the host shows in WorkerW: the one dashboard, or each
    /// visible floating panel (#170). The host's own window is a transparent
    /// 1x1 px anchor either way.
    windows: Vec<HostWindow>,
    /// The profile uses floating panels (`Settings::floating_mode`) — fixed
    /// for this host's life; a change restarts it.
    floating: bool,
    /// Floating panels' scale (`Settings::floating_panel_scale`).
    panel_scale: f32,
    /// The host is closing (left wallpaper mode, another profile or layout):
    /// no window is shown again.
    closing: bool,
    /// What the panel windows were built from; a change (another profile's
    /// positions, a panel shown or hidden, a new size) restarts the host —
    /// positions are only given while a window is still top-level, so a
    /// fresh host is the simple way to move them.
    panels_layout: String,
    /// The screens' layout (physical pixels), to move the windows along with
    /// their screen when a game changes the main screen's resolution (#690).
    screen_watch: rigstats_egui::geometry::LayoutWatch,
}

/// What one of the host's windows draws.
enum WindowContent {
    /// The whole dashboard, starting at `size` and fitting its height to the
    /// content.
    Dashboard { size: [f32; 2] },
    /// One floating panel (#170), by its panel key.
    Panel(String),
}

impl WindowContent {
    /// The name in the window's title and the log.
    fn name(&self) -> &str {
        match self {
            Self::Dashboard { .. } => "dashboard",
            Self::Panel(key) => key,
        }
    }
}

/// One window the host shows in WorkerW.
struct HostWindow {
    content: WindowContent,
    /// Screen position: the dashboard's from `wallpaper_position`, a panel's
    /// from the profile's `panel_layouts` like floating mode.
    pos: [f32; 2],
    dcomp: rigstats_egui::dcomp_burst::DcompRevealBurst,
    started: bool,
    hwnd: isize,
    attached: bool,
    /// Where the window was on the screen (physical pixels) at the last
    /// layout check — after a change, where it was on the old layout (#690).
    screen_pos: Option<[f32; 2]>,
}

impl HostWindow {
    fn new(content: WindowContent, pos: [f32; 2]) -> Self {
        Self {
            content,
            pos,
            dcomp: Default::default(),
            started: false,
            hwnd: 0,
            attached: false,
            screen_pos: None,
        }
    }
}

/// The panel windows for floating panels in the wallpaper: every visible
/// panel at its saved floating position, or the floating mode's default.
fn panel_windows(s: &settings::Settings) -> Vec<HostWindow> {
    s.visible_panels
        .iter()
        .enumerate()
        .map(|(idx, key)| {
            let default_pos = [100.0 + idx as f32 * 20.0, 80.0 + idx as f32 * 30.0];
            let saved = s
                .panel_layouts
                .get(key)
                .map_or(default_pos, |p| [p.x as f32, p.y as f32]);
            HostWindow::new(
                WindowContent::Panel(key.clone()),
                guard_panel_position(saved, default_pos),
            )
        })
        .collect()
}

/// Everything the panel windows are built from, as one comparable string.
fn panels_layout(s: &settings::Settings) -> String {
    let mut parts = vec![
        format!("scale {:.2}", s.floating_panel_scale),
        format!("compact {:?}", s.compact_panels),
    ];
    for key in &s.visible_panels {
        let pos = s.panel_layouts.get(key).map(|p| (p.x, p.y));
        parts.push(format!("{key} {pos:?}"));
    }
    parts.join("; ")
}

impl WallpaperHost {
    /// The host's windows — the dashboard, or the floating panels (#170): each
    /// gets the floating panels' hide-until-settled DComp burst
    /// (`app/floating.rs`, #169) and is reparented into WorkerW as soon as its
    /// window exists, still hidden — the host's own window is attached the
    /// same way. Display-only: no drag or padlock here.
    #[cfg(windows)]
    fn render_windows(&mut self, ctx: &egui::Context) {
        let mut windows = std::mem::take(&mut self.windows);
        let sc = self.panel_scale;
        let opacity = self.opacity;
        for p in &mut windows {
            if !p.started {
                p.dcomp.start(true);
                p.started = true;
            }
            let title = format!("RIGStats Wallpaper \u{2014} {}", p.content.name());
            let ([w, h], fill) = match &p.content {
                WindowContent::Dashboard { size } => {
                    (*size, theme::premul(theme::PANEL_FILL, opacity))
                }
                // A floating panel paints its own frame.
                WindowContent::Panel(key) => (
                    [
                        floating_panel_w(self.runtime.is_compact(key), sc),
                        panel_initial_h(key) * sc,
                    ],
                    egui::Color32::TRANSPARENT,
                ),
            };
            let profile = &self.profile;
            let shown = &self.runtime.visible_panels;
            let builder = egui::ViewportBuilder::default()
                .with_title(title.clone())
                .with_inner_size([w, h])
                .with_position(p.pos)
                .with_decorations(false)
                // No DWM frame behind a transparent window (third_party/egui-winit).
                .with_has_shadow(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_transparent(true)
                .with_visible(!p.dcomp.pending_reveal());
            let view = self.runtime.view();
            let dir = &self.dir;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of(&title),
                builder,
                |cui, _class| {
                    let cctx = cui.ctx().clone();
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE.fill(fill))
                        .show(cui, |ui| {
                            // Measured on the content: a CentralPanel's own
                            // `min_rect` is the whole window, so it could only
                            // grow — a landscape dashboard stayed at its
                            // start-up estimate, ~200 px too tall.
                            let content = ui.scope(|ui| match &p.content {
                                WindowContent::Dashboard { .. } => {
                                    draw_dashboard(ui, &view, profile, shown, opacity);
                                }
                                WindowContent::Panel(key) => {
                                    view.draw_one_panel(ui, key, sc, opacity, None);
                                }
                            });
                            // Fit the height to the content, only when it changed.
                            let used_h = content.response.rect.height().round();
                            let current_h = cctx
                                .input(|i| i.viewport().inner_rect)
                                .map_or(0.0, |r| r.height().round());
                            if used_h > 10.0 && (used_h - current_h).abs() > 0.5 {
                                cctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                    egui::Vec2::new(w, used_h),
                                ));
                            }
                        });
                    let tick = p.dcomp.tick(|| win_opacity::find_hwnd(&title), true);
                    if tick.just_started {
                        p.hwnd = p.dcomp.hwnd();
                        // A genuine resize on creation, as floating mode does.
                        cctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                            w + 2.0,
                            h,
                        )));
                    }
                    if tick.reapply_style {
                        win_opacity::set_no_redirection_bitmap(p.dcomp.hwnd());
                    }
                    // Into WorkerW once the window exists, and back in if
                    // Explorer rebuilt the hierarchy; logged only on a change.
                    if p.hwnd != 0 && (!p.attached || !win32_wallpaper::is_attached(p.hwnd)) {
                        let was = p.attached;
                        p.attached = win32_wallpaper::attach(p.hwnd);
                        if p.attached != was || was {
                            debug::append_debug_log(
                                dir,
                                &format!(
                                    "rigstats-wallpaper: panel {} {}",
                                    p.content.name(),
                                    match (was, p.attached) {
                                        (_, false) => "waits for a wallpaper WorkerW",
                                        (true, true) => "attached to WorkerW again",
                                        (false, true) => "attached to WorkerW",
                                    }
                                ),
                            );
                        }
                    }
                    if tick.force_repaint {
                        win_opacity::force_repaint(p.dcomp.hwnd());
                        cctx.request_repaint();
                    }
                    if tick.reveal {
                        cctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    }
                },
            );
        }
        self.windows = windows;
    }
}

#[cfg(windows)]
impl WallpaperHost {
    /// When the screens' layout changes (#690) — a game in exclusive
    /// fullscreen at a lower resolution narrows the main screen and Windows
    /// moves a screen to its right — each window moves as far as its screen
    /// did, and back when the game ends. In WorkerW a window keeps its place
    /// relative to the virtual screen, not to its screen, so nothing else
    /// moves it.
    fn follow_screen_layout(&mut self) {
        use rigstats_egui::geometry::{follow_layout, layout_line, LayoutPoll};
        match self.screen_watch.poll() {
            LayoutPoll::NotYet => {}
            LayoutPoll::Same => {
                for w in &mut self.windows {
                    if w.attached {
                        w.screen_pos = win32_wallpaper::screen_pos(w.hwnd);
                    }
                }
            }
            LayoutPoll::Changed(old, new) => {
                let mut moved = Vec::new();
                for w in &mut self.windows {
                    let Some(to) = w
                        .screen_pos
                        .filter(|_| w.attached)
                        .and_then(|pos| follow_layout(pos, &old, &new))
                    else {
                        continue;
                    };
                    if win32_wallpaper::move_to_screen(w.hwnd, to) {
                        moved.push(format!("{} → {},{}", w.content.name(), to[0], to[1]));
                        w.screen_pos = Some(to);
                    }
                }
                debug::append_debug_log(
                    &self.dir,
                    &format!(
                        "rigstats-wallpaper: screens changed — {} (was {}){}",
                        layout_line(&new),
                        layout_line(&old),
                        if moved.is_empty() {
                            String::new()
                        } else {
                            format!("; moved {}", moved.join(", "))
                        }
                    ),
                );
            }
        }
    }
}

/// The whole dashboard: the landscape grid at natural cell sizes (there is no
/// Fill Screen in wallpaper mode), or the portrait stack.
#[cfg(windows)]
fn draw_dashboard(
    ui: &mut egui::Ui,
    view: &DashboardView<'_>,
    profile: &str,
    shown: &[String],
    opacity: f32,
) {
    // The main window's drag strip (`app/main_window.rs`), empty here: the
    // panels sit where they do in the other layers, not 17 px higher.
    ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::DRAG_HANDLE_H),
        egui::Sense::hover(),
    );
    if profile_is_landscape(profile) {
        let ref_h = profile_to_size(profile)[1];
        view.render_landscape_grid(ui, shown, None, false, ref_h, opacity);
    } else {
        let sc = profile_scale(profile);
        for panel in shown {
            view.draw_one_panel(ui, panel, sc, opacity, None);
            ui.add_space((view.panel_spacing * sc).round());
        }
    }
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
        // Left wallpaper mode (another window layer): the supervisor expects
        // us to exit so it can take the dashboard back.
        if s.window_layer != "wallpaper" {
            debug::append_debug_log(
                &self.dir,
                "rigstats-wallpaper: wallpaper mode left, closing",
            );
            return true;
        }
        // Floating panels switched on or off (the profile's choice, #599), or
        // the panels moved, shown, hidden or resized (#170): a fresh host
        // builds the right windows at the right places.
        let layout = if s.floating_mode {
            panels_layout(&s)
        } else {
            String::new()
        };
        if s.floating_mode != self.floating || layout != self.panels_layout {
            debug::log_debug(
                &self.dir,
                "rigstats-wallpaper: floating panels changed, closing for relaunch",
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
        // Picked from the main app's tray "GPU" submenu or Settings — the
        // only ways to change it while the dashboard is click-through.
        *self.preferred_gpu.lock_safe() = s.preferred_gpu.clone();
        // The dashboard window fits its height to the content each frame.
        self.runtime.apply_settings(&s);
        // Read by every window each frame.
        self.opacity = s.opacity.clamp(0.1, 1.0) as f32;
        false
    }
}

impl eframe::App for WallpaperHost {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Only an invisible anchor: the dashboard or the floating panels
        // (#170) are their own windows.
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // A fatal wgpu device error was flagged from off the UI thread (see
        // `gpu_guard`). Close cleanly rather than touching the (likely dead)
        // device further — the supervisor in the main app respawns us.
        if self.gpu_lost.load(Ordering::Relaxed) {
            debug::log_warn(&self.dir, "rigstats-wallpaper: gpu device error, closing");
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

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
                // Required for the DComp-backed swap chain (see `wgpu_options` in
                // `main()`) to actually composite transparently — see doc comment
                // on `set_no_redirection_bitmap` (issue #131 spike).
                win_opacity::set_no_redirection_bitmap(self.hwnd);
                if win32_wallpaper::attach(self.hwnd) {
                    debug::append_debug_log(&self.dir, "rigstats-wallpaper: attached to WorkerW");
                } else {
                    debug::append_debug_log(
                        &self.dir,
                        "rigstats-wallpaper: no wallpaper WorkerW yet — trying again every second",
                    );
                    self.unattached_since = Some(Instant::now());
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
                self.closing = true;
            }
        }
        // Closing takes a few frames. Show no window in them: a viewport left
        // out of one frame is destroyed, and showing it again would create a
        // fresh top-level window outside WorkerW — a white flash on leaving
        // wallpaper mode.
        if self.closing {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Re-attach safety net (~1 Hz): if Explorer rebuilt the WorkerW hierarchy
        // underneath us, the window survives but is detached — snap it back.
        #[cfg(windows)]
        if self.last_attach_check.elapsed() >= Duration::from_secs(1) {
            self.last_attach_check = Instant::now();
            if self.hwnd != 0 && !win32_wallpaper::is_attached(self.hwnd) {
                if win32_wallpaper::attach(self.hwnd) {
                    if let Some(since) = self.unattached_since.take() {
                        debug::append_debug_log(
                            &self.dir,
                            &format!(
                                "rigstats-wallpaper: attached to WorkerW after {:.1} s",
                                since.elapsed().as_secs_f32()
                            ),
                        );
                    }
                    self.attach_warned = false;
                } else {
                    let since = *self.unattached_since.get_or_insert_with(Instant::now);
                    if !self.attach_warned && since.elapsed() >= Duration::from_secs(10) {
                        debug::log_warn(
                            &self.dir,
                            "rigstats-wallpaper: still no wallpaper WorkerW after 10 s — the \
                             dashboard is a normal window, not in the desktop background",
                        );
                        self.attach_warned = true;
                    }
                }
            }
        }

        #[cfg(windows)]
        self.follow_screen_layout();

        // Every window the host shows is its own viewport: the dashboard, or
        // each floating panel (#170). The host's own window stays a 1x1
        // anchor, so the dashboard takes the floating panels' path — one way
        // to create, reveal, attach and fit a window (#664).
        #[cfg(windows)]
        {
            let ctx = ui.ctx().clone();
            self.render_windows(&ctx);
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

/// A panic on the UI thread ends the process, after the panic logger has
/// logged it. A new desktop background makes Explorer replace the WorkerW and
/// destroy our window with it; winit then panics (`ClientToScreen failure`)
/// and the process could live on without a window — the supervisor saw it
/// running and never respawned it, so the dashboard was gone until the mode
/// was toggled. Exiting lets the supervisor start a new host at once.
fn exit_on_ui_panic() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        if std::thread::current().name() == Some("main") {
            std::process::exit(101);
        }
    }));
}

fn main() {
    let dir = app_data_dir();
    debug::install_panic_logger(&dir);
    exit_on_ui_panic();
    debug::append_debug_log(&dir, "rigstats-wallpaper starting");

    let parent_pid: Option<u32> = std::env::var("RIGSTATS_PARENT_PID")
        .ok()
        .and_then(|v| v.parse().ok());

    let s = settings::load_settings(&dir);
    let profile = s.dashboard_profile.clone();
    let opacity = s.opacity.clamp(0.1, 1.0) as f32;

    // Size the dashboard window to fit its content (the per-frame fit in
    // `render_windows` then refines this estimate): the landscape grid uses
    // natural cell sizes, the portrait stack its panel-stack height. Never stretch to the full
    // profile height — that leaves a black gap below the content. Width is
    // the profile width so panel proportions are preserved, less 2 px in
    // portrait like the main window (`app/main_window.rs`), so the dashboard
    // covers the same pixels in every layer. Use the
    // position the user set in a normal layer before switching
    // (s.wallpaper_position) when it is still on a connected monitor; otherwise
    // centre on the matching monitor. The wallpaper shows around it.
    let [pw, ph] = profile_to_size(&profile);
    let size = if profile_is_landscape(&profile) {
        [
            pw,
            compute_landscape_window_height(&s.visible_panels, pw, ph, f32::from(s.panel_spacing)),
        ]
    } else {
        [
            pw - 2.0,
            compute_window_height(
                &s.visible_panels,
                profile_scale(&profile),
                f32::from(s.panel_spacing),
            ),
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
    // The host's own window is a 1x1 anchor; the dashboard or the floating
    // panels (#170) get their own windows.
    let floating = s.floating_mode;
    let windows = if floating {
        panel_windows(&s)
    } else {
        vec![HostWindow::new(WindowContent::Dashboard { size }, pos)]
    };
    let size = [1.0, 1.0];
    let layout = if floating {
        panels_layout(&s)
    } else {
        String::new()
    };
    let panel_scale = s.floating_panel_scale.clamp(0.4, 2.0) as f32;

    debug::log_debug(
        &dir,
        &format!(
            "wallpaper host: profile={profile} pos=({:.0},{:.0}) size=({:.0}x{:.0}){}",
            pos[0],
            pos[1],
            size[0],
            size[1],
            if floating {
                format!(", floating panels: {}", panels_layout(&s))
            } else {
                String::new()
            }
        ),
    );

    // Two workers: a 1 Hz poll loop and the control pipe, all async (blocking
    // work goes to spawn_blocking's own pool) — not one per core (#467).
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
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
    // The host is the active poller — never paused. Session recording is driven
    // by the on-disk session index (see `poll_loop`), so a tray-started
    // recording in the main app is picked up here too even though this is a
    // separate process.
    let full = PollModeHandle::new(PollMode::Full);
    // The wallpaper host never connects to the control pipe (#187 is a
    // main-app concept — no dialogs/tray here), so this stays `None` forever;
    // the session-recording CSV column is simply empty for rows it logs.
    let no_active_profile: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    // Restarted after a panic (#219) — the wallpaper would freeze otherwise.
    runtime.spawn(debug::supervise(dir_poll.clone(), "Poll loop", move || {
        poll_loop(
            tx.clone(),
            dir_poll.clone(),
            pref_poll.clone(),
            settings_poll.clone(),
            full.clone(),
            no_active_profile.clone(),
        )
    }));

    let viewport = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_inner_size(size)
        .with_position(pos)
        .with_decorations(false)
        // No DWM frame behind a transparent window (third_party/egui-winit).
        .with_has_shadow(false)
        .with_taskbar(false)
        .with_window_level(egui::WindowLevel::Normal)
        // Needed for egui-wgpu's alpha-mode picker to select a transparent
        // CompositeAlphaMode (see `egui-wgpu-0.34.3/src/winit.rs`); the actual
        // per-pixel compositing is provided by the DComp swap chain configured
        // via `wgpu_options` below plus `win_opacity::set_no_redirection_bitmap`.
        .with_transparent(true);

    // Force the DX12 backend with a DirectComposition-backed swap chain
    // (`DxgiFromVisual`), so the host renders with real per-pixel alpha instead
    // of DWM's normal opaque flip-model swap chain — the only way to get
    // translucency once the window is reparented into WorkerW, where
    // WS_EX_LAYERED (used for opacity in the other window layers) is rejected.
    // See issue #131 and `docs/architecture.md` "Desktop wallpaper mode".
    let wgpu_options = eframe::egui_wgpu::WgpuConfiguration {
        wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(
            eframe::egui_wgpu::WgpuSetupCreateNew {
                instance_descriptor: eframe::wgpu::InstanceDescriptor {
                    backends: eframe::wgpu::Backends::DX12,
                    backend_options: eframe::wgpu::BackendOptions {
                        dx12: eframe::wgpu::Dx12BackendOptions {
                            presentation_system: eframe::wgpu::Dx12SwapchainKind::DxgiFromVisual,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..eframe::wgpu::InstanceDescriptor::new_without_display_handle()
                },
                power_preference: render_power_preference(),
                ..eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle()
            },
        ),
        ..Default::default()
    };

    let options = eframe::NativeOptions {
        viewport,
        wgpu_options,
        ..Default::default()
    };

    // `dir` is moved into the eframe closure (the host owns it); keep a copy for
    // logging the run result after run_native returns, and an `Arc` copy for
    // the GPU-loss guard's callbacks.
    let log_dir = dir.clone();
    let dir_arc = Arc::new(dir.clone());

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

            let gpu_lost = Arc::new(AtomicBool::new(false));
            if let Some(render_state) = cc.wgpu_render_state.as_ref() {
                log_render_adapter(render_state, &dir_arc);
                install_gpu_loss_guard(
                    render_state,
                    cc.egui_ctx.clone(),
                    dir_arc.clone(),
                    gpu_lost.clone(),
                );
            }

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
                unattached_since: None,
                attach_warned: false,
                last_parent_check: Instant::now(),
                gpu_lost,
                preferred_gpu: preferred_gpu_arc,
                floating,
                panel_scale,
                windows,
                closing: false,
                panels_layout: layout,
                screen_watch: rigstats_egui::geometry::LayoutWatch::new(true),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn floating() -> settings::Settings {
        let mut s = settings::Settings::default();
        s.floating_mode = true;
        s.visible_panels = vec!["cpu".into(), "gpu".into()];
        s.panel_layouts
            .insert("cpu".into(), settings::PanelLayout { x: 300, y: 400 });
        s
    }

    #[test]
    fn panels_open_where_floating_mode_left_them() {
        // #170: the saved floating position, else floating mode's default.
        let w = panel_windows(&floating());
        assert_eq!(
            w.iter().map(|p| p.content.name()).collect::<Vec<_>>(),
            ["cpu", "gpu"]
        );
        assert_eq!(w[0].pos, [300.0, 400.0]);
        assert_eq!(w[1].pos, [120.0, 110.0]);
    }

    #[test]
    fn a_moved_shown_or_resized_panel_rebuilds_the_windows() {
        let s = floating();
        let before = panels_layout(&s);
        let mut moved = s.clone();
        moved
            .panel_layouts
            .insert("gpu".into(), settings::PanelLayout { x: 900, y: 100 });
        assert_ne!(panels_layout(&moved), before);
        let mut shown = s.clone();
        shown.visible_panels.push("ram".into());
        assert_ne!(panels_layout(&shown), before);
        let mut bigger = s.clone();
        bigger.floating_panel_scale = 1.5;
        assert_ne!(panels_layout(&bigger), before);
        // A hidden panel's position isn't part of it.
        let mut other = s.clone();
        other
            .panel_layouts
            .insert("disk".into(), settings::PanelLayout { x: 1, y: 1 });
        assert_eq!(panels_layout(&other), before);
    }
}
