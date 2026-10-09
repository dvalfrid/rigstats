//! Groups of `RigStatsApp` fields that belong to one concern (#225).

use rigstats_backend::settings::Settings;
use rigstats_egui::dcomp_burst::DcompRevealBurst;
use rigstats_egui::dialog_reveal::DialogReveal;
use rigstats_egui::windows;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The dialog windows — Settings, About, Status, Updates, Session History
/// and the Control Center: whether each is open, the one-shot flag that
/// focuses it, and its state.
pub(crate) struct DialogStates {
    pub(crate) settings_open: Arc<AtomicBool>,
    pub(crate) about_open: Arc<AtomicBool>,
    pub(crate) status_open: Arc<AtomicBool>,
    pub(crate) updater_open: Arc<AtomicBool>,
    pub(crate) history_open: Arc<AtomicBool>,
    /// Control Center window (#187) — opened via the header panel's
    /// active-profile chip (`"open_control_center"` temp flag, consumed in
    /// `draw_one_panel`) or the tray.
    pub(crate) control_open: Arc<AtomicBool>,
    // Set to true when a dialog is opened; cleared on first callback frame to send Focus.
    pub(crate) settings_focus: Arc<AtomicBool>,
    pub(crate) about_focus: Arc<AtomicBool>,
    pub(crate) status_focus: Arc<AtomicBool>,
    pub(crate) updater_focus: Arc<AtomicBool>,
    pub(crate) history_focus: Arc<AtomicBool>,
    pub(crate) control_focus: Arc<AtomicBool>,
    pub(crate) settings_win: Arc<Mutex<windows::settings::SettingsWindow>>,
    pub(crate) status_win: Arc<Mutex<windows::status::StatusState>>,
    pub(crate) status_refreshing: Arc<AtomicBool>,
    pub(crate) status_collecting: Arc<AtomicBool>,
    pub(crate) updater_win: Arc<Mutex<windows::updater::UpdaterState>>,
    pub(crate) history_win: Arc<Mutex<windows::history::HistoryState>>,
    pub(crate) history_refreshing: Arc<AtomicBool>,
    pub(crate) history_loading_rows: Arc<AtomicBool>,
    /// Control Center tab/selection and unsaved fan-curve draft (#188).
    pub(crate) control_ui: windows::control::ControlUi,
    /// Keeps each freshly opened dialog hidden until it has rendered (no
    /// white flash) — see `dialog_reveal`.
    pub(crate) dialog_reveal: DialogReveal,
    /// Whether any dialog was open last frame; used to restore dark visuals when the
    /// last dialog closes (avoids calling set_visuals every frame).
    pub(crate) any_dialog_open_prev: bool,
}

impl DialogStates {
    /// All closed. The updater's state and flags come from `main()`, which
    /// opens it at start-up after an update and runs the background check.
    pub(crate) fn new(
        settings_win: windows::settings::SettingsWindow,
        updater_win: Arc<Mutex<windows::updater::UpdaterState>>,
        updater_open: Arc<AtomicBool>,
        updater_focus: Arc<AtomicBool>,
    ) -> Self {
        let flag = || Arc::new(AtomicBool::new(false));
        Self {
            settings_open: flag(),
            about_open: flag(),
            status_open: flag(),
            updater_open,
            history_open: flag(),
            control_open: flag(),
            settings_focus: flag(),
            about_focus: flag(),
            status_focus: flag(),
            updater_focus,
            history_focus: flag(),
            control_focus: flag(),
            settings_win: Arc::new(Mutex::new(settings_win)),
            status_win: Arc::new(Mutex::new(windows::status::StatusState::placeholder())),
            status_refreshing: flag(),
            status_collecting: flag(),
            updater_win,
            history_win: Arc::new(Mutex::new(windows::history::HistoryState::placeholder())),
            history_refreshing: flag(),
            history_loading_rows: flag(),
            control_ui: windows::control::ControlUi::default(),
            dialog_reveal: DialogReveal::default(),
            any_dialog_open_prev: false,
        }
    }
}

/// The game overlay (issue #183): an independent add-on window — not a
/// `window_layer` value — that can be shown/hidden regardless of what the
/// main window (Normal/Floating/Wallpaper/etc.) is doing, the same way
/// in-game overlays coexist with whatever else is on screen. Rendered via
/// its own `show_viewport_immediate` viewport (`render_overlay_viewport`),
/// mirroring how floating panels each own a secondary viewport, rather than
/// reusing the root window.
pub(crate) struct OverlayState {
    /// Whether the overlay viewport was shown last frame — edge-detects
    /// enable/disable transitions so `dcomp`/`positioned` reset cleanly
    /// each time it's freshly (re-)enabled.
    pub(crate) active: bool,
    /// Cached from settings — the overlay's on/off state.
    pub(crate) enabled: bool,
    /// Single source of truth for click-through: the tray, the global
    /// hotkey, and the Settings switch all just flip and persist this —
    /// applied immediately (bypassing Settings dialog Save/Cancel) via the
    /// guarded `MousePassthrough` dispatch inside the overlay's own viewport.
    pub(crate) click_through: bool,
    /// Last click-through value actually sent via `MousePassthrough` —
    /// avoids re-sending the same viewport command every frame.
    pub(crate) last_applied_click_through: Option<bool>,
    /// DComp reveal-burst state for the overlay's own OS window — see
    /// `dcomp_burst::DcompRevealBurst`. Reset (`start()`) each time the
    /// overlay is freshly (re-)enabled, since the window is destroyed and a
    /// fresh one is created next time it's shown.
    pub(crate) dcomp: DcompRevealBurst,
    /// Whether the overlay's initial position (anchor or saved free-drag
    /// spot) has been applied for the current activation — mirrors floating
    /// panels' `panels_positioned`: after the first frame, a `"free"`-anchored
    /// overlay's position is OS-owned (via drag) rather than re-sent, which
    /// would fight the OS and cause sub-pixel blur. Reset to `false` whenever
    /// the overlay transitions from disabled to enabled.
    pub(crate) positioned: bool,
    /// Content inset (`overlay::content_inset`) applied last frame, used to
    /// detect a Background/Scale-driven change so a `"free"`-anchored
    /// overlay's saved drag position can be compensated by the same delta —
    /// otherwise, since a `"free"` position's window origin is never
    /// resent once dragged (see `overlay_anchor_position`'s doc comment),
    /// toggling Background would grow the window from that fixed origin and
    /// visibly shift the content by the new padding instead of leaving it
    /// in place.
    pub(crate) last_content_inset: Option<[f32; 2]>,
    /// Last overlay content size seen, used to detect the very first size
    /// after each activation — only that first size gets the full DComp
    /// hide/burst treatment (`dcomp.start()`); later resizes (e.g.
    /// dragging Scale) just resize immediately, since the DComp binding
    /// survives an ordinary resize once correctly established.
    pub(crate) last_size: Option<[f32; 2]>,
    /// Current free-drag position for the overlay this session. Loaded from
    /// `Settings.overlay_position` at startup; updated on drag and persisted
    /// (debounced via `position_dirty`), mirroring `floating_positions`.
    pub(crate) position: Option<[f32; 2]>,
    /// Set true when the overlay is dragged this frame; consumed in `ui()`
    /// to debounce the settings write to once per tick.
    pub(crate) position_dirty: bool,
}

impl OverlayState {
    pub(crate) fn new(s: &Settings) -> Self {
        Self {
            active: false,
            enabled: s.overlay_enabled,
            click_through: s.overlay_click_through,
            last_applied_click_through: None,
            dcomp: DcompRevealBurst::default(),
            positioned: false,
            last_content_inset: None,
            last_size: None,
            position: s.overlay_position.map(|p| [p[0] as f32, p[1] as f32]),
            position_dirty: false,
        }
    }
}

/// Recovery from a lost GPU device (e.g. a hybrid iGPU/dGPU switch): the
/// app relaunches itself, backing off when it keeps failing fast.
pub(crate) struct GpuRecovery {
    /// Set by `gpu_guard::install_gpu_loss_guard`'s callbacks when wgpu
    /// reports a fatal device error. Checked once per frame in `update()`.
    pub(crate) lost: Arc<AtomicBool>,
    /// True once the `lost` relaunch-and-close sequence has been kicked
    /// off, so it only runs once even though `update()` keeps being called
    /// for the few frames it takes `ViewportCommand::Close` to take effect.
    pub(crate) relaunch_triggered: bool,
    /// When this app instance was created. Used to tell a GPU error that
    /// strikes again within seconds of a relaunch (still-unsettled GPU
    /// state — back off) from one after a long healthy run (a fresh,
    /// unrelated hiccup — always retry).
    pub(crate) started_at: Instant,
    /// Consecutive fast GPU-relaunch failures, carried in from the
    /// `RIGSTATS_GPU_RETRY_COUNT` env var set by the process that spawned
    /// this one. `0` for a normal (non-relaunch) start.
    pub(crate) retry_count: u32,
}

/// Forced font-atlas rebuilds after a viewport was minimized (see
/// `refresh_font_atlas_after_minimize`).
pub(crate) struct FontAtlasRefresh {
    /// Set while any viewport is minimized.
    pub(crate) stale: bool,
    /// Last forced font-atlas rebuild.
    pub(crate) rebuilt_at: Instant,
    /// Flips on every forced rebuild so the `FontDefinitions` passed to
    /// `set_fonts` always differs from whatever it last saw — `set_fonts`
    /// silently no-ops otherwise.
    pub(crate) toggle: bool,
}

/// The tray's recording indicator: a blinking icon while a session is
/// being recorded.
pub(crate) struct RecordingIndicator {
    /// True while a session is being recorded — drives the blinking tray dot.
    pub(crate) active: bool,
    /// Mirrors `active` for the tray-polling background thread (see its use
    /// of `win_opacity::force_repaint` in `app::background`, #177): while a
    /// context menu is open, winit's own event loop — and so `ui()` — doesn't
    /// run at all, and neither `request_repaint()`/`request_repaint_of()` nor a
    /// single pre-emptive `force_repaint()` reliably revives it once the menu
    /// closes (both empirically confirmed unreliable here). The poller instead
    /// keeps posting `force_repaint()` on every tick for as long as this is
    /// true, so the very next tick after the menu closes — whenever that is —
    /// lands a real repaint no matter how it closed.
    pub(crate) active_shared: Arc<AtomicBool>,
    /// Current phase of the blink (dot shown vs. hidden).
    pub(crate) blink_on: bool,
    /// When the blink last flipped.
    pub(crate) blink_at: Instant,
}
