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
/// Generous — `apply_profile` can involve validate→snapshot→apply→verify
/// across several providers; `hello`/`capabilities`/etc. return far sooner.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
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
    pub aura: Option<serde_json::Value>,
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
    #[serde(default)]
    pub details: Option<serde_json::Value>,
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
    pub capabilities: Vec<CapabilitySet>,
    pub profiles: Vec<Profile>,
    pub active_profile: Option<String>,
    pub dry_run: bool,
    pub last_apply_result: Option<ApplyResult>,
    pub last_error: Option<String>,
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
    /// A running `preview`: which profile, and when the service reverts it.
    pub preview: Option<PreviewState>,
    /// Shown once a preview ended without "Keep" (timed out or undone).
    pub preview_reverted: bool,
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
                self.protocol_mismatch = None;
                self.last_error = None;
            }
            ControlEvent::Disconnected => self.connected = false,
            ControlEvent::ProtocolMismatch { expected, got } => {
                self.connected = false;
                self.protocol_mismatch = Some(ProtocolMismatch { expected, got });
            }
            ControlEvent::Capabilities(c) => self.capabilities = c,
            ControlEvent::Profiles(p) => self.profiles = p,
            ControlEvent::ActiveProfile(p) => self.active_profile = p,
            ControlEvent::DryRun(d) => self.dry_run = d,
            ControlEvent::ApplyResult(r) => self.last_apply_result = Some(r),
            ControlEvent::Error(e) => self.last_error = Some(e),
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
    PreviewStarted {
        profile_id: String,
        revert_in: Duration,
    },
    /// Kept (`confirm`), or reverted — by the user or by the service's timer.
    PreviewEnded {
        kept: bool,
    },
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
    /// Delete a custom profile; the service applies whichever profile becomes
    /// active if it was the active one.
    DeleteProfile(String),
    /// Put a built-in profile back to its defaults (re-applied when active).
    ResetProfile(String),
    Preview(Box<Profile>),
    ConfirmPreview {
        keep: bool,
    },
}

// ── The task ────────────────────────────────────────────────────────────

/// Owns the control pipe connection for the app's lifetime: connects,
/// performs the `hello` handshake, fetches initial state, then services
/// [`ControlCmd`]s from the UI and forwards server-pushed events — all
/// events funnel through `event_tx` as [`ControlEvent`]s. Reconnects with a
/// fixed backoff on any disconnect, except a protocol mismatch (doc: "the
/// UI disables control and says why" — retrying can't fix a version skew).
pub async fn control_task(
    mut cmd_rx: AsyncReceiver<ControlCmd>,
    event_tx: SyncSender<ControlEvent>,
    dir: PathBuf,
    app_version: String,
) {
    loop {
        let Some(client) = connect(&dir).await else {
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
            _ => {
                // Connect failure or an error response to hello — treat like
                // any other failed connection attempt.
                tokio::time::sleep(RECONNECT_BACKOFF).await;
                continue;
            }
        }

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

async fn connect(dir: &Path) -> Option<NamedPipeClient> {
    match ClientOptions::new().open(PIPE_NAME) {
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
async fn request(
    writer: &mut (impl AsyncWrite + Unpin),
    reader: &mut (impl AsyncBufRead + Unpin),
    next_id: &mut u64,
    method: &str,
    params: Option<serde_json::Value>,
    event_tx: &SyncSender<ControlEvent>,
    dir: &Path,
) -> Result<Option<serde_json::Value>, ()> {
    let id = *next_id;
    *next_id += 1;

    let mut line = serde_json::to_string(&RequestOut { id, method, params }).map_err(|_| ())?;
    line.push('\n');
    if writer.write_all(line.as_bytes()).await.is_err() {
        return Err(());
    }

    let deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            log_pipe_trouble_throttled(
                dir,
                &format!("control: '{method}' timed out waiting for a response"),
            );
            return Err(());
        }

        let mut buf = String::new();
        let read = tokio::time::timeout(remaining, reader.read_line(&mut buf)).await;
        match read {
            Ok(Ok(0)) => return Err(()), // EOF.
            Ok(Ok(_)) => {
                if buf.len() > MAX_PIPE_LINE_BYTES {
                    log_warn(
                        dir,
                        &format!(
                            "control: oversized frame ({} bytes) — dropping connection",
                            buf.len()
                        ),
                    );
                    return Err(());
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
                    Ok(IncomingLine::Event(ev)) => handle_event(ev, event_tx),
                    Err(e) => {
                        let preview = buf.trim().chars().take(120).collect::<String>();
                        log_error(
                            dir,
                            &format!("control: JSON parse error: {e} — raw: {preview}"),
                        );
                    }
                }
            }
            Ok(Err(_)) | Err(_) => return Err(()),
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
    }
}

async fn handle_cmd(
    cmd: ControlCmd,
    writer: &mut (impl AsyncWrite + Unpin),
    reader: &mut (impl AsyncBufRead + Unpin),
    next_id: &mut u64,
    event_tx: &SyncSender<ControlEvent>,
    dir: &Path,
) -> Result<(), ()> {
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
                        let _ = event_tx.send(ControlEvent::PreviewEnded { kept: false });
                    }
                    let _ = event_tx.send(ControlEvent::ApplyResult(r));
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
            let params = serde_json::to_value(&profile).map_err(|_| ())?;
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
                    let _ = event_tx.send(ControlEvent::ApplyResult(r));
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
                    let _ = event_tx.send(ControlEvent::ApplyResult(r));
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
    }
}

/// A line read outside of any pending `request()` call — i.e. genuinely
/// unsolicited. A response here (no matching request in flight) is logged
/// as a protocol oddity rather than silently dropped.
fn handle_pushed_line(text: &str, event_tx: &SyncSender<ControlEvent>, dir: &Path) {
    match serde_json::from_str::<IncomingLine>(text.trim()) {
        Ok(IncomingLine::Event(ev)) => handle_event(ev, event_tx),
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
fn handle_event(ev: EventIn, event_tx: &SyncSender<ControlEvent>) {
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
                    let _ = event_tx.send(ControlEvent::ApplyResult(result));
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
        handle_event(ev, &tx);

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
        handle_event(ev, &tx);

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
        handle_event(ev, &tx);
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
    fn safety_tripped_event_carries_the_reason() {
        let (tx, rx) = std::sync::mpsc::channel();
        let ev: EventIn =
            serde_json::from_str(r#"{"event":"safety_tripped","data":{"reason":"CPU 97°C"}}"#)
                .unwrap();
        handle_event(ev, &tx);
        assert!(matches!(rx.try_recv(), Ok(ControlEvent::SafetyTripped(r)) if r == "CPU 97°C"));
    }
}
