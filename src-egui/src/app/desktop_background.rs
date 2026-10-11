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
use rigstats_egui::we_link::{WeAction, WeLink};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Work for the worker thread.
enum Job {
    Background(DesktopBackground),
    WallpaperEngine(Install, Vec<WeCommand>),
    Launch(Install),
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
    /// Whether it was found at the last check (`None` before the first):
    /// changes are logged once.
    we_found: Option<bool>,
    /// `config.json` couldn't be read — logged once until it can again.
    we_config_broken: bool,
    /// Shared with the Control Center's Wallpaper Engine group.
    pub(crate) link: WeLink,
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
        let link = WeLink::default();
        let shared = link.clone();
        let _ = std::thread::Builder::new()
            .name("desktop-background".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    let (mut bg, mut we, mut launch) = (None, None, None);
                    for job in std::iter::once(first).chain(rx.try_iter()) {
                        match job {
                            Job::Background(b) => bg = Some(b),
                            Job::WallpaperEngine(i, c) => we = Some((i, c)),
                            Job::Launch(i) => launch = Some(i),
                        }
                    }
                    if let Some(install) = launch {
                        match install.launch() {
                            Ok(()) => debug::append_debug_log(
                                &dir,
                                &format!("wallpaper engine: starting {}", install.exe.display()),
                            ),
                            Err(e) => {
                                debug::log_error(
                                    &dir,
                                    &format!(
                                        "wallpaper engine: couldn't start {} — {e}",
                                        install.exe.display()
                                    ),
                                );
                                let mut s = shared.lock_safe();
                                s.starting = false;
                                s.last_error = Some(format!("Couldn't start it: {e}"));
                            }
                        }
                    }
                    if let Some(bg) = bg {
                        show_background(&dir, &bg);
                    }
                    if let Some((install, commands)) = we {
                        let mut failed = None;
                        for c in &commands {
                            let what = c.args()[1..].join(" ");
                            match install.send(c) {
                                Ok(()) => {
                                    debug::log_debug(&dir, &format!("wallpaper engine: {what}"))
                                }
                                Err(e) => {
                                    debug::log_warn(
                                        &dir,
                                        &format!("wallpaper engine: '{what}' failed — {e}"),
                                    );
                                    failed = Some(format!("'{}' failed: {e}", c.args()[1]));
                                }
                            }
                        }
                        shared.lock_safe().last_error = failed;
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
            we_found: None,
            we_config_broken: false,
            link,
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
    /// wallpapers, so the profile's choice is applied again. Every change
    /// of state is logged once, for troubleshooting.
    fn sync_wallpaper_engine(&mut self) {
        let dir = self.dir.clone();
        let sync = &mut self.desktop_background;
        let requests = std::mem::take(&mut sync.link.lock_safe().requests);
        if !requests.is_empty() {
            debug::log_debug(&dir, &format!("wallpaper engine: requested {requests:?}"));
        }
        if sync
            .we_checked
            .map_or(true, |t| t.elapsed() > Duration::from_secs(5))
            || !requests.is_empty()
        {
            sync.we_checked = Some(Instant::now());
            sync.we = wallpaper_engine::find();
            let found = sync.we.is_some();
            if sync.we_found != Some(found) {
                match &sync.we {
                    Some(i) => debug::append_debug_log(
                        &dir,
                        &format!("wallpaper engine: found at {}", i.exe.display()),
                    ),
                    None => debug::append_debug_log(&dir, "wallpaper engine: not installed"),
                }
                sync.we_found = Some(found);
            }
            let running = found && wallpaper_engine::is_running();
            if running != sync.we_running {
                if running {
                    debug::append_debug_log(&dir, "wallpaper engine: running");
                    sync.we_seen = None;
                    sync.we_paused = None;
                } else {
                    debug::log_warn(&dir, "wallpaper engine: not running (closed or crashed)");
                }
            }
            sync.we_running = running;
            let mut shared = sync.link.lock_safe();
            if running {
                shared.starting = false;
                shared.starting_since = None;
            } else if shared
                .starting_since
                .is_some_and(|t| t.elapsed() > Duration::from_secs(30))
            {
                debug::log_warn(
                    &dir,
                    "wallpaper engine: started, but not running 30 s later — it may be waiting for an answer in its own window",
                );
                shared.starting = false;
                shared.starting_since = None;
                shared.last_error =
                    Some("It didn't come up — open Wallpaper Engine yourself to see why".into());
            }
            if running && !shared.running {
                // It starts playing its own wallpapers.
                shared.stopped = false;
            }
            shared.running = running;
        }
        let Some(install) = sync.we.clone() else {
            if !requests.is_empty() {
                debug::log_warn(
                    &dir,
                    "wallpaper engine: a button was clicked, but it isn't installed (any more)",
                );
            }
            return;
        };
        for action in requests {
            match action {
                WeAction::Start if !sync.we_running => {
                    let mut shared = sync.link.lock_safe();
                    shared.starting = true;
                    shared.starting_since = Some(Instant::now());
                    shared.last_error = None;
                    drop(shared);
                    let _ = sync.tx.send(Job::Launch(install.clone()));
                }
                WeAction::Stop if sync.we_running => {
                    debug::append_debug_log(&dir, "wallpaper engine: stopped by the user");
                    sync.link.lock_safe().stopped = true;
                    // Whatever plays next needs an explicit play.
                    sync.we_paused = None;
                    let _ = sync
                        .tx
                        .send(Job::WallpaperEngine(install.clone(), vec![WeCommand::Stop]));
                }
                WeAction::Resume | WeAction::Start if sync.we_running => {
                    debug::append_debug_log(&dir, "wallpaper engine: resumed by the user");
                    sync.link.lock_safe().stopped = false;
                    sync.we_paused = Some(false);
                    let _ = sync
                        .tx
                        .send(Job::WallpaperEngine(install.clone(), vec![WeCommand::Play]));
                }
                other => debug::log_debug(
                    &dir,
                    &format!(
                        "wallpaper engine: {other:?} ignored (running: {})",
                        sync.we_running
                    ),
                ),
            }
        }
        if !sync.we_running {
            return;
        }
        let mut s = self.current_settings.lock_safe();
        if self.desktop_background.we_seen.as_ref() == Some(&s.wallpaper_engine) {
            return;
        }
        self.desktop_background.we_seen = Some(s.wallpaper_engine.clone());
        let config = match install.config() {
            Some(c) => {
                self.desktop_background.we_config_broken = false;
                c
            }
            None => {
                if !self.desktop_background.we_config_broken {
                    debug::log_warn(
                        &self.dir,
                        "wallpaper engine: couldn't read its config.json — screens can't be matched",
                    );
                    self.desktop_background.we_config_broken = true;
                }
                Default::default()
            }
        };
        if let Some(WeChoice::Show { screens }) = &s.wallpaper_engine {
            for sp in screens {
                if config.monitor_of(&sp.screen).is_none() {
                    debug::log_warn(
                        &self.dir,
                        &format!(
                            "wallpaper engine: screen {} isn't in its monitor map — skipped",
                            sp.screen
                        ),
                    );
                }
            }
        }
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
        // A wallpaper that's gone (unsubscribed) isn't sent.
        let present: Vec<WeCommand> = step
            .commands
            .into_iter()
            .filter(|c| match c {
                WeCommand::Open { monitor, file } if !std::path::Path::new(file).is_file() => {
                    debug::log_warn(
                        &self.dir,
                        &format!("wallpaper engine: skipped screen {monitor} — {file} is gone"),
                    );
                    false
                }
                _ => true,
            })
            .collect();
        let commands = wallpaper_engine::prune(present, &config.selected, sync.we_paused);
        for c in &commands {
            match c {
                WeCommand::Pause => sync.we_paused = Some(true),
                WeCommand::Play => {
                    sync.we_paused = Some(false);
                    sync.link.lock_safe().stopped = false;
                }
                WeCommand::Stop | WeCommand::Open { .. } => {}
            }
        }
        if !commands.is_empty() {
            let _ = sync.tx.send(Job::WallpaperEngine(install, commands));
        }
    }
}
