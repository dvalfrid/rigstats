//! Control Center window: everything a profile does, in one place. A
//! profile bar on top (chips to switch, + to copy, ⋯ to rename/delete/reset)
//! says which profile the rest belongs to; the sidebar lists its pages —
//! an Overview of tiles, the profile's RIGStats look (Dashboard, Overlay,
//! Alerts — `profile_look.rs`, #305) and its hardware (Power #187, Fans
//! #188, CPU #189, Graphics card, Lighting), hardware pages only when the
//! service reports the capability. Built from `ui_kit` and the dialog
//! contract in `src-egui/src/windows/CLAUDE.md`.
//! Actions are fire-and-forget `ControlCmd`s sent to `control_task`; results
//! come back later as `ControlEvent`s already folded into `control` by the
//! time this renders. The only local state is [`ControlUi`]: the page,
//! the selected fan header and unsaved drafts. CPU limit changes go through
//! `preview`: the service reverts them by itself unless "Keep" arrives in time.

mod cpu;
mod fans;
mod gpu;
mod headset;
mod keyboard;
mod lighting;
mod mouse;
mod overview;
mod power;
mod profile_bar;

use crate::lock_ext::LockSafe;
use crate::theme::{self, DialogColors};
use crate::windows::profile_look::{self, LookEditor, LookLink};
use crate::windows::ui_kit::{self, Icon};
use crate::windows::OpenRequests;
use cpu::{cpu_tab, curve_opt_card};
use fans::fans_tab;
use gpu::{gpu_tab, saved_gpu_limits};
use headset::headset_page;
use keyboard::keyboard_page;
use lighting::{lighting_tab, merge_preview, preview_delta};
use mouse::mouse_page;
use overview::overview_page;
use power::power_tab;
use profile_bar::profile_bar;
use rigstats_backend::control::{
    normalized_co, saved_cpu_limits, saved_curve_opt, saved_fan_headers, AmdCpuLimit, ApplyResult,
    AuraPart, ControlCmd, ControlState, CpuLimitPart, CurveOptPart, FanHeaderConfig, FanPart,
    GpuAdapterConfig, GpuPart, HeadsetEq, HeadsetSettings, KeyboardSettings, MouseSettings,
    Problem, Profile, Severity,
};
use rigstats_backend::lhm::Peripheral;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Footer buttons stay disabled at most this long waiting for the service
/// (its request timeout is 5 s).
const AWAIT_TIMEOUT: Duration = Duration::from_secs(6);

/// egui temp-data key carrying a Motherboard-panel fan label ("Fan #5") from
/// the main window to [`show`], which resolves it to the header driving it.
pub const SELECT_FAN_ID: &str = "control_select_fan";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Overview,
    Dashboard,
    Overlay,
    Alerts,
    Desktop,
    Power,
    Fans,
    Cpu,
    Gpu,
    Lighting,
    Headset,
    Mouse,
    Keyboard,
    Rules,
}

/// Unsaved edits to one profile's fan headers.
#[derive(Debug, Clone, PartialEq)]
struct FanDraft {
    profile_id: String,
    headers: BTreeMap<String, FanHeaderConfig>,
}

/// Unsaved edits to one profile's AMD limits; `None` = the BIOS value.
#[derive(Debug, Clone, PartialEq)]
struct CpuDraft {
    profile_id: String,
    limits: AmdCpuLimit,
}

/// What the profile list's action row is doing.
#[derive(Debug, Clone, PartialEq)]
enum ProfileEdit {
    Renaming { id: String, name: String },
    ConfirmDelete { id: String },
}

/// Unsaved lighting for one profile (`None` = leave the lights as they are).
#[derive(Debug, Clone, PartialEq)]
struct AuraDraft {
    profile_id: String,
    part: Option<AuraPart>,
}

/// Unsaved Curve Optimizer offsets for one profile.
#[derive(Debug, Clone, PartialEq)]
struct CoDraft {
    profile_id: String,
    part: CurveOptPart,
}

/// Unsaved GPU power limits per adapter id; a missing adapter = its original value.
#[derive(Debug, Clone, PartialEq)]
struct GpuDraft {
    profile_id: String,
    limits: BTreeMap<String, i32>,
}

/// Per-window UI state, owned by `RigStatsApp` across frames.
#[derive(Debug, Default)]
pub struct ControlUi {
    tab: Page,
    selected_header: Option<String>,
    draft: Option<FanDraft>,
    cpu_draft: Option<CpuDraft>,
    gpu_draft: Option<GpuDraft>,
    co_draft: Option<CoDraft>,
    aura_draft: Option<AuraDraft>,
    /// Live lighting preview: the latest unsent state and when the last
    /// one went out (a colour drag is throttled to ~10 Hz).
    aura_pending: Option<AuraPart>,
    aura_sent: Option<Instant>,
    /// Curve Optimizer per-core sliders shown (UI only).
    co_per_core: bool,
    /// An inline rename or delete confirmation in the profile list.
    profile_edit: Option<ProfileEdit>,
    /// The window was open last frame — opening it re-reads the service's
    /// capabilities (another app may have taken or released a device since).
    shown: bool,
    dragging: Option<usize>,
    identifying: Option<(String, Instant)>,
    /// Shown atop the Fans tab, e.g. when a clicked fan isn't mapped yet.
    notice: Option<String>,
    /// A `preview` was running last frame — to notice it ending.
    previewing: bool,
    /// A Try/Keep/Undo/Save click the service hasn't answered yet: the
    /// footer buttons stay disabled, so a second click can't land on the
    /// button that replaces the first one (Keep → Try) or confirm twice.
    awaiting: Option<Awaiting>,
    /// The Hue Bridge address typed in, for when the search finds nothing.
    hue_ip: String,
    /// `ControlState::errors` when the footer's error was dismissed.
    dismissed_errors: u32,
    /// The partly applied profile's message the footer no longer shows.
    dismissed_apply: Option<ApplyResult>,
    /// The Dashboard/Overlay/Alerts pages' edit (#305).
    look: LookEditor,
    /// When a page that saves at once (Power, App rules) last saved — the
    /// footer confirms it for a moment.
    saved_at: Option<(Instant, &'static str)>,
    /// A change that couldn't be queued for the service (#677) — the footer
    /// says so until it is dismissed or the next change goes out.
    unsent: bool,
    /// The App rules page's "Add a rule" form (#307).
    rule_form: crate::windows::rules_page::RulesPage,
    /// The Headset page asked the service for the headsets' settings since
    /// it was opened (#428).
    headsets_asked: bool,
    /// A headset's settings as just changed here, shown until the service's
    /// read-back arrives (a slider would otherwise jump back meanwhile).
    headset_pending: Option<(String, HeadsetSettings)>,
    /// A headset's equalizer as just changed here, until its read-back
    /// arrives (#483).
    eq_pending: Option<(String, HeadsetEq)>,
    /// The Control Center profile active when the headsets were last read.
    headsets_read_for: (Option<String>, u64),
    /// Which mouse, keyboard and headset the page shows when more than one
    /// is connected (#296) — one device at a time, picked in a row of tabs.
    mouse_tab: usize,
    /// When a silent keyboard's settings were last asked for again.
    keyboard_retry_at: Option<Instant>,
    keyboard_tab: usize,
    headset_tab: usize,
    /// The Mouse page asked for the mice's settings since it was opened (#431).
    mice_asked: bool,
    /// The Control Center profile active when the mice were last read — a
    /// switch can move the mouse to another onboard profile.
    mice_read_for: (Option<String>, u64),
    /// A mouse's settings as just changed here, until its read-back arrives.
    mouse_pending: Option<(String, MouseSettings)>,
    /// A change made while a slider, number or colour picker was held, not
    /// sent yet.
    mouse_unsent: bool,
    /// The DPI level being changed on a mouse whose level in use only its
    /// DPI button picks (#432).
    mouse_edit_stage: u8,
    /// The Keyboard page asked for the keyboards' energy settings since it
    /// was opened (#460).
    keyboards_asked: bool,
    /// A keyboard's settings as just changed here, until its read-back arrives.
    keyboard_pending: Option<(String, KeyboardSettings)>,
}

#[derive(Debug)]
struct Awaiting {
    since: Instant,
    /// `last_apply_result` at click time — a new one is an answer.
    result: Option<ApplyResult>,
}

impl ControlUi {
    /// Shows `page` the next time the window renders (a link from Settings).
    pub fn open_page(&mut self, page: Page) {
        self.tab = page;
    }
}

impl ControlUi {
    /// A Motherboard-panel fan was clicked: show the Fans tab on the header
    /// that drives it, or explain how to find it when it hasn't been
    /// identified yet.
    fn select_fan(&mut self, fan: &str, control: &ControlState) {
        self.tab = Page::Fans;
        self.dragging = None;
        match control.header_for_fan(fan) {
            Some(header) => {
                self.selected_header = Some(header);
                self.notice = None;
            }
            None => {
                self.notice = Some(format!(
                    "{fan} isn't linked to a fan channel yet. Run Identify on each channel \
                     below; the one that makes {fan} speed up drives it."
                ));
            }
        }
    }

    /// Follows a `preview` across frames; true when one started or ended.
    fn track_preview(&mut self, running: bool, reverted: bool) -> bool {
        if running == self.previewing {
            return false;
        }
        self.previewing = running;
        self.awaiting = None; // a preview starting or ending answers the click.
        if !running && reverted {
            self.cpu_draft = None;
            self.gpu_draft = None;
            self.co_draft = None;
        }
        true
    }

    /// Drops an unsaved lighting draft and puts the saved lighting back on
    /// the controller (the draft was shown live).
    fn discard_aura(
        &mut self,
        saved: Option<&AuraPart>,
        cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ) {
        self.aura_pending = None;
        if let Some(draft) = self.aura_draft.take() {
            // Only what the draft changed goes back — a lamp switched from
            // the tray stays as it is when only the colour was tried.
            if let Some(back) = saved.and_then(|s| preview_delta(draft.part.as_ref(), s)) {
                let _ = cmd_tx.try_send(ControlCmd::AuraPreview(back));
            }
        }
    }

    /// Queues a live preview of what changed from `before` to `after`,
    /// merged with one still waiting for the throttle.
    fn queue_aura_preview(&mut self, before: Option<&AuraPart>, after: &AuraPart) {
        if let Some(delta) = preview_delta(before, after) {
            self.aura_pending = Some(match self.aura_pending.take() {
                Some(waiting) => merge_preview(waiting, delta),
                None => delta,
            });
        }
    }

    /// Sends the pending live preview, at most every 100 ms. Returns
    /// whether one is still waiting (the caller keeps repainting).
    fn flush_aura_preview(&mut self, cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>) -> bool {
        let Some(part) = self.aura_pending.clone() else {
            return false;
        };
        if self
            .aura_sent
            .is_some_and(|t| t.elapsed() < Duration::from_millis(100))
        {
            return true;
        }
        let _ = cmd_tx.try_send(ControlCmd::AuraPreview(part));
        self.aura_pending = None;
        self.aura_sent = Some(Instant::now());
        false
    }

    /// CPU and GPU limit edits are tried first (preview, auto-revert).
    fn has_limit_draft(&self) -> bool {
        self.cpu_draft.is_some() || self.gpu_draft.is_some() || self.co_draft.is_some()
    }

    /// Queues a command that changes something. The queue is short and the
    /// control task doesn't read it while it reconnects or waits for an
    /// answer, so a full one would drop a save without a word (#677): the
    /// footer then says it wasn't sent. Reads and live previews stay
    /// best-effort `try_send`s. Returns whether it was queued.
    fn send(&mut self, cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>, cmd: ControlCmd) -> bool {
        let sent = cmd_tx.try_send(cmd).is_ok();
        self.unsent = !sent;
        sent
    }

    fn await_reply(&mut self, control: &ControlState) {
        self.awaiting = Some(Awaiting {
            since: Instant::now(),
            result: control.last_apply_result.clone(),
        });
    }

    /// Whether a click is still unanswered. A new apply result answers it;
    /// the request timeout bounds it if no answer ever comes.
    fn busy(&mut self, control: &ControlState) -> bool {
        let answered = self.awaiting.as_ref().is_some_and(|a| {
            a.result != control.last_apply_result || a.since.elapsed() > AWAIT_TIMEOUT
        });
        if answered {
            self.awaiting = None;
        }
        self.awaiting.is_some()
    }

    /// The headers being edited, creating the draft from `saved` on first
    /// edit.
    fn edit(
        &mut self,
        profile_id: &str,
        saved: &BTreeMap<String, FanHeaderConfig>,
    ) -> &mut BTreeMap<String, FanHeaderConfig> {
        &mut self
            .draft
            .get_or_insert_with(|| FanDraft {
                profile_id: profile_id.to_owned(),
                headers: saved.clone(),
            })
            .headers
    }
}

/// A card for the hardware pages' free-form content — the shared card
/// with room for sliders and editors.
fn card_frame(dc: &DialogColors) -> egui::Frame {
    ui_kit::card_frame(dc).inner_margin(egui::Margin::symmetric(16, 14))
}

fn section_label(ui: &mut egui::Ui, dc: &DialogColors, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(12.0)
            .strong()
            .color(dc.label),
    );
}

/// The active profile with every unsaved draft folded in.
fn with_drafts(profile: &Profile, ui_state: &ControlUi) -> Profile {
    let mut profile = profile.clone();
    if let Some(draft) = &ui_state.draft {
        profile.part.fan = Some(FanPart {
            headers: Some(draft.headers.clone()),
        });
    }
    if let Some(draft) = &ui_state.cpu_draft {
        let intel = profile.part.cpu_limit.and_then(|c| c.intel);
        profile.part.cpu_limit = Some(CpuLimitPart {
            amd: (!draft.limits.is_stock()).then_some(draft.limits),
            intel,
        });
    }
    if let Some(draft) = &ui_state.co_draft {
        profile.part.curve_opt = Some(draft.part.clone());
    }
    if let Some(draft) = &ui_state.aura_draft {
        profile.part.aura = draft.part.clone();
    }
    if let Some(draft) = &ui_state.gpu_draft {
        let adapters: BTreeMap<String, GpuAdapterConfig> = draft
            .limits
            .iter()
            .map(|(id, &pct)| {
                (
                    id.clone(),
                    GpuAdapterConfig {
                        power_limit_pct: Some(pct),
                    },
                )
            })
            .collect();
        profile.part.gpu = Some(GpuPart {
            adapters: (!adapters.is_empty()).then_some(adapters),
        });
    }
    profile
}

/// A problem and its buttons, the same in every window (#370):
/// `ui_kit::notice` shows it, `run_problem_action` does what was clicked.
fn problem_notice(ui: &mut egui::Ui, dc: &DialogColors, problem: &Problem, open: &OpenRequests) {
    if let Some(action) = ui_kit::notice(ui, dc, problem) {
        crate::windows::run_problem_action(ui.ctx(), &action, open);
    }
}

/// Why the Control Center can't change anything, when it's a problem rather
/// than a moment of connecting: refused (not the installed RIGStats), or the
/// app and the service are different versions.
fn connection_problem(control: &ControlState) -> Option<Problem> {
    match (&control.protocol_mismatch, control.refused) {
        (Some(mismatch), _) => Some(crate::problems::version_mismatch(mismatch)),
        (None, true) => Some(crate::problems::service_refused()),
        (None, false) => None,
    }
}

/// Why lighting is unavailable, when it failed rather than being absent: the
/// service's problem, or — from an older service that only flags the
/// conflict — one built from its reason.
fn lighting_problem(control: &ControlState) -> Option<Problem> {
    control.domain_problem("aura").cloned().or_else(|| {
        control.aura_unavailable().map(|reason| Problem {
            id: "aura_conflict".into(),
            severity: Severity::Attention,
            title: "Another program controls the lighting".into(),
            message: reason,
            ..Default::default()
        })
    })
}

/// A wireless device's charge (coloured by the battery alert levels, a
/// triangle while charging) and how it is connected — greyed with "Not
/// connected" while it shows its last reading (#351). For a row's
/// right-to-left layout.
fn battery_and_link(ui: &mut egui::Ui, dc: &DialogColors, p: &Peripheral, (warn, crit): (u8, u8)) {
    use crate::panels::peripherals::{link_label, link_of, paint_battery_glyph, paint_link_icon};
    let color = if p.connected {
        crate::panels::battery::charge_color(p.battery, p.charging, warn, crit)
    } else {
        dc.muted
    };
    ui.add_sized(
        [40.0, 20.0],
        egui::Label::new(
            egui::RichText::new(format!("{} %", p.battery))
                .size(12.0)
                .color(color),
        ),
    );
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 13.0), egui::Sense::hover());
    paint_battery_glyph(ui.painter(), rect, p.battery, color, dc.muted);
    if p.charging {
        resp.on_hover_text("Charging");
    }
    if let Some(link) = link_of(&p.connection) {
        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(if p.connected {
                link_label(link)
            } else {
                "Not connected"
            })
            .size(12.0)
            .color(dc.muted),
        );
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        paint_link_icon(ui.painter(), rect.center(), 12.0, link, dc.muted);
    }
}

/// The battery alert levels (warn, crit) of the profile being looked at.
fn battery_levels(ui_state: &ControlUi) -> (u8, u8) {
    ui_state
        .look
        .current()
        .and_then(|l| l.thresholds.get("battery").cloned())
        .map_or((20, 10), |t| (t.warn.unwrap_or(20), t.crit.unwrap_or(10)))
}

/// A row of tabs, one per device, when a page has more than one (#296):
/// the page then shows the picked one only, not every device in one long
/// list. Returns the picked index, kept inside the list.
fn device_tabs<'a>(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    names: impl Iterator<Item = &'a str>,
    picked: &mut usize,
) -> usize {
    let names: Vec<&str> = names.collect();
    *picked = (*picked).min(names.len().saturating_sub(1));
    if names.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            for (i, name) in names.iter().enumerate() {
                if ui_kit::chip(ui, dc, name, i == *picked).clicked() {
                    *picked = i;
                }
            }
        });
        ui.add_space(12.0);
    }
    *picked
}

pub fn show(
    ui: &mut egui::Ui,
    main_ctx: &egui::Context,
    open: &Arc<AtomicBool>,
    needs_focus: &Arc<AtomicBool>,
    control: &ControlState,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    dc: &DialogColors,
    ui_state: &mut ControlUi,
    look_link: &LookLink,
    peripherals: &[Peripheral],
) {
    let ctx = &ui.ctx().clone();
    dc.apply_to_ctx(ctx);
    if !ui_state.shown {
        ui_state.shown = true;
        let _ = cmd_tx.try_send(ControlCmd::Refresh);
    }
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
    ui_state
        .look
        .sync(control.active_profile.as_deref(), look_link);

    let fan_caps = control.fan_caps();
    if let Some(fan) = ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::unique(SELECT_FAN_ID))) {
        ui_state.select_fan(&fan, control);
    }
    let cpu_caps = control.cpu_caps();
    let co_caps = control.curve_opt_caps();
    let aura_caps = control.aura_caps();
    let aura_blocked = control.aura_unavailable();
    let hue_caps = control.hue_caps();
    let lighting_shown = aura_caps.is_some()
        || aura_blocked.is_some()
        || hue_caps.is_some()
        || control.domain_problem("aura").is_some();
    // A domain that failed keeps its page, to say so (#373); one this PC
    // doesn't have stays hidden.
    let fan_problem = control.domain_problem("fan").cloned();
    let cpu_problem = control
        .domain_problem("cpu_limit")
        .or_else(|| control.domain_problem("curve_opt"))
        .cloned();
    let gpu_caps = control.gpu_caps();
    // A GPU domain that failed (the driver crashed, #370) keeps its page, to
    // say so; one this PC doesn't have stays hidden.
    let gpu_problem = control.domain_problem("gpu").cloned();
    // The Headset page reads the headsets each time it is opened (#428).
    if ui_state.tab != Page::Mouse {
        ui_state.mice_asked = false;
    }
    if ui_state.tab != Page::Headset {
        ui_state.headsets_asked = false;
    }
    if ui_state.tab != Page::Keyboard {
        ui_state.keyboards_asked = false;
    }
    match ui_state.tab {
        Page::Fans if fan_caps.is_none() && fan_problem.is_none() => ui_state.tab = Page::Overview,
        Page::Cpu if cpu_caps.is_none() && co_caps.is_none() && cpu_problem.is_none() => {
            ui_state.tab = Page::Overview
        }
        Page::Gpu if gpu_caps.is_none() && gpu_problem.is_none() => ui_state.tab = Page::Overview,
        Page::Lighting if !lighting_shown => ui_state.tab = Page::Overview,
        Page::Headset if !control.has_headset() => ui_state.tab = Page::Overview,
        Page::Mouse if !control.has_mouse() => ui_state.tab = Page::Overview,
        Page::Keyboard if control.keyboards().is_empty() => ui_state.tab = Page::Overview,
        _ => {}
    }
    let active = control.active();
    let saved_headers = saved_fan_headers(active);
    let saved_cpu = saved_cpu_limits(active);
    let saved_gpu = saved_gpu_limits(active);
    let saved_co = saved_curve_opt(active);
    let saved_aura = active.and_then(|p| p.part.aura.clone());
    // A draft belongs to one profile; switching profiles (or the save
    // landing, which makes the draft equal what's stored) ends it.
    if let Some(draft) = &ui_state.draft {
        let other_profile = Some(draft.profile_id.as_str()) != control.active_profile.as_deref();
        let saved = draft.headers == saved_headers && ui_state.dragging.is_none();
        if other_profile || saved {
            ui_state.draft = None;
        }
    }
    if let Some(draft) = &ui_state.cpu_draft {
        let other_profile = Some(draft.profile_id.as_str()) != control.active_profile.as_deref();
        if other_profile || draft.limits == saved_cpu {
            ui_state.cpu_draft = None;
        }
    }
    if let Some(draft) = &ui_state.gpu_draft {
        let other_profile = Some(draft.profile_id.as_str()) != control.active_profile.as_deref();
        if other_profile || draft.limits == saved_gpu {
            ui_state.gpu_draft = None;
        }
    }
    if let Some(draft) = &ui_state.co_draft {
        let other_profile = Some(draft.profile_id.as_str()) != control.active_profile.as_deref();
        if other_profile || normalized_co(&draft.part) == saved_co {
            ui_state.co_draft = None;
        }
    }
    // A try just started or ended: the limits in force changed, so re-read
    // them ("In force now"). Ended without Keep: drop the CPU draft so the
    // sliders show what is in force again, not the values that were undone.
    if ui_state.track_preview(control.preview.is_some(), control.preview_reverted) {
        let _ = cmd_tx.try_send(ControlCmd::Refresh);
    }
    if let Some(draft) = &ui_state.aura_draft {
        let other_profile = Some(draft.profile_id.as_str()) != control.active_profile.as_deref();
        if other_profile || draft.part == saved_aura {
            ui_state.aura_draft = None;
        }
    }
    if ui_state.flush_aura_preview(cmd_tx) {
        ctx.request_repaint_after(Duration::from_millis(100));
    }
    let hardware_dirty =
        ui_state.draft.is_some() || ui_state.has_limit_draft() || ui_state.aura_draft.is_some();
    let dirty = hardware_dirty || ui_state.look.dirty();
    // The countdown needs a repaint every second without user input.
    let preview_left = control
        .preview
        .as_ref()
        .map(|p| p.reverts_at.saturating_duration_since(Instant::now()));
    let busy = ui_state.busy(control);
    if preview_left.is_some() || busy {
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    // ── Hero ─────────────────────────────────────────────────────────────
    ui_kit::hero(ui, dc, "control", "Control Center", |ui| {
        use crate::problems::{STATE_CONNECTED, STATE_CONNECTING, STATE_MISMATCH, STATE_REFUSED};
        let (dot, text) = if control.protocol_mismatch.is_some() {
            (ui_kit::C_BAD, STATE_MISMATCH)
        } else if control.connected {
            (ui_kit::C_GOOD, STATE_CONNECTED)
        } else if control.refused {
            (ui_kit::C_BAD, STATE_REFUSED)
        } else {
            (ui_kit::C_ATTENTION, STATE_CONNECTING)
        };
        ui.label(egui::RichText::new(text).size(11.0).color(dc.muted));
        // Painted dot: the embedded font has no bullet glyph.
        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 11.0), egui::Sense::hover());
        ui.painter().circle_filled(dot_rect.center(), 3.5, dot);
    });

    // ── Profile bar: which profile everything below belongs to ───────────
    egui::Panel::top("control_profiles")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 10,
            bottom: 10,
        }))
        .show_separator_line(true)
        .show(ui, |ui| {
            profile_bar(ui, dc, control, ui_state, cmd_tx, busy);
        });

    // ── Footer ───────────────────────────────────────────────────────────
    egui::Panel::bottom("control_footer")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 10,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ui, |ui| {
            // The last apply: failed (red) or partly applied (amber), in the
            // service's words — the same colours as every problem (#373).
            // Until dismissed: a partly applied profile stayed until the next
            // switch, even after its sleeping keyboard woke (#296).
            if let Some(result) = control
                .last_apply_result
                .as_ref()
                .filter(|r| ui_state.dismissed_apply.as_ref() != Some(*r))
            {
                match (result.ok, result.message.as_deref()) {
                    (false, message) => {
                        ui_kit::row_problem(
                            ui,
                            dc,
                            Severity::Error,
                            message.unwrap_or("Apply failed."),
                        );
                        ui.add_space(6.0);
                    }
                    (true, Some(message)) => {
                        if ui_kit::row_problem_dismiss(ui, dc, Severity::Attention, message) {
                            ui_state.dismissed_apply = Some(result.clone());
                        }
                        ui.add_space(6.0);
                    }
                    (true, None) => {}
                }
            }
            if ui_state.unsent
                && ui_kit::row_problem_dismiss(
                    ui,
                    dc,
                    Severity::Attention,
                    crate::problems::CHANGE_NOT_SENT,
                )
            {
                ui_state.unsent = false;
            }
            if ui_state.unsent {
                ui.add_space(6.0);
            }
            // A request the service turned down (save, delete, reset, ...) —
            // shown until dismissed; the Hue card shows its own (#219).
            if let Some(error) = control.last_error.as_ref().filter(|e| {
                control.errors != ui_state.dismissed_errors
                    && control.hue_message.as_ref() != Some(*e)
            }) {
                let title = crate::problems::service_error(error).title;
                if ui_kit::row_problem_dismiss(
                    ui,
                    dc,
                    Severity::Attention,
                    &format!("{title}: {error}"),
                ) {
                    ui_state.dismissed_errors = control.errors;
                }
                ui.add_space(6.0);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::dialog_btn_primary(ui, "Close").clicked() {
                    ui_state.discard_aura(saved_aura.as_ref(), cmd_tx);
                    ui_state.look.discard(look_link);
                    ui_state.shown = false;
                    open.store(false, Ordering::Relaxed);
                    ui_state.notice = None;
                    main_ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
                if busy {
                    ui.disable();
                }
                if let Some(left) = preview_left {
                    ui.add_space(6.0);
                    if theme::dialog_btn_primary(ui, "Keep").clicked()
                        && ui_state.send(cmd_tx, ControlCmd::ConfirmPreview { keep: true })
                    {
                        ui_state.await_reply(control);
                    }
                    ui.add_space(6.0);
                    if theme::dialog_btn_secondary(ui, "Undo", dc).clicked()
                        && ui_state.send(cmd_tx, ControlCmd::ConfirmPreview { keep: false })
                    {
                        ui_state.await_reply(control);
                    }
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "Keep the new limits? Reverting in {} s",
                            left.as_secs_f32().ceil()
                        ))
                        .size(11.0)
                        .color(dc.text),
                    );
                } else if dirty {
                    ui.add_space(6.0);
                    // CPU limits are tried first (auto-revert); everything
                    // else is saved directly.
                    let label = if ui_state.has_limit_draft() {
                        "Try new limits"
                    } else {
                        "Save"
                    };
                    if theme::dialog_btn_primary(ui, label).clicked() {
                        ui_state.look.save(look_link);
                        if hardware_dirty {
                            if let Some(profile) = active {
                                let profile = with_drafts(profile, ui_state);
                                let cmd = if ui_state.has_limit_draft() {
                                    ControlCmd::Preview(Box::new(profile))
                                } else {
                                    ControlCmd::SaveProfile {
                                        profile: Box::new(profile),
                                        apply: true,
                                    }
                                };
                                if ui_state.send(cmd_tx, cmd) {
                                    ui_state.await_reply(control);
                                }
                            }
                        }
                    }
                    ui.add_space(6.0);
                    if theme::dialog_btn_secondary(ui, "Revert", dc).clicked() {
                        ui_state.draft = None;
                        ui_state.cpu_draft = None;
                        ui_state.gpu_draft = None;
                        ui_state.co_draft = None;
                        ui_state.discard_aura(saved_aura.as_ref(), cmd_tx);
                        ui_state.look.discard(look_link);
                        ui_state.dragging = None;
                    }
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "Unsaved changes to {}",
                            active.map_or("this profile", |p| p.name.as_str())
                        ))
                        .size(11.0)
                        .color(dc.muted),
                    );
                } else if let Some((_, text)) = ui_state
                    .saved_at
                    .filter(|(t, _)| t.elapsed() < Duration::from_secs(3))
                {
                    ui.add_space(10.0);
                    ui_kit::status(ui, ui_kit::C_GOOD, text);
                    ui.ctx().request_repaint_after(Duration::from_millis(500));
                } else if control.preview_reverted {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("The new limits were not kept and have been reverted.")
                            .size(11.0)
                            .color(dc.muted),
                    );
                }
            });
        });

    // ── Sidebar: the profile's pages ─────────────────────────────────────
    let cpu_tab_shown = cpu_caps.is_some() || co_caps.is_some() || cpu_problem.is_some();
    let shown = PageShown {
        fans: fan_caps.is_some() || fan_problem.is_some(),
        cpu: cpu_tab_shown,
        gpu: gpu_caps.is_some() || gpu_problem.is_some(),
        lighting: lighting_shown,
        headset: control.has_headset(),
        mouse: control.has_mouse(),
        keyboard: !control.keyboards().is_empty(),
    };
    egui::Panel::left("control_nav")
        .resizable(false)
        .exact_size(ui_kit::SIDEBAR_W)
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin::same(10)))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                for &(page, icon, label) in PAGES {
                    if !shown.has(page) {
                        continue;
                    }
                    if let Some(header) = nav_section(page) {
                        ui_kit::nav_header(ui, dc, header);
                    }
                    if ui_kit::nav_item(ui, dc, icon, label, ui_state.tab == page).clicked() {
                        ui_state.tab = page;
                    }
                }
            });
        });

    // ── The page ─────────────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: 18,
            bottom: 8,
        }))
        .show(ui, |ui| {
            // Why nothing can be changed — refused, or another version — above
            // every page (#373). Still connecting is normal for a moment.
            if let Some(problem) = connection_problem(control) {
                problem_notice(ui, dc, &problem, look_link.open);
            }
            // Profiles restored from a backup: above every page.
            if let Some(problem) = control.profiles_problem() {
                problem_notice(ui, dc, &problem, look_link.open);
            }
            let profile_name = active.map_or("this profile", |p| p.name.as_str());
            // The fan curve editor sizes itself to the page: no scrolling.
            if let (Page::Fans, Some(caps)) = (ui_state.tab, &fan_caps) {
                ui_kit::page_header(
                    ui,
                    dc,
                    "Fans",
                    &format!("How fast each fan runs in {profile_name}."),
                );
                fans_tab(
                    ui,
                    dc,
                    control,
                    caps,
                    active,
                    &saved_headers,
                    ui_state,
                    cmd_tx,
                );
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match ui_state.tab {
                    Page::Overview => overview_page(
                        ui,
                        dc,
                        control,
                        active,
                        &shown,
                        &saved_headers,
                        &saved_cpu,
                        &saved_co,
                        &saved_gpu,
                        saved_aura.as_ref(),
                        ui_state,
                        &look_link.settings.lock_safe().app_rules.clone(),
                    ),
                    Page::Dashboard => profile_look::dashboard_page(
                        ui,
                        dc,
                        profile_name,
                        &mut ui_state.look,
                        look_link,
                    ),
                    Page::Overlay => profile_look::overlay_page(
                        ui,
                        dc,
                        profile_name,
                        &mut ui_state.look,
                        look_link,
                    ),
                    Page::Desktop => profile_look::desktop_page(
                        ui,
                        dc,
                        profile_name,
                        &mut ui_state.look,
                        look_link,
                    ),
                    Page::Alerts => profile_look::alerts_page(
                        ui,
                        dc,
                        profile_name,
                        &mut ui_state.look,
                        look_link,
                    ),
                    Page::Cpu => {
                        ui_kit::page_header(
                            ui,
                            dc,
                            "CPU",
                            &format!("The processor's power limits in {profile_name}."),
                        );
                        // CPU limits kept at BIOS values after a crash: on its own
                        // above the CPU cards, whichever of them this CPU has.
                        if let Some(problem) = control.crash_guard_problem() {
                            problem_notice(ui, dc, &problem, look_link.open);
                        }
                        if let Some(caps) = &cpu_caps {
                            cpu_tab(ui, dc, control, caps, &saved_cpu, ui_state);
                            ui.add_space(10.0);
                        } else if let Some(problem) = control.domain_problem("cpu_limit") {
                            problem_notice(ui, dc, problem, look_link.open);
                        }
                        if let Some(caps) = &co_caps {
                            curve_opt_card(ui, dc, control, caps, &saved_co, ui_state);
                        } else if let Some(problem) = control.domain_problem("curve_opt") {
                            problem_notice(ui, dc, problem, look_link.open);
                        }
                    }
                    Page::Lighting => {
                        ui_kit::page_header(
                            ui,
                            dc,
                            "Lighting",
                            &format!("The RGB lighting of your devices in {profile_name}."),
                        );
                        lighting_tab(
                            ui,
                            dc,
                            control,
                            aura_caps.as_ref(),
                            saved_aura.as_ref(),
                            cmd_tx,
                            ui_state,
                            peripherals,
                            look_link.open,
                        );
                    }
                    Page::Keyboard => {
                        ui_kit::page_header(
                            ui,
                            dc,
                            "Keyboard",
                            "Which of the keyboard's own profiles this profile uses. Saved at once.",
                        );
                        keyboard_page(ui, dc, control, cmd_tx, ui_state);
                    }
                    Page::Mouse => {
                        ui_kit::page_header(
                            ui,
                            dc,
                            "Mouse",
                            "Settings kept in the mouse itself. Changes are sent at once.",
                        );
                        mouse_page(
                            ui,
                            dc,
                            control,
                            cmd_tx,
                            ui_state,
                            peripherals,
                            battery_levels(ui_state),
                        );
                    }
                    Page::Headset => {
                        ui_kit::page_header(
                            ui,
                            dc,
                            "Headset",
                            "Settings kept in the headset itself, for every profile. Changes are sent at once.",
                        );
                        headset_page(ui, dc, control, cmd_tx, ui_state, peripherals, battery_levels(ui_state));
                    }
                    Page::Gpu => {
                        ui_kit::page_header(
                            ui,
                            dc,
                            "Graphics card",
                            &format!("The graphics card's power limit in {profile_name}."),
                        );
                        if let Some(problem) = &gpu_problem {
                            problem_notice(ui, dc, problem, look_link.open);
                        } else if let Some(caps) = &gpu_caps {
                            gpu_tab(ui, dc, control, caps, &saved_gpu, ui_state);
                        }
                    }
                    Page::Rules => {
                        if crate::windows::rules_page::rules_page(
                            ui,
                            dc,
                            &control.profiles,
                            look_link,
                            &mut ui_state.rule_form,
                        ) {
                            ui_state.saved_at = Some((Instant::now(), "Saved"));
                        }
                    }
                    // Here only when the fan domain failed: the curve editor
                    // takes the page above otherwise.
                    Page::Fans => {
                        ui_kit::page_header(
                            ui,
                            dc,
                            "Fans",
                            &format!("How fast each fan runs in {profile_name}."),
                        );
                        if let Some(problem) = &fan_problem {
                            problem_notice(ui, dc, problem, look_link.open);
                        }
                    }
                    Page::Power => {
                        ui_kit::page_header(
                            ui,
                            dc,
                            "Power",
                            &format!(
                                "The Windows power plan in {profile_name}. Changes are saved at once."
                            ),
                        );
                        if power_tab(ui, dc, control, cmd_tx, ui_state, look_link.open) {
                            ui_state.saved_at = Some((Instant::now(), "Saved"));
                        }
                    }
                });
        });

    if ctx.input(|i| i.viewport().close_requested()) {
        ui_state.discard_aura(saved_aura.as_ref(), cmd_tx);
        ui_state.look.discard(look_link);
        ui_state.shown = false;
        open.store(false, Ordering::Relaxed);
        ui_state.notice = None;
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// The Control Center's pages, in sidebar order: the overview, the
/// profile's RIGStats look, its Windows desktop, its hardware, then the
/// app rules.
const PAGES: &[(Page, Icon, &str)] = &[
    (Page::Overview, Icon::Overview, "Overview"),
    (Page::Dashboard, Icon::Dashboard, "Dashboard"),
    (Page::Overlay, Icon::Overlay, "Overlay"),
    (Page::Alerts, Icon::Alerts, "Alerts"),
    (Page::Desktop, Icon::Desktop, "Desktop"),
    (Page::Power, Icon::Power, "Power"),
    (Page::Fans, Icon::Fans, "Fans"),
    (Page::Cpu, Icon::Cpu, "CPU"),
    (Page::Gpu, Icon::Gpu, "Graphics card"),
    (Page::Lighting, Icon::Lighting, "Lighting"),
    (Page::Headset, Icon::Headset, "Headset"),
    (Page::Mouse, Icon::Mouse, "Mouse"),
    (Page::Keyboard, Icon::Keyboard, "Keyboard"),
    (Page::Rules, Icon::Rules, "App rules"),
];

/// The sidebar section a page starts, if it starts one. The Overview's
/// tile groups use the same names.
fn nav_section(page: Page) -> Option<&'static str> {
    match page {
        Page::Dashboard => Some("RIGStats"),
        Page::Desktop => Some("Windows"),
        Page::Power => Some("Hardware"),
        Page::Rules => Some("Automation"),
        _ => None,
    }
}

/// Which hardware pages this PC has (the service's capabilities);
/// unsupported features are hidden, not greyed out.
struct PageShown {
    fans: bool,
    cpu: bool,
    gpu: bool,
    lighting: bool,
    headset: bool,
    mouse: bool,
    keyboard: bool,
}

impl PageShown {
    fn has(&self, page: Page) -> bool {
        match page {
            Page::Fans => self.fans,
            Page::Cpu => self.cpu,
            Page::Gpu => self.gpu,
            Page::Lighting => self.lighting,
            Page::Headset => self.headset,
            Page::Mouse => self.mouse,
            Page::Keyboard => self.keyboard,
            _ => true,
        }
    }
}

fn dry_run_note(ui: &mut egui::Ui, dc: &DialogColors, control: &ControlState) {
    if control.dry_run {
        ui.add_space(6.0);
        ui_kit::notice(ui, dc, &crate::problems::dry_run());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #677: a change the full queue can't take is reported, not dropped
    /// silently, and the next one that goes out clears the message.
    #[test]
    fn a_change_the_full_queue_cannot_take_is_reported() {
        let mut ui = ControlUi::default();
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        assert!(ui.send(&tx, ControlCmd::Refresh));
        assert!(!ui.unsent);
        assert!(!ui.send(&tx, ControlCmd::DeleteProfile("custom".into())));
        assert!(ui.unsent, "the footer says it wasn't sent");

        rx.try_recv().unwrap();
        assert!(ui.send(&tx, ControlCmd::DeleteProfile("custom".into())));
        assert!(!ui.unsent);
    }

    #[test]
    fn with_drafts_folds_cpu_limits_into_the_profile_and_keeps_intel() {
        let profile = Profile {
            id: "gaming".into(),
            name: "Gaming".into(),
            icon: None,
            builtin: true,
            part: rigstats_backend::control::ProfilePart {
                cpu_limit: Some(CpuLimitPart {
                    amd: None,
                    intel: Some(serde_json::json!({ "pl1_w": 125 })),
                }),
                ..Default::default()
            },
        };
        let mut ui = ControlUi {
            cpu_draft: Some(CpuDraft {
                profile_id: "gaming".into(),
                limits: AmdCpuLimit {
                    ppt_w: Some(88.0),
                    ..Default::default()
                },
            }),
            ..Default::default()
        };
        let merged = with_drafts(&profile, &ui).part.cpu_limit.unwrap();
        assert_eq!(merged.amd.unwrap().ppt_w, Some(88.0));
        assert!(merged.intel.is_some());

        // Back to BIOS values: an empty amd part, not a stored set of maxima.
        ui.cpu_draft.as_mut().unwrap().limits = AmdCpuLimit::default();
        assert_eq!(with_drafts(&profile, &ui).part.cpu_limit.unwrap().amd, None);
    }

    #[test]
    fn a_reverted_try_drops_the_cpu_draft_but_a_kept_one_does_not() {
        let draft = || CpuDraft {
            profile_id: "gaming".into(),
            limits: AmdCpuLimit {
                ppt_w: Some(88.0),
                ..Default::default()
            },
        };
        let mut ui = ControlUi {
            cpu_draft: Some(draft()),
            ..Default::default()
        };

        assert!(ui.track_preview(true, false)); // started → refresh
        assert!(!ui.track_preview(true, false)); // still running → nothing
        assert!(ui.track_preview(false, true)); // reverted → refresh
        assert!(ui.cpu_draft.is_none());

        ui.cpu_draft = Some(draft());
        ui.track_preview(true, false);
        ui.track_preview(false, false); // kept: the save makes the draft equal, show() drops it
        assert!(ui.cpu_draft.is_some());
    }

    #[test]
    fn a_click_stays_busy_until_the_service_answers() {
        let mut ui = ControlUi::default();
        let mut control = ControlState::default();
        assert!(!ui.busy(&control));

        // Keep: answered by the preview ending, not by an apply result.
        ui.track_preview(true, false);
        ui.await_reply(&control);
        assert!(ui.busy(&control));
        ui.track_preview(false, false);
        assert!(!ui.busy(&control));

        // Save & apply / a failed Try: answered by a new apply result.
        ui.await_reply(&control);
        control.last_apply_result = Some(ApplyResult {
            ok: false,
            message: Some("nope".into()),
            profile_id: None,
        });
        assert!(!ui.busy(&control));
    }

    #[test]
    fn with_drafts_folds_gpu_limits_into_the_profile() {
        let profile = Profile {
            id: "gaming".into(),
            name: "Gaming".into(),
            icon: None,
            builtin: true,
            part: Default::default(),
        };
        let mut ui = ControlUi {
            gpu_draft: Some(GpuDraft {
                profile_id: "gaming".into(),
                limits: BTreeMap::from([("pci-9070".to_owned(), -15)]),
            }),
            ..Default::default()
        };
        let merged = with_drafts(&profile, &ui);
        assert_eq!(saved_gpu_limits(Some(&merged)).get("pci-9070"), Some(&-15));

        // Back to the original: an empty part, not a stored 0.
        ui.gpu_draft.as_mut().unwrap().limits.clear();
        assert_eq!(
            with_drafts(&profile, &ui).part.gpu,
            Some(GpuPart { adapters: None })
        );
    }

    #[test]
    fn a_reverted_try_drops_the_gpu_draft_too() {
        let mut ui = ControlUi {
            gpu_draft: Some(GpuDraft {
                profile_id: "gaming".into(),
                limits: BTreeMap::from([("pci-9070".to_owned(), -15)]),
            }),
            ..Default::default()
        };
        assert!(ui.has_limit_draft());
        ui.track_preview(true, false);
        ui.track_preview(false, true);
        assert!(!ui.has_limit_draft());
    }

    #[test]
    fn with_drafts_folds_curve_optimizer_into_the_profile() {
        let profile = Profile {
            id: "gaming".into(),
            name: "Gaming".into(),
            icon: None,
            builtin: true,
            part: Default::default(),
        };
        let ui = ControlUi {
            co_draft: Some(CoDraft {
                profile_id: "gaming".into(),
                part: CurveOptPart {
                    all_core: Some(-15),
                    per_core: Some(BTreeMap::from([("3".to_owned(), -20)])),
                },
            }),
            ..Default::default()
        };
        let merged = with_drafts(&profile, &ui);
        assert_eq!(saved_curve_opt(Some(&merged)).all_core, Some(-15));
        assert!(ui.has_limit_draft()); // CO goes through preview too
    }

    #[test]
    fn discarding_a_lighting_draft_puts_the_saved_lights_back() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let saved = AuraPart {
            effect: Some("static".into()),
            color: Some("#00ff00".into()),
            ..Default::default()
        };
        let mut ui = ControlUi {
            aura_draft: Some(AuraDraft {
                profile_id: "p".into(),
                part: Some(AuraPart {
                    effect: Some("off".into()),
                    ..Default::default()
                }),
            }),
            ..Default::default()
        };

        ui.discard_aura(Some(&saved), &tx);

        assert!(ui.aura_draft.is_none());
        assert!(matches!(rx.try_recv(), Ok(ControlCmd::AuraPreview(p)) if p == saved));
        ui.discard_aura(Some(&saved), &tx); // no draft: nothing sent
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn live_preview_is_throttled() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let mut ui = ControlUi {
            aura_pending: Some(AuraPart::default()),
            ..Default::default()
        };
        assert!(!ui.flush_aura_preview(&tx)); // first goes out at once
        assert!(rx.try_recv().is_ok());

        ui.aura_pending = Some(AuraPart::default());
        assert!(ui.flush_aura_preview(&tx)); // within 100 ms: held back
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn refused_or_another_version_is_explained_but_connecting_is_not() {
        let mut control = ControlState::default();
        assert_eq!(connection_problem(&control), None); // a moment of connecting

        control.refused = true;
        assert_eq!(
            connection_problem(&control).map(|p| p.id),
            Some("service_refused".into())
        );

        control.protocol_mismatch = Some(rigstats_backend::control::ProtocolMismatch {
            expected: 1,
            got: 2,
        });
        assert_eq!(
            connection_problem(&control).map(|p| p.id),
            Some("version_mismatch".into())
        );
    }

    #[test]
    fn lighting_taken_by_another_app_is_a_problem_from_any_service() {
        let conflict = |problem: Option<Problem>| ControlState {
            capabilities: vec![rigstats_backend::control::CapabilitySet {
                domain: "aura".into(),
                supported: false,
                reason: Some("Armoury Crate (LightingService) controls the lighting.".into()),
                problem,
                details: Some(serde_json::json!({ "conflict": true })),
            }],
            ..Default::default()
        };

        // This service: its own problem.
        let own = Problem {
            id: "aura_conflict".into(),
            title: "Armoury Crate controls the lighting".into(),
            ..Default::default()
        };
        assert_eq!(lighting_problem(&conflict(Some(own.clone()))), Some(own));

        // An older service: built from its reason.
        let built = lighting_problem(&conflict(None)).unwrap();
        assert_eq!(built.severity, Severity::Attention);
        assert_eq!(
            built.message,
            "Armoury Crate (LightingService) controls the lighting."
        );

        // No controller at all: not a problem.
        assert_eq!(lighting_problem(&ControlState::default()), None);
    }
}
