//! Control pipe client (`\\.\pipe\rigstats-control`) — Control Center phase 0
//! foundation (#187, Stage 2 of the plan). Duplex, newline-delimited JSON,
//! request/response correlated by `id`, plus server-pushed `event` lines.
//! Runs on its own tokio task (`control_task`), the same way `poll_loop`
//! does — the UI thread never blocks on the pipe, it only drains
//! [`ControlEvent`]s via a channel each frame. See
//! `docs/control-architecture.md`, "Control pipe protocol" and "App
//! integration (Rust / egui)".
//!
//! Wire shapes here mirror `sensor-sidecar/Control/ControlProtocol.cs`
//! field-for-field (snake_case on both sides, since this is Rust↔C# IPC,
//! not a settings file — unlike `settings.rs`'s `camelCase`).

use crate::debug::{append_debug_log, log_error, log_warn};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender as SyncSender;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use tokio::sync::mpsc::Receiver as AsyncReceiver;

const PIPE_NAME: &str = r"\\.\pipe\rigstats-control";
const PROTOCOL_VERSION: u32 = 1;
const RECONNECT_BACKOFF: Duration = Duration::from_secs(2);
/// Upper bound of the retry delay while the service keeps refusing this app
/// (#231) — it doubles from `RECONNECT_BACKOFF` up to this.
const REFUSED_BACKOFF_MAX: Duration = Duration::from_secs(60);
/// Consecutive refusals before the UI says "refused" rather than
/// "connecting" — a single close can also be the service restarting.
const REFUSALS_SHOWN_AFTER: u32 = 2;
/// Generous — `apply_profile` can involve validate→snapshot→apply→verify
/// across several providers; `hello`/`capabilities`/etc. return far sooner.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Hue requests talk to the bridge over the network: pairing is up to four
/// HTTPS calls of up to 3 s each in the service.
const HUE_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Mirrors `lhm.rs`'s `MAX_PIPE_LINE_BYTES` — a healthy service emits at
/// most a few KB per line; anything past this means a buggy/runaway peer.
const MAX_PIPE_LINE_BYTES: usize = 256 * 1024;

// ── Wire protocol ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct RequestOut<'a> {
    id: u64,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ResponseIn {
    id: u64,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct EventIn {
    event: String,
    #[serde(default)]
    data: Option<serde_json::Value>,
}

/// One incoming NDJSON line is either a response to a request we sent
/// (has `id`) or a server-pushed event (has `event`) — untagged
/// deserialization disambiguates by which required field is present.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum IncomingLine {
    Response(ResponseIn),
    Event(EventIn),
}

// ── Profile model (mirrors ControlProtocol.cs) ─────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfilePart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_plan: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan: Option<FanPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_limit: Option<CpuLimitPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curve_opt: Option<CurveOptPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aura: Option<AuraPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouse: Option<MousePart>,
}

/// A profile's mouse part (#431): which of the mouse's own onboard profiles
/// it uses (1-based); `None` leaves the mouse as it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MousePart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<u8>,
}

/// One DPI stage: its value and, on models with stage colours, "#rrggbb".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DpiStage {
    pub dpi: u32,
    #[serde(default)]
    pub color: String,
}

/// A ROG mouse's settings in its active onboard profile (#431) — kept in
/// the mouse (`mouse_settings` / `mouse_set`). A setting the model lacks is
/// `None`. `debounce` is the button-response code (ms = 4 × code + 4),
/// `lift_off` 0 low / 1 high, `power_off` the auto-power-off code (0–4 =
/// 1, 2, 3, 5, 10 min; 255 = never).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MouseSettings {
    pub profile: u8,
    pub active_stage: u8,
    pub stages: Vec<DpiStage>,
    pub polling_hz: Option<u32>,
    pub debounce: Option<u8>,
    pub angle_snapping: Option<bool>,
    pub angle_tuning: Option<i16>,
    pub acceleration: Option<u8>,
    pub deceleration: Option<u8>,
    pub motion_sync: Option<bool>,
    pub lift_off: Option<u8>,
    pub power_off: Option<u8>,
    pub low_battery_warning: Option<u8>,
    /// Each button's action code, in `MouseSpec::buttons` order (#433).
    #[serde(default)]
    pub buttons: Option<Vec<u16>>,
}

/// A mouse button that can be bound (#433): its own action code and name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct MouseButton {
    pub source: u16,
    pub name: String,
}

/// Something a mouse button can do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct MouseAction {
    pub code: u16,
    pub name: String,
}

/// What a mouse model offers (`MouseCatalog` in the service).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct MouseSpec {
    pub profiles: u8,
    pub dpi_stages: u8,
    pub dpi_min: u32,
    pub dpi_max: u32,
    pub dpi_step: u32,
    #[serde(default)]
    pub dpi_colors: bool,
    #[serde(default)]
    pub polling_hz: Vec<u32>,
    /// Button-response codes (min, max).
    #[serde(default)]
    pub debounce: Option<(u8, u8)>,
    #[serde(default)]
    pub lift_off: bool,
    #[serde(default)]
    pub angle_snapping: bool,
    /// Degrees (min, max, step).
    #[serde(default)]
    pub angle_tuning: Option<(i16, i16, i16)>,
    /// Maximum, 0 = not offered.
    #[serde(default)]
    pub acceleration: u8,
    #[serde(default)]
    pub deceleration: u8,
    #[serde(default)]
    pub motion_sync: bool,
    #[serde(default)]
    pub auto_power_off: bool,
    /// Percent (step, max).
    #[serde(default)]
    pub low_battery_warning: Option<(u8, u8)>,
    /// Bindable buttons (#433): each by its own action code, which is also
    /// its default. `None` for models whose layout isn't built.
    #[serde(default)]
    pub buttons: Option<Vec<MouseButton>>,
    /// What a button can be bound to.
    #[serde(default)]
    pub actions: Option<Vec<MouseAction>>,
    /// Where the DPI button sits when known (#435): "underside" or "top".
    #[serde(default)]
    pub dpi_button: Option<String>,
    /// The DPI level in use can be picked from the PC (#432); false on older
    /// models where only the mouse's DPI button picks it.
    #[serde(default = "yes")]
    pub can_switch_stage: bool,
}

fn yes() -> bool {
    true
}

/// A connected ROG mouse with known settings (`mouse_settings`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct MouseInfo {
    pub id: String,
    pub name: String,
    /// "wired", "dongle" or "omni".
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub verified: bool,
    pub spec: MouseSpec,
    /// `None` while the mouse doesn't answer (asleep, out of range).
    pub settings: Option<MouseSettings>,
}

/// A profile's fan part (#188). Headers missing from `headers` are on BIOS
/// control; an empty map releases every header when the profile applies.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FanPart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, FanHeaderConfig>>,
}

/// A profile's CPU package limits (#189). An empty part (or a `None`
/// value) means the BIOS value for this boot.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CpuLimitPart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amd: Option<AmdCpuLimit>,
    /// Intel PL1/PL2 — not implemented by the service yet; passed through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intel: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct AmdCpuLimit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ppt_w: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tdc_a: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edc_a: Option<f64>,
}

impl AmdCpuLimit {
    pub fn is_stock(&self) -> bool {
        self.ppt_w.is_none() && self.tdc_a.is_none() && self.edc_a.is_none()
    }
}

/// A profile's lighting (#192): one effect for every zone. No part = the
/// lights are left as they are.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AuraPart {
    /// "off", "static", "breathing", "spectrum_cycle".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    /// "#rrggbb".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// 0–1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f64>,
    /// White lamps beside the RGB (the ROG light bar's desk lamp, #214).
    /// `None` leaves them alone; a part with a lamp and no effect leaves the
    /// RGB alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lamp: Option<LampPart>,
}

/// A white lamp: on or off, how bright (0–1 of its allowed maximum) and how
/// warm (kelvin, 2700–6500).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LampPart {
    pub on: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<u32>,
}

impl LampPart {
    pub const MIN_KELVIN: u32 = 2700;
    pub const MAX_KELVIN: u32 = 6500;
    /// The service's defaults for a lamp switched on without values.
    pub const DEFAULT_BRIGHTNESS: f64 = 0.5;
    pub const DEFAULT_KELVIN: u32 = 4000;
}

/// The "aura" capability's `details`, typed. Aura Sync: the profile's one
/// effect goes to every device listed.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct AuraCaps {
    #[serde(default)]
    pub effects: Vec<String>,
    #[serde(default)]
    pub devices: Vec<AuraDeviceCap>,
}

/// The paired Hue Bridge (#215), from the "aura" capability's `details.hue`
/// — present even when no other lighting device is, so a bridge can be
/// paired there. Its chosen rooms are also among the `devices`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct HueCaps {
    #[serde(default)]
    pub paired: bool,
    #[serde(default)]
    pub bridge: Option<HueBridgeCap>,
    #[serde(default)]
    pub groups: Vec<HueGroupCap>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct HueBridgeCap {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub firmware: String,
    #[serde(default)]
    pub ip: String,
}

/// A room or zone on the bridge; `chosen` ones follow the rig.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct HueGroupCap {
    pub id: String,
    pub name: String,
    /// "room" or "zone".
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub chosen: bool,
}

/// A headset's own settings (#428): sidetone and microphone noise
/// reduction, kept in the headset (`headset_settings` / `headset_set`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadsetSettings {
    pub sidetone_on: bool,
    pub sidetone_level: u8,
    pub noise_reduction_on: bool,
    pub noise_reduction_level: u8,
}

/// A connected headset with settings RIGStats knows (`headset_settings`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct HeadsetInfo {
    /// The lighting device id — the same id as its battery in the telemetry.
    pub id: String,
    pub name: String,
    /// Settings seen working on this model.
    #[serde(default)]
    pub verified: bool,
    pub sidetone_max: u8,
    /// Noise-reduction levels; 0 = on/off only.
    #[serde(default)]
    pub noise_levels: u8,
    /// `None` while the headset doesn't answer (off behind its dongle).
    pub settings: Option<HeadsetSettings>,
}

/// A bridge found on the network (`hue_discover`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct HueBridgeFound {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub model: String,
    pub ip: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct AuraDeviceCap {
    pub id: String,
    pub name: String,
    /// "motherboard", later "monitor", "keyboard", ...
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub firmware: String,
    /// Why another controller owns it (Windows Dynamic Lighting) — skipped.
    #[serde(default)]
    pub blocked: Option<String>,
    /// Why its last write failed (a Hue room whose bridge doesn't answer) —
    /// as of when the capabilities were asked for.
    #[serde(default)]
    pub problem: Option<String>,
    /// Has a white lamp beside its RGB (`AuraPart::lamp`).
    #[serde(default)]
    pub lamp: bool,
    /// Whether that lamp is lit now (read when the capabilities were asked
    /// for) — the tray or its own button may differ from the profile.
    #[serde(default)]
    pub lamp_on: Option<bool>,
    /// The device's own lighting mode, for devices that offer switching it
    /// (#236): `Some(false)` = "Device Lighting", every lighting write is
    /// ignored until [`ControlCmd::EnableWdl`]; `None` = not offered or no answer.
    #[serde(default)]
    pub wdl_on: Option<bool>,
    /// `Some(false)`: a known model without that setting (newer ASUS mice
    /// follow Dynamic Lighting directly) — `None` when the model is unknown.
    #[serde(default)]
    pub wdl_toggle: Option<bool>,
    /// A headset with sidetone / noise-reduction settings (#428): the
    /// Control Center shows its Headset page.
    #[serde(default)]
    pub headset_settings: bool,
    #[serde(default)]
    pub zones: Vec<AuraZoneCap>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct AuraZoneCap {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub addressable: bool,
    #[serde(default)]
    pub leds: u32,
}

/// One Windows power scheme from the "power_plan" capability: `id` is the
/// symbolic name for the well-known schemes ("balanced", ...) or the GUID.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PowerScheme {
    pub id: String,
    pub name: String,
}

/// A profile's Curve Optimizer offsets (#191), in CO counts. A core's
/// value is `per_core[index]`, else `all_core`, else the BIOS value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CurveOptPart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_core: Option<i32>,
    /// Keyed by core index as a string ("0", "1", ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_core: Option<BTreeMap<String, i32>>,
}

impl CurveOptPart {
    pub fn is_bios(&self) -> bool {
        self.all_core.is_none() && self.per_core.as_ref().map_or(true, BTreeMap::is_empty)
    }
}

/// The "curve_opt" capability's `details`, typed.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CurveOptCaps {
    pub min: i32,
    pub max: i32,
    pub cores: usize,
    /// Whether per-core values are offered (unambiguous core numbering).
    #[serde(default)]
    pub per_core: bool,
    #[serde(default)]
    pub bios: Vec<i32>,
    #[serde(default)]
    pub current: Vec<i32>,
}

/// A profile's GPU power limits (#190). Adapters missing from `adapters`
/// (or a `None` value) go back to what was in force before RIGStats changed
/// them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GpuPart {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapters: Option<BTreeMap<String, GpuAdapterConfig>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct GpuAdapterConfig {
    /// Offset from the driver default in % (0 = default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_limit_pct: Option<i32>,
}

/// The "gpu" capability's `details`, typed.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct GpuCaps {
    #[serde(default)]
    pub adapters: Vec<GpuAdapterCap>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct GpuAdapterCap {
    /// PNP device instance path.
    pub id: String,
    pub name: String,
    pub min: i32,
    pub max: i32,
    #[serde(default = "one")]
    pub step: i32,
    #[serde(default)]
    pub default: i32,
    /// In force before RIGStats changed it — what "no value" means.
    #[serde(default)]
    pub original: i32,
    #[serde(default)]
    pub current: i32,
}

fn one() -> i32 {
    1
}

/// The "cpu_limit" capability's `details`, typed. Limits only go down:
/// `stock` (the BIOS values this boot) is the ceiling, `min` the floor.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CpuLimitCaps {
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub generation: String,
    pub stock: AmdLimitValues,
    pub min: AmdLimitValues,
    pub current: AmdLimitValues,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub struct AmdLimitValues {
    pub ppt_w: f64,
    pub tdc_a: f64,
    pub edc_a: f64,
}

/// One header's curve, keyed by LHM control identifier in [`FanPart`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FanHeaderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// `"cpu_package"`, `"gpu"`, or `"mb:<label>"` — see [`FanCaps::sources`].
    pub source: String,
    /// `[tempC, dutyPct]` points, ascending by temperature.
    pub curve: Vec<[f64; 2]>,
    #[serde(default = "default_hysteresis_c")]
    pub hysteresis_c: f64,
}

fn default_hysteresis_c() -> f64 {
    3.0
}

/// The "fan" capability's `details`, typed.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct FanCaps {
    #[serde(default)]
    pub headers: Vec<FanHeaderCap>,
    #[serde(default)]
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FanHeaderCap {
    pub id: String,
    pub label: String,
    pub min: f64,
    pub max: f64,
    /// RPM sensors this header drove when last identified (persisted by
    /// the service); `None` until it has been identified.
    #[serde(default)]
    pub drives: Option<Vec<String>>,
}

/// An RPM sensor that sped up while a header was identified (#207).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FanResponder {
    pub label: String,
    pub before_rpm: f64,
    pub peak_rpm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub builtin: bool,
    pub part: ProfilePart,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub domain: String,
    #[serde(default)]
    pub supported: bool,
    #[serde(default)]
    pub reason: Option<String>,
    /// Set when the domain is unavailable because something went wrong, not
    /// because the PC lacks it: the page is shown with the problem (#370).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<Problem>,
    #[serde(default)]
    pub details: Option<serde_json::Value>,
}

/// Something the user should know about and may be able to fix (#370) — the
/// one shape every problem takes, from the service or the app itself, shown
/// the same way everywhere (`ui_kit::notice`). Mirrors the service's
/// `Problem` (`sensor-sidecar/Control/ControlProtocol.cs`), pinned by the
/// shared contract example.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    /// Stable ("gpu_driver_crashed"), to tell problems apart.
    pub id: String,
    pub severity: Severity,
    /// Short, in sentence case.
    pub title: String,
    /// What happened, what still works, what to do.
    pub message: String,
    #[serde(default)]
    pub actions: Vec<ProblemAction>,
    /// The technical detail for a bug report (exit code, driver files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Error,
    /// Also what a newer service's unknown severity reads as (`serde(other)`
    /// has to be the last variant).
    #[default]
    #[serde(other)]
    Attention,
}

/// A button on a problem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProblemAction {
    pub kind: ProblemActionKind,
    pub label: String,
    /// The URL for `OpenUrl`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemActionKind {
    OpenUrl,
    CollectDiagnostics,
    /// App-only: try the failed thing again (the window that shows the
    /// problem handles it — an update check, a save).
    Retry,
    /// App-only: open the debug log (Status).
    OpenLog,
    /// App-only: open Windows' notification settings (#449). A fixed target,
    /// so nothing from the service can open another page.
    OpenNotificationSettings,
    /// A kind this app doesn't know (a newer service): not shown.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApplyResult {
    pub ok: bool,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
}

// ── UI-facing state and channels ───────────────────────────────────────

/// What `DashboardRuntime` needs each frame — updated by folding
/// [`ControlEvent`]s drained from `control_task`'s channel (mirrors how
/// `PollStats` feeds `DashboardRuntime::drain`). Populated in
/// `src-egui/src/dashboard.rs` (Stage 3), defined here so the wire types and
/// the state they feed live together.
#[derive(Debug, Clone, Default)]
pub struct ControlState {
    pub connected: bool,
    /// Set (and `connected` forced false) when `hello` reports a protocol
    /// version this app doesn't understand — the UI should disable control
    /// and show why, per the design doc, rather than keep retrying.
    pub protocol_mismatch: Option<ProtocolMismatch>,
    /// The service keeps closing the connection before answering `hello` —
    /// it refuses this app (e.g. not the installed, signed `rigstats.exe`).
    pub refused: bool,
    pub capabilities: Vec<CapabilitySet>,
    pub profiles: Vec<Profile>,
    pub active_profile: Option<String>,
    pub dry_run: bool,
    pub last_apply_result: Option<ApplyResult>,
    pub last_error: Option<String>,
    /// Number of `Error` events this session, so the UI can tell a new
    /// error from one already dismissed (the text alone may repeat).
    pub errors: u32,
    /// Live commanded duty % per controlled header (`fan_duty` events).
    pub fan_duty: BTreeMap<String, f64>,
    /// Reason of the last critical-temperature override (`safety_tripped`).
    pub safety_tripped: Option<String>,
    /// Number of `safety_tripped` events this session, so the app can raise
    /// one notification per trip (the reason text alone may repeat).
    pub safety_trips: u32,
    /// Per header id: the RPM sensors its last identify made speed up
    /// (`fan_identified`). Channel and RPM sensor numbers don't always match.
    pub fan_identified: BTreeMap<String, Vec<FanResponder>>,
    /// Set when the boot-crash guard kept CPU limits at BIOS values this boot.
    pub crash_guard_notice: Option<String>,
    /// `profiles.json` could not be read and was restored from its backup or
    /// reset to the built-ins (#221) — until profiles are saved again.
    pub profiles_notice: Option<String>,
    /// A running `preview`: which profile, and when the service reverts it.
    pub preview: Option<PreviewState>,
    /// Shown once a preview ended without "Keep" (timed out or undone).
    pub preview_reverted: bool,
    /// A Hue request (search, pair, ...) is waiting for the service.
    pub hue_busy: bool,
    /// The bridges the last search found.
    pub hue_found: Vec<HueBridgeFound>,
    /// Why the last Hue request failed ("press the link button", ...).
    pub hue_message: Option<String>,
    /// ROG mice with settings (#431), as last read from the service.
    pub mice: Vec<MouseInfo>,
    /// A mouse request is in flight.
    pub mouse_busy: bool,
    /// Headsets with settings (#428), as last read from the service.
    pub headsets: Vec<HeadsetInfo>,
    /// A headset request is in flight.
    pub headset_busy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreviewState {
    pub profile_id: String,
    pub reverts_at: std::time::Instant,
}

impl ControlState {
    /// The fan capability, when the service reports writable headers.
    pub fn fan_caps(&self) -> Option<FanCaps> {
        self.capabilities
            .iter()
            .find(|c| c.domain == "fan" && c.supported)
            .and_then(|c| c.details.clone())
            .and_then(|d| serde_json::from_value(d).ok())
    }

    /// Cheap check (no `details` parsing) for per-frame use by panels.
    pub fn has_fan_control(&self) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.domain == "fan" && c.supported)
    }

    /// The header that drives the RPM sensor `fan_label` ("Fan #5"), as
    /// measured by identify: this session's result first, then the map the
    /// service persisted. Never guessed from names — on some boards one
    /// channel drives several fans.
    pub fn header_for_fan(&self, fan_label: &str) -> Option<String> {
        self.fan_identified
            .iter()
            .find(|(_, rs)| rs.iter().any(|r| r.label == fan_label))
            .map(|(id, _)| id.clone())
            .or_else(|| {
                self.fan_caps()?
                    .headers
                    .into_iter()
                    .find(|h| {
                        h.drives
                            .as_ref()
                            .is_some_and(|d| d.iter().any(|l| l == fan_label))
                    })
                    .map(|h| h.id)
            })
    }

    /// The CPU limit capability, when the service can set limits here.
    pub fn cpu_caps(&self) -> Option<CpuLimitCaps> {
        self.capabilities
            .iter()
            .find(|c| c.domain == "cpu_limit" && c.supported)
            .and_then(|c| c.details.clone())
            .and_then(|d| serde_json::from_value(d).ok())
    }

    /// The PPT limit the active profile lowers to, for the CPU panel —
    /// `None` at BIOS values, or when the crash guard skipped limits.
    pub fn active_ppt_limit(&self) -> Option<f64> {
        if self.crash_guard_notice.is_some() {
            return None;
        }
        self.active()?.part.cpu_limit.as_ref()?.amd?.ppt_w
    }

    /// The lighting capability, when a controller is usable here.
    pub fn aura_caps(&self) -> Option<AuraCaps> {
        self.capabilities
            .iter()
            .find(|c| c.domain == "aura" && c.supported)
            .and_then(|c| c.details.clone())
            .and_then(|d| serde_json::from_value(d).ok())
    }

    /// The Hue Bridge pairing state — whether or not lighting is otherwise
    /// supported (a PC with no RGB device can still pair a bridge).
    pub fn hue_caps(&self) -> Option<HueCaps> {
        self.capabilities
            .iter()
            .find(|c| c.domain == "aura")
            .and_then(|c| c.details.as_ref()?.get("hue").cloned())
            .and_then(|h| serde_json::from_value(h).ok())
    }

    /// A ROG mouse with known settings is connected (#431) — the Control
    /// Center's Mouse page.
    pub fn has_mouse(&self) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.domain == "mouse" && c.supported)
    }

    /// A headset with settings RIGStats knows is connected (#428) — the
    /// Control Center's Headset page.
    pub fn has_headset(&self) -> bool {
        self.aura_caps()
            .is_some_and(|c| c.devices.iter().any(|d| d.headset_settings))
    }

    /// A lighting device with a desk lamp is connected (the tray toggle).
    pub fn has_lamp(&self) -> bool {
        self.aura_caps()
            .is_some_and(|c| c.devices.iter().any(|d| d.lamp && d.blocked.is_none()))
    }

    /// Why lighting is blocked by another app (Armoury Crate) — the one
    /// unsupported case the Lighting tab shows instead of hiding. `None`
    /// when it works or there is simply no controller.
    pub fn aura_unavailable(&self) -> Option<String> {
        self.capabilities
            .iter()
            .find(|c| {
                c.domain == "aura"
                    && !c.supported
                    && c.details
                        .as_ref()
                        .and_then(|d| d.get("conflict")?.as_bool())
                        == Some(true)
            })
            .and_then(|c| c.reason.clone())
    }

    /// The problem that makes `domain` unavailable, if it failed rather than
    /// being absent (#370). Its page is shown with the problem.
    pub fn domain_problem(&self, domain: &str) -> Option<&Problem> {
        self.capabilities
            .iter()
            .find(|c| c.domain == domain && !c.supported)
            .and_then(|c| c.problem.as_ref())
    }

    /// The service's notices, as problems: CPU limits kept at BIOS values
    /// after a crash (shown on the CPU page), and profiles restored from a
    /// backup (shown above every page).
    pub fn crash_guard_problem(&self) -> Option<Problem> {
        self.crash_guard_notice.as_ref().map(|notice| Problem {
            id: "crash_guard".into(),
            severity: Severity::Attention,
            title: "Reverted after a restart".into(),
            message: notice.clone(),
            actions: Vec::new(),
            detail: None,
        })
    }

    pub fn profiles_problem(&self) -> Option<Problem> {
        self.profiles_notice.as_ref().map(|notice| Problem {
            id: "profiles_restored".into(),
            severity: Severity::Attention,
            title: "Profiles recovered".into(),
            message: notice.clone(),
            actions: Vec::new(),
            detail: None,
        })
    }

    /// The power schemes a profile can choose from.
    pub fn power_schemes(&self) -> Vec<PowerScheme> {
        self.capabilities
            .iter()
            .find(|c| c.domain == "power_plan" && c.supported)
            .and_then(|c| c.details.as_ref()?.get("schemes").cloned())
            .and_then(|s| serde_json::from_value(s).ok())
            .unwrap_or_default()
    }

    /// The Curve Optimizer capability, when this CPU supports it.
    pub fn curve_opt_caps(&self) -> Option<CurveOptCaps> {
        self.capabilities
            .iter()
            .find(|c| c.domain == "curve_opt" && c.supported)
            .and_then(|c| c.details.clone())
            .and_then(|d| serde_json::from_value(d).ok())
    }

    /// The GPU capability, when the service can set a power limit here.
    pub fn gpu_caps(&self) -> Option<GpuCaps> {
        self.capabilities
            .iter()
            .find(|c| c.domain == "gpu" && c.supported)
            .and_then(|c| c.details.clone())
            .and_then(|d| serde_json::from_value(d).ok())
    }

    /// The power limit the active profile sets on the GPU named
    /// `gpu_name` (as the GPU panel shows it), for that panel — `None` when
    /// it leaves the GPU at its original value.
    pub fn active_gpu_limit(&self, gpu_name: &str) -> Option<i32> {
        let caps = self.gpu_caps()?;
        let adapter = caps
            .adapters
            .iter()
            .find(|a| crate::lhm::gpu_names_match(&a.name, gpu_name))?;
        self.active()?
            .part
            .gpu
            .as_ref()?
            .adapters
            .as_ref()?
            .get(&adapter.id)?
            .power_limit_pct
            .filter(|&pct| pct != adapter.original)
    }

    pub fn active(&self) -> Option<&Profile> {
        let id = self.active_profile.as_deref()?;
        self.profiles.iter().find(|p| p.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolMismatch {
    pub expected: u32,
    pub got: u32,
}

impl ControlState {
    /// Folds one event into the state. Returns whether anything visible
    /// changed, mirroring `DashboardRuntime::drain`'s `bool` return.
    pub fn apply(&mut self, event: ControlEvent) -> bool {
        match event {
            ControlEvent::Connected => {
                self.connected = true;
                self.refused = false;
                self.protocol_mismatch = None;
                self.last_error = None;
            }
            ControlEvent::Disconnected => self.connected = false,
            ControlEvent::Refused => {
                self.connected = false;
                self.refused = true;
            }
            ControlEvent::ProtocolMismatch { expected, got } => {
                self.connected = false;
                self.protocol_mismatch = Some(ProtocolMismatch { expected, got });
            }
            ControlEvent::Capabilities(c) => self.capabilities = c,
            ControlEvent::Profiles(p) => self.profiles = p,
            ControlEvent::ActiveProfile(p) => self.active_profile = p,
            ControlEvent::DryRun(d) => self.dry_run = d,
            ControlEvent::ApplyResult(r) => self.last_apply_result = Some(r),
            ControlEvent::Error(e) => {
                // The Lighting tab shows why its Hue request failed.
                if self.hue_busy {
                    self.hue_message = Some(e.clone());
                }
                self.errors += 1;
                self.last_error = Some(e);
            }
            ControlEvent::HueBusy(busy) => {
                if busy {
                    self.hue_message = None;
                }
                self.hue_busy = busy;
            }
            ControlEvent::HueFound(found) => self.hue_found = found,
            ControlEvent::Headsets(list) => {
                self.headsets = list;
                self.headset_busy = false;
            }
            ControlEvent::HeadsetUpdated(headset) => {
                match self.headsets.iter_mut().find(|h| h.id == headset.id) {
                    Some(known) => *known = headset,
                    None => self.headsets.push(headset),
                }
                self.headset_busy = false;
            }
            ControlEvent::HeadsetBusy(busy) => self.headset_busy = busy,
            ControlEvent::Mice(list) => {
                self.mice = list;
                self.mouse_busy = false;
            }
            ControlEvent::MouseUpdated(mouse) => {
                let mouse = *mouse;
                match self.mice.iter_mut().find(|m| m.id == mouse.id) {
                    Some(known) => *known = mouse,
                    None => self.mice.push(mouse),
                }
                self.mouse_busy = false;
            }
            ControlEvent::MouseBusy(busy) => self.mouse_busy = busy,
            ControlEvent::FanDuty(duty) => {
                // Arrives every second while curves run — only a change is
                // worth a repaint.
                if self.fan_duty == duty {
                    return false;
                }
                self.fan_duty = duty;
            }
            ControlEvent::SafetyTripped(reason) => {
                self.safety_tripped = Some(reason);
                self.safety_trips += 1;
            }
            ControlEvent::FanIdentified { header, responders } => {
                self.fan_identified.insert(header, responders);
            }
            ControlEvent::CrashGuardNotice(n) => self.crash_guard_notice = n,
            ControlEvent::ProfilesNotice(n) => self.profiles_notice = n,
            ControlEvent::PreviewStarted {
                profile_id,
                revert_in,
            } => {
                self.preview_reverted = false;
                self.preview = Some(PreviewState {
                    profile_id,
                    reverts_at: std::time::Instant::now() + revert_in,
                });
            }
            ControlEvent::PreviewEnded { kept } => {
                // Only a preview that was running can have been reverted.
                // (`take()` first: inside `!kept && …` it would be skipped
                // for a kept preview, leaving it running in the UI.)
                let was_running = self.preview.take().is_some();
                self.preview_reverted = !kept && was_running;
            }
        }
        true
    }
}

/// Pushed from `control_task` to the UI thread over a `std::sync::mpsc`
/// channel (the UI's frame loop is synchronous — it drains with
/// `try_recv()`, the same shape as `PollStats`, not an async channel).
#[derive(Debug, Clone)]
pub enum ControlEvent {
    Connected,
    Disconnected,
    /// The service refused this app (see [`ControlState::refused`]).
    Refused,
    ProtocolMismatch {
        expected: u32,
        got: u32,
    },
    Capabilities(Vec<CapabilitySet>),
    Profiles(Vec<Profile>),
    ActiveProfile(Option<String>),
    DryRun(bool),
    ApplyResult(ApplyResult),
    /// An error response to a request the UI cares about (distinct from
    /// `last_apply_result`'s own `ok: false` — this is for requests like
    /// `apply_profile` on an unknown id, `save_profile`, etc.).
    Error(String),
    FanDuty(BTreeMap<String, f64>),
    SafetyTripped(String),
    FanIdentified {
        header: String,
        responders: Vec<FanResponder>,
    },
    CrashGuardNotice(Option<String>),
    ProfilesNotice(Option<String>),
    PreviewStarted {
        profile_id: String,
        revert_in: Duration,
    },
    /// Kept (`confirm`), or reverted — by the user or by the service's timer.
    PreviewEnded {
        kept: bool,
    },
    /// A Hue request started (`true`) or was answered (`false`).
    HueBusy(bool),
    /// The bridges a search found.
    HueFound(Vec<HueBridgeFound>),
    /// Every headset with settings, freshly read (#428).
    Headsets(Vec<HeadsetInfo>),
    /// One headset after a change, as it read back.
    HeadsetUpdated(HeadsetInfo),
    /// Every ROG mouse with settings, freshly read (#431).
    Mice(Vec<MouseInfo>),
    /// One mouse after a change, as it read back.
    MouseUpdated(Box<MouseInfo>),
    /// A mouse request started (`true`) or ended without an answer.
    MouseBusy(bool),
    /// A headset request started (`true`) or ended without an answer.
    HeadsetBusy(bool),
}

/// Sent from the UI to `control_task` (async-native channel — the task
/// `.await`s `recv()` inside its own loop; the UI sends with the
/// non-blocking `try_send`).
#[derive(Debug, Clone)]
pub enum ControlCmd {
    ApplyProfile(String),
    /// Re-fetch capabilities/profiles/active state (e.g. after the Control
    /// Center window is opened).
    Refresh,
    ReleaseToFirmware,
    /// Store `profile`, then apply it when `apply` (the Fans tab edits the
    /// active profile, so its changes take effect on save).
    SaveProfile {
        profile: Box<Profile>,
        apply: bool,
    },
    /// Spin one fan header to full speed for a few seconds.
    IdentifyFan(String),
    /// Apply an edited, unsaved profile for a while; the service reverts it
    /// unless [`ControlCmd::ConfirmPreview`] keeps it (which also saves it).
    /// Set the lights at once without saving — the Lighting tab's live
    /// preview while an effect or colour is being picked.
    AuraPreview(AuraPart),
    /// Switch the desk lamp(s) off, or back on as they last were (the tray).
    /// Not saved in any profile.
    ToggleLamp,
    /// Let this lighting device (id) follow lighting writes — switches a
    /// setting saved in the device, so only on the user's request.
    EnableWdl(String),
    /// Delete a custom profile; the service applies whichever profile becomes
    /// active if it was the active one.
    DeleteProfile(String),
    /// Put a built-in profile back to its defaults (re-applied when active).
    ResetProfile(String),
    Preview(Box<Profile>),
    ConfirmPreview {
        keep: bool,
    },
    /// Philips Hue (#215): look for bridges — every one on the network, or
    /// only the one at this address.
    HueDiscover(Option<String>),
    /// Pair with the bridge at this address (after its link button).
    HuePair(String),
    /// Read the paired bridge's rooms and zones again.
    HueRefresh,
    /// The rooms and zones (ids) that follow the rig.
    HueChoose(Vec<String>),
    /// Forget the paired bridge.
    HueUnpair,
    /// Read every ROG mouse's settings (#431) — when the Mouse page opens.
    ReadMice,
    /// Change one mouse's settings in its active onboard profile.
    SetMouse {
        id: String,
        settings: Box<MouseSettings>,
    },
    /// Read every headset's settings (#428) — when the Headset page opens.
    ReadHeadsets,
    /// Change one headset's settings; saved in the headset.
    SetHeadset {
        id: String,
        settings: HeadsetSettings,
    },
}

// ── The task ────────────────────────────────────────────────────────────

/// Owns the control pipe connection for the app's lifetime: connects,
/// performs the `hello` handshake, fetches initial state, then services
/// [`ControlCmd`]s from the UI and forwards server-pushed events — all
/// events funnel through `event_tx` as [`ControlEvent`]s. Reconnects with a
/// fixed backoff on any disconnect, except a protocol mismatch (doc: "the
/// UI disables control and says why" — retrying can't fix a version skew).
/// `cmd_rx` is shared so a restart after a panic (`debug::supervise`) can
/// take the same channel up again.
pub async fn control_task(
    cmd_rx: std::sync::Arc<tokio::sync::Mutex<AsyncReceiver<ControlCmd>>>,
    event_tx: SyncSender<ControlEvent>,
    dir: PathBuf,
    app_version: String,
) {
    control_task_on(PIPE_NAME, cmd_rx, event_tx, dir, app_version).await;
}

/// [`control_task`] on any pipe — tests serve their own (#226).
async fn control_task_on(
    pipe_name: &str,
    cmd_rx: std::sync::Arc<tokio::sync::Mutex<AsyncReceiver<ControlCmd>>>,
    event_tx: SyncSender<ControlEvent>,
    dir: PathBuf,
    app_version: String,
) {
    let mut cmd_rx = cmd_rx.lock().await;
    // Connections the service closed before answering `hello` in a row.
    let mut refusals: u32 = 0;
    loop {
        let Some(client) = connect(pipe_name, &dir).await else {
            tokio::time::sleep(RECONNECT_BACKOFF).await;
            continue;
        };
        let (read_half, mut writer) = tokio::io::split(client);
        let mut reader = BufReader::new(read_half);
        let mut next_id: u64 = 1;

        let hello_params =
            serde_json::json!({ "protocol": PROTOCOL_VERSION, "app_version": app_version });
        match request(
            &mut writer,
            &mut reader,
            &mut next_id,
            "hello",
            Some(hello_params),
            &event_tx,
            &dir,
        )
        .await
        {
            Ok(Some(result)) => {
                let protocol = result
                    .get("protocol")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
                if protocol != PROTOCOL_VERSION {
                    log_error(
                        &dir,
                        &format!("control: protocol mismatch (app={PROTOCOL_VERSION}, service={protocol}) — control disabled"),
                    );
                    let _ = event_tx.send(ControlEvent::ProtocolMismatch {
                        expected: PROTOCOL_VERSION,
                        got: protocol,
                    });
                    return; // Not retryable — a reconnect won't change the service's protocol version.
                }
            }
            Err(RequestError::Closed) => {
                // The service verifies the client before reading anything and
                // just closes on a refusal, so retrying fast only fills its
                // log (#231). "Pipe not there" stays on the fast retry above.
                refusals += 1;
                log_pipe_trouble_throttled(
                    &dir,
                    "control: the service closed the connection before answering hello — refused?",
                );
                if refusals == REFUSALS_SHOWN_AFTER {
                    let _ = event_tx.send(ControlEvent::Refused);
                }
                tokio::time::sleep(refused_backoff(refusals)).await;
                continue;
            }
            _ => {
                // An error response or no answer to hello — treat like any
                // other failed connection attempt.
                tokio::time::sleep(RECONNECT_BACKOFF).await;
                continue;
            }
        }

        refusals = 0;
        append_debug_log(&dir, "control: connected to rigstats-control");
        let _ = event_tx.send(ControlEvent::Connected);
        fetch_and_publish(&mut writer, &mut reader, &mut next_id, &event_tx, &dir).await;
        // Start the event stream (fan_duty, safety_tripped) — the service
        // pushes nothing to a connection until it subscribes.
        let _ = request(
            &mut writer,
            &mut reader,
            &mut next_id,
            "subscribe",
            None,
            &event_tx,
            &dir,
        )
        .await;

        // Main loop: whichever happens first — a command from the UI, or a
        // line pushed by the server with nothing pending (an unsolicited
        // event; a response would only arrive while `request` itself is
        // reading, not here).
        loop {
            tokio::select! {
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(cmd) => {
                            if handle_cmd(cmd, &mut writer, &mut reader, &mut next_id, &event_tx, &dir).await.is_err() {
                                break;
                            }
                        }
                        None => return, // UI dropped its sender — app is shutting down.
                    }
                }
                line = read_line(&mut reader) => {
                    match line {
                        Ok(Some(text)) => handle_pushed_line(&text, &event_tx, &dir),
                        _ => break, // EOF or read error — reconnect.
                    }
                }
            }
        }

        log_warn(&dir, "control: disconnected — reconnecting");
        let _ = event_tx.send(ControlEvent::Disconnected);
        tokio::time::sleep(RECONNECT_BACKOFF).await;
    }
}

/// Retry delay after the `refusals`-th refusal in a row: `RECONNECT_BACKOFF`,
/// doubling, capped at `REFUSED_BACKOFF_MAX`.
fn refused_backoff(refusals: u32) -> Duration {
    let doublings = refusals.saturating_sub(1).min(16);
    (RECONNECT_BACKOFF * 2u32.pow(doublings)).min(REFUSED_BACKOFF_MAX)
}

async fn connect(pipe_name: &str, dir: &Path) -> Option<NamedPipeClient> {
    match ClientOptions::new().open(pipe_name) {
        Ok(client) if !crate::pipe_server::is_service_pipe(&client) => {
            log_pipe_trouble_throttled(
                dir,
                "control: rigstats-control is not served by the RIGStats service — ignoring it",
            );
            None
        }
        Ok(client) => Some(client),
        Err(e) => {
            log_pipe_trouble_throttled(
                dir,
                &format!("control: connect failed: {e} (os={:?})", e.raw_os_error()),
            );
            None
        }
    }
}

async fn read_line(reader: &mut (impl AsyncBufRead + Unpin)) -> std::io::Result<Option<String>> {
    let mut buf = String::new();
    let n = reader.read_line(&mut buf).await?;
    Ok(if n == 0 { None } else { Some(buf) })
}

/// Sends one request and reads lines until the matching response arrives
/// (any event lines seen along the way are forwarded immediately, not
/// buffered — see the module doc's note on why this connection doesn't need
/// a general pending-request map: the UI only ever has one command
/// in flight at a time).
/// Why a [`request`] got no response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestError {
    /// The service closed the pipe (EOF, broken pipe).
    Closed,
    /// Timed out, an oversized frame, or the request couldn't be encoded.
    Failed,
}

async fn request(
    writer: &mut (impl AsyncWrite + Unpin),
    reader: &mut (impl AsyncBufRead + Unpin),
    next_id: &mut u64,
    method: &str,
    params: Option<serde_json::Value>,
    event_tx: &SyncSender<ControlEvent>,
    dir: &Path,
) -> Result<Option<serde_json::Value>, RequestError> {
    let id = *next_id;
    *next_id += 1;

    let mut line = serde_json::to_string(&RequestOut { id, method, params })
        .map_err(|_| RequestError::Failed)?;
    line.push('\n');
    if writer.write_all(line.as_bytes()).await.is_err() {
        return Err(RequestError::Closed);
    }

    let timeout = if method.starts_with("hue_") {
        HUE_REQUEST_TIMEOUT
    } else {
        REQUEST_TIMEOUT
    };
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            log_pipe_trouble_throttled(
                dir,
                &format!("control: '{method}' timed out waiting for a response"),
            );
            return Err(RequestError::Failed);
        }

        let mut buf = String::new();
        let read = tokio::time::timeout(remaining, reader.read_line(&mut buf)).await;
        match read {
            Ok(Ok(0)) => return Err(RequestError::Closed), // EOF.
            Ok(Ok(_)) => {
                if buf.len() > MAX_PIPE_LINE_BYTES {
                    log_warn(
                        dir,
                        &format!(
                            "control: oversized frame ({} bytes) — dropping connection",
                            buf.len()
                        ),
                    );
                    return Err(RequestError::Failed);
                }
                match serde_json::from_str::<IncomingLine>(buf.trim()) {
                    Ok(IncomingLine::Response(resp)) if resp.id == id => {
                        if let Some(err) = resp.error {
                            log_warn(
                                dir,
                                &format!(
                                    "control: '{method}' failed: {} — {}",
                                    err.code, err.message
                                ),
                            );
                            let _ = event_tx.send(ControlEvent::Error(err.message));
                            return Ok(None);
                        }
                        return Ok(resp.result);
                    }
                    // A response to some earlier, already-abandoned request
                    // (e.g. after a prior timeout) — ignore and keep waiting.
                    Ok(IncomingLine::Response(_)) => {}
                    Ok(IncomingLine::Event(ev)) => handle_event(ev, event_tx, dir),
                    Err(e) => {
                        let preview = buf.trim().chars().take(120).collect::<String>();
                        log_error(
                            dir,
                            &format!("control: JSON parse error: {e} — raw: {preview}"),
                        );
                    }
                }
            }
            Ok(Err(_)) => return Err(RequestError::Closed),
            Err(_) => return Err(RequestError::Failed),
        }
    }
}

async fn fetch_and_publish(
    writer: &mut (impl AsyncWrite + Unpin),
    reader: &mut (impl AsyncBufRead + Unpin),
    next_id: &mut u64,
    event_tx: &SyncSender<ControlEvent>,
    dir: &Path,
) {
    if let Ok(Some(caps)) =
        request(writer, reader, next_id, "capabilities", None, event_tx, dir).await
    {
        if let Ok(list) = serde_json::from_value::<Vec<CapabilitySet>>(caps) {
            let _ = event_tx.send(ControlEvent::Capabilities(list));
        }
    }
    if let Ok(Some(profiles)) = request(
        writer,
        reader,
        next_id,
        "list_profiles",
        None,
        event_tx,
        dir,
    )
    .await
    {
        if let Ok(list) = serde_json::from_value::<Vec<Profile>>(profiles) {
            let _ = event_tx.send(ControlEvent::Profiles(list));
        }
    }
    if let Ok(Some(state)) =
        request(writer, reader, next_id, "get_state", None, event_tx, dir).await
    {
        let active = state
            .get("active_profile")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let _ = event_tx.send(ControlEvent::ActiveProfile(active));
        if let Some(dry_run) = state.get("dry_run").and_then(serde_json::Value::as_bool) {
            let _ = event_tx.send(ControlEvent::DryRun(dry_run));
        }
        let notice = state
            .get("crash_guard_notice")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let _ = event_tx.send(ControlEvent::CrashGuardNotice(notice));
        let notice = state
            .get("profiles_notice")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let _ = event_tx.send(ControlEvent::ProfilesNotice(notice));
    }
}

async fn handle_cmd(
    cmd: ControlCmd,
    writer: &mut (impl AsyncWrite + Unpin),
    reader: &mut (impl AsyncBufRead + Unpin),
    next_id: &mut u64,
    event_tx: &SyncSender<ControlEvent>,
    dir: &Path,
) -> Result<(), RequestError> {
    match cmd {
        ControlCmd::ApplyProfile(id) => {
            let params = serde_json::json!({ "id": id.clone() });
            let result = request(
                writer,
                reader,
                next_id,
                "apply_profile",
                Some(params),
                event_tx,
                dir,
            )
            .await?;
            if let Some(value) = result {
                if let Ok(r) = serde_json::from_value::<ApplyResult>(value) {
                    // The service doesn't push a separate "profile_changed"
                    // event for the apply that just succeeded (only for
                    // later out-of-band changes, e.g. a future admin tool) —
                    // update active_profile from this same response instead
                    // of waiting for one that never comes. Without this the
                    // tray checkmark / header chip / Control Center's
                    // highlighted row never move after a successful switch.
                    if r.ok {
                        let _ = event_tx.send(ControlEvent::ActiveProfile(Some(
                            r.profile_id.clone().unwrap_or(id),
                        )));
                        // The service acknowledged the crash-guard notice and
                        // reverted any pending preview before this apply.
                        let _ = event_tx.send(ControlEvent::CrashGuardNotice(None));
                        // Applying saved the active profile, which ends the
                        // profiles notice the same way.
                        let _ = event_tx.send(ControlEvent::ProfilesNotice(None));
                        let _ = event_tx.send(ControlEvent::PreviewEnded { kept: false });
                    }
                    publish_apply_result(r, event_tx, dir);
                }
            }
            Ok(())
        }
        ControlCmd::Refresh => {
            fetch_and_publish(writer, reader, next_id, event_tx, dir).await;
            Ok(())
        }
        ControlCmd::ReleaseToFirmware => {
            request(
                writer,
                reader,
                next_id,
                "release_to_firmware",
                None,
                event_tx,
                dir,
            )
            .await?;
            Ok(())
        }
        ControlCmd::SaveProfile { profile, apply } => {
            let id = profile.id.clone();
            let params = serde_json::to_value(&profile).map_err(|_| RequestError::Failed)?;
            let saved = request(
                writer,
                reader,
                next_id,
                "save_profile",
                Some(params),
                event_tx,
                dir,
            )
            .await?;
            // `request` already forwarded an error response as
            // ControlEvent::Error; don't apply a profile that wasn't stored.
            if saved.is_some() {
                // The service cleared it with this save.
                let _ = event_tx.send(ControlEvent::ProfilesNotice(None));
                if let Some(Ok(list)) = request(
                    writer,
                    reader,
                    next_id,
                    "list_profiles",
                    None,
                    event_tx,
                    dir,
                )
                .await?
                .map(serde_json::from_value::<Vec<Profile>>)
                {
                    let _ = event_tx.send(ControlEvent::Profiles(list));
                }
                if apply {
                    Box::pin(handle_cmd(
                        ControlCmd::ApplyProfile(id),
                        writer,
                        reader,
                        next_id,
                        event_tx,
                        dir,
                    ))
                    .await?;
                }
            }
            Ok(())
        }
        ControlCmd::IdentifyFan(header) => {
            request(
                writer,
                reader,
                next_id,
                "identify_fan",
                Some(serde_json::json!({ "header": header })),
                event_tx,
                dir,
            )
            .await?;
            Ok(())
        }
        ControlCmd::AuraPreview(aura) => {
            request(
                writer,
                reader,
                next_id,
                "aura_preview",
                Some(serde_json::json!({ "aura": aura })),
                event_tx,
                dir,
            )
            .await?;
            Ok(())
        }
        ControlCmd::ToggleLamp => {
            request(writer, reader, next_id, "lamp_toggle", None, event_tx, dir).await?;
            Ok(())
        }
        ControlCmd::ReadMice => {
            let _ = event_tx.send(ControlEvent::MouseBusy(true));
            let result = request(
                writer,
                reader,
                next_id,
                "mouse_settings",
                None,
                event_tx,
                dir,
            )
            .await;
            match result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref()?.get("mice").cloned())
            {
                Some(list) => {
                    let list = serde_json::from_value(list).unwrap_or_default();
                    let _ = event_tx.send(ControlEvent::Mice(list));
                }
                None => {
                    let _ = event_tx.send(ControlEvent::MouseBusy(false));
                }
            }
            result.map(|_| ())
        }
        ControlCmd::SetMouse { id, settings } => {
            let _ = event_tx.send(ControlEvent::MouseBusy(true));
            let result = request(
                writer,
                reader,
                next_id,
                "mouse_set",
                Some(serde_json::json!({ "id": id, "settings": settings })),
                event_tx,
                dir,
            )
            .await;
            match result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref()?.get("mouse").cloned())
                .and_then(|m| serde_json::from_value(m).ok())
            {
                Some(mouse) => {
                    let _ = event_tx.send(ControlEvent::MouseUpdated(Box::new(mouse)));
                }
                None => {
                    let _ = event_tx.send(ControlEvent::MouseBusy(false));
                }
            }
            result.map(|_| ())
        }
        ControlCmd::ReadHeadsets => {
            let _ = event_tx.send(ControlEvent::HeadsetBusy(true));
            let result = request(
                writer,
                reader,
                next_id,
                "headset_settings",
                None,
                event_tx,
                dir,
            )
            .await;
            match result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref()?.get("headsets").cloned())
            {
                Some(list) => {
                    let list = serde_json::from_value(list).unwrap_or_default();
                    let _ = event_tx.send(ControlEvent::Headsets(list));
                }
                None => {
                    let _ = event_tx.send(ControlEvent::HeadsetBusy(false));
                }
            }
            result.map(|_| ())
        }
        ControlCmd::SetHeadset { id, settings } => {
            let _ = event_tx.send(ControlEvent::HeadsetBusy(true));
            let result = request(
                writer,
                reader,
                next_id,
                "headset_set",
                Some(serde_json::json!({ "id": id, "settings": settings })),
                event_tx,
                dir,
            )
            .await;
            match result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref()?.get("headset").cloned())
                .and_then(|h| serde_json::from_value(h).ok())
            {
                Some(headset) => {
                    let _ = event_tx.send(ControlEvent::HeadsetUpdated(headset));
                }
                None => {
                    let _ = event_tx.send(ControlEvent::HeadsetBusy(false));
                }
            }
            result.map(|_| ())
        }
        ControlCmd::EnableWdl(id) => {
            request(
                writer,
                reader,
                next_id,
                "aura_enable_wdl",
                Some(serde_json::json!({ "id": id })),
                event_tx,
                dir,
            )
            .await?;
            // The Lighting tab shows the new mode from the capabilities.
            fetch_and_publish(writer, reader, next_id, event_tx, dir).await;
            Ok(())
        }
        ControlCmd::DeleteProfile(id) => {
            request(
                writer,
                reader,
                next_id,
                "delete_profile",
                Some(serde_json::json!({ "id": id })),
                event_tx,
                dir,
            )
            .await?;
            fetch_and_publish(writer, reader, next_id, event_tx, dir).await;
            Ok(())
        }
        ControlCmd::ResetProfile(id) => {
            if let Some(value) = request(
                writer,
                reader,
                next_id,
                "reset_profile",
                Some(serde_json::json!({ "id": id })),
                event_tx,
                dir,
            )
            .await?
            {
                if let Ok(r) = serde_json::from_value::<ApplyResult>(value) {
                    publish_apply_result(r, event_tx, dir);
                }
            }
            fetch_and_publish(writer, reader, next_id, event_tx, dir).await;
            Ok(())
        }
        ControlCmd::Preview(profile) => {
            let params = serde_json::json!({ "profile": profile.as_ref() });
            if let Some(value) = request(
                writer,
                reader,
                next_id,
                "preview",
                Some(params),
                event_tx,
                dir,
            )
            .await?
            {
                if let Ok(r) = serde_json::from_value::<ApplyResult>(value.clone()) {
                    let revert_in = value
                        .get("revert_in_s")
                        .and_then(serde_json::Value::as_u64)
                        .map(Duration::from_secs);
                    if let (true, Some(revert_in)) = (r.ok, revert_in) {
                        let _ = event_tx.send(ControlEvent::PreviewStarted {
                            profile_id: profile.id.clone(),
                            revert_in,
                        });
                    }
                    publish_apply_result(r, event_tx, dir);
                }
            }
            Ok(())
        }
        ControlCmd::ConfirmPreview { keep } => {
            let result = request(
                writer,
                reader,
                next_id,
                "confirm",
                Some(serde_json::json!({ "keep": keep })),
                event_tx,
                dir,
            )
            .await?;
            // Kept: the service saved it and made it active. Either way the
            // limits in force changed — refresh profiles, active and caps
            // *before* ending the preview, so the UI never sees "preview over"
            // with the old profile list (its CPU draft would look unsaved and
            // the footer would offer "Try" again for a moment).
            fetch_and_publish(writer, reader, next_id, event_tx, dir).await;
            // An error response (already forwarded as ControlEvent::Error)
            // means the service had reverted it already.
            let _ = event_tx.send(ControlEvent::PreviewEnded {
                kept: keep && result.is_some(),
            });
            Ok(())
        }
        ControlCmd::HueDiscover(ip) => {
            let params = ip.map(|ip| serde_json::json!({ "ip": ip }));
            let _ = event_tx.send(ControlEvent::HueBusy(true));
            let result = request(
                writer,
                reader,
                next_id,
                "hue_discover",
                params,
                event_tx,
                dir,
            )
            .await;
            let found = result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref()?.get("bridges").cloned())
                .and_then(|b| serde_json::from_value::<Vec<HueBridgeFound>>(b).ok())
                .unwrap_or_default();
            if result.as_ref().is_ok_and(Option::is_some) && found.is_empty() {
                let _ = event_tx.send(ControlEvent::Error(
                    "No Hue Bridge answered. Check that it is on and on this network, or enter its IP address."
                        .to_owned(),
                ));
            }
            let _ = event_tx.send(ControlEvent::HueFound(found));
            hue_done(result, event_tx)
        }
        ControlCmd::HuePair(ip) => {
            hue_request(
                "hue_pair",
                Some(serde_json::json!({ "ip": ip })),
                writer,
                reader,
                next_id,
                event_tx,
                dir,
            )
            .await
        }
        ControlCmd::HueRefresh => {
            hue_request("hue_refresh", None, writer, reader, next_id, event_tx, dir).await
        }
        ControlCmd::HueChoose(groups) => {
            hue_request(
                "hue_choose",
                Some(serde_json::json!({ "groups": groups })),
                writer,
                reader,
                next_id,
                event_tx,
                dir,
            )
            .await
        }
        ControlCmd::HueUnpair => {
            hue_request("hue_unpair", None, writer, reader, next_id, event_tx, dir).await
        }
    }
}

/// A Hue request that changes the pairing or the chosen rooms, then the
/// capabilities again — the Lighting tab shows the new state from there.
async fn hue_request(
    method: &str,
    params: Option<serde_json::Value>,
    writer: &mut (impl AsyncWrite + Unpin),
    reader: &mut (impl AsyncBufRead + Unpin),
    next_id: &mut u64,
    event_tx: &SyncSender<ControlEvent>,
    dir: &Path,
) -> Result<(), RequestError> {
    let _ = event_tx.send(ControlEvent::HueBusy(true));
    let result = request(writer, reader, next_id, method, params, event_tx, dir).await;
    if result.as_ref().is_ok_and(Option::is_some) {
        let _ = event_tx.send(ControlEvent::HueFound(Vec::new()));
        fetch_and_publish(writer, reader, next_id, event_tx, dir).await;
    }
    hue_done(result, event_tx)
}

/// Ends a Hue request. One that got no answer at all (the connection is
/// dropped and re-made) still says so on the Hue card.
fn hue_done(
    result: Result<Option<serde_json::Value>, RequestError>,
    event_tx: &SyncSender<ControlEvent>,
) -> Result<(), RequestError> {
    if result.is_err() {
        let _ = event_tx.send(ControlEvent::Error(
            "The RIGStats service didn't answer in time. Try again in a moment.".to_owned(),
        ));
    }
    let _ = event_tx.send(ControlEvent::HueBusy(false));
    result.map(|_| ())
}

/// A line read outside of any pending `request()` call — i.e. genuinely
/// unsolicited. A response here (no matching request in flight) is logged
/// as a protocol oddity rather than silently dropped.
fn handle_pushed_line(text: &str, event_tx: &SyncSender<ControlEvent>, dir: &Path) {
    match serde_json::from_str::<IncomingLine>(text.trim()) {
        Ok(IncomingLine::Event(ev)) => handle_event(ev, event_tx, dir),
        Ok(IncomingLine::Response(resp)) => {
            log_warn(
                dir,
                &format!(
                    "control: response id={} with no matching pending request",
                    resp.id
                ),
            );
        }
        Err(e) => log_warn(
            dir,
            &format!("control: JSON parse error on pushed line: {e}"),
        ),
    }
}

/// `subscribe`'s event stream: `profile_changed`, `apply_result`, and the
/// fan loop's `fan_duty` / `safety_tripped` (#188). Unknown events are
/// ignored, so a newer service can add events without breaking this app.
/// Hands an apply result to the UI and writes a failed one to the debug log
/// (the Status window shows that log; the service logs its own side).
fn publish_apply_result(r: ApplyResult, event_tx: &SyncSender<ControlEvent>, dir: &Path) {
    if !r.ok {
        let profile = r.profile_id.as_deref().unwrap_or("?");
        let why = r.message.as_deref().unwrap_or("no reason given");
        log_warn(
            dir,
            &format!("control: applying profile '{profile}' failed — {why}"),
        );
    }
    let _ = event_tx.send(ControlEvent::ApplyResult(r));
}

fn handle_event(ev: EventIn, event_tx: &SyncSender<ControlEvent>, dir: &Path) {
    match ev.event.as_str() {
        "profile_changed" => {
            let id = ev
                .data
                .as_ref()
                .and_then(|d| d.get("id"))
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let _ = event_tx.send(ControlEvent::ActiveProfile(id));
        }
        "apply_result" => {
            if let Some(data) = ev.data {
                if let Ok(result) = serde_json::from_value::<ApplyResult>(data) {
                    publish_apply_result(result, event_tx, dir);
                }
            }
        }
        "fan_duty" => {
            if let Some(duty) = ev
                .data
                .and_then(|mut d| d.get_mut("duty").map(serde_json::Value::take))
                .and_then(|d| serde_json::from_value::<BTreeMap<String, f64>>(d).ok())
            {
                let _ = event_tx.send(ControlEvent::FanDuty(duty));
            }
        }
        "safety_tripped" => {
            let reason = ev
                .data
                .as_ref()
                .and_then(|d| d.get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("Critical temperature")
                .to_owned();
            let _ = event_tx.send(ControlEvent::SafetyTripped(reason));
        }
        "fan_identified" => {
            #[derive(Deserialize)]
            struct Identified {
                header: String,
                #[serde(default)]
                responders: Vec<FanResponder>,
            }
            if let Some(Ok(id)) = ev.data.map(serde_json::from_value::<Identified>) {
                let _ = event_tx.send(ControlEvent::FanIdentified {
                    header: id.header,
                    responders: id.responders,
                });
            }
        }
        "preview_reverted" => {
            let _ = event_tx.send(ControlEvent::PreviewEnded { kept: false });
        }
        _ => {}
    }
}

/// Unix timestamp of the last pipe-trouble log message — throttled to one
/// entry per 30 s window, mirroring `lhm.rs`'s `log_pipe_trouble_throttled`
/// (kept as a separate, module-private throttle window: a broken telemetry
/// pipe and a broken control pipe are independent failures worth their own
/// log cadence, not sharing one budget).
static LAST_PIPE_FAIL_LOG_SECS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn log_pipe_trouble_throttled(dir: &Path, msg: &str) {
    use crate::debug::unix_now_secs;
    use std::sync::atomic::Ordering;
    let now = unix_now_secs();
    let last = LAST_PIPE_FAIL_LOG_SECS.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= 30 {
        LAST_PIPE_FAIL_LOG_SECS.store(now, Ordering::Relaxed);
        log_warn(dir, msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `mouse_settings`' answer as `MouseProvider.List` writes it, and the
    /// settings `mouse_set` sends as `MouseProtocolTests` reads them (#431).
    #[test]
    fn mouse_wire_shapes_match_the_service() {
        let mouse: MouseInfo = serde_json::from_value(serde_json::json!({
            "id": "asus-mouse-1a94-omni", "name": "ROG Harpe Ace Aim Lab Edition",
            "link": "omni", "verified": false,
            "spec": {"profiles": 5, "dpi_stages": 4, "dpi_min": 50, "dpi_max": 36000, "dpi_step": 50,
                     "dpi_colors": true, "polling_hz": [125, 250, 500, 1000], "debounce": [2, 7],
                     "lift_off": true, "angle_snapping": true, "angle_tuning": [-30, 30, 1],
                     "acceleration": 9, "deceleration": 9, "motion_sync": false,
                     "auto_power_off": true, "low_battery_warning": [10, 50],
                     "buttons": [{"source": 496, "name": "Left click"}, {"source": 486, "name": "DPI button"}],
                     "actions": [{"code": 483, "name": "Double click"}, {"code": 0, "name": "Disabled"}]},
            "settings": {"profile": 2, "active_stage": 3,
                         "stages": [{"dpi": 400, "color": "#ff0000"}, {"dpi": 800, "color": "#ff00ff"}],
                         "polling_hz": 1000, "debounce": 2, "angle_snapping": false, "angle_tuning": -5,
                         "acceleration": 2, "deceleration": 1, "motion_sync": null, "lift_off": 1,
                         "power_off": 2, "low_battery_warning": 20, "buttons": [496, 0]}
        }))
        .unwrap();
        assert_eq!(mouse.spec.angle_tuning, Some((-30, 30, 1)));
        assert_eq!(mouse.spec.low_battery_warning, Some((10, 50)));
        assert_eq!(
            mouse.spec.buttons.as_ref().map(|b| b[1].source),
            Some(0x01E6)
        );
        assert_eq!(mouse.spec.actions.as_ref().map(|a| a.len()), Some(2));
        assert_eq!(
            mouse.settings.as_ref().unwrap().buttons,
            Some(vec![0x01F0, 0])
        );
        let settings = mouse.settings.clone().unwrap();
        assert_eq!(settings.stages[1].dpi, 800);
        assert_eq!(settings.angle_tuning, Some(-5));
        let sent = serde_json::to_value(&settings).unwrap();
        assert_eq!(sent["active_stage"], 3);
        assert_eq!(sent["stages"][0]["color"], "#ff0000");
        assert_eq!(sent["motion_sync"], serde_json::Value::Null);

        let mut state = ControlState::default();
        state.apply(ControlEvent::Mice(vec![mouse.clone()]));
        let mut changed = mouse;
        changed.settings.as_mut().unwrap().polling_hz = Some(500);
        state.apply(ControlEvent::MouseUpdated(Box::new(changed)));
        assert_eq!(state.mice.len(), 1);
        assert_eq!(
            state.mice[0].settings.as_ref().unwrap().polling_hz,
            Some(500)
        );

        // A profile's mouse part round-trips; a profile without one stays without.
        let part: ProfilePart =
            serde_json::from_value(serde_json::json!({"mouse": {"profile": 3}})).unwrap();
        assert_eq!(part.mouse, Some(MousePart { profile: Some(3) }));
        assert!(serde_json::to_value(ProfilePart::default())
            .unwrap()
            .get("mouse")
            .is_none());
    }

    /// `headset_settings`' answer as `LightingProvider.Headsets` writes it,
    /// and `headset_set`'s settings as `HeadsetSettingsTests` reads them (#428).
    #[test]
    fn headset_wire_shapes_match_the_service() {
        let list: Vec<HeadsetInfo> = serde_json::from_value(serde_json::json!([
            {"id": "asus-headset-1afa-1", "name": "ROG Delta II", "verified": false,
             "sidetone_max": 20, "noise_levels": 3,
             "settings": {"sidetone_on": true, "sidetone_level": 8,
                          "noise_reduction_on": false, "noise_reduction_level": 1}},
            {"id": "asus-headset-1afa-2", "name": "ROG Delta II (2)", "verified": false,
             "sidetone_max": 20, "noise_levels": 3, "settings": null}
        ]))
        .unwrap();
        let settings = HeadsetSettings {
            sidetone_on: true,
            sidetone_level: 8,
            noise_reduction_on: false,
            noise_reduction_level: 1,
        };
        assert_eq!(list[0].settings, Some(settings));
        assert_eq!(list[1].settings, None);
        assert_eq!(
            serde_json::to_value(settings).unwrap(),
            serde_json::json!({"sidetone_on": true, "sidetone_level": 8,
                               "noise_reduction_on": false, "noise_reduction_level": 1})
        );

        let mut state = ControlState::default();
        state.apply(ControlEvent::Headsets(list));
        let mut changed = state.headsets[1].clone();
        changed.settings = Some(settings);
        state.apply(ControlEvent::HeadsetUpdated(changed));
        assert_eq!(state.headsets.len(), 2);
        assert_eq!(state.headsets[1].settings, Some(settings));
    }

    /// The problem's wire shape against the example the service's
    /// `GpuWorkerTests.Capability_problem_serializes_as_the_shared_contract_example`
    /// writes — neither side can rename a field alone (#370).
    #[test]
    fn capability_problem_contract_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../sensor-sidecar.Tests/contract/control-capability-problem.json");
        let text = std::fs::read_to_string(&path).expect("read the contract example");
        let caps: CapabilitySet = serde_json::from_str(&text).expect("parse the contract example");

        assert_eq!(caps.domain, "gpu");
        assert!(!caps.supported);
        let problem = caps.problem.expect("the problem");
        assert_eq!(problem.id, "gpu_driver_crashed");
        assert_eq!(problem.severity, Severity::Error);
        assert_eq!(problem.title, "The graphics driver crashed");
        assert!(problem
            .message
            .contains("Sensors, fans and lighting keep working"));
        assert_eq!(
            problem
                .actions
                .iter()
                .map(|a| (a.kind, a.label.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (ProblemActionKind::OpenUrl, "Get the AMD driver"),
                (
                    ProblemActionKind::CollectDiagnostics,
                    "Collect diagnostics…"
                ),
                (ProblemActionKind::OpenUrl, "Read more"),
            ]
        );
        assert_eq!(
            problem.actions[0].target.as_deref(),
            Some("https://www.amd.com/en/support/download/drivers.html")
        );
        assert!(problem.detail.unwrap().contains("0xC0000005"));
    }

    #[test]
    fn a_newer_services_unknown_severity_and_action_still_parse() {
        let problem: Problem = serde_json::from_str(
            r#"{"id":"x","severity":"catastrophic","title":"T","message":"M",
                "actions":[{"kind":"reboot_now","label":"Reboot"}]}"#,
        )
        .unwrap();

        assert_eq!(problem.severity, Severity::Attention);
        assert_eq!(problem.actions[0].kind, ProblemActionKind::Unknown);
    }

    #[test]
    fn an_older_service_without_problems_still_parses() {
        let caps: CapabilitySet = serde_json::from_str(
            r#"{"domain":"gpu","supported":false,"reason":"GPU power limits are unavailable."}"#,
        )
        .unwrap();

        assert_eq!(caps.problem, None);
    }

    fn problem(id: &str) -> Problem {
        Problem {
            id: id.into(),
            severity: Severity::Error,
            title: "T".into(),
            message: "M".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_domain_problem_counts_only_when_the_domain_is_unavailable() {
        let mut state = ControlState {
            capabilities: vec![
                CapabilitySet {
                    domain: "gpu".into(),
                    supported: false,
                    problem: Some(problem("gpu_driver_crashed")),
                    ..Default::default()
                },
                CapabilitySet {
                    domain: "fan".into(),
                    supported: false,
                    reason: Some("No writable fan headers found.".into()),
                    problem: None,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            state.domain_problem("gpu").map(|p| p.id.as_str()),
            Some("gpu_driver_crashed")
        );
        assert_eq!(state.domain_problem("fan"), None); // absent, not failed: hidden
        assert_eq!(state.domain_problem("aura"), None);

        state.capabilities[0].supported = true; // a stale problem on a working domain
        assert_eq!(state.domain_problem("gpu"), None);
    }

    #[test]
    fn the_services_notices_read_as_problems() {
        let state = ControlState {
            crash_guard_notice: Some("The PC restarted shortly after …".into()),
            profiles_notice: Some("Profiles could not be read …".into()),
            ..Default::default()
        };

        let crash = state.crash_guard_problem().unwrap();
        assert_eq!(
            (crash.id.as_str(), crash.severity, crash.title.as_str()),
            (
                "crash_guard",
                Severity::Attention,
                "Reverted after a restart"
            )
        );
        assert_eq!(crash.message, "The PC restarted shortly after …");
        assert_eq!(
            state.profiles_problem().map(|p| p.title),
            Some("Profiles recovered".into())
        );
        assert_eq!(ControlState::default().crash_guard_problem(), None);
        assert_eq!(ControlState::default().profiles_problem(), None);
    }

    #[test]
    fn a_failed_apply_is_written_to_the_debug_log() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let failed = ApplyResult {
            ok: false,
            message: Some("fan header control/1 did not accept 40 %".into()),
            profile_id: Some("gaming".into()),
        };
        publish_apply_result(failed.clone(), &tx, dir.path());
        assert!(matches!(rx.try_recv(), Ok(ControlEvent::ApplyResult(r)) if r == failed));
        let log = std::fs::read_to_string(dir.path().join("rigstats-debug.log")).unwrap();
        assert!(log.contains("applying profile 'gaming' failed"), "{log}");
        assert!(log.contains("did not accept 40 %"), "{log}");
    }

    #[test]
    fn a_successful_apply_is_not_logged_as_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let ok = ApplyResult {
            ok: true,
            message: None,
            profile_id: Some("gaming".into()),
        };
        publish_apply_result(ok, &tx, dir.path());
        let log =
            std::fs::read_to_string(dir.path().join("rigstats-debug.log")).unwrap_or_default();
        assert!(!log.contains("failed"), "{log}");
    }

    #[test]
    fn control_state_apply_connected_clears_mismatch_and_error() {
        let mut state = ControlState {
            protocol_mismatch: Some(ProtocolMismatch {
                expected: 1,
                got: 2,
            }),
            last_error: Some("boom".to_string()),
            ..Default::default()
        };

        state.apply(ControlEvent::Connected);

        assert!(state.connected);
        assert!(state.protocol_mismatch.is_none());
        assert!(state.last_error.is_none());
    }

    /// One session of a canned control service: answers the handshake and
    /// initial fetch, pushes a `fan_duty` event after `subscribe`, then hangs up.
    async fn serve_canned_session(server: tokio::net::windows::named_pipe::NamedPipeServer) {
        server.connect().await.expect("client connects");
        let (read_half, mut writer) = tokio::io::split(server);
        let mut lines = BufReader::new(read_half).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let request: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
            let method = request["method"].as_str().unwrap_or_default().to_owned();
            let result = match method.as_str() {
                "hello" => {
                    serde_json::json!({ "protocol": PROTOCOL_VERSION, "service_version": "test" })
                }
                "capabilities" => serde_json::json!([{ "domain": "fan", "supported": true }]),
                "list_profiles" => serde_json::json!([
                    { "id": "balanced", "name": "Balanced", "builtin": true, "part": {} }
                ]),
                "get_state" => {
                    serde_json::json!({ "active_profile": "balanced", "dry_run": false })
                }
                _ => serde_json::json!({ "ok": true }),
            };
            let response = serde_json::json!({ "id": request["id"], "result": result });
            writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
            if method == "subscribe" {
                let event = r#"{"event":"fan_duty","data":{"duty":{"h0":42.0}}}"#;
                writer
                    .write_all(format!("{event}\n").as_bytes())
                    .await
                    .unwrap();
                break;
            }
        }
    }

    /// Waits for the first event `pick` accepts, yielding to the runtime.
    async fn next_matching<T>(
        rx: &std::sync::mpsc::Receiver<ControlEvent>,
        mut pick: impl FnMut(ControlEvent) -> Option<T>,
    ) -> T {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            while let Ok(event) = rx.try_recv() {
                if let Some(found) = pick(event) {
                    return found;
                }
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "event not seen in time"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn control_task_fetches_state_forwards_events_and_reconnects() {
        use tokio::net::windows::named_pipe::ServerOptions;

        let name: &'static str = Box::leak(
            format!(r"\\.\pipe\rigstats-control-test-{}", std::process::id()).into_boxed_str(),
        );
        let first = ServerOptions::new()
            .first_pipe_instance(true)
            .create(name)
            .unwrap();
        // The next instance exists before the first hangs up, so the name is
        // never free and the reconnect finds it.
        let second = ServerOptions::new().create(name).unwrap();

        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(8);
        let task = tokio::spawn(control_task_on(
            name,
            std::sync::Arc::new(tokio::sync::Mutex::new(cmd_rx)),
            event_tx,
            std::env::temp_dir(),
            "test".to_owned(),
        ));

        let server = tokio::spawn(async move {
            serve_canned_session(first).await;
            serve_canned_session(second).await;
        });

        let mut state = ControlState::default();
        next_matching(&event_rx, |e| {
            let duty = matches!(e, ControlEvent::FanDuty(_));
            state.apply(e);
            duty.then_some(())
        })
        .await;
        assert_eq!(state.capabilities.len(), 1);
        assert_eq!(state.profiles.len(), 1);
        assert_eq!(state.active_profile.as_deref(), Some("balanced"));
        assert_eq!(state.fan_duty.get("h0"), Some(&42.0));

        // The service hangs up: the task says so and connects again.
        next_matching(&event_rx, |e| {
            matches!(e, ControlEvent::Disconnected).then_some(())
        })
        .await;
        next_matching(&event_rx, |e| {
            matches!(e, ControlEvent::Connected).then_some(())
        })
        .await;

        task.abort();
        server.abort();
    }

    #[test]
    fn control_state_profiles_notice_set_and_cleared() {
        let mut state = ControlState::default();

        state.apply(ControlEvent::ProfilesNotice(Some("restored".into())));
        assert_eq!(state.profiles_notice.as_deref(), Some("restored"));

        state.apply(ControlEvent::ProfilesNotice(None));
        assert_eq!(state.profiles_notice, None);
    }

    #[test]
    fn control_state_refused_until_connected() {
        let mut state = ControlState::default();

        state.apply(ControlEvent::Refused);
        assert!(state.refused);
        assert!(!state.connected);

        state.apply(ControlEvent::Connected);
        assert!(!state.refused);
        assert!(state.connected);
    }

    #[test]
    fn refused_backoff_doubles_up_to_the_cap() {
        let delays: Vec<u64> = (1..=8).map(|n| refused_backoff(n).as_secs()).collect();
        assert_eq!(delays, [2, 4, 8, 16, 32, 60, 60, 60]);
        // Never overflows, however long the service keeps refusing.
        assert_eq!(refused_backoff(u32::MAX), REFUSED_BACKOFF_MAX);
    }

    #[tokio::test]
    async fn request_reports_a_closed_pipe_as_closed() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let dir = std::env::temp_dir();
        let mut next_id = 1;

        // The service closes before answering (a refusal): EOF.
        let mut reader = BufReader::new(&b""[..]);
        let result = request(
            &mut tokio::io::sink(),
            &mut reader,
            &mut next_id,
            "hello",
            None,
            &tx,
            &dir,
        )
        .await;
        assert_eq!(result, Err(RequestError::Closed));
    }

    #[test]
    fn control_state_apply_protocol_mismatch_forces_disconnected() {
        let mut state = ControlState {
            connected: true,
            ..Default::default()
        };

        state.apply(ControlEvent::ProtocolMismatch {
            expected: 1,
            got: 2,
        });

        assert!(!state.connected);
        assert_eq!(
            state.protocol_mismatch,
            Some(ProtocolMismatch {
                expected: 1,
                got: 2
            })
        );
    }

    #[test]
    fn control_state_apply_updates_the_targeted_field_only() {
        let mut state = ControlState::default();

        state.apply(ControlEvent::Profiles(vec![Profile {
            id: "gaming".into(),
            name: "Gaming".into(),
            icon: None,
            builtin: true,
            part: ProfilePart {
                power_plan: Some("high_performance".into()),
                ..Default::default()
            },
        }]));
        state.apply(ControlEvent::ActiveProfile(Some("gaming".into())));
        state.apply(ControlEvent::DryRun(true));

        assert_eq!(state.profiles.len(), 1);
        assert_eq!(state.active_profile.as_deref(), Some("gaming"));
        assert!(state.dry_run);
    }

    #[test]
    fn incoming_line_disambiguates_response_vs_event() {
        let response: IncomingLine =
            serde_json::from_str(r#"{"id":1,"result":{"ok":true}}"#).unwrap();
        assert!(matches!(response, IncomingLine::Response(r) if r.id == 1));

        let event: IncomingLine =
            serde_json::from_str(r#"{"event":"profile_changed","data":{"id":"gaming"}}"#).unwrap();
        assert!(matches!(event, IncomingLine::Event(e) if e.event == "profile_changed"));
    }

    #[test]
    fn request_out_serializes_snake_case_and_omits_null_params() {
        let req = RequestOut {
            id: 1,
            method: "hello",
            params: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(json, r#"{"id":1,"method":"hello"}"#);
    }

    #[test]
    fn profile_round_trips_the_doc_gaming_example_shape() {
        let json = r#"{"id":"gaming","name":"Gaming","icon":"bolt","builtin":true,"part":{"power_plan":"high_performance"}}"#;
        let profile: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.id, "gaming");
        assert_eq!(profile.part.power_plan.as_deref(), Some("high_performance"));

        let round_tripped: Profile =
            serde_json::from_str(&serde_json::to_string(&profile).unwrap()).unwrap();
        assert_eq!(round_tripped, profile);
    }

    #[test]
    fn fan_part_round_trips_the_service_shape() {
        // Same shape FanProvider's snapshot/ProfileStore write (snake_case,
        // [temp, duty] pairs, header ids as map keys).
        let json = r#"{"headers":{"/lpc/nct6799d/0/control/1":{"label":"CPU cooler","source":"cpu_package","curve":[[40.0,30.0],[85.0,100.0]],"hysteresis_c":3.0}}}"#;
        let part: FanPart = serde_json::from_str(json).unwrap();
        let header = &part.headers.as_ref().unwrap()["/lpc/nct6799d/0/control/1"];
        assert_eq!(header.label.as_deref(), Some("CPU cooler"));
        assert_eq!(header.curve, vec![[40.0, 30.0], [85.0, 100.0]]);
        assert_eq!(serde_json::to_string(&part).unwrap(), json);
    }

    #[test]
    fn fan_header_hysteresis_defaults_when_missing() {
        let header: FanHeaderConfig =
            serde_json::from_str(r#"{"source":"gpu","curve":[[50,40]]}"#).unwrap();
        assert!((header.hysteresis_c - 3.0).abs() < f64::EPSILON);
        assert_eq!(header.label, None);
    }

    #[test]
    fn hue_caps_are_read_even_when_no_rgb_device_makes_lighting_supported() {
        let state = ControlState {
            capabilities: vec![CapabilitySet {
                domain: "aura".into(),
                supported: false,
                reason: Some("No lighting controller found.".into()),
                problem: None,
                details: Some(serde_json::json!({
                    "hue": {
                        "paired": true,
                        "bridge": {"id": "001788fffe000001", "name": "Hue Bridge", "model": "BSB002",
                                   "firmware": "1967054020", "ip": "192.168.1.20"},
                        "groups": [{"id": "r1", "name": "Office", "kind": "room", "chosen": true}],
                    }
                })),
            }],
            ..ControlState::default()
        };
        assert!(state.aura_caps().is_none());
        let hue = state.hue_caps().unwrap();
        assert!(hue.paired);
        assert_eq!(hue.bridge.unwrap().ip, "192.168.1.20");
        assert!(hue.groups[0].chosen);
    }

    #[test]
    fn a_failed_hue_request_leaves_its_message_until_the_next_one() {
        let mut state = ControlState::default();
        state.apply(ControlEvent::HueBusy(true));
        state.apply(ControlEvent::Error("Press the link button".into()));
        state.apply(ControlEvent::HueBusy(false));
        assert_eq!(state.hue_message.as_deref(), Some("Press the link button"));
        assert_eq!(state.errors, 1);
        assert!(!state.hue_busy);
        // An error from anything else isn't the Hue card's.
        state.apply(ControlEvent::HueBusy(true));
        state.apply(ControlEvent::HueBusy(false));
        state.apply(ControlEvent::Error("save failed".into()));
        assert!(state.hue_message.is_none());
    }

    #[test]
    fn fan_caps_are_read_only_from_a_supported_fan_capability() {
        let details = serde_json::json!({
            "headers": [{"id": "/lpc/x/0/control/0", "label": "Fan #1", "min": 0.0, "max": 100.0}],
            "sources": ["cpu_package", "mb:System"],
        });
        let mut state = ControlState {
            capabilities: vec![CapabilitySet {
                domain: "fan".into(),
                supported: true,
                reason: None,
                problem: None,
                details: Some(details),
            }],
            ..ControlState::default()
        };
        let caps = state.fan_caps().unwrap();
        assert_eq!(caps.headers[0].label, "Fan #1");
        assert_eq!(caps.sources, vec!["cpu_package", "mb:System"]);

        state.capabilities[0].supported = false;
        assert!(state.fan_caps().is_none());
    }

    #[test]
    fn fan_duty_event_updates_state_and_repaints_only_on_change() {
        let (tx, rx) = std::sync::mpsc::channel();
        let ev: EventIn = serde_json::from_str(
            r#"{"event":"fan_duty","data":{"duty":{"/lpc/nct6799d/0/control/1":42.5}}}"#,
        )
        .unwrap();
        handle_event(ev, &tx, &std::env::temp_dir());

        let mut state = ControlState::default();
        let event = rx.try_recv().unwrap();
        assert!(state.apply(event.clone()));
        assert!((state.fan_duty["/lpc/nct6799d/0/control/1"] - 42.5).abs() < f64::EPSILON);
        assert!(!state.apply(event));
    }

    #[test]
    fn fan_identified_event_records_the_responders_per_header() {
        let (tx, rx) = std::sync::mpsc::channel();
        let ev: EventIn = serde_json::from_str(
            r#"{"event":"fan_identified","data":{"header":"/lpc/x/0/control/1","responders":[{"label":"Fan #2","before_rpm":996,"peak_rpm":2015},{"label":"Fan #5","before_rpm":1007,"peak_rpm":1988}]}}"#,
        )
        .unwrap();
        handle_event(ev, &tx, &std::env::temp_dir());

        let mut state = ControlState::default();
        assert!(state.apply(rx.try_recv().unwrap()));
        let responders = &state.fan_identified["/lpc/x/0/control/1"];
        assert_eq!(
            responders
                .iter()
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Fan #2", "Fan #5"]
        );
        assert!((responders[0].peak_rpm - 2015.0).abs() < f64::EPSILON);
    }

    #[test]
    fn header_for_fan_prefers_this_sessions_identify_then_the_stored_map() {
        let details = serde_json::json!({
            "headers": [
                {"id": "c0", "label": "Fan #1", "min": 0.0, "max": 100.0, "drives": ["Fan #1"]},
                {"id": "c1", "label": "Fan #2", "min": 0.0, "max": 100.0, "drives": ["Fan #2", "Fan #5"]},
                {"id": "c4", "label": "Fan #5", "min": 0.0, "max": 100.0},
            ],
        });
        let mut state = ControlState {
            capabilities: vec![CapabilitySet {
                domain: "fan".into(),
                supported: true,
                reason: None,
                problem: None,
                details: Some(details),
            }],
            ..ControlState::default()
        };
        assert!(state.has_fan_control());
        // Not "c4": channel 5's name matches, but channel 2 drives that fan.
        assert_eq!(state.header_for_fan("Fan #5").as_deref(), Some("c1"));
        assert_eq!(state.header_for_fan("Fan #7"), None);

        state.fan_identified.insert(
            "c4".into(),
            vec![FanResponder {
                label: "Fan #7".into(),
                before_rpm: 900.0,
                peak_rpm: 1800.0,
            }],
        );
        assert_eq!(state.header_for_fan("Fan #7").as_deref(), Some("c4"));
    }

    #[test]
    fn cpu_limit_part_matches_the_service_shape() {
        let json = r#"{"amd":{"ppt_w":88.0,"edc_a":150.0}}"#;
        let part: CpuLimitPart = serde_json::from_str(json).unwrap();
        let amd = part.amd.unwrap();
        assert_eq!(amd.ppt_w, Some(88.0));
        assert_eq!(amd.tdc_a, None);
        assert_eq!(serde_json::to_string(&part).unwrap(), json);

        // The built-ins' "BIOS values" part.
        let stock: CpuLimitPart = serde_json::from_str("{}").unwrap();
        assert_eq!(stock, CpuLimitPart::default());
        assert!(AmdCpuLimit::default().is_stock());
    }

    #[test]
    fn cpu_caps_parse_the_capability_details() {
        let details = serde_json::json!({
            "vendor": "amd", "generation": "Granite Ridge", "pm_table": "0x620105",
            "stock": {"ppt_w": 162.0, "tdc_a": 120.0, "edc_a": 180.0},
            "min": {"ppt_w": 45.0, "tdc_a": 30.0, "edc_a": 45.0},
            "current": {"ppt_w": 88.0, "tdc_a": 120.0, "edc_a": 180.0},
        });
        let state = ControlState {
            capabilities: vec![CapabilitySet {
                domain: "cpu_limit".into(),
                supported: true,
                reason: None,
                problem: None,
                details: Some(details),
            }],
            ..ControlState::default()
        };
        let caps = state.cpu_caps().unwrap();
        assert_eq!(caps.generation, "Granite Ridge");
        assert!((caps.stock.edc_a - 180.0).abs() < f64::EPSILON);
        assert!((caps.current.ppt_w - 88.0).abs() < f64::EPSILON);
    }

    #[test]
    fn active_ppt_limit_is_hidden_at_bios_values_and_after_a_crash_guard_skip() {
        let profile = |cpu_limit| Profile {
            id: "p".into(),
            name: "P".into(),
            icon: None,
            builtin: false,
            part: ProfilePart {
                cpu_limit,
                ..Default::default()
            },
        };
        let mut state = ControlState {
            profiles: vec![profile(Some(CpuLimitPart {
                amd: Some(AmdCpuLimit {
                    ppt_w: Some(88.0),
                    ..Default::default()
                }),
                intel: None,
            }))],
            active_profile: Some("p".into()),
            ..ControlState::default()
        };
        assert_eq!(state.active_ppt_limit(), Some(88.0));

        state.crash_guard_notice = Some("skipped".into());
        assert_eq!(state.active_ppt_limit(), None);

        state.crash_guard_notice = None;
        state.profiles = vec![profile(Some(CpuLimitPart::default()))];
        assert_eq!(state.active_ppt_limit(), None);
    }

    #[test]
    fn preview_ends_as_reverted_only_if_one_was_running() {
        let mut state = ControlState::default();
        state.apply(ControlEvent::PreviewEnded { kept: false });
        assert!(!state.preview_reverted);

        state.apply(ControlEvent::PreviewStarted {
            profile_id: "p".into(),
            revert_in: Duration::from_secs(15),
        });
        assert!(state.preview.is_some());
        state.apply(ControlEvent::PreviewEnded { kept: false });
        assert!(state.preview.is_none());
        assert!(state.preview_reverted);

        state.apply(ControlEvent::PreviewStarted {
            profile_id: "p".into(),
            revert_in: Duration::from_secs(15),
        });
        assert!(!state.preview_reverted);
        state.apply(ControlEvent::PreviewEnded { kept: true });
        assert!(state.preview.is_none()); // kept also ends it
        assert!(!state.preview_reverted);
    }

    #[test]
    fn preview_reverted_event_ends_the_preview() {
        let (tx, rx) = std::sync::mpsc::channel();
        let ev: EventIn =
            serde_json::from_str(r#"{"event":"preview_reverted","data":{"id":"p"}}"#).unwrap();
        handle_event(ev, &tx, &std::env::temp_dir());
        assert!(matches!(
            rx.try_recv(),
            Ok(ControlEvent::PreviewEnded { kept: false })
        ));
    }

    #[test]
    fn gpu_part_matches_the_service_shape() {
        let json = r#"{"adapters":{"PCI\\VEN_1002&DEV_7550":{"power_limit_pct":-15}}}"#;
        let part: GpuPart = serde_json::from_str(json).unwrap();
        let adapter = part.adapters.as_ref().unwrap()["PCI\\VEN_1002&DEV_7550"];
        assert_eq!(adapter.power_limit_pct, Some(-15));
        assert_eq!(serde_json::to_string(&part).unwrap(), json);
        assert_eq!(
            serde_json::from_str::<GpuPart>("{}").unwrap(),
            GpuPart::default()
        );
    }

    fn gpu_state(profile_pct: Option<i32>, original: i32) -> ControlState {
        let details = serde_json::json!({
            "adapters": [{
                "id": "pci-9070", "name": "AMD Radeon RX 9070 XT",
                "min": -30, "max": 10, "step": 1, "default": 0,
                "original": original, "current": profile_pct.unwrap_or(original),
            }],
        });
        let mut adapters = BTreeMap::new();
        adapters.insert(
            "pci-9070".to_owned(),
            GpuAdapterConfig {
                power_limit_pct: profile_pct,
            },
        );
        ControlState {
            capabilities: vec![CapabilitySet {
                domain: "gpu".into(),
                supported: true,
                reason: None,
                problem: None,
                details: Some(details),
            }],
            profiles: vec![Profile {
                id: "p".into(),
                name: "P".into(),
                icon: None,
                builtin: false,
                part: ProfilePart {
                    gpu: Some(GpuPart {
                        adapters: Some(adapters),
                    }),
                    ..Default::default()
                },
            }],
            active_profile: Some("p".into()),
            ..ControlState::default()
        }
    }

    #[test]
    fn gpu_caps_parse_the_capability_details() {
        let caps = gpu_state(None, 0).gpu_caps().unwrap();
        let adapter = &caps.adapters[0];
        assert_eq!(adapter.name, "AMD Radeon RX 9070 XT");
        assert_eq!((adapter.min, adapter.max, adapter.step), (-30, 10, 1));
    }

    #[test]
    fn active_gpu_limit_matches_the_panels_gpu_by_name() {
        let state = gpu_state(Some(-15), 0);
        // LHM and ADLX spell names differently; the panel uses LHM's.
        assert_eq!(state.active_gpu_limit("AMD Radeon RX 9070 XT"), Some(-15));
        assert_eq!(state.active_gpu_limit("AMD Radeon(TM) Graphics"), None);

        // No value, or the original value, is nothing worth showing.
        assert_eq!(
            gpu_state(None, 0).active_gpu_limit("AMD Radeon RX 9070 XT"),
            None
        );
        assert_eq!(
            gpu_state(Some(5), 5).active_gpu_limit("AMD Radeon RX 9070 XT"),
            None
        );
    }

    #[test]
    fn curve_opt_part_matches_the_service_shape() {
        let json = r#"{"all_core":-15,"per_core":{"2":-20}}"#;
        let part: CurveOptPart = serde_json::from_str(json).unwrap();
        assert_eq!(part.all_core, Some(-15));
        assert_eq!(part.per_core.as_ref().unwrap()["2"], -20);
        assert_eq!(serde_json::to_string(&part).unwrap(), json);
        assert!(!part.is_bios());

        assert!(serde_json::from_str::<CurveOptPart>("{}")
            .unwrap()
            .is_bios());
        let empty_map = CurveOptPart {
            all_core: None,
            per_core: Some(BTreeMap::new()),
        };
        assert!(empty_map.is_bios());
    }

    #[test]
    fn curve_opt_caps_parse_the_capability_details() {
        let details = serde_json::json!({
            "min": -30, "max": 0, "cores": 8, "per_core": true,
            "bios": [0, 0, 0, 0, 0, 0, 0, 0], "current": [-15, -15, -15, -15, -15, -15, -15, -15],
        });
        let state = ControlState {
            capabilities: vec![CapabilitySet {
                domain: "curve_opt".into(),
                supported: true,
                reason: None,
                problem: None,
                details: Some(details),
            }],
            ..ControlState::default()
        };
        let caps = state.curve_opt_caps().unwrap();
        assert_eq!((caps.min, caps.max, caps.cores), (-30, 0, 8));
        assert!(caps.per_core);
        assert_eq!(caps.current[0], -15);
    }

    #[test]
    fn power_schemes_come_from_the_power_plan_capability() {
        let state = ControlState {
            capabilities: vec![CapabilitySet {
                domain: "power_plan".into(),
                supported: true,
                reason: None,
                problem: None,
                details: Some(serde_json::json!({
                    "schemes": [{"id": "balanced", "name": "Balanced"}, {"id": "e9a42b02", "name": "Ultimate"}],
                })),
            }],
            ..ControlState::default()
        };
        let schemes = state.power_schemes();
        assert_eq!(schemes.len(), 2);
        assert_eq!(schemes[1].name, "Ultimate");
        assert!(ControlState::default().power_schemes().is_empty());
    }

    #[test]
    fn aura_part_matches_the_service_shape() {
        let json = r##"{"effect":"static","color":"#ff0033","brightness":0.8}"##;
        let part: AuraPart = serde_json::from_str(json).unwrap();
        assert_eq!(part.effect.as_deref(), Some("static"));
        assert_eq!(serde_json::to_string(&part).unwrap(), json);
    }

    #[test]
    fn a_lamp_only_part_matches_the_service_shape() {
        let json = r#"{"lamp":{"on":true,"brightness":0.5,"temperature":4000}}"#;
        let part: AuraPart = serde_json::from_str(json).unwrap();
        assert_eq!(part.effect, None);
        assert_eq!(part.lamp.as_ref().map(|l| l.on), Some(true));
        assert_eq!(serde_json::to_string(&part).unwrap(), json);
    }

    #[test]
    fn lighting_is_shown_when_supported_or_blocked_by_armoury_crate_only() {
        let cap =
            |supported: bool, reason: Option<&str>, details: serde_json::Value| CapabilitySet {
                domain: "aura".into(),
                supported,
                reason: reason.map(str::to_owned),
                problem: None,
                details: Some(details),
            };
        let supported = ControlState {
            capabilities: vec![cap(
                true,
                None,
                serde_json::json!({
                    "effects": ["off", "static"],
                    "devices": [{
                        "id": "aura-usb-19af", "name": "ASUS Aura motherboard controller", "kind": "motherboard",
                        "firmware": "AULA3-AR32-0304",
                        "zones": [{"id": "argb1", "name": "ARGB header 1", "addressable": true, "leds": 1}],
                    }, {
                        "id": "lamparray-0b05-1ace-2", "name": "ROG Harpe Ace Aim Lab Edition", "kind": "mouse",
                        "wdl_on": false, "wdl_toggle": true, "zones": [],
                    }],
                }),
            )],
            ..ControlState::default()
        };
        let caps = supported.aura_caps().unwrap();
        assert_eq!(caps.devices[0].kind, "motherboard");
        assert_eq!(caps.devices[0].zones[0].id, "argb1");
        // Absent = not offered; false = the device ignores lighting (#236).
        assert_eq!(caps.devices[0].wdl_on, None);
        assert_eq!(caps.devices[1].wdl_on, Some(false));
        assert_eq!(caps.devices[1].wdl_toggle, Some(true));
        assert_eq!(caps.devices[0].wdl_toggle, None);
        assert_eq!(supported.aura_unavailable(), None);

        let blocked = ControlState {
            capabilities: vec![cap(
                false,
                Some("Armoury Crate controls the lighting."),
                serde_json::json!({"conflict": true}),
            )],
            ..ControlState::default()
        };
        assert!(blocked.aura_caps().is_none());
        assert!(blocked
            .aura_unavailable()
            .unwrap()
            .contains("Armoury Crate"));

        // No controller at all: hidden, not explained.
        let absent = ControlState {
            capabilities: vec![cap(
                false,
                Some("No ASUS Aura USB controller found."),
                serde_json::Value::Null,
            )],
            ..ControlState::default()
        };
        assert_eq!(absent.aura_unavailable(), None);
    }

    #[test]
    fn safety_tripped_event_carries_the_reason() {
        let (tx, rx) = std::sync::mpsc::channel();
        let ev: EventIn =
            serde_json::from_str(r#"{"event":"safety_tripped","data":{"reason":"CPU 97°C"}}"#)
                .unwrap();
        handle_event(ev, &tx, &std::env::temp_dir());
        assert!(matches!(rx.try_recv(), Ok(ControlEvent::SafetyTripped(r)) if r == "CPU 97°C"));
    }
}
