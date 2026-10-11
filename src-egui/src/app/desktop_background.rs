//! Shows the active profile's desktop background and Wallpaper Engine
//! choice (#306). The current settings are the active profile's look, so a
//! profile switch, a preview in the Control Center and a Revert all arrive
//! as a change of `Settings::desktop_background` / `wallpaper_engine`;
//! `desktop_background::step` and `wallpaper_engine::step` decide what to do.

use crate::RigStatsApp;
use rigstats_backend::debug;
use rigstats_backend::desktop_background::{self, DesktopBackground};
use rigstats_backend::wallpaper_engine::{self, Install, WeChoice, WeCommand};
use rigstats_egui::lock_ext::LockSafe;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Work for the worker thread.
enum Job {
    Background(DesktopBackground),
    WallpaperEngine(Install, Vec<WeCommand>),
}

/// What was last acted on, and the worker that does it.
pub(crate) struct DesktopBackgroundSync {
    /// The profile's background last acted on; `None` until the first frame.
    seen: Option<Option<DesktopBackground>>,
    /// The profile's Wallpaper Engine choice last acted on.
    we_seen: Option<Option<WeChoice>>,
    /// Wallpaper Engine's install and whether it runs, looked up every few
    /// seconds.
    we: Option<Install>,
    we_running: bool,
    we_checked: Option<Instant>,
    /// Whether RIGStats last paused (`Some(true)`) or resumed it; `None`
    /// until then, and again after it (re)started.
    we_paused: Option<bool>,
    tx: mpsc::Sender<Job>,
}

impl DesktopBackgroundSync {
    /// Starts the worker: setting the background blocks while Windows tells
    /// every window, and Wallpaper Engine's commands take a moment, so they
    /// run off the UI thread. Of several quick changes (a preview being
    /// tried) only the latest of each kind is carried out — each job is the
    /// whole of what should show.
    pub(crate) fn new(dir: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let _ = std::thread::Builder::new()
            .name("desktop-background".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    let (mut bg, mut we) = (None, None);
                    for job in std::iter::once(first).chain(rx.try_iter()) {
                        match job {
                            Job::Background(b) => bg = Some(b),
                            Job::WallpaperEngine(i, c) => we = Some((i, c)),
                        }
                    }
                    if let Some(bg) = bg {
                        show_background(&dir, &bg);
                    }
                    if let Some((install, commands)) = we {
                        for c in &commands {
                            match install.send(c) {
                                Ok(()) => debug::log_debug(
                                    &dir,
                                    &format!("wallpaper engine: {}", c.args()[1..].join(" ")),
                                ),
                                Err(e) => debug::log_warn(
                                    &dir,
                                    &format!("wallpaper engine: {:?} failed — {e}", c),
                                ),
                            }
                        }
                    }
                }
            });
        Self {
            seen: None,
            we_seen: None,
            we: None,
            we_running: false,
            we_checked: None,
            we_paused: None,
            tx,
        }
    }
}

fn show_background(dir: &std::path::Path, bg: &DesktopBackground) {
    match desktop_background::show(bg) {
        Ok(()) if bg.screens.is_empty() => {
            debug::log_debug(dir, &format!("desktop background: showing '{}'", bg.path))
        }
        Ok(()) => {
            let each: Vec<&str> = bg.screens.iter().map(|s| s.path.as_str()).collect();
            debug::log_debug(dir, &format!("desktop background: one per screen {each:?}"))
        }
        Err(e) => debug::log_warn(
            dir,
            &format!("desktop background: couldn't show '{}' — {e}", bg.path),
        ),
    }
}

impl RigStatsApp {
    /// Runs every frame: acts when the profile's background or Wallpaper
    /// Engine choice has changed.
    pub(crate) fn sync_desktop_background(&mut self) {
        self.sync_windows_background();
        self.sync_wallpaper_engine();
    }

    fn sync_windows_background(&mut self) {
        let mut s = self.current_settings.lock_safe();
        if self.desktop_background.seen.as_ref() == Some(&s.desktop_background) {
            return;
        }
        self.desktop_background.seen = Some(s.desktop_background.clone());
        let step = desktop_background::step(
            s.desktop_background.as_ref(),
            s.desktop_background_before.as_ref(),
            desktop_background::current,
        );
        if step.before != s.desktop_background_before {
            s.desktop_background_before = step.before;
            self.persist_settings_logged(&s);
        }
        drop(s);
        if let Some(bg) = step.show {
            let _ = self.desktop_background.tx.send(Job::Background(bg));
        }
    }

    /// Commands only go to a running Wallpaper Engine. When it starts (after
    /// RIGStats at boot, or restarted by the user) it shows its own
    /// wallpapers, so the profile's choice is applied again.
    fn sync_wallpaper_engine(&mut self) {
        let sync = &mut self.desktop_background;
        if sync
            .we_checked
            .map_or(true, |t| t.elapsed() > Duration::from_secs(5))
        {
            sync.we_checked = Some(Instant::now());
            sync.we = wallpaper_engine::find();
            let running = sync.we.is_some() && wallpaper_engine::is_running();
            if running && !sync.we_running {
                sync.we_seen = None;
                sync.we_paused = None;
            }
            sync.we_running = running;
        }
        let (Some(install), true) = (sync.we.clone(), sync.we_running) else {
            return;
        };
        let mut s = self.current_settings.lock_safe();
        if self.desktop_background.we_seen.as_ref() == Some(&s.wallpaper_engine) {
            return;
        }
        self.desktop_background.we_seen = Some(s.wallpaper_engine.clone());
        let config = install.config().unwrap_or_default();
        let step = wallpaper_engine::step(
            s.wallpaper_engine.as_ref(),
            s.wallpaper_engine_before.as_deref(),
            || (!config.selected.is_empty()).then(|| config.selected.clone()),
            |id| config.monitor_of(id),
        );
        if step.before != s.wallpaper_engine_before {
            s.wallpaper_engine_before = step.before;
            self.persist_settings_logged(&s);
        }
        drop(s);
        let sync = &mut self.desktop_background;
        let commands = wallpaper_engine::prune(step.commands, &config.selected, sync.we_paused);
        for c in &commands {
            match c {
                WeCommand::Pause => sync.we_paused = Some(true),
                WeCommand::Play => sync.we_paused = Some(false),
                WeCommand::Open { .. } => {}
            }
        }
        if !commands.is_empty() {
            let _ = sync.tx.send(Job::WallpaperEngine(install, commands));
        }
    }
}
