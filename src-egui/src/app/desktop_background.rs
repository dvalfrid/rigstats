//! Shows the active profile's desktop background (#306). The current
//! settings are the active profile's look, so a profile switch, a preview
//! in the Control Center and a Revert all arrive as a change of
//! `Settings::desktop_background`; `desktop_background::step` decides what
//! Windows shows.

use crate::RigStatsApp;
use rigstats_backend::debug;
use rigstats_backend::desktop_background::{self, DesktopBackground};
use rigstats_egui::lock_ext::LockSafe;
use std::path::PathBuf;
use std::sync::mpsc;

/// What was last handed to Windows, and the worker that does it.
pub(crate) struct DesktopBackgroundSync {
    /// The profile's background last acted on; `None` until the first frame.
    seen: Option<Option<DesktopBackground>>,
    tx: mpsc::Sender<DesktopBackground>,
}

impl DesktopBackgroundSync {
    /// Starts the worker: setting the background blocks while Windows tells
    /// every window, so it runs off the UI thread, and only the latest of
    /// several quick changes (a preview being tried) is shown.
    pub(crate) fn new(dir: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<DesktopBackground>();
        let _ = std::thread::Builder::new()
            .name("desktop-background".into())
            .spawn(move || {
                while let Ok(mut bg) = rx.recv() {
                    while let Ok(newer) = rx.try_recv() {
                        bg = newer;
                    }
                    match desktop_background::show(&bg) {
                        Ok(()) if bg.screens.is_empty() => debug::log_debug(
                            &dir,
                            &format!("desktop background: showing '{}'", bg.path),
                        ),
                        Ok(()) => {
                            let each: Vec<&str> =
                                bg.screens.iter().map(|s| s.path.as_str()).collect();
                            debug::log_debug(
                                &dir,
                                &format!("desktop background: one per screen {each:?}"),
                            )
                        }
                        Err(e) => debug::log_warn(
                            &dir,
                            &format!("desktop background: couldn't show '{}' — {e}", bg.path),
                        ),
                    }
                }
            });
        Self { seen: None, tx }
    }
}

impl RigStatsApp {
    /// Runs every frame: acts when the profile's background has changed.
    pub(crate) fn sync_desktop_background(&mut self) {
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
            let _ = self.desktop_background.tx.send(bg);
        }
    }
}
