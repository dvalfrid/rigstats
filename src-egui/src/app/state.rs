//! Groups of `RigStatsApp` fields that belong to one concern (#225).

use rigstats_backend::settings::Settings;
use rigstats_egui::dcomp_burst::DcompRevealBurst;

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
