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
    pub cpu_limit: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curve_opt: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<serde_json::Value>,
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
            ControlEvent::SafetyTripped(reason) => self.safety_tripped = Some(reason),
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
    fn safety_tripped_event_carries_the_reason() {
        let (tx, rx) = std::sync::mpsc::channel();
        let ev: EventIn =
            serde_json::from_str(r#"{"event":"safety_tripped","data":{"reason":"CPU 97°C"}}"#)
                .unwrap();
        handle_event(ev, &tx);
        assert!(matches!(rx.try_recv(), Ok(ControlEvent::SafetyTripped(r)) if r == "CPU 97°C"));
    }
}
