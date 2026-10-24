//! Persistent user settings model and file I/O helpers.

use crate::desktop_background::DesktopBackground;
use crate::shortcut::Shortcuts;
use crate::wallpaper_engine::{WeChoice, WeSlot};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// --- Panel layout ----------------------------------------------------------

/// Saved screen position for a single floating panel window.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PanelLayout {
    pub x: i32,
    pub y: i32,
}

// --- Component thresholds --------------------------------------------------

/// Warn/critical threshold pair for a hardware component.
///
/// Semantics differ by component type:
/// - Temperature (cpu/gpu/ram/disk): fires when reading **exceeds** the threshold.
/// - Load/usage percentage (cpu_load/gpu_load/ram_load): fires when the
///   reading **exceeds** the threshold, same direction as temperature but a
///   distinct key since the values are percentages, not °C.
/// - Battery: fires when charge % **drops below** the threshold (warn > crit).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ComponentThresholds {
    pub warn: Option<u8>,
    pub crit: Option<u8>,
}

/// Default threshold map applied on fresh installs and as migration fallback.
pub fn default_thresholds() -> HashMap<String, ComponentThresholds> {
    [
        (
            "cpu",
            ComponentThresholds {
                warn: Some(80),
                crit: Some(90),
            },
        ),
        (
            "gpu",
            ComponentThresholds {
                warn: Some(80),
                crit: Some(90),
            },
        ),
        (
            "cpu_load",
            ComponentThresholds {
                warn: Some(80),
                crit: Some(95),
            },
        ),
        (
            "gpu_load",
            ComponentThresholds {
                warn: Some(80),
                crit: Some(95),
            },
        ),
        (
            "ram",
            ComponentThresholds {
                warn: Some(50),
                crit: Some(65),
            },
        ),
        (
            "ram_load",
            ComponentThresholds {
                warn: Some(80),
                crit: Some(95),
            },
        ),
        (
            "disk",
            ComponentThresholds {
                warn: Some(55),
                crit: Some(70),
            },
        ),
        (
            // Disk space used %, per drive — was a hardcoded 75/90 cutoff in
            // the Disk panel's usage bar; matches those defaults now that
            // it's user-configurable.
            "disk_usage",
            ComponentThresholds {
                warn: Some(75),
                crit: Some(90),
            },
        ),
        (
            // Battery charge: fires when % drops BELOW threshold (warn > crit).
            "battery",
            ComponentThresholds {
                warn: Some(20),
                crit: Some(10),
            },
        ),
        (
            // Battery power draw: fires when watts exceed threshold (warn < crit).
            "battery_power",
            ComponentThresholds {
                warn: Some(15),
                crit: Some(25),
            },
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Which releases the updater offers (#535).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    /// Releases only (`releases/latest`).
    #[default]
    Stable,
    /// The newest release candidate too, when it is newer than the newest
    /// release.
    ReleaseCandidates,
}

// --- Settings struct -------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Panel alpha in the range [0.0, 1.0].
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    /// Active colour theme key (e.g. `"dark-cyan"`).
    #[serde(default = "default_theme")]
    pub theme: String,
    /// User-defined model label shown in the header panel.
    #[serde(default = "default_model_name")]
    pub model_name: String,
    /// Active dashboard size profile.
    #[serde(default = "default_dashboard_profile")]
    pub dashboard_profile: String,
    /// Keep the dashboard window above other windows.
    #[serde(default)]
    pub always_on_top: bool,
    /// Ordered list of visible dashboard panels.
    #[serde(default = "default_visible_panels")]
    pub visible_panels: Vec<String>,
    /// Panels drawn in their compact layout: the key values only, no graphs
    /// or per-core bars (#596). Empty = every panel full.
    #[serde(default)]
    pub compact_panels: Vec<String>,
    /// Launch the dashboard automatically when the user logs in.
    #[serde(default)]
    pub autostart_enabled: bool,
    /// Version seen on last launch, used to detect first run after an update.
    #[serde(default)]
    pub last_seen_version: String,
    /// Which releases the updater offers (#535).
    #[serde(default)]
    pub update_channel: UpdateChannel,
    /// Per-component temperature alert thresholds.
    /// Keys: "cpu", "gpu", "ram", "disk" (and any future components).
    #[serde(default)]
    pub thresholds: HashMap<String, ComponentThresholds>,
    /// Minimum seconds between repeated notifications for the same component+level.
    /// Floored at 60 s on save to prevent notification spam.
    #[serde(default = "default_alert_cooldown_secs")]
    pub alert_cooldown_secs: u64,
    /// Whether to send notifications when a WARNING threshold is crossed.
    #[serde(default = "default_true")]
    pub notify_on_warn: bool,
    /// Whether to send notifications when a CRITICAL threshold is crossed.
    #[serde(default = "default_true")]
    pub notify_on_crit: bool,
    /// Which microphone its shortcut and the tray mute (#425): a Windows
    /// endpoint id from `microphone::list`, or `None` for the one Windows
    /// uses for calls (also the fallback when the chosen one isn't connected).
    #[serde(default)]
    pub microphone_device: Option<String>,
    /// Whether muting or unmuting the microphone shows a notification (#425).
    #[serde(default = "default_true")]
    pub microphone_notify: bool,
    /// The global shortcuts' chords, or off (#569); a file without them
    /// gets the chords from before (`Shortcuts::default`).
    #[serde(default)]
    pub shortcuts: Shortcuts,
    /// Open each visible panel as its own frameless window instead of one portrait window.
    #[serde(default)]
    pub floating_mode: bool,
    /// Scale factor for floating panel windows in the range [0.4, 2.0].
    #[serde(default = "default_floating_panel_scale")]
    pub floating_panel_scale: f64,
    /// Last known screen position for each floating panel, keyed by panel
    /// key — the active profile's (#599).
    #[serde(default)]
    pub panel_layouts: HashMap<String, PanelLayout>,
    /// The floating panels' positions of the other Control Center profiles
    /// (#599), keyed by profile id, swapped in `switch_profile_look`. Kept
    /// beside the profile's look, not in it: positions change by dragging,
    /// not in the Control Center's edit with its preview and Revert.
    #[serde(default)]
    pub profile_panel_layouts: HashMap<String, HashMap<String, PanelLayout>>,
    /// Schema version used to detect and apply one-time migrations.
    /// 0 = legacy flat threshold fields (pre-map), 1 = current map format.
    #[serde(default)]
    pub settings_version: u8,
    /// User's preferred GPU device name for stable display when multiple GPUs are available.
    /// If set and the GPU exists, that GPU will be displayed. Otherwise, auto-selection by load is used.
    #[serde(default)]
    pub preferred_gpu: Option<String>,
    /// Window z-layer mode: `"normal"`, `"on_top"` (always on top), `"behind"`
    /// (HWND_BOTTOM), or `"wallpaper"` (reparented into the desktop WorkerW layer
    /// via the `rigstats-wallpaper` host process — survives Win+D).
    #[serde(default = "default_window_layer")]
    pub window_layer: String,
    /// When true, floating panel windows cannot be moved by dragging.
    #[serde(default)]
    pub floating_panels_locked: bool,
    /// Number of days to retain finished, unpinned recording sessions before automatic pruning.
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u32,
    /// Fill the whole monitor in non-floating mode; panels keep their size, the
    /// dashboard background fills the rest.
    #[serde(default)]
    pub fullscreen_mode: bool,
    /// Vertical placement of the panel stack when fullscreen: `"top"` | `"center"`.
    #[serde(default = "default_fullscreen_align")]
    pub fullscreen_align: String,
    /// When true, the non-floating (landscape/portrait) dashboard window is pinned:
    /// it cannot be dragged and its position is restored from `pinned_positions`
    /// across restarts instead of auto-targeting the matching monitor.
    #[serde(default)]
    pub dashboard_pinned: bool,
    /// Pinned window position `[x, y]` for the non-floating dashboard, keyed by
    /// dashboard profile (positions differ per profile/monitor).
    #[serde(default)]
    pub pinned_positions: HashMap<String, [i32; 2]>,
    /// Screen position `[x, y]` for the wallpaper-mode host window. Captured from
    /// the main window when switching into wallpaper mode (so the user positions it
    /// in a normal layer, then switches), and used by the `rigstats-wallpaper` host
    /// instead of centring on the monitor. `None` until first set.
    #[serde(default)]
    pub wallpaper_position: Option<[i32; 2]>,
    /// User-specified PSU rated wattage for the System Power panel bar scale.
    /// `None` = automatic reference (500 W desktop / 120 W laptop).
    #[serde(default)]
    pub psu_watts: Option<u16>,
    /// Ordered list of metric keys shown in Overlay mode (see `overlay.rs`).
    #[serde(default = "default_overlay_metrics")]
    pub overlay_metrics: Vec<String>,
    /// Overlay column count: `0` = every metric on one row, otherwise 1..=6
    /// columns filled row by row (1 = a vertical list).
    #[serde(default)]
    pub overlay_columns: u8,
    /// Overlay screen anchor: `"top-left"` | `"top-right"` | `"bottom-left"` |
    /// `"bottom-right"` | `"free"` (user-dragged, see `overlay_position`).
    #[serde(default = "default_overlay_anchor")]
    pub overlay_anchor: String,
    /// Margin in px from the anchored corner. Unused when `overlay_anchor == "free"`.
    #[serde(default = "default_overlay_margin")]
    pub overlay_margin: i32,
    /// Screen position `[x, y]` for a free-dragged overlay. `None` until first set.
    #[serde(default)]
    pub overlay_position: Option<[i32; 2]>,
    /// Overlay content scale factor.
    #[serde(default = "default_overlay_scale")]
    pub overlay_scale: f64,
    /// Overlay window alpha in the range [0.0, 1.0]. Independent of `opacity`,
    /// which controls the non-overlay dashboard window.
    #[serde(default = "default_overlay_opacity")]
    pub overlay_opacity: f64,
    /// Draw a background card behind overlay text. When false, text renders
    /// directly over whatever is beneath it (the per-pixel-alpha stack).
    #[serde(default = "default_true")]
    pub overlay_background: bool,
    /// When true, mouse clicks pass through the overlay window. Applied
    /// immediately when toggled (tray, hotkey, or Settings), independent of
    /// the Settings dialog's Save/Cancel flow.
    #[serde(default)]
    pub overlay_click_through: bool,
    /// Whether the overlay is currently shown, as an independent add-on
    /// window that coexists with whatever `window_layer`/`floating_mode` the
    /// main dashboard is in — not a `window_layer` value itself. Applied
    /// immediately when toggled (tray, hotkey, or Settings), same as
    /// `overlay_click_through` and for the same reason (it can be flipped
    /// from outside the Settings dialog while it's open).
    #[serde(default)]
    pub overlay_enabled: bool,
    /// The dashboard look of each Control Center profile (#305), keyed by
    /// profile id. The current settings are the active profile's look; a
    /// switch stores them in the outgoing profile and loads the incoming one
    /// (`switch_profile_look`).
    #[serde(default)]
    pub profile_looks: HashMap<String, ProfileLook>,
    /// Id of the profile the current look belongs to.
    #[serde(default)]
    pub look_profile: Option<String>,
    /// Programs that switch the Control Center profile while they run
    /// (#307), checked in order; see `profile_rules`.
    #[serde(default)]
    pub app_rules: Vec<crate::profile_rules::AppRule>,
    /// The active profile's desktop background (#306); `None` = Windows'
    /// own. Part of the profile's look.
    #[serde(default)]
    pub desktop_background: Option<DesktopBackground>,
    /// Windows' own background, kept the first time a profile replaced it
    /// and put back in a profile without one (`desktop_background::step`).
    #[serde(default)]
    pub desktop_background_before: Option<DesktopBackground>,
    /// The active profile's Wallpaper Engine choice (#306); `None` = leave
    /// it alone. Part of the profile's look.
    #[serde(default)]
    pub wallpaper_engine: Option<WeChoice>,
    /// Wallpaper Engine's own wallpapers, kept the first time a profile
    /// changed them and put back in a profile that leaves it alone
    /// (`wallpaper_engine::step`).
    #[serde(default)]
    pub wallpaper_engine_before: Option<Vec<WeSlot>>,

    // ---- Legacy migration shims (schema version 0) --------------------------
    // These fields existed in older settings files as eight flat values.
    // They are read from disk but never written back (`skip_serializing`).
    // `load_settings` copies them into `thresholds` exactly once, then bumps
    // `settings_version` to 1 so the migration never re-runs.
    #[serde(default, skip_serializing)]
    warning_cpu_temp: Option<u8>,
    #[serde(default, skip_serializing)]
    critical_cpu_temp: Option<u8>,
    #[serde(default, skip_serializing)]
    warning_gpu_temp: Option<u8>,
    #[serde(default, skip_serializing)]
    critical_gpu_temp: Option<u8>,
    #[serde(default, skip_serializing)]
    warning_ram_temp: Option<u8>,
    #[serde(default, skip_serializing)]
    critical_ram_temp: Option<u8>,
    #[serde(default, skip_serializing)]
    warning_disk_temp: Option<u8>,
    #[serde(default, skip_serializing)]
    critical_disk_temp: Option<u8>,

    // Pre-`overlay_columns` layout (`"horizontal"` | `"vertical"` | `"grid"`),
    // read once by `migrate_overlay_layout` and never written back.
    #[serde(default, skip_serializing)]
    overlay_layout: Option<String>,
}

fn default_alert_cooldown_secs() -> u64 {
    60
}

fn default_true() -> bool {
    true
}

fn default_floating_panel_scale() -> f64 {
    1.0
}

fn default_theme() -> String {
    "dark-cyan".to_string()
}

fn default_log_retention_days() -> u32 {
    7
}

fn default_window_layer() -> String {
    "normal".to_string()
}

fn default_fullscreen_align() -> String {
    "center".to_string()
}

fn default_overlay_metrics() -> Vec<String> {
    vec![
        "cpu_load".to_string(),
        "cpu_temp".to_string(),
        "gpu_load".to_string(),
        "gpu_temp".to_string(),
        "ram_pct".to_string(),
    ]
}

fn default_overlay_anchor() -> String {
    "top-right".to_string()
}

fn default_overlay_margin() -> i32 {
    16
}

fn default_overlay_scale() -> f64 {
    1.0
}

fn default_overlay_opacity() -> f64 {
    0.85
}

fn default_opacity() -> f64 {
    0.55
}

fn default_model_name() -> String {
    String::new()
}

fn default_dashboard_profile() -> String {
    "portrait-xl".to_string()
}

fn default_visible_panels() -> Vec<String> {
    vec![
        "header".to_string(),
        "clock".to_string(),
        "cpu".to_string(),
        "gpu".to_string(),
        "ram".to_string(),
        "net".to_string(),
        "disk".to_string(),
    ]
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            opacity: default_opacity(),
            theme: default_theme(),
            model_name: default_model_name(),
            dashboard_profile: default_dashboard_profile(),
            always_on_top: false,
            visible_panels: default_visible_panels(),
            compact_panels: Vec::new(),
            autostart_enabled: false,
            last_seen_version: String::new(),
            update_channel: UpdateChannel::Stable,
            thresholds: default_thresholds(),
            alert_cooldown_secs: default_alert_cooldown_secs(),
            notify_on_warn: true,
            notify_on_crit: true,
            microphone_device: None,
            microphone_notify: true,
            shortcuts: Shortcuts::default(),
            floating_mode: false,
            floating_panel_scale: default_floating_panel_scale(),
            panel_layouts: HashMap::new(),
            profile_panel_layouts: HashMap::new(),
            settings_version: 1, // New installs start at current version — no migration needed.
            preferred_gpu: None,
            window_layer: default_window_layer(),
            floating_panels_locked: false,
            log_retention_days: default_log_retention_days(),
            fullscreen_mode: false,
            fullscreen_align: default_fullscreen_align(),
            dashboard_pinned: false,
            pinned_positions: HashMap::new(),
            wallpaper_position: None,
            psu_watts: None,
            overlay_metrics: default_overlay_metrics(),
            overlay_columns: 0,
            overlay_anchor: default_overlay_anchor(),
            overlay_margin: default_overlay_margin(),
            overlay_position: None,
            overlay_scale: default_overlay_scale(),
            overlay_opacity: default_overlay_opacity(),
            overlay_background: true,
            overlay_click_through: false,
            overlay_enabled: false,
            profile_looks: HashMap::new(),
            look_profile: None,
            app_rules: Vec::new(),
            desktop_background: None,
            desktop_background_before: None,
            wallpaper_engine: None,
            wallpaper_engine_before: None,
            warning_cpu_temp: None,
            critical_cpu_temp: None,
            warning_gpu_temp: None,
            critical_gpu_temp: None,
            warning_ram_temp: None,
            critical_ram_temp: None,
            warning_disk_temp: None,
            critical_disk_temp: None,
            overlay_layout: None,
        }
    }
}

// --- Dashboard look per Control Center profile (#305) ---------------------

/// What a Control Center profile holds of the app's own settings: how the
/// dashboard looks (floating or not, and the floating panels' size, #599),
/// the overlay's content, the alert thresholds, the desktop background and
/// what Wallpaper Engine does. A
/// profile always holds all of them. Everything else (window layer,
/// display profile, overlay placement, notifications, start-up, …) applies
/// to the whole app and stays out of profiles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileLook {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    #[serde(default = "default_visible_panels")]
    pub visible_panels: Vec<String>,
    #[serde(default)]
    pub compact_panels: Vec<String>,
    #[serde(default)]
    pub overlay_enabled: bool,
    #[serde(default = "default_overlay_metrics")]
    pub overlay_metrics: Vec<String>,
    #[serde(default)]
    pub overlay_columns: u8,
    #[serde(default = "default_thresholds")]
    pub thresholds: HashMap<String, ComponentThresholds>,
    #[serde(default)]
    pub desktop_background: Option<DesktopBackground>,
    #[serde(default)]
    pub wallpaper_engine: Option<WeChoice>,
    /// Floating panels or the fixed dashboard (#599). `None` in a look
    /// saved before it was part of the profile: the mode is left as it is,
    /// so nobody's floating panels turn off at the first switch.
    #[serde(default)]
    pub floating_mode: Option<bool>,
    /// The floating panels' size (#599); `None` as for `floating_mode`.
    #[serde(default)]
    pub floating_panel_scale: Option<f64>,
}

impl ProfileLook {
    /// The look the app has now.
    pub fn capture(s: &Settings) -> Self {
        Self {
            theme: s.theme.clone(),
            opacity: s.opacity,
            visible_panels: s.visible_panels.clone(),
            compact_panels: s.compact_panels.clone(),
            overlay_enabled: s.overlay_enabled,
            overlay_metrics: s.overlay_metrics.clone(),
            overlay_columns: s.overlay_columns,
            thresholds: s.thresholds.clone(),
            desktop_background: s.desktop_background.clone(),
            wallpaper_engine: s.wallpaper_engine.clone(),
            floating_mode: Some(s.floating_mode),
            floating_panel_scale: Some(s.floating_panel_scale),
        }
    }

    /// Makes this the app's look. Returns whether anything changed.
    pub fn apply_to(&self, s: &mut Settings) -> bool {
        let before = Self::capture(s);
        s.theme = self.theme.clone();
        s.opacity = self.opacity.clamp(0.1, 1.0);
        s.visible_panels = self.visible_panels.clone();
        s.compact_panels = self.compact_panels.clone();
        s.overlay_enabled = self.overlay_enabled;
        s.overlay_metrics = self.overlay_metrics.clone();
        s.overlay_columns = self.overlay_columns.min(6);
        s.thresholds = self.thresholds.clone();
        s.desktop_background = self.desktop_background.clone();
        s.wallpaper_engine = self.wallpaper_engine.clone();
        if let Some(on) = self.floating_mode {
            s.floating_mode = on;
        }
        if let Some(scale) = self.floating_panel_scale {
            s.floating_panel_scale = scale.clamp(0.4, 2.0);
        }
        Self::capture(s) != before
    }
}

/// Called when the active Control Center profile is `profile_id`. Returns
/// `None` when the current look already belongs to it (a restart or pipe
/// reconnect). Otherwise the current look is stored in the outgoing
/// profile and the incoming one's is loaded — a profile seen for the first
/// time starts from the current look — and it returns whether the look or
/// the floating panels' positions changed. Either way the caller persists.
pub fn switch_profile_look(s: &mut Settings, profile_id: &str) -> Option<bool> {
    if s.look_profile.as_deref() == Some(profile_id) {
        return None;
    }
    let current = ProfileLook::capture(s);
    if let Some(old) = s.look_profile.take() {
        s.profile_looks.insert(old.clone(), current.clone());
        s.profile_panel_layouts.insert(old, s.panel_layouts.clone());
    }
    s.look_profile = Some(profile_id.to_string());
    let look_changed = match s.profile_looks.get(profile_id).cloned() {
        Some(look) => look.apply_to(s),
        None => {
            s.profile_looks.insert(profile_id.to_string(), current);
            false
        }
    };
    // The incoming profile's positions over the current ones: a panel it
    // has none for (never placed there, or a profile from before #599)
    // stays where it is.
    let before = s.panel_layouts.clone();
    if let Some(layouts) = s.profile_panel_layouts.get(profile_id) {
        s.panel_layouts
            .extend(layouts.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    Some(look_changed || s.panel_layouts != before)
}

/// Stores the current look in the active profile, e.g. after a Save in the
/// Control Center, so `profile_looks` is up to date for the overview.
pub fn store_active_look(s: &mut Settings) {
    if let Some(id) = s.look_profile.clone() {
        let look = ProfileLook::capture(s);
        s.profile_looks.insert(id, look);
    }
}

/// Forgets the looks of profiles that no longer exist.
pub fn prune_profile_looks(s: &mut Settings, profile_ids: &[&str]) -> bool {
    let before = s.profile_looks.len() + s.profile_panel_layouts.len();
    s.profile_looks
        .retain(|id, _| profile_ids.contains(&id.as_str()));
    s.profile_panel_layouts
        .retain(|id, _| profile_ids.contains(&id.as_str()));
    s.profile_looks.len() + s.profile_panel_layouts.len() != before
}

/// Maps the old `overlayLayout` onto `overlay_columns`: vertical → 1 column,
/// horizontal → one row (0), grid → the column count it used to compute
/// (⌈√metrics⌉). Idempotent: the old field is never written back, so the
/// next persist drops it.
fn migrate_overlay_layout(s: &mut Settings) {
    if let Some(layout) = s.overlay_layout.take() {
        s.overlay_columns = match layout.as_str() {
            "vertical" => 1,
            "grid" => (s.overlay_metrics.len() as f64)
                .sqrt()
                .ceil()
                .clamp(1.0, 6.0) as u8,
            _ => 0,
        };
    }
}

// --- File I/O --------------------------------------------------------------

pub fn settings_path(dir: &Path) -> PathBuf {
    dir.join("rigstats-settings.json")
}

pub fn load_settings(dir: &Path) -> Settings {
    // On parse/read failure, return defaults to keep startup robust.
    let path = settings_path(dir);
    let mut settings = match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<Settings>(&raw) {
            Ok(s) => s,
            Err(e) => {
                crate::debug::log_error(
                    dir,
                    &format!("settings: parse error — {e}, using defaults"),
                );
                Settings::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
        Err(e) => {
            crate::debug::log_error(dir, &format!("settings: read error — {e}, using defaults"));
            Settings::default()
        }
    };

    // Migrate always_on_top bool → window_layer string (pre-1.24 settings files).
    // window_layer defaults to "normal", so if an old file had always_on_top: true
    // and no window_layer field, we promote it to "on_top" here.
    if settings.window_layer == default_window_layer() && settings.always_on_top {
        settings.window_layer = "on_top".to_string();
    }
    // Keep always_on_top in sync so main.rs startup reads the right value.
    settings.always_on_top = settings.window_layer == "on_top";

    migrate_overlay_layout(&mut settings);

    // One-time migration from schema version 0 (flat threshold fields) to
    // version 1 (thresholds map). Runs once, then persists the new format.
    if settings.settings_version == 0 {
        migrate_v0_thresholds(&mut settings);
        settings.settings_version = 1;
        // Persist immediately so the migration is not repeated on the next launch.
        // Failures are non-fatal: the migrated settings are held in memory and
        // will be written again the next time the user saves settings — but log it
        // so a recurring migration (e.g. a read-only appdata dir) is diagnosable.
        if let Err(e) = persist_settings(dir, &settings) {
            crate::debug::log_error(dir, &format!("settings: migration persist failed — {e}"));
        }
    }

    settings
}

/// Copies schema-version-0 flat threshold fields into the `thresholds` map.
///
/// If at least one flat field was set, the user's values are preserved exactly.
/// If all flat fields are `None` (either never configured or explicitly cleared),
/// default thresholds are applied so the dashboard starts with sensible alert
/// levels rather than all alerts silently disabled.
fn migrate_v0_thresholds(s: &mut Settings) {
    let candidates = [
        ("cpu", s.warning_cpu_temp, s.critical_cpu_temp),
        ("gpu", s.warning_gpu_temp, s.critical_gpu_temp),
        ("ram", s.warning_ram_temp, s.critical_ram_temp),
        ("disk", s.warning_disk_temp, s.critical_disk_temp),
    ];
    let any_configured = candidates
        .iter()
        .any(|(_, w, c)| w.is_some() || c.is_some());
    if any_configured {
        for (key, warn, crit) in candidates {
            s.thresholds
                .insert(key.to_string(), ComponentThresholds { warn, crit });
        }
    } else {
        s.thresholds = default_thresholds();
    }
}

/// Writes `content` to `path` via an adjacent `.tmp` file that is renamed into
/// place. On the same filesystem, `rename` is atomic, so a hard shutdown
/// mid-write leaves either the old file intact or the new file complete —
/// never a truncated or partially-written result.
pub(crate) fn atomic_write(path: &std::path::Path, content: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, content).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn persist_settings(dir: &Path, settings: &Settings) -> Result<(), String> {
    let path = settings_path(dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    atomic_write(&path, &json)
}

#[cfg(test)]
mod tests {
    use super::atomic_write;

    #[test]
    fn atomic_write_creates_file_with_correct_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        atomic_write(&path, r#"{"ok":true}"#).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"ok":true}"#);
    }

    #[test]
    fn atomic_write_leaves_no_tmp_file_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        atomic_write(&path, "{}").unwrap();
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        atomic_write(&path, r#"{"v":1}"#).unwrap();
        atomic_write(&path, r#"{"v":2}"#).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"v":2}"#);
    }

    #[test]
    fn atomic_write_replaces_stale_tmp_from_previous_crash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        // Simulate a .tmp left behind by a previous hard-killed write.
        std::fs::write(path.with_extension("json.tmp"), "corrupted").unwrap();
        atomic_write(&path, r#"{"recovered":true}"#).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"recovered":true}"#
        );
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn default_thresholds_includes_battery_charge_and_power_keys() {
        let t = super::default_thresholds();
        let bat = t.get("battery").expect("battery key missing");
        assert_eq!(bat.warn, Some(20), "battery warn should be 20 %");
        assert_eq!(bat.crit, Some(10), "battery crit should be 10 %");
        let pwr = t.get("battery_power").expect("battery_power key missing");
        assert_eq!(pwr.warn, Some(15), "battery_power warn should be 15 W");
        assert_eq!(pwr.crit, Some(25), "battery_power crit should be 25 W");
    }

    /// A v0 settings file as it would deserialize: empty `thresholds` map, the
    /// legacy flat fields read from disk, version still 0.
    fn v0_settings() -> super::Settings {
        let mut s = super::Settings::default();
        s.thresholds.clear();
        s.settings_version = 0;
        s
    }

    #[test]
    fn migrate_preserves_user_values_when_any_flat_field_is_set() {
        let mut s = v0_settings();
        s.warning_cpu_temp = Some(72);
        // critical_cpu_temp and every other flat field stay None.
        super::migrate_v0_thresholds(&mut s);

        let cpu = s.thresholds.get("cpu").expect("cpu key");
        assert_eq!(cpu.warn, Some(72), "user warn value must survive migration");
        assert_eq!(
            cpu.crit, None,
            "unset flat field maps to None, not a default"
        );

        // The other components are still inserted, with their (None) flat values —
        // defaults are NOT injected once any field was configured.
        let gpu = s.thresholds.get("gpu").expect("gpu key");
        assert_eq!(gpu.warn, None);
        assert_eq!(gpu.crit, None);
    }

    #[test]
    fn migrate_applies_defaults_when_all_flat_fields_are_none() {
        let mut s = v0_settings();
        super::migrate_v0_thresholds(&mut s);
        let defaults = super::default_thresholds();
        assert_eq!(
            s.thresholds.get("cpu"),
            defaults.get("cpu"),
            "unconfigured v0 install must inherit default cpu thresholds"
        );
        assert_eq!(s.thresholds.get("battery"), defaults.get("battery"));
    }

    #[test]
    fn load_settings_bumps_version_and_does_not_re_migrate() {
        let dir = tempfile::tempdir().unwrap();
        let path = super::settings_path(dir.path());
        // A minimal v0 file: version 0 plus one legacy flat field.
        // Settings uses #[serde(rename_all = "camelCase")], so keys are camelCase.
        std::fs::write(&path, r#"{"settingsVersion":0,"warningCpuTemp":81}"#).unwrap();

        let first = super::load_settings(dir.path());
        assert_eq!(
            first.settings_version, 1,
            "migration must bump version to 1"
        );
        assert_eq!(
            first.thresholds.get("cpu").and_then(|t| t.warn),
            Some(81),
            "flat value must be copied into the thresholds map"
        );

        // The migration must have been persisted, so a second load sees version 1
        // and the same value without re-running the migration.
        let second = super::load_settings(dir.path());
        assert_eq!(second.settings_version, 1);
        assert_eq!(second.thresholds.get("cpu").and_then(|t| t.warn), Some(81));
    }

    #[test]
    fn load_settings_promotes_legacy_always_on_top_to_window_layer() {
        let dir = tempfile::tempdir().unwrap();
        let path = super::settings_path(dir.path());
        // A pre-1.24 file: always_on_top set, no window_layer field at all.
        std::fs::write(&path, r#"{"settingsVersion":1,"alwaysOnTop":true}"#).unwrap();

        let s = super::load_settings(dir.path());
        assert_eq!(
            s.window_layer, "on_top",
            "legacy always_on_top:true must promote the default window_layer to on_top"
        );
        assert!(
            s.always_on_top,
            "always_on_top must stay in sync with the resolved layer"
        );
    }

    #[test]
    fn load_settings_respects_explicit_window_layer_over_always_on_top() {
        let dir = tempfile::tempdir().unwrap();
        let path = super::settings_path(dir.path());
        // A modern file: an explicit non-default layer must win, and always_on_top is
        // re-derived from it (false here, since the layer is wallpaper, not on_top).
        std::fs::write(
            &path,
            r#"{"settingsVersion":1,"windowLayer":"wallpaper","alwaysOnTop":true}"#,
        )
        .unwrap();

        let s = super::load_settings(dir.path());
        assert_eq!(
            s.window_layer, "wallpaper",
            "an explicit window_layer must not be overwritten by the legacy promotion"
        );
        assert!(
            !s.always_on_top,
            "always_on_top must be re-derived from the layer (wallpaper => false)"
        );
    }

    #[test]
    fn load_settings_defaults_overlay_fields_when_absent_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = super::settings_path(dir.path());
        // A settings file predating the overlay fields entirely.
        std::fs::write(&path, r#"{"settingsVersion":1}"#).unwrap();

        let s = super::load_settings(dir.path());
        assert_eq!(s.overlay_metrics, super::default_overlay_metrics());
        assert_eq!(
            s.overlay_columns, 0,
            "one row, like the old horizontal default"
        );
        assert_eq!(s.overlay_anchor, "top-right");
        assert_eq!(s.overlay_margin, 16);
        assert_eq!(s.overlay_position, None);
        assert_eq!(s.overlay_scale, 1.0);
        assert_eq!(s.overlay_opacity, 0.85);
        assert!(s.overlay_background);
        assert!(!s.overlay_click_through);
        assert!(!s.overlay_enabled);
    }

    #[test]
    fn a_settings_file_from_before_569_keeps_the_old_shortcuts() {
        let dir = tempfile::tempdir().unwrap();
        let path = super::settings_path(dir.path());
        std::fs::write(&path, r#"{"settingsVersion":1,"microphoneNotify":false}"#).unwrap();

        let s = super::load_settings(dir.path());
        assert!(!s.microphone_notify, "the file was read, not replaced");
        assert_eq!(s.shortcuts, super::Shortcuts::default());
    }

    #[test]
    fn changed_shortcuts_survive_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = super::Settings::default();
        s.shortcuts.microphone = None;
        s.shortcuts.overlay = crate::shortcut::Shortcut::parse("Ctrl+Shift+F10");
        super::persist_settings(dir.path(), &s).unwrap();

        assert_eq!(super::load_settings(dir.path()).shortcuts, s.shortcuts);
    }

    #[test]
    fn persist_then_load_round_trips_overlay_metrics_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = super::Settings::default();
        s.overlay_metrics = vec![
            "gpu_temp".to_string(),
            "cpu_load".to_string(),
            "net_ping".to_string(),
        ];
        super::persist_settings(dir.path(), &s).unwrap();

        let loaded = super::load_settings(dir.path());
        assert_eq!(loaded.overlay_metrics, s.overlay_metrics);
    }

    #[test]
    fn load_settings_migrates_old_overlay_layout_to_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = super::settings_path(dir.path());
        let metrics = r#""overlayMetrics":["cpu_load","cpu_temp","gpu_load","gpu_temp","ram_pct"]"#;
        for (layout, want) in [("horizontal", 0), ("vertical", 1), ("grid", 3)] {
            std::fs::write(
                &path,
                format!(r#"{{"settingsVersion":1,{metrics},"overlayLayout":"{layout}"}}"#),
            )
            .unwrap();
            let s = super::load_settings(dir.path());
            assert_eq!(s.overlay_columns, want, "{layout}");

            super::persist_settings(dir.path(), &s).unwrap();
            let raw = std::fs::read_to_string(&path).unwrap();
            assert!(
                !raw.contains("overlayLayout"),
                "old field must not be written back"
            );
            assert_eq!(super::load_settings(dir.path()).overlay_columns, want);
        }
    }
}

#[cfg(test)]
mod profile_look_tests {
    use super::*;

    fn with_theme(theme: &str) -> Settings {
        Settings {
            theme: theme.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_first_switch_keeps_the_look_and_remembers_it_for_the_profile() {
        let mut s = with_theme("amber");
        assert_eq!(switch_profile_look(&mut s, "gaming"), Some(false));
        assert_eq!(s.theme, "amber");
        assert_eq!(s.look_profile.as_deref(), Some("gaming"));
        assert_eq!(s.profile_looks["gaming"].theme, "amber");
    }

    #[test]
    fn switching_stores_the_outgoing_look_and_loads_the_incoming_one() {
        let mut s = with_theme("amber");
        switch_profile_look(&mut s, "gaming");
        s.theme = "red".into(); // changed while Gaming was active
        switch_profile_look(&mut s, "silent");
        assert_eq!(s.profile_looks["gaming"].theme, "red");
        s.theme = "blue".into();
        assert_eq!(switch_profile_look(&mut s, "gaming"), Some(true));
        assert_eq!(s.theme, "red");
        assert_eq!(s.profile_looks["silent"].theme, "blue");
    }

    #[test]
    fn the_desktop_background_belongs_to_the_profile() {
        let calm = DesktopBackground {
            path: r"C:\calm.jpg".into(),
            fit: crate::desktop_background::Fit::Fit,
            screens: Vec::new(),
        };
        let mut s = Settings::default();
        switch_profile_look(&mut s, "silent");
        s.desktop_background = Some(calm.clone());
        s.desktop_background_before = Some(DesktopBackground {
            path: r"C:\own.jpg".into(),
            fit: Default::default(),
            screens: Vec::new(),
        });
        assert_eq!(switch_profile_look(&mut s, "gaming"), Some(false));
        assert_eq!(
            s.desktop_background,
            Some(calm.clone()),
            "a new profile starts from the current look"
        );
        s.desktop_background = None;
        assert_eq!(switch_profile_look(&mut s, "silent"), Some(true));
        assert_eq!(s.desktop_background, Some(calm));
        assert_eq!(s.profile_looks["gaming"].desktop_background, None);
        s.wallpaper_engine = Some(WeChoice::Pause);
        switch_profile_look(&mut s, "gaming");
        assert_eq!(
            s.profile_looks["silent"].wallpaper_engine,
            Some(WeChoice::Pause),
            "Wallpaper Engine's choice belongs to the profile too"
        );
        assert_eq!(s.wallpaper_engine, None);
        assert!(
            s.desktop_background_before.is_some(),
            "app-wide, not in a look"
        );
    }

    #[test]
    fn the_same_profile_again_changes_nothing() {
        // A restart or reconnect on the profile keeps the user's changes.
        let mut s = with_theme("amber");
        switch_profile_look(&mut s, "gaming");
        s.theme = "light".into();
        assert_eq!(switch_profile_look(&mut s, "gaming"), None);
        assert_eq!(s.theme, "light");
    }

    #[test]
    fn only_the_look_follows_the_profile() {
        let mut s = Settings::default();
        switch_profile_look(&mut s, "a");
        s.window_layer = "wallpaper".into();
        s.overlay_enabled = true;
        switch_profile_look(&mut s, "b");
        s.overlay_enabled = false;
        switch_profile_look(&mut s, "a");
        assert!(s.overlay_enabled, "the overlay belongs to the profile");
        assert_eq!(s.window_layer, "wallpaper", "the window layer is app-wide");
    }

    #[test]
    fn values_are_clamped() {
        let mut s = Settings::default();
        let look = ProfileLook {
            opacity: 5.0,
            overlay_columns: 40,
            ..ProfileLook::capture(&s)
        };
        look.apply_to(&mut s);
        assert_eq!(s.opacity, 1.0);
        assert_eq!(s.overlay_columns, 6);
    }

    #[test]
    fn store_and_prune() {
        let mut s = with_theme("amber");
        switch_profile_look(&mut s, "gaming");
        s.theme = "red".into();
        store_active_look(&mut s);
        assert_eq!(s.profile_looks["gaming"].theme, "red");
        s.profile_looks
            .insert("gone".into(), ProfileLook::capture(&s));
        assert!(prune_profile_looks(&mut s, &["gaming"]));
        assert!(!s.profile_looks.contains_key("gone"));
        assert!(!prune_profile_looks(&mut s, &["gaming"]));
    }

    #[test]
    fn round_trips_through_json_and_old_files_load() {
        let mut s = Settings::default();
        switch_profile_look(&mut s, "gaming");
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"profileLooks\""));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.profile_looks, s.profile_looks);
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(old.profile_looks.is_empty());
        // A look written by an older version without a field gets its default.
        let look: ProfileLook = serde_json::from_str(r#"{"theme":"red"}"#).unwrap();
        assert_eq!(look.visible_panels, default_visible_panels());
        assert!(look.compact_panels.is_empty());
    }

    /// The compact panels (#596) belong to the profile's look: switching
    /// profiles switches them, and a profile without the field stays full.
    #[test]
    fn compact_panels_follow_the_profile() {
        let mut s = Settings::default();
        assert!(s.compact_panels.is_empty());
        s.compact_panels = vec!["cpu".into(), "net".into()];
        let gaming = ProfileLook::capture(&s);
        assert_eq!(gaming.compact_panels, ["cpu", "net"]);

        s.compact_panels.clear();
        assert!(
            gaming.apply_to(&mut s),
            "applying a different look changes it"
        );
        assert_eq!(s.compact_panels, ["cpu", "net"]);
        assert!(!gaming.apply_to(&mut s), "the same look again is no change");

        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""compactPanels":["cpu","net"]"#), "{json}");
    }

    #[test]
    fn floating_mode_and_size_follow_the_profile() {
        let mut s = Settings::default();
        assert_eq!(switch_profile_look(&mut s, "balanced"), Some(false));
        s.floating_mode = true;
        s.floating_panel_scale = 1.7;
        // To Gaming (first time: it starts from the current look), then the
        // fixed dashboard there.
        switch_profile_look(&mut s, "gaming");
        s.floating_mode = false;
        s.floating_panel_scale = 1.0;
        // Back to Balanced: floating again, at its own size.
        assert_eq!(switch_profile_look(&mut s, "balanced"), Some(true));
        assert!(s.floating_mode);
        assert_eq!(s.floating_panel_scale, 1.7);
        assert_eq!(switch_profile_look(&mut s, "gaming"), Some(true));
        assert!(!s.floating_mode);
        assert_eq!(s.floating_panel_scale, 1.0);
    }

    #[test]
    fn a_look_saved_before_floating_was_in_profiles_leaves_it_alone() {
        // A look saved before #599 has no floatingMode / floatingPanelScale.
        let old: ProfileLook = serde_json::from_str(r#"{"theme":"red"}"#).unwrap();
        assert_eq!(old.floating_mode, None);
        assert_eq!(old.floating_panel_scale, None);
        let mut s = Settings::default();
        s.floating_mode = true;
        s.floating_panel_scale = 1.2;
        old.apply_to(&mut s);
        assert!(s.floating_mode, "floating stays on");
        assert_eq!(s.floating_panel_scale, 1.2);
        assert_eq!(s.theme, "red");
    }

    fn at(x: i32, y: i32) -> PanelLayout {
        PanelLayout { x, y }
    }

    #[test]
    fn floating_positions_follow_the_profile() {
        let mut s = Settings::default();
        switch_profile_look(&mut s, "silent");
        s.panel_layouts.insert("cpu".into(), at(100, 100));
        s.panel_layouts.insert("gpu".into(), at(100, 400));
        // Gaming, first time: the panels stay where they are, then move.
        switch_profile_look(&mut s, "gaming");
        assert_eq!(s.panel_layouts["cpu"], at(100, 100));
        s.panel_layouts.insert("cpu".into(), at(2000, 50));
        // Back to Silent: its own places, and the switch says so.
        assert_eq!(switch_profile_look(&mut s, "silent"), Some(true));
        assert_eq!(s.panel_layouts["cpu"], at(100, 100));
        assert_eq!(s.panel_layouts["gpu"], at(100, 400));
        assert_eq!(switch_profile_look(&mut s, "gaming"), Some(true));
        assert_eq!(s.panel_layouts["cpu"], at(2000, 50));
    }

    #[test]
    fn a_panel_without_a_place_in_the_profile_stays_where_it_is() {
        let mut s = Settings::default();
        switch_profile_look(&mut s, "silent");
        s.panel_layouts.insert("cpu".into(), at(10, 10));
        switch_profile_look(&mut s, "gaming");
        // Shown for the first time in Gaming, placed there.
        s.panel_layouts.insert("ram".into(), at(500, 500));
        switch_profile_look(&mut s, "silent");
        assert_eq!(s.panel_layouts["cpu"], at(10, 10));
        assert_eq!(
            s.panel_layouts["ram"],
            at(500, 500),
            "Silent has none for it"
        );
    }

    #[test]
    fn deleted_profiles_lose_their_positions_too() {
        let mut s = Settings::default();
        switch_profile_look(&mut s, "silent");
        switch_profile_look(&mut s, "custom");
        switch_profile_look(&mut s, "silent");
        assert!(s.profile_panel_layouts.contains_key("custom"));
        assert!(prune_profile_looks(&mut s, &["silent"]));
        assert!(!s.profile_panel_layouts.contains_key("custom"));
        assert!(!s.profile_looks.contains_key("custom"));
    }
}
