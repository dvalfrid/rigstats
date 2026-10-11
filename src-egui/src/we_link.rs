//! What the Control Center's Wallpaper Engine group and the main app share
//! (#306): the buttons queue an action, the app carries it out on its
//! worker thread (`app/desktop_background.rs`) and reports back whether
//! Wallpaper Engine runs, whether RIGStats stopped it, and the last error.

use std::sync::{Arc, Mutex};

/// A button's request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeAction {
    /// Launch Wallpaper Engine.
    Start,
    /// Stop its wallpapers: the Windows background shows.
    Stop,
    /// Bring stopped wallpapers back.
    Resume,
}

/// The shared state.
#[derive(Debug, Default)]
pub struct WeShared {
    /// Installed and running, as last checked (every few seconds).
    pub running: bool,
    /// RIGStats stopped its wallpapers (Stop); cleared by Resume, by a
    /// profile that plays it, and when Wallpaper Engine starts again.
    pub stopped: bool,
    /// Launched, and not seen running yet.
    pub starting: bool,
    /// When it was launched: if it isn't running half a minute later, that
    /// is reported instead of "Starting…" for ever.
    pub starting_since: Option<std::time::Instant>,
    /// What went wrong last, until something succeeds.
    pub last_error: Option<String>,
    /// Button clicks waiting for the app.
    pub requests: Vec<WeAction>,
}

pub type WeLink = Arc<Mutex<WeShared>>;
