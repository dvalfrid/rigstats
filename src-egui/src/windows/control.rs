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

use crate::lock_ext::LockSafe;
use crate::theme::{self, DialogColors};
use crate::windows::profile_look::{self, LookEditor, LookLink};
use crate::windows::ui_kit::{self, Icon};
use crate::windows::OpenRequests;
use rigstats_backend::control::{
    AmdCpuLimit, AmdLimitValues, ApplyResult, AuraCaps, AuraPart, ControlCmd, ControlState,
    CpuLimitCaps, CpuLimitPart, CurveOptCaps, CurveOptPart, FanCaps, FanHeaderCap, FanHeaderConfig,
    FanPart, FanResponder, GpuAdapterCap, GpuAdapterConfig, GpuCaps, GpuPart, HeadsetSettings,
    HueCaps, KeyboardPart, LampPart, MouseInfo, MousePart, MouseSettings, PowerScheme, Problem,
    Profile, Severity,
};
use rigstats_backend::lhm::Peripheral;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Curve editor axes: temperature range shown, duty is always 0–100 %.
const T_MIN: f64 = 20.0;
const T_MAX: f64 = 100.0;
/// How close (px) the pointer must be to grab/remove a curve point.
const GRAB_RADIUS: f32 = 12.0;
const CURVE_COLOR: egui::Color32 = egui::Color32::from_rgb(0x3a, 0x9b, 0xff);
/// Mirrors `FanProvider.IdentifyDuration` in the service.
const IDENTIFY_DURATION: Duration = Duration::from_secs(5);
/// Footer buttons stay disabled at most this long waiting for the service
/// (its request timeout is 5 s).
const AWAIT_TIMEOUT: Duration = Duration::from_secs(6);

/// Starting curves offered when a header is switched to curve control.
/// Floors stay at ≥25 % so common fans never stall at idle.
const PRESETS: [(&str, &[[f64; 2]]); 3] = [
    (
        "Silent",
        &[
            [30.0, 25.0],
            [50.0, 30.0],
            [65.0, 45.0],
            [75.0, 65.0],
            [85.0, 100.0],
        ],
    ),
    (
        "Balanced",
        &[
            [30.0, 30.0],
            [50.0, 40.0],
            [65.0, 60.0],
            [75.0, 80.0],
            [85.0, 100.0],
        ],
    ),
    (
        "Performance",
        &[
            [30.0, 40.0],
            [50.0, 55.0],
            [60.0, 75.0],
            [70.0, 90.0],
            [80.0, 100.0],
        ],
    ),
];

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
    /// The Dashboard/Overlay/Alerts pages' edit (#305).
    look: LookEditor,
    /// When a page that saves at once (Power, App rules) last saved — the
    /// footer confirms it for a moment.
    saved_at: Option<(Instant, &'static str)>,
    /// The App rules page's "Add a rule" form (#307).
    rule_form: crate::windows::rules_page::RulesPage,
    /// The Headset page asked the service for the headsets' settings since
    /// it was opened (#428).
    headsets_asked: bool,
    /// A headset's settings as just changed here, shown until the service's
    /// read-back arrives (a slider would otherwise jump back meanwhile).
    headset_pending: Option<(String, HeadsetSettings)>,
    /// The Mouse page asked for the mice's settings since it was opened (#431).
    mice_asked: bool,
    /// The Control Center profile active when the mice were last read — a
    /// switch can move the mouse to another onboard profile.
    mice_read_for: Option<String>,
    /// A mouse's settings as just changed here, until its read-back arrives.
    mouse_pending: Option<(String, MouseSettings)>,
    /// A change made while a slider, number or colour picker was held, not
    /// sent yet.
    mouse_unsent: bool,
    /// The DPI level being changed on a mouse whose level in use only its
    /// DPI button picks (#432).
    mouse_edit_stage: u8,
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

fn saved_fan_headers(profile: Option<&Profile>) -> BTreeMap<String, FanHeaderConfig> {
    profile
        .and_then(|p| p.part.fan.as_ref())
        .and_then(|f| f.headers.clone())
        .unwrap_or_default()
}

fn saved_cpu_limits(profile: Option<&Profile>) -> AmdCpuLimit {
    profile
        .and_then(|p| p.part.cpu_limit.as_ref())
        .and_then(|c| c.amd)
        .unwrap_or_default()
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

/// AMD's own Eco Mode limits (PPT W, TDC A, EDC A), offered as starting
/// points; anything above this CPU's BIOS value stays at the BIOS value.
const CPU_PRESETS: [(&str, [f64; 3]); 2] = [
    ("105 W Eco", [142.0, 110.0, 170.0]),
    ("65 W Eco", [88.0, 75.0, 150.0]),
];

/// A limit at or above the BIOS value is no limit at all — stored as `None`.
fn below_stock(value: f64, stock: f64) -> Option<f64> {
    (value.round() < stock.round()).then_some(value.round())
}

fn preset_limits([ppt, tdc, edc]: [f64; 3], stock: &AmdLimitValues) -> AmdCpuLimit {
    AmdCpuLimit {
        ppt_w: below_stock(ppt, stock.ppt_w),
        tdc_a: below_stock(tdc, stock.tdc_a),
        edc_a: below_stock(edc, stock.edc_a),
    }
}

fn cpu_tab(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    caps: &CpuLimitCaps,
    saved: &AmdCpuLimit,
    ui_state: &mut ControlUi,
) {
    let Some(profile_id) = control.active_profile.clone() else {
        return;
    };
    let mut limits = ui_state.cpu_draft.as_ref().map_or(*saved, |d| d.limits);
    let before = limits;
    let now = caps.current;
    let footer = format!(
        "AMD Ryzen ({}). Lower limits run cooler and quieter; the maximum is what the BIOS \
         set. New limits are tried first and revert by themselves unless you keep them.",
        caps.generation
    );
    ui_kit::group(ui, dc, Some("Power limits"), Some(&footer), |g| {
        // No edits while a try is running: Keep/Undo in the footer first.
        let editable = control.preview.is_none();
        g.row("Preset", None, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                // Which preset the limits are, if any (none lit = custom).
                let mut chosen = if limits.is_stock() {
                    0
                } else {
                    CPU_PRESETS
                        .iter()
                        .position(|(_, v)| preset_limits(*v, &caps.stock) == limits)
                        .map_or(usize::MAX, |i| i + 1)
                };
                let mut options = vec![(0, "BIOS")];
                options.extend(
                    CPU_PRESETS
                        .iter()
                        .enumerate()
                        .map(|(i, (n, _))| (i + 1, *n)),
                );
                if ui_kit::segmented(ui, dc, &mut chosen, &options) {
                    limits = match chosen {
                        0 => AmdCpuLimit::default(),
                        i => preset_limits(CPU_PRESETS[i - 1].1, &caps.stock),
                    };
                }
            });
        });
        let rows: [LimitRow<'_>; 3] = [
            (
                "Package power (PPT)",
                "W",
                &mut limits.ppt_w,
                caps.min.ppt_w,
                caps.stock.ppt_w,
                now.ppt_w,
            ),
            (
                "Sustained current (TDC)",
                "A",
                &mut limits.tdc_a,
                caps.min.tdc_a,
                caps.stock.tdc_a,
                now.tdc_a,
            ),
            (
                "Peak current (EDC)",
                "A",
                &mut limits.edc_a,
                caps.min.edc_a,
                caps.stock.edc_a,
                now.edc_a,
            ),
        ];
        for (label, unit, value, min, stock, in_force) in rows {
            g.row(
                label,
                Some(&format!("In force now: {in_force:.0} {unit}")),
                |ui| {
                    ui.add_enabled_ui(editable, |ui| limit_slider(ui, dc, unit, value, min, stock));
                },
            );
        }
    });
    dry_run_note(ui, dc, control);

    if limits != before {
        ui_state.cpu_draft = Some(CpuDraft { profile_id, limits });
    }
}

/// One power-limit row: label, unit, the value, its minimum, the BIOS
/// value and the value in force now.
type LimitRow<'a> = (&'a str, &'a str, &'a mut Option<f64>, f64, f64, f64);

/// A limit's slider with its value beside it; a value at the BIOS
/// maximum is no limit at all. For a row's right-to-left layout.
fn limit_slider(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    unit: &str,
    value: &mut Option<f64>,
    min: f64,
    stock: f64,
) {
    let text = match value {
        Some(v) => format!("{v:.0} {unit}"),
        None => format!("{stock:.0} {unit} (BIOS)"),
    };
    ui.add_sized(
        [90.0, 20.0],
        egui::Label::new(egui::RichText::new(text).size(12.0).color(dc.text)),
    );
    ui.spacing_mut().slider_width = 170.0;
    // Whole units, compared before writing, so drawing never edits.
    let mut v = value.unwrap_or(stock).round() as i32;
    let slider = egui::Slider::new(&mut v, min.round() as i32..=stock.round() as i32)
        .show_value(false)
        .trailing_fill(true);
    if ui.add(slider).changed() {
        *value = below_stock(f64::from(v), stock);
    }
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

/// An Overview tile's summary and dot: the problem's title in its colour
/// when the domain failed, else what the profile does.
fn tile_summary(
    problem: Option<&Problem>,
    dc: &DialogColors,
    ok: String,
) -> (String, Option<egui::Color32>) {
    match problem {
        Some(p) => (
            p.title.clone(),
            Some(ui_kit::severity_color(p.severity, dc)),
        ),
        None => (ok, None),
    }
}

/// An empty per-core map means the same as none.
fn normalized_co(part: &CurveOptPart) -> CurveOptPart {
    CurveOptPart {
        all_core: part.all_core,
        per_core: part.per_core.clone().filter(|m| !m.is_empty()),
    }
}

fn saved_curve_opt(profile: Option<&Profile>) -> CurveOptPart {
    profile
        .and_then(|p| p.part.curve_opt.as_ref())
        .map(normalized_co)
        .unwrap_or_default()
}

/// "−15", "0".
fn co_text(value: i32) -> String {
    if value == 0 {
        "0".to_owned()
    } else {
        format!("{value:+}")
    }
}

/// Per-core rows are shown when the user ticked "Per core", or when the
/// profile already has per-core values (they must never be hidden).
fn per_core_shown(ticked: bool, part: &CurveOptPart) -> bool {
    ticked || part.per_core.as_ref().is_some_and(|m| !m.is_empty())
}

/// The offsets in force, compactly: one value when all cores agree.
fn co_summary(values: &[i32]) -> String {
    match values.split_first() {
        None => "—".to_owned(),
        Some((first, rest)) if rest.iter().all(|v| v == first) => {
            format!("all cores {}", co_text(*first))
        }
        Some(_) => values
            .iter()
            .map(|v| co_text(*v))
            .collect::<Vec<_>>()
            .join(" · "),
    }
}

/// Curve Optimizer (#191) on the CPU page: all-core, optionally per core.
fn curve_opt_card(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    caps: &CurveOptCaps,
    saved: &CurveOptPart,
    ui_state: &mut ControlUi,
) {
    let Some(profile_id) = control.active_profile.clone() else {
        return;
    };
    let mut part = ui_state
        .co_draft
        .as_ref()
        .map_or_else(|| saved.clone(), |d| d.part.clone());
    let before = part.clone();
    let bios = |i: usize| caps.bios.get(i).copied().unwrap_or(0);
    let editable = control.preview.is_none();
    let mut per_core_ticked = ui_state.co_per_core;

    ui_kit::group(
        ui,
        dc,
        Some("Curve Optimizer (undervolt)"),
        Some(
            "A change runs for 15 s unless you keep it; if the PC restarts within 3 minutes \
             of a change, it is not applied again at start-up.",
        ),
        |g| {
            g.block(|ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "Negative offsets lower the CPU voltage: cooler, and often faster \
                             under load. Too far and the PC becomes unstable or crashes, so \
                             start small (−5 … −15) and test under load.",
                        )
                        .size(11.0)
                        .color(ui_kit::C_ATTENTION),
                    )
                    .wrap(),
                );
            });
            let in_force = format!("In force now: {}", co_summary(&caps.current));
            g.row("All cores", Some(&in_force), |ui| {
                ui.add_enabled_ui(editable, |ui| {
                    let text = part.all_core.map_or_else(|| "BIOS".to_owned(), co_text);
                    if let Some(v) = co_slider(
                        ui,
                        dc,
                        text,
                        part.all_core.unwrap_or(0),
                        caps.min..=caps.max,
                    ) {
                        part.all_core = Some(v);
                    }
                    if part.all_core.is_some()
                        && theme::dialog_btn_secondary(ui, "BIOS", dc).clicked()
                    {
                        part = CurveOptPart::default();
                    }
                });
            });
            if caps.per_core {
                // Per-core values in the profile always show their rows;
                // switching off removes them (a draft, tried like any change).
                let mut shown = per_core_shown(per_core_ticked, &part);
                g.row("Set each core", Some("Its own offset per core"), |ui| {
                    ui.add_enabled_ui(editable, |ui| {
                        if ui_kit::toggle(ui, dc, &mut shown).changed() {
                            per_core_ticked = shown;
                            if !shown {
                                part.per_core = None;
                            }
                        }
                    });
                });
                if shown {
                    for core in 0..caps.cores {
                        let key = core.to_string();
                        let own = part.per_core.as_ref().and_then(|m| m.get(&key).copied());
                        let inherited = part.all_core.unwrap_or(bios(core));
                        let sub = if own.is_some() {
                            "Its own offset"
                        } else {
                            "Follows all cores"
                        };
                        g.row(&format!("Core {core}"), Some(sub), |ui| {
                            ui.add_enabled_ui(editable, |ui| {
                                let text = own.map_or_else(|| co_text(inherited), co_text);
                                if let Some(v) = co_slider(
                                    ui,
                                    dc,
                                    text,
                                    own.unwrap_or(inherited),
                                    caps.min..=caps.max,
                                ) {
                                    part.per_core
                                        .get_or_insert_with(BTreeMap::new)
                                        .insert(key.clone(), v);
                                }
                                if own.is_some()
                                    && theme::dialog_btn_secondary(ui, "Follow all", dc).clicked()
                                {
                                    if let Some(map) = part.per_core.as_mut() {
                                        map.remove(&key);
                                    }
                                }
                            });
                        });
                    }
                }
            }
        },
    );
    ui_state.co_per_core = per_core_ticked;
    dry_run_note(ui, dc, control);

    let part = normalized_co(&part);
    if part != normalized_co(&before) {
        ui_state.co_draft = Some(CoDraft { profile_id, part });
    }
}

/// An offset slider with its value beside it; `Some(new)` when moved.
fn co_slider(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    text: String,
    value: i32,
    range: std::ops::RangeInclusive<i32>,
) -> Option<i32> {
    ui.add_sized(
        [56.0, 20.0],
        egui::Label::new(egui::RichText::new(text).size(12.0).color(dc.text)),
    );
    ui.spacing_mut().slider_width = 170.0;
    let mut v = value;
    let slider = egui::Slider::new(&mut v, range)
        .show_value(false)
        .trailing_fill(true);
    (ui.add(slider).changed() && v != value).then_some(v)
}

/// Lighting effects in the order the Lighting tab offers them.
const AURA_EFFECTS: [(&str, &str); 4] = [
    ("off", "Off"),
    ("static", "Static"),
    ("breathing", "Breathing"),
    ("spectrum_cycle", "Colour cycle"),
];

fn effect_name(effect: Option<&str>) -> &'static str {
    match effect {
        None => "Unchanged",
        Some(e) => AURA_EFFECTS
            .iter()
            .find(|(id, _)| *id == e)
            .map_or("Static", |(_, name)| name),
    }
}

/// Quick-pick colours beside the colour picker: fixed values, so a pick is
/// always the same colour. Saturated, with orange and yellow low on green
/// and purple and pink tuned to look like their names on RGB LEDs.
const AURA_SWATCHES: [(&str, [u8; 3]); 9] = [
    ("Red", [255, 0, 0]),
    ("Orange", [255, 64, 0]),
    ("Yellow", [255, 176, 0]),
    ("Green", [0, 255, 0]),
    ("Cyan", [0, 255, 255]),
    ("Blue", [0, 0, 255]),
    ("Purple", [128, 0, 255]),
    ("Pink", [255, 0, 128]),
    ("White", [255, 255, 255]),
];

/// The quick-pick swatches as colour dots, the current one ringed; returns
/// the one clicked. For a row's right-to-left layout (added in reverse so
/// they read left to right).
fn color_swatches(ui: &mut egui::Ui, dc: &DialogColors, current: [u8; 3]) -> Option<[u8; 3]> {
    let mut picked = None;
    ui.spacing_mut().item_spacing.x = 6.0;
    for (name, rgb) in AURA_SWATCHES.iter().rev() {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
        let fill = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        let selected = *rgb == current;
        if selected {
            ui.painter()
                .circle_stroke(rect.center(), 10.5, egui::Stroke::new(2.0_f32, dc.title));
        }
        ui.painter()
            .circle_filled(rect.center(), if selected { 7.0 } else { 9.0 }, fill);
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.on_hover_text(*name).clicked() {
            picked = Some(*rgb);
        }
    }
    picked
}

/// "#ff0033" ↔ [255, 0, 51]; white when missing or malformed.
fn parse_hex_color(hex: Option<&str>) -> [u8; 3] {
    let parse = |h: &str| -> Option<[u8; 3]> {
        let h = h.strip_prefix('#')?;
        if h.len() != 6 {
            return None;
        }
        Some([
            u8::from_str_radix(&h[0..2], 16).ok()?,
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
        ])
    };
    hex.and_then(parse).unwrap_or([255, 255, 255])
}

fn hex_color([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Lighting (#192): one effect for every lighting device (Aura Sync), the
/// desk lamp, the devices themselves and Philips Hue — each a group of
/// rows. Changes show on the lights at once (live preview); Save keeps them
/// in the profile.
// The page needs the control state, its caps, the saved part, the command
// channel, the UI state and the telemetry's peripherals.
#[allow(clippy::too_many_arguments)]
fn lighting_tab(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    caps: Option<&AuraCaps>,
    saved: Option<&AuraPart>,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ui_state: &mut ControlUi,
    peripherals: &[Peripheral],
    open: &OpenRequests,
) {
    let hue = control.hue_caps();
    let Some(caps) = caps else {
        // Present but taken by another app — explained, not hidden (#373).
        if let Some(problem) = lighting_problem(control) {
            problem_notice(ui, dc, &problem, open);
        } else {
            let reason = if hue.is_some() {
                "No RGB device found. Pair a Hue Bridge below to light the room."
            } else {
                "Lighting is unavailable."
            };
            ui_kit::group(ui, dc, None, None, |g| {
                g.block(|ui| {
                    ui.label(egui::RichText::new(reason).size(12.0).color(dc.muted));
                });
            });
        }
        if let Some(hue) = &hue {
            hue_card(
                ui,
                dc,
                control,
                hue,
                cmd_tx,
                &mut ui_state.hue_ip,
                &mut ui_state.saved_at,
            );
        }
        return;
    };
    let Some(profile_id) = control.active_profile.clone() else {
        return;
    };
    let mut part = ui_state
        .aura_draft
        .as_ref()
        .map_or_else(|| saved.cloned(), |d| d.part.clone());
    let before = part.clone();
    let battery_levels = battery_levels(ui_state);

    // ── The effect ───────────────────────────────────────────────────────
    ui_kit::group(
        ui,
        dc,
        Some("Effect"),
        Some("Every device below shows it. Changes show on the lights at once; Save keeps them."),
        |g| {
            g.row("Effect", None, |ui| {
                let mut effect = part.as_ref().and_then(|p| p.effect.clone());
                let options: Vec<(Option<String>, &str)> = std::iter::once((None, "Unchanged"))
                    .chain(
                        AURA_EFFECTS
                            .iter()
                            .map(|(id, name)| (Some((*id).to_owned()), *name)),
                    )
                    .collect();
                if ui_kit::segmented(ui, dc, &mut effect, &options) {
                    part = with_effect(part.take(), effect);
                }
            });
            let effect = part.as_ref().and_then(|p| p.effect.clone());
            let takes_color = matches!(effect.as_deref(), Some("static" | "breathing"));
            if let (true, Some(p)) = (takes_color, part.as_mut()) {
                g.row("Colour", None, |ui| {
                    let mut rgb = parse_hex_color(p.color.as_deref());
                    if egui::color_picker::color_edit_button_srgb(ui, &mut rgb)
                        .on_hover_text("Any colour")
                        .changed()
                    {
                        p.color = Some(hex_color(rgb));
                    }
                    ui.add_space(6.0);
                    if let Some(picked) = color_swatches(ui, dc, rgb) {
                        p.color = Some(hex_color(picked));
                    }
                });
                g.row("Brightness", None, |ui| {
                    let mut b = p.brightness.unwrap_or(1.0);
                    if ui_kit::slider_pct(ui, dc, &mut b, 0.0..=1.0) {
                        p.brightness = Some(b);
                    }
                });
            }
        },
    );

    // ── The desk lamp ────────────────────────────────────────────────────
    if caps.devices.iter().any(|d| d.lamp && d.blocked.is_none()) {
        let lamp_now = caps
            .devices
            .iter()
            .filter(|d| d.lamp && d.blocked.is_none())
            .filter_map(|d| d.lamp_on)
            .reduce(|a, b| a || b);
        let note = lamp_state_note(lamp_now, saved, part.as_ref());
        ui_kit::group(ui, dc, Some("Desk lamp"), note, |g| {
            lamp_rows(g, dc, &mut part)
        });
    }

    // ── The devices ──────────────────────────────────────────────────────
    ui_kit::group(ui, dc, Some("Devices"), None, |g| {
        for device in &caps.devices {
            let zones = device
                .zones
                .iter()
                .map(|z| z.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let kind = device_kind(&device.kind);
            let about = if device.firmware.is_empty() {
                kind.clone()
            } else {
                format!("{kind}, firmware {}", device.firmware)
            };
            // A wireless device's battery and link, from the telemetry (same id).
            let wireless = peripherals.iter().find(|p| p.id == device.id);
            g.row(&device.name, Some(&format!("{kind} · {zones}")), |ui| {
                if device.blocked.is_some() || device.problem.is_some() {
                    ui_kit::status(ui, ui_kit::C_ATTENTION, "Needs attention");
                    ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover())
                        .on_hover_text(about);
                } else if let Some(p) = wireless {
                    battery_and_link(ui, dc, p, battery_levels);
                }
            });
            let warnings: Vec<&str> = [device.blocked.as_deref(), device.problem.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            // An ASUS Dynamic Lighting device in "Device Lighting" mode
            // takes every write and ignores it. Where the service can read
            // that mode (#236) it offers the switch; elsewhere say where the
            // switch is. Not on laptop keyboards: Gear Link doesn't manage
            // those.
            let wdl_off = device.blocked.is_none() && device.wdl_on == Some(false);
            let wdl_hint = device.blocked.is_none()
                && device.wdl_on.is_none()
                && device.wdl_toggle != Some(false)
                && device.id.starts_with("lamparray-0b05-")
                && device.kind != "keyboard";
            if warnings.is_empty() && !wdl_off && !wdl_hint {
                continue;
            }
            g.block(|ui| {
                for w in &warnings {
                    ui_kit::row_problem(ui, dc, Severity::Attention, w);
                }
                if wdl_off {
                    ui_kit::row_problem(
                        ui,
                        dc,
                        Severity::Attention,
                        "Ignores lighting changes: its Cross-device Lighting Toggle is set to \
                         \"Device Lighting\".",
                    );
                    ui.horizontal(|ui| {
                        if theme::dialog_btn_secondary(ui, "Let RIGStats control it", dc)
                            .on_hover_text(
                                "Switches the device to \"Aura Sync & Windows Dynamic Lighting\" \
                                 — the same setting as in Gear Link or Armoury Crate, saved in \
                                 the device.",
                            )
                            .clicked()
                        {
                            let _ = cmd_tx.try_send(ControlCmd::EnableWdl(device.id.clone()));
                            ui_state.saved_at = Some((Instant::now(), "Sent to the device"));
                        }
                    });
                } else if wdl_hint {
                    ui_kit::footnote(
                        ui,
                        dc,
                        "No change on the device? Set its Cross-device Lighting Toggle to \
                         \"Aura Sync & Windows Dynamic Lighting\" in Gear Link or Armoury Crate.",
                    );
                }
            });
        }
        g.row(
            "Supported devices",
            Some("Every lighting device RIGStats drives, and which are verified"),
            |ui| {
                if theme::dialog_btn_secondary(ui, "Open list", dc).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(
                        "https://rigstats.app/#supported-devices",
                    ));
                }
            },
        );
    });
    dry_run_note(ui, dc, control);

    if part != before {
        if let Some(p) = &part {
            ui_state.queue_aura_preview(before.as_ref(), p);
        }
        ui_state.aura_draft = Some(AuraDraft { profile_id, part });
    }
    if let Some(hue) = &hue {
        ui.add_space(16.0);
        hue_card(
            ui,
            dc,
            control,
            hue,
            cmd_tx,
            &mut ui_state.hue_ip,
            &mut ui_state.saved_at,
        );
    }
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

/// The Headset page (#428): each connected headset with settings RIGStats
/// knows — sidetone and microphone noise reduction — read from the headset
/// when the page opens and sent to it as soon as one changes (they are kept
/// in the headset, like GearLink's, not in a profile).
fn headset_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ui_state: &mut ControlUi,
    peripherals: &[Peripheral],
    battery_levels: (u8, u8),
) {
    if !ui_state.headsets_asked {
        ui_state.headsets_asked = true;
        ui_state.headset_pending = None;
        let _ = cmd_tx.try_send(ControlCmd::ReadHeadsets);
    }
    // The read-back arrived: show the headset's own values again.
    if let Some((id, pending)) = &ui_state.headset_pending {
        let landed = control
            .headsets
            .iter()
            .any(|h| &h.id == id && h.settings.as_ref() == Some(pending));
        if landed || !control.headset_busy {
            ui_state.headset_pending = None;
        }
    }
    if control.headsets.is_empty() {
        ui_kit::group(ui, dc, None, None, |g| {
            g.row(
                if control.headset_busy {
                    "Reading the headset…"
                } else {
                    "No headset answered"
                },
                Some("Switch the headset on, then open this page again"),
                |ui| {
                    if !control.headset_busy
                        && theme::dialog_btn_secondary(ui, "Try again", dc).clicked()
                    {
                        let _ = cmd_tx.try_send(ControlCmd::ReadHeadsets);
                    }
                },
            );
        });
        return;
    }
    let mut send: Option<(String, HeadsetSettings)> = None;
    for headset in &control.headsets {
        let wireless = peripherals.iter().find(|p| p.id == headset.id);
        let subtitle = if headset.verified {
            "Headset"
        } else {
            "Headset · settings not yet verified on this model"
        };
        ui_kit::group(ui, dc, None, None, |g| {
            g.row(&headset.name, Some(subtitle), |ui| {
                if let Some(p) = wireless {
                    battery_and_link(ui, dc, p, battery_levels);
                }
            });
        });
        let Some(read) = headset.settings else {
            ui_kit::group(ui, dc, None, None, |g| {
                g.row(
                    "The headset doesn't answer",
                    Some("Switch it on or bring it closer to its dongle"),
                    |ui| {
                        if theme::dialog_btn_secondary(ui, "Try again", dc).clicked() {
                            let _ = cmd_tx.try_send(ControlCmd::ReadHeadsets);
                        }
                    },
                );
            });
            continue;
        };
        let mut s = match &ui_state.headset_pending {
            Some((id, pending)) if id == &headset.id => *pending,
            _ => read,
        };
        let before = s;
        let mut release = false;
        ui_kit::group(
            ui,
            dc,
            Some("Sidetone"),
            Some("Hear your own voice in the headset while you talk, so you don't raise it. The headset plays it only while a program uses the microphone — a call, a recording, or the level meter in Settings."),
            |g| {
                g.row("Sidetone", None, |ui| {
                    ui_kit::toggle(ui, dc, &mut s.sidetone_on);
                });
                g.row("Level", Some("How loud your voice comes back"), |ui| {
                    ui.add_enabled_ui(s.sidetone_on, |ui| {
                        let resp = ui_kit::slider_steps(
                            ui,
                            dc,
                            &mut s.sidetone_level,
                            headset.sidetone_max,
                        );
                        release |= resp.drag_stopped() || (resp.changed() && !resp.dragged());
                    });
                });
            },
        );
        ui_kit::group(
            ui,
            dc,
            Some("Noise reduction"),
            Some("Filters background noise — keyboard, fans — out of what your microphone sends; what you hear in the headset is unchanged."),
            |g| {
                g.row("Noise reduction", None, |ui| {
                    ui_kit::toggle(ui, dc, &mut s.noise_reduction_on);
                });
                if headset.noise_levels == 3 {
                    g.row("Strength", None, |ui| {
                        ui.add_enabled_ui(s.noise_reduction_on, |ui| {
                            ui_kit::segmented(
                                ui,
                                dc,
                                &mut s.noise_reduction_level,
                                &[(0, "Low"), (1, "Medium"), (2, "High")],
                            );
                        });
                    });
                }
            },
        );
        if s != before {
            // A slider shows its value while dragging and is sent when let go;
            // a switch or a choice is sent at once.
            let dragging = s.sidetone_level != before.sidetone_level && !release;
            ui_state.headset_pending = Some((headset.id.clone(), s));
            if !dragging {
                send = Some((headset.id.clone(), s));
            }
        } else if release {
            send = Some((headset.id.clone(), s));
        }
    }
    if let Some((id, settings)) = send {
        if cmd_tx
            .try_send(ControlCmd::SetHeadset { id, settings })
            .is_ok()
        {
            ui_state.saved_at = Some((Instant::now(), "Sent to the headset"));
        }
    }
}

/// A bindable button's own function and where it sits, by its action code
/// (#433) — the place only where it is the same on every ROG mouse.
fn mouse_button_help(source: u16) -> (&'static str, Option<&'static str>) {
    match source {
        0x01F0 => ("Left click", Some("The main left button")),
        0x01F1 => ("Right click", Some("The main right button")),
        0x01F2 => ("Middle click", Some("Pressing the scroll wheel")),
        0x01E4 => ("Back", Some("Thumb button nearest your wrist")),
        0x01E5 => ("Forward", Some("Thumb button nearest the front")),
        0x01E6 => ("DPI switch", None),
        0x01E8 => ("Scroll up", Some("Rolling the wheel up")),
        0x01E9 => ("Scroll down", Some("Rolling the wheel down")),
        _ => ("Its own function", None),
    }
}

/// Auto-power-off codes the mice use, with their words.
const MOUSE_POWER_OFF: [(u8, &str); 6] = [
    (0, "1 minute"),
    (1, "2 minutes"),
    (2, "3 minutes"),
    (3, "5 minutes"),
    (4, "10 minutes"),
    (255, "Never"),
];

/// "dongle" → "2.4 GHz dongle".
fn mouse_link(link: &str) -> &'static str {
    match link {
        "wired" => "Cable",
        "omni" => "ROG Omni receiver",
        _ => "2.4 GHz dongle",
    }
}

/// The Mouse page (#431): each connected ROG mouse with known settings — the
/// onboard profile the RIGStats profile uses, then the DPI stages,
/// performance and energy settings of the profile the mouse uses now. Read
/// from the mouse when the page opens; each change is sent at once and kept
/// in the mouse (sliders and numbers when let go).
fn mouse_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ui_state: &mut ControlUi,
    peripherals: &[Peripheral],
    battery_levels: (u8, u8),
) {
    // Read when the page opens, and again whenever the Control Center
    // profile changes (profile bar, hotkey, tray, an app rule): it may switch
    // the mouse to another onboard profile, whose settings are its own.
    if !ui_state.mice_asked || ui_state.mice_read_for != control.active_profile {
        ui_state.mice_asked = true;
        ui_state.mice_read_for = control.active_profile.clone();
        ui_state.mouse_pending = None;
        ui_state.mouse_unsent = false;
        let _ = cmd_tx.try_send(ControlCmd::ReadMice);
    }
    if let Some((id, pending)) = &ui_state.mouse_pending {
        let landed = control
            .mice
            .iter()
            .any(|m| &m.id == id && m.settings.as_ref() == Some(pending));
        // Not while a change is still held (a drag, an open colour picker):
        // dropping it then would snap the control back to the old value.
        if !ui_state.mouse_unsent && (landed || !control.mouse_busy) {
            ui_state.mouse_pending = None;
        }
    }
    if control.mice.is_empty() {
        ui_kit::group(ui, dc, None, None, |g| {
            g.row(
                if control.mouse_busy {
                    "Reading the mouse…"
                } else {
                    "No mouse answered"
                },
                Some("Wake the mouse (move it), then open this page again"),
                |ui| {
                    if !control.mouse_busy
                        && theme::dialog_btn_secondary(ui, "Try again", dc).clicked()
                    {
                        let _ = cmd_tx.try_send(ControlCmd::ReadMice);
                    }
                },
            );
        });
        return;
    }
    let profile = control.active().cloned();
    let mut send: Option<(String, MouseSettings)> = None;
    let mut save_profile: Option<Option<u8>> = None;
    for mouse in &control.mice {
        mouse_card(
            ui,
            dc,
            mouse,
            profile.as_ref(),
            peripherals,
            battery_levels,
            cmd_tx,
            ui_state,
            &mut send,
            &mut save_profile,
        );
    }
    if let (Some(choice), Some(mut profile)) = (save_profile, profile) {
        profile.part.mouse = choice.map(|p| MousePart { profile: Some(p) });
        let _ = cmd_tx.try_send(ControlCmd::SaveProfile {
            profile: Box::new(profile),
            apply: true,
        });
        // The mouse may now use another onboard profile: show its settings.
        let _ = cmd_tx.try_send(ControlCmd::ReadMice);
        ui_state.saved_at = Some((Instant::now(), "Saved"));
    }
    if let Some((id, settings)) = send {
        if cmd_tx
            .try_send(ControlCmd::SetMouse {
                id,
                settings: Box::new(settings),
            })
            .is_ok()
        {
            ui_state.saved_at = Some((Instant::now(), "Sent to the mouse"));
        }
    }
}

/// The Keyboard page (#458): which of the keyboard's own onboard profiles
/// the RIGStats profile uses, saved at once in the profile — the keyboard
/// switches with it. One choice for every keyboard (a profile has one
/// keyboard part).
fn keyboard_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ui_state: &mut ControlUi,
) {
    let keyboards = control.keyboards();
    let Some(mut profile) = control.active().cloned() else {
        return;
    };
    ui_kit::group(ui, dc, None, None, |g| {
        for keyboard in &keyboards {
            g.row(
                &keyboard.name,
                Some(&format!(
                    "Keyboard · {} profiles of its own",
                    keyboard.profiles
                )),
                |_| {},
            );
        }
    });
    let count = keyboards.iter().map(|k| k.profiles).max().unwrap_or(1);
    let chosen = profile.part.keyboard.and_then(|k| k.profile);
    let mut pick = chosen;
    let footnote = format!(
        "The keyboard keeps {count} profiles of its own — its lighting and keys — and Fn plus a number key switches between them on the keyboard. Switching to {} switches the keyboard too.",
        profile.name
    );
    ui_kit::group(ui, dc, Some("Profile"), Some(&footnote), |g| {
        g.row(
            &format!("Keyboard profile in {}", profile.name),
            Some("Saved at once"),
            |ui| {
                let label =
                    chosen.map_or_else(|| "Leave unchanged".to_owned(), |p| format!("Profile {p}"));
                ui_kit::dropdown(ui, "keyboard_profile", label, |ui| {
                    ui.selectable_value(&mut pick, None, "Leave unchanged");
                    for p in 1..=count {
                        ui.selectable_value(&mut pick, Some(p), format!("Profile {p}"));
                    }
                });
            },
        );
    });
    if pick != chosen {
        profile.part.keyboard = pick.map(|p| KeyboardPart { profile: Some(p) });
        let _ = cmd_tx.try_send(ControlCmd::SaveProfile {
            profile: Box::new(profile),
            apply: true,
        });
        ui_state.saved_at = Some((Instant::now(), "Saved"));
    }
}

/// One mouse on the Mouse page.
#[allow(clippy::too_many_arguments)] // one card's inputs and its two outputs
fn mouse_card(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    mouse: &MouseInfo,
    profile: Option<&Profile>,
    peripherals: &[Peripheral],
    battery_levels: (u8, u8),
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ui_state: &mut ControlUi,
    send: &mut Option<(String, MouseSettings)>,
    save_profile: &mut Option<Option<u8>>,
) {
    let spec = &mouse.spec;
    // Its battery from the telemetry — the lighting side names the mouse the same.
    let wireless = peripherals
        .iter()
        .find(|p| p.id == mouse.id || p.name == mouse.name);
    let subtitle = if mouse.verified {
        format!("Mouse · {}", mouse_link(&mouse.link))
    } else {
        format!(
            "Mouse · {} · settings not yet verified on this model",
            mouse_link(&mouse.link)
        )
    };
    ui_kit::group(ui, dc, None, None, |g| {
        g.row(&mouse.name, Some(&subtitle), |ui| {
            if let Some(p) = wireless {
                battery_and_link(ui, dc, p, battery_levels);
            }
        });
    });

    // ── Onboard profile, per RIGStats profile ────────────────────────────
    if spec.profiles > 1 {
        if let Some(profile) = profile {
            let chosen = profile.part.mouse.and_then(|m| m.profile);
            let footnote = format!(
                "The mouse keeps {} profiles of its own. Switching to {} switches the mouse too; \
                 the settings below are those of the profile the mouse uses now.",
                spec.profiles, profile.name
            );
            ui_kit::group(ui, dc, Some("Profile"), Some(&footnote), |g| {
                g.row(
                    &format!("Mouse profile in {}", profile.name),
                    Some("Saved at once"),
                    |ui| {
                        let label = chosen.map_or_else(
                            || "Leave unchanged".to_owned(),
                            |p| format!("Profile {p}"),
                        );
                        let mut pick = chosen;
                        ui_kit::dropdown(ui, "mouse_profile", label, |ui| {
                            ui.selectable_value(&mut pick, None, "Leave unchanged");
                            for p in 1..=spec.profiles {
                                ui.selectable_value(&mut pick, Some(p), format!("Profile {p}"));
                            }
                        });
                        if pick != chosen {
                            *save_profile = Some(pick);
                        }
                    },
                );
            });
        }
    }

    let Some(read) = mouse.settings.clone() else {
        ui_kit::group(ui, dc, None, None, |g| {
            g.row(
                "The mouse doesn't answer",
                Some("Move it to wake it, then try again"),
                |ui| {
                    if theme::dialog_btn_secondary(ui, "Try again", dc).clicked() {
                        let _ = cmd_tx.try_send(ControlCmd::ReadMice);
                    }
                },
            );
        });
        return;
    };
    let mut s = match &ui_state.mouse_pending {
        Some((id, pending)) if id == &mouse.id => pending.clone(),
        _ => read,
    };
    let before = s.clone();
    // Sliders and numbers are sent once let go; switches and choices at once.
    // A change made while one is held stays unsent (`mouse_unsent`) until
    // nothing is held.
    let busy = std::cell::Cell::new(false);
    let track = |resp: &egui::Response| {
        if resp.dragged() || resp.has_focus() {
            busy.set(true);
        }
    };

    // ── DPI ─────────────────────────────────────────────────────────────
    // Older mice (#432) can't be switched to a level from the PC: the chips
    // then only pick the level to change.
    let mut edit_stage = if spec.can_switch_stage {
        s.active_stage
    } else {
        ui_state.mouse_edit_stage.clamp(1, spec.dpi_stages.max(1))
    };
    ui_kit::group(
        ui,
        dc,
        Some(&format!("DPI — mouse profile {}", s.profile)),
        Some(if spec.can_switch_stage {
            "Picking a level uses it now; the mouse's DPI button steps through the same levels."
        } else {
            "Pick a level to change it; the mouse's DPI button picks the level in use."
        }),
        |g| {
            if spec.dpi_stages > 1 {
                g.row("DPI levels", Some("The levels stored in the mouse"), |ui| {
                    let labels: Vec<(u8, String)> = s
                        .stages
                        .iter()
                        .enumerate()
                        .map(|(i, st)| (i as u8 + 1, st.dpi.to_string()))
                        .collect();
                    let options: Vec<(u8, &str)> =
                        labels.iter().map(|(i, l)| (*i, l.as_str())).collect();
                    ui_kit::segmented(ui, dc, &mut edit_stage, &options);
                });
            }
            let at = usize::from(edit_stage.max(1)) - 1;
            if let Some(stage) = s.stages.get_mut(at) {
                let title = if spec.dpi_stages > 1 {
                    format!("Change level {}", at + 1)
                } else {
                    "DPI".to_owned()
                };
                g.row(
                    &title,
                    Some(if spec.can_switch_stage {
                        "The level in use — how far the pointer moves"
                    } else {
                        "How far the pointer moves at this level"
                    }),
                    |ui| {
                        let mut v = stage.dpi as i32;
                        let resp = ui_kit::slider_range(
                            ui,
                            dc,
                            &mut v,
                            spec.dpi_min as i32..=spec.dpi_max as i32,
                            spec.dpi_step as i32,
                            "",
                        );
                        stage.dpi = v as u32;
                        track(&resp);
                    },
                );
            }
        },
    );
    if spec.can_switch_stage {
        s.active_stage = edit_stage;
    } else {
        ui_state.mouse_edit_stage = edit_stage;
    }

    // ── Performance ─────────────────────────────────────────────────────
    ui_kit::group(ui, dc, Some("Performance"), None, |g| {
        if let Some(hz) = s.polling_hz.as_mut() {
            if !spec.polling_hz.is_empty() {
                g.row("Polling rate", Some("How often the mouse reports"), |ui| {
                    ui_kit::dropdown(ui, "mouse_polling", format!("{hz} Hz"), |ui| {
                        for rate in &spec.polling_hz {
                            ui.selectable_value(hz, *rate, format!("{rate} Hz"));
                        }
                    });
                });
            }
        }
        if let (Some(code), Some((min, max))) = (s.debounce.as_mut(), spec.debounce) {
            g.row(
                "Button response",
                Some("How long a click must settle"),
                |ui| {
                    ui_kit::dropdown(
                        ui,
                        "mouse_debounce",
                        format!("{} ms", 4 * u32::from(*code) + 4),
                        |ui| {
                            for c in min..=max {
                                ui.selectable_value(
                                    code,
                                    c,
                                    format!("{} ms", 4 * u32::from(c) + 4),
                                );
                            }
                        },
                    );
                },
            );
        }
        if let Some(height) = s.lift_off.as_mut() {
            g.row(
                "Lift-off distance",
                Some("How high the mouse stops tracking"),
                |ui| {
                    ui_kit::segmented(ui, dc, height, &[(0, "Low"), (1, "High")]);
                },
            );
        }
        if let Some(on) = s.angle_snapping.as_mut() {
            g.row(
                "Angle snapping",
                Some("Straightens nearly straight lines"),
                |ui| {
                    ui_kit::toggle(ui, dc, on);
                },
            );
        }
        if let (Some(angle), Some((min, max, step))) = (s.angle_tuning.as_mut(), spec.angle_tuning)
        {
            g.row("Angle tuning", Some("Turns the sensor's axis"), |ui| {
                let mut v = i32::from(*angle);
                let resp = ui_kit::slider_range(
                    ui,
                    dc,
                    &mut v,
                    i32::from(min)..=i32::from(max),
                    i32::from(step),
                    "°",
                );
                *angle = v as i16;
                track(&resp);
            });
        }
        for (label, value, max) in [
            ("Acceleration", s.acceleration.as_mut(), spec.acceleration),
            ("Deceleration", s.deceleration.as_mut(), spec.deceleration),
        ] {
            if let Some(value) = value.filter(|_| max > 0) {
                g.row(label, None, |ui| {
                    let mut v = i32::from(*value);
                    let resp = ui_kit::slider_range(ui, dc, &mut v, 0..=i32::from(max), 1, "");
                    *value = v as u8;
                    track(&resp);
                });
            }
        }
        if let Some(on) = s.motion_sync.as_mut() {
            g.row(
                "Motion sync",
                Some("Lines reports up with the PC's polling"),
                |ui| {
                    ui_kit::toggle(ui, dc, on);
                },
            );
        }
    });

    // ── Buttons ─────────────────────────────────────────────────────────
    if let (Some(slots), Some(actions), Some(bound)) = (
        spec.buttons.as_ref(),
        spec.actions.as_ref(),
        s.buttons.as_mut(),
    ) {
        // What `code` is called on `source`'s button: its own function shows
        // as "Default (…)".
        let label = |source: u16, code: u16| {
            if code == source {
                format!("Default ({})", mouse_button_help(source).0)
            } else {
                actions
                    .iter()
                    .find(|a| a.code == code)
                    .map_or_else(|| format!("Other (0x{code:04X})"), |a| a.name.clone())
            }
        };
        // One button's choice: Default, then every other action.
        let choose = |ui: &mut egui::Ui, source: u16, code: &mut u16, width: f32| {
            let id = format!("mouse_button_{source}");
            ui_kit::combo(ui, &id, width, label(source, *code), |ui| {
                ui.selectable_value(code, source, label(source, source));
                for action in actions.iter().filter(|a| a.code != source) {
                    ui.selectable_value(code, action.code, &action.name);
                }
            })
        };
        ui_kit::group(
            ui,
            dc,
            Some(&format!("Buttons — mouse profile {}", s.profile)),
            Some("What each button does in this mouse profile. Hover a choice to see its button."),
            |g| {
                g.block(|ui| {
                    let names = |source: u16| {
                        slots
                            .iter()
                            .find(|s| s.source == source)
                            .map_or_else(String::new, |s| s.name.clone())
                    };
                    let mut pick = |ui: &mut egui::Ui, source: u16, width: f32| match slots
                        .iter()
                        .position(|s| s.source == source)
                    {
                        Some(i) => choose(ui, source, &mut bound[i], width),
                        None => ui.label(""),
                    };
                    crate::windows::mouse_drawing::draw(ui, dc, &names, &mut pick);
                });
                // The buttons not in the drawing (the DPI button), as rows.
                for (i, slot) in slots.iter().enumerate() {
                    if crate::windows::mouse_drawing::drawn(slot.source) {
                        continue;
                    }
                    let place = match spec.dpi_button.as_deref() {
                        Some("underside") if slot.source == 0x01E6 => {
                            Some("On the underside of the mouse")
                        }
                        Some("top") if slot.source == 0x01E6 => {
                            Some("On top, behind the scroll wheel")
                        }
                        _ => None,
                    };
                    g.row(&slot.name, place, |ui| {
                        choose(ui, slot.source, &mut bound[i], ui_kit::CONTROL_W);
                    });
                }
                if bound.iter().zip(slots).any(|(c, s)| *c != s.source) {
                    g.row("All buttons", Some("Back to their own functions"), |ui| {
                        if theme::dialog_btn_secondary(ui, "Reset to defaults", dc).clicked() {
                            for (c, s) in bound.iter_mut().zip(slots) {
                                *c = s.source;
                            }
                        }
                    });
                }
            },
        );
    }

    // ── Energy ──────────────────────────────────────────────────────────
    let mut open_alerts = false;
    if s.power_off.is_some() || s.low_battery_warning.is_some() {
        ui_kit::group(ui, dc, Some("Energy"), None, |g| {
            if let Some(code) = s.power_off.as_mut() {
                g.row("Turn off after", Some("Without movement"), |ui| {
                    let label = MOUSE_POWER_OFF
                        .iter()
                        .find(|(c, _)| c == code)
                        .map_or("—", |(_, l)| *l);
                    ui_kit::dropdown(ui, "mouse_power_off", label, |ui| {
                        for (c, l) in MOUSE_POWER_OFF {
                            ui.selectable_value(code, c, l);
                        }
                    });
                });
            }
            if let (Some(level), Some((step, max))) =
                (s.low_battery_warning.as_mut(), spec.low_battery_warning)
            {
                g.row(
                    "Low-battery warning at",
                    Some("The mouse's own warning, kept in the mouse"),
                    |ui| {
                        let mut v = i32::from(*level);
                        let resp = ui_kit::slider_range(
                            ui,
                            dc,
                            &mut v,
                            i32::from(step)..=i32::from(max),
                            i32::from(step),
                            " %",
                        );
                        *level = v as u8;
                        track(&resp);
                    },
                );
            }
            // RIGStats' own notifications are a separate setting (#437).
            g.row(
                "Low-battery notifications",
                Some("RIGStats' own, for this profile — set under Alerts"),
                |ui| {
                    if theme::dialog_btn_secondary(ui, "Open Alerts", dc).clicked() {
                        open_alerts = true;
                    }
                },
            );
        });
    }

    if open_alerts {
        ui_state.tab = Page::Alerts;
    }

    // An open dropdown counts as held until it closes.
    if egui::Popup::is_any_open(ui.ctx()) {
        busy.set(true);
    }
    if s != before {
        ui_state.mouse_pending = Some((mouse.id.clone(), s.clone()));
        ui_state.mouse_unsent = true;
    }
    if ui_state.mouse_unsent && !busy.get() {
        ui_state.mouse_unsent = false;
        *send = Some((mouse.id.clone(), s));
    }
}

/// "monitor_light_bar" → "Monitor light bar".
fn device_kind(kind: &str) -> String {
    let words = kind.replace('_', " ");
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// Philips Hue (#215): find and pair a bridge, then choose which rooms and
/// zones follow the rig. Applies at once — the pairing isn't part of a
/// profile; the profile's lighting is what the chosen rooms then show.
fn hue_card(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    hue: &HueCaps,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    typed_ip: &mut String,
    saved_at: &mut Option<(Instant, &'static str)>,
) {
    let busy = control.hue_busy;
    let paired = matches!((&hue.bridge, hue.paired), (Some(_), true));
    let footer = if paired {
        "Breathing and colour cycle run slowly on Hue: the bridge takes about one command a \
         second, so it fades between steps. Other lights are never touched."
    } else if !control.hue_found.is_empty() {
        "Press the round link button on the bridge, then Pair within 30 seconds."
    } else {
        "Room lights through a Hue Bridge on this network — no cloud account needed."
    };
    ui_kit::group(ui, dc, Some("Philips Hue"), Some(footer), |g| {
        match (&hue.bridge, hue.paired) {
            (Some(bridge), true) => {
                let model = if bridge.model.is_empty() {
                    String::new()
                } else {
                    format!("{} · ", bridge.model)
                };
                g.row(&bridge.name, Some(&format!("{model}{}", bridge.ip)), |ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        if theme::dialog_btn_secondary(ui, "Unpair", dc).clicked() {
                            let _ = cmd_tx.try_send(ControlCmd::HueUnpair);
                        }
                        if theme::dialog_btn_secondary(ui, "Refresh rooms", dc).clicked() {
                            let _ = cmd_tx.try_send(ControlCmd::HueRefresh);
                        }
                    });
                });
                if hue.groups.is_empty() {
                    g.block(|ui| {
                        ui_kit::footnote(
                            ui,
                            dc,
                            "The bridge has no rooms or zones yet — add them in the Hue app.",
                        );
                    });
                }
                let mut chosen: Vec<String> = hue
                    .groups
                    .iter()
                    .filter(|g| g.chosen)
                    .map(|g| g.id.clone())
                    .collect();
                let mut changed = false;
                for group in &hue.groups {
                    let kind = device_kind(&group.kind);
                    g.row(
                        &group.name,
                        Some(&format!("{kind} · follows the rig")),
                        |ui| {
                            ui.add_enabled_ui(!busy, |ui| {
                                let mut on = group.chosen;
                                if ui_kit::toggle(ui, dc, &mut on).changed() {
                                    chosen.retain(|id| id != &group.id);
                                    if on {
                                        chosen.push(group.id.clone());
                                    }
                                    changed = true;
                                }
                            });
                        },
                    );
                }
                if changed {
                    let _ = cmd_tx.try_send(ControlCmd::HueChoose(chosen));
                    *saved_at = Some((Instant::now(), "Saved"));
                }
            }
            _ => {
                g.row("Find a Hue Bridge", Some("Searches this network"), |ui| {
                    ui.add_enabled_ui(!busy, |ui| {
                        if theme::dialog_btn_secondary(ui, "Search", dc).clicked() {
                            let _ = cmd_tx.try_send(ControlCmd::HueDiscover(None));
                        }
                    });
                });
                g.row(
                    "Bridge address",
                    Some("If the search finds nothing"),
                    |ui| {
                        ui.add_enabled_ui(!busy, |ui| {
                            let ip = typed_ip.trim().to_owned();
                            if !ip.is_empty()
                                && theme::dialog_btn_secondary(ui, "Look up", dc).clicked()
                            {
                                let _ = cmd_tx.try_send(ControlCmd::HueDiscover(Some(ip)));
                            }
                            ui.add(
                                egui::TextEdit::singleline(typed_ip)
                                    .vertical_align(egui::Align::Center)
                                    .desired_width(130.0)
                                    .min_size(egui::vec2(0.0, ui_kit::CONTROL_H))
                                    .hint_text("192.168.1.20"),
                            );
                        });
                    },
                );
                for bridge in &control.hue_found {
                    g.row(
                        &bridge.name,
                        Some(&format!("{} · {}", bridge.model, bridge.ip)),
                        |ui| {
                            ui.add_enabled_ui(!busy, |ui| {
                                if theme::dialog_btn_primary(ui, "Pair").clicked() {
                                    let _ = cmd_tx.try_send(ControlCmd::HuePair(bridge.ip.clone()));
                                }
                            });
                        },
                    );
                }
            }
        }
        if busy {
            g.block(|ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui_kit::footnote(ui, dc, "Talking to the Hue Bridge…");
                });
            });
        } else if let Some(message) = &control.hue_message {
            g.block(|ui| {
                ui_kit::row_problem(ui, dc, Severity::Attention, message);
            });
        }
    });
}

/// What a live preview sends when the lighting goes from `before` to
/// `after`: only the part that changed — the RGB effect or the lamp — so a
/// colour change leaves a lamp switched from the tray alone, and the other
/// way round. `None` when nothing that can be shown changed.
fn preview_delta(before: Option<&AuraPart>, after: &AuraPart) -> Option<AuraPart> {
    let rgb_changed = before.map_or(true, |b| {
        (&b.effect, &b.color, b.brightness) != (&after.effect, &after.color, after.brightness)
    });
    let lamp_changed = before.map_or(true, |b| b.lamp != after.lamp);
    let rgb = rgb_changed && after.effect.is_some();
    let lamp = lamp_changed && after.lamp.is_some();
    match (rgb, lamp) {
        (false, false) => None,
        (true, true) => Some(after.clone()),
        (true, false) => Some(AuraPart {
            lamp: None,
            ..after.clone()
        }),
        (false, true) => Some(AuraPart {
            lamp: after.lamp.clone(),
            ..Default::default()
        }),
    }
}

/// Explains a lamp that differs from what the profile says — switched from
/// the tray or its own button. Only while the lamp row is unedited (an edit
/// is already shown on the lamp); `now` is read when the window opened.
fn lamp_state_note(
    now: Option<bool>,
    saved: Option<&AuraPart>,
    shown: Option<&AuraPart>,
) -> Option<&'static str> {
    let shown_lamp = shown.and_then(|p| p.lamp.as_ref());
    if shown_lamp != saved.and_then(|p| p.lamp.as_ref()) {
        return None;
    }
    let (now, lamp) = (now?, shown_lamp?);
    (now != lamp.on).then_some(if now {
        "The lamp is on now (from the tray or its own button). Save turns it off."
    } else {
        "The lamp is off now (from the tray or its own button). Save turns it on."
    })
}

/// Two previews in one: the newer one's RGB and lamp where it has them.
fn merge_preview(older: AuraPart, newer: AuraPart) -> AuraPart {
    let (effect, color, brightness) = if newer.effect.is_some() {
        (newer.effect, newer.color, newer.brightness)
    } else {
        (older.effect, older.color, older.brightness)
    };
    AuraPart {
        effect,
        color,
        brightness,
        lamp: newer.lamp.or(older.lamp),
    }
}

/// The lighting with a new effect (`None` = leave the RGB as is). A part
/// that then sets nothing is no part at all.
fn with_effect(part: Option<AuraPart>, effect: Option<String>) -> Option<AuraPart> {
    let lamp = part.as_ref().and_then(|p| p.lamp.clone());
    match effect {
        Some(e) => Some(AuraPart {
            effect: Some(e),
            ..part.unwrap_or_default()
        }),
        None => lamp.map(|lamp| AuraPart {
            lamp: Some(lamp),
            ..Default::default()
        }),
    }
}

/// The lighting with a new lamp setting (`None` = leave the lamp as is).
fn with_lamp(part: Option<AuraPart>, lamp: Option<LampPart>) -> Option<AuraPart> {
    match (part, lamp) {
        (Some(p), None) if p.effect.is_none() => None,
        (Some(p), lamp) => Some(AuraPart { lamp, ..p }),
        (None, lamp) => lamp.map(|lamp| AuraPart {
            lamp: Some(lamp),
            ..Default::default()
        }),
    }
}

/// Desk lamp (#214): on/off, brightness and colour temperature, separate
/// from the Aura Sync effect.
fn lamp_rows(g: &mut ui_kit::Group<'_>, dc: &DialogColors, part: &mut Option<AuraPart>) {
    let lamp = part.as_ref().and_then(|p| p.lamp.clone());
    g.row("Lamp", None, |ui| {
        let mut on = lamp.as_ref().map(|l| l.on);
        let options = [
            (None, "Unchanged"),
            (Some(true), "On"),
            (Some(false), "Off"),
        ];
        if ui_kit::segmented(ui, dc, &mut on, &options) {
            // Brightness and warmth are kept while the lamp is off.
            let next = on.map(|on| LampPart {
                on,
                brightness: lamp
                    .as_ref()
                    .and_then(|l| l.brightness)
                    .or(Some(LampPart::DEFAULT_BRIGHTNESS)),
                temperature: lamp
                    .as_ref()
                    .and_then(|l| l.temperature)
                    .or(Some(LampPart::DEFAULT_KELVIN)),
            });
            *part = with_lamp(part.take(), next);
        }
    });

    let Some(l) = part.as_mut().and_then(|p| p.lamp.as_mut()).filter(|l| l.on) else {
        return;
    };
    g.row("Brightness", None, |ui| {
        let mut b = l.brightness.unwrap_or(LampPart::DEFAULT_BRIGHTNESS);
        if ui_kit::slider_pct(ui, dc, &mut b, 0.01..=1.0) {
            l.brightness = Some(b);
        }
    });
    g.row("Colour temperature", Some("Lower is warmer"), |ui| {
        let mut kelvin = l.temperature.unwrap_or(LampPart::DEFAULT_KELVIN);
        ui.add_sized(
            [52.0, 20.0],
            egui::Label::new(
                egui::RichText::new(format!("{kelvin:.0} K"))
                    .size(12.0)
                    .color(dc.text),
            ),
        );
        ui.spacing_mut().slider_width = 150.0;
        let slider = egui::Slider::new(&mut kelvin, LampPart::MIN_KELVIN..=LampPart::MAX_KELVIN)
            .step_by(100.0)
            .show_value(false)
            .trailing_fill(true);
        if ui.add(slider).changed() {
            l.temperature = Some(kelvin);
        }
    });
}

/// The active profile's GPU limits that are set (a missing adapter = original).
fn saved_gpu_limits(profile: Option<&Profile>) -> BTreeMap<String, i32> {
    profile
        .and_then(|p| p.part.gpu.as_ref())
        .and_then(|g| g.adapters.as_ref())
        .map(|adapters| {
            adapters
                .iter()
                .filter_map(|(id, c)| Some((id.clone(), c.power_limit_pct?)))
                .collect()
        })
        .unwrap_or_default()
}

/// "−15 %", "+5 %", "0 %".
fn pct_text(value: i32) -> String {
    if value == 0 {
        "0 %".to_owned()
    } else {
        format!("{value:+} %")
    }
}

fn gpu_tab(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    caps: &GpuCaps,
    saved: &BTreeMap<String, i32>,
    ui_state: &mut ControlUi,
) {
    let Some(profile_id) = control.active_profile.clone() else {
        return;
    };
    let mut limits = ui_state
        .gpu_draft
        .as_ref()
        .map_or_else(|| saved.clone(), |d| d.limits.clone());
    let before = limits.clone();

    ui_kit::group(
        ui,
        dc,
        Some("Power limit"),
        Some(
            "The same setting as the Power Limit in AMD Adrenalin or MSI Afterburner, relative \
             to the driver default. Lower runs cooler and quieter; higher allows more boost. \
             New limits are tried first and revert by themselves unless you keep them.",
        ),
        |g| {
            for adapter in &caps.adapters {
                let in_force = format!("In force now: {}", pct_text(adapter.current));
                g.row(&adapter.name, Some(&in_force), |ui| {
                    ui.add_enabled_ui(control.preview.is_none(), |ui| {
                        gpu_limit_slider(ui, dc, adapter, &mut limits);
                    });
                });
            }
        },
    );
    dry_run_note(ui, dc, control);

    if limits != before {
        ui_state.gpu_draft = Some(GpuDraft { profile_id, limits });
    }
}

/// One adapter's slider over the driver's range, its value and an
/// "Original" reset. A value equal to the original is stored as no value.
fn gpu_limit_slider(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    adapter: &GpuAdapterCap,
    limits: &mut BTreeMap<String, i32>,
) {
    let set = limits.get(&adapter.id).copied();
    if set.is_some() && theme::dialog_btn_secondary(ui, "Original", dc).clicked() {
        limits.remove(&adapter.id);
    }
    let text = match set {
        Some(v) => pct_text(v),
        None => format!("{} (original)", pct_text(adapter.original)),
    };
    ui.add_sized(
        [96.0, 20.0],
        egui::Label::new(egui::RichText::new(text).size(12.0).color(dc.text)),
    );
    ui.spacing_mut().slider_width = 170.0;
    let current = set.unwrap_or(adapter.original);
    let mut v = current;
    let step = adapter.step.max(1);
    let slider = egui::Slider::new(&mut v, adapter.min..=adapter.max)
        .show_value(false)
        .trailing_fill(true);
    if ui.add(slider).changed() && v != current {
        // Snap to the driver's step only on a real move.
        let snapped = adapter.min + ((v - adapter.min) as f32 / step as f32).round() as i32 * step;
        set_gpu_limit(limits, adapter, snapped.clamp(adapter.min, adapter.max));
    }
}

fn set_gpu_limit(limits: &mut BTreeMap<String, i32>, adapter: &GpuAdapterCap, value: i32) {
    if value == adapter.original {
        limits.remove(&adapter.id);
    } else {
        limits.insert(adapter.id.clone(), value);
    }
}

#[allow(deprecated)]
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
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
    if let Some(fan) = ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::new(SELECT_FAN_ID))) {
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
    ui_kit::hero(ctx, dc, "control", "Control Center", |ui| {
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
    egui::TopBottomPanel::top("control_profiles")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 10,
            bottom: 10,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            profile_bar(ui, dc, control, ui_state, cmd_tx, busy);
        });

    // ── Footer ───────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("control_footer")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 10,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            // The last apply: failed (red) or partly applied (amber), in the
            // service's words — the same colours as every problem (#373).
            if let Some(result) = &control.last_apply_result {
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
                        ui_kit::row_problem(ui, dc, Severity::Attention, message);
                        ui.add_space(6.0);
                    }
                    (true, None) => {}
                }
            }
            // A request the service turned down (save, delete, reset, ...) —
            // shown until dismissed; the Hue card shows its own (#219).
            if let Some(error) = control.last_error.as_ref().filter(|e| {
                control.errors != ui_state.dismissed_errors
                    && control.hue_message.as_ref() != Some(*e)
            }) {
                ui.horizontal(|ui| {
                    let title = crate::problems::service_error(error).title;
                    ui_kit::row_problem(ui, dc, Severity::Attention, &format!("{title}: {error}"));
                    if ui.small_button("✕").on_hover_text("Dismiss").clicked() {
                        ui_state.dismissed_errors = control.errors;
                    }
                });
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
                    if theme::dialog_btn_primary(ui, "Keep").clicked() {
                        let _ = cmd_tx.try_send(ControlCmd::ConfirmPreview { keep: true });
                        ui_state.await_reply(control);
                    }
                    ui.add_space(6.0);
                    if theme::dialog_btn_secondary(ui, "Undo", dc).clicked() {
                        let _ = cmd_tx.try_send(ControlCmd::ConfirmPreview { keep: false });
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
                                let _ = cmd_tx.try_send(cmd);
                                ui_state.await_reply(control);
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
    egui::SidePanel::left("control_nav")
        .resizable(false)
        .exact_width(ui_kit::SIDEBAR_W)
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin::same(10)))
        .show(ctx, |ui| {
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
        .show(ctx, |ui| {
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
                        if power_tab(ui, dc, control, cmd_tx, look_link.open) {
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

/// The profile chips, an add button and a menu with the profile's actions.
/// Renaming and deleting replace the chips with an inline editor.
fn profile_bar(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    ui_state: &mut ControlUi,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    busy: bool,
) {
    // An edit for a profile that is no longer the active one is stale.
    let active = control.active();
    if let Some(ProfileEdit::Renaming { id, .. } | ProfileEdit::ConfirmDelete { id }) =
        &ui_state.profile_edit
    {
        if active.map(|a| &a.id) != Some(id) {
            ui_state.profile_edit = None;
        }
    }
    // Actions act on the active profile: not while a try runs or a click
    // awaits its answer.
    let can_edit = control.connected && control.preview.is_none() && !busy;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(
            egui::RichText::new("Profile")
                .size(12.0)
                .strong()
                .color(dc.label),
        );
        ui.add_space(4.0);
        match (ui_state.profile_edit.clone(), active) {
            (Some(ProfileEdit::Renaming { id, mut name }), Some(active)) => {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .vertical_align(egui::Align::Center)
                        .desired_width(200.0)
                        .min_size(egui::vec2(0.0, ui_kit::CONTROL_H))
                        .hint_text("Profile name"),
                );
                edit.request_focus();
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                let trimmed = name.trim().to_owned();
                let mut done = escape;
                if (theme::dialog_btn_primary(ui, "Rename").clicked() || enter)
                    && !trimmed.is_empty()
                {
                    let mut profile = active.clone();
                    profile.name = trimmed;
                    let _ = cmd_tx.try_send(ControlCmd::SaveProfile {
                        profile: Box::new(profile),
                        apply: false,
                    });
                    done = true;
                }
                if theme::dialog_btn_secondary(ui, "Cancel", dc).clicked() {
                    done = true;
                }
                ui_state.profile_edit = (!done).then_some(ProfileEdit::Renaming { id, name });
            }
            (Some(ProfileEdit::ConfirmDelete { id }), Some(active)) => {
                ui.label(
                    egui::RichText::new(format!(
                        "Delete {}? Its fan curves, limits and look go with it.",
                        active.name
                    ))
                    .size(12.0)
                    .color(dc.title),
                );
                if theme::dialog_btn_primary(ui, "Delete").clicked() {
                    let _ = cmd_tx.try_send(ControlCmd::DeleteProfile(id));
                    ui_state.profile_edit = None;
                }
                if theme::dialog_btn_secondary(ui, "Cancel", dc).clicked() {
                    ui_state.profile_edit = None;
                }
            }
            _ => {
                if control.profiles.is_empty() {
                    ui.label(
                        egui::RichText::new("No profiles yet.")
                            .size(12.0)
                            .color(dc.muted),
                    );
                }
                for (index, profile) in control.profiles.iter().enumerate() {
                    let is_active = active.map(|a| &a.id) == Some(&profile.id);
                    let mut resp = ui_kit::chip(ui, dc, &profile.name, is_active);
                    // Its hotkey (#456), for binding a macro key to it.
                    if let Some(chord) = crate::hotkey::profile_chord(index) {
                        resp = resp.on_hover_text(format!("{chord} switches to {}", profile.name));
                    }
                    if resp.clicked() && !is_active {
                        let _ = cmd_tx.try_send(ControlCmd::ApplyProfile(profile.id.clone()));
                    }
                }
                let Some(active) = active.filter(|_| can_edit) else {
                    return;
                };
                ui.add_space(4.0);
                if ui_kit::icon_button(
                    ui,
                    dc,
                    Icon::Plus,
                    &format!("New profile — a copy of {}", active.name),
                )
                .clicked()
                {
                    let copy = duplicate_profile(active, &control.profiles);
                    let _ = cmd_tx.try_send(ControlCmd::SaveProfile {
                        profile: Box::new(copy),
                        apply: true,
                    });
                }
                let more = ui_kit::icon_button(ui, dc, Icon::More, "Rename, delete or reset");
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(170.0);
                    if ui.button(format!("Rename {}…", active.name)).clicked() {
                        ui_state.profile_edit = Some(ProfileEdit::Renaming {
                            id: active.id.clone(),
                            name: active.name.clone(),
                        });
                    }
                    if ui.button(format!("Duplicate {}", active.name)).clicked() {
                        let copy = duplicate_profile(active, &control.profiles);
                        let _ = cmd_tx.try_send(ControlCmd::SaveProfile {
                            profile: Box::new(copy),
                            apply: true,
                        });
                    }
                    ui.separator();
                    if active.builtin {
                        // Built-ins can't be deleted, only put back.
                        if ui
                            .button("Reset to defaults")
                            .on_hover_text("Back to this built-in profile's hardware defaults")
                            .clicked()
                        {
                            let _ = cmd_tx.try_send(ControlCmd::ResetProfile(active.id.clone()));
                            ui_state.saved_at = Some((Instant::now(), "Reset to its defaults"));
                        }
                    } else if ui.button(format!("Delete {}…", active.name)).clicked() {
                        ui_state.profile_edit = Some(ProfileEdit::ConfirmDelete {
                            id: active.id.clone(),
                        });
                    }
                });
            }
        }
    });
}

/// One-line summaries of a profile's parts, for the overview tiles.
fn fans_summary(headers: &BTreeMap<String, FanHeaderConfig>) -> String {
    match headers.len() {
        0 => "Motherboard default".to_owned(),
        1 => "Custom curve on 1 fan header".to_owned(),
        n => format!("Custom curves on {n} fan headers"),
    }
}

fn cpu_summary(limits: &AmdCpuLimit, co: &CurveOptPart) -> String {
    let limit = match limits.ppt_w {
        Some(ppt) => format!("{ppt:.0} W power limit"),
        None if limits.is_stock() => "Stock limits".to_owned(),
        None => "Custom limits".to_owned(),
    };
    if co.is_bios() {
        limit
    } else {
        format!("{limit} · Curve Optimizer")
    }
}

fn gpu_summary(limits: &BTreeMap<String, i32>) -> String {
    match limits.values().next() {
        None => "Stock power limit".to_owned(),
        Some(v) => format!("Power limit {v:+} %"),
    }
}

fn lighting_summary(aura: Option<&AuraPart>) -> (String, Option<egui::Color32>) {
    let Some(aura) = aura else {
        return ("Left as it is".to_owned(), None);
    };
    match aura.effect.as_deref() {
        Some(effect) => {
            let dot = (effect != "off" && effect != "spectrum_cycle").then(|| {
                let [r, g, b] = parse_hex_color(aura.color.as_deref());
                egui::Color32::from_rgb(r, g, b)
            });
            (effect_name(Some(effect)).to_owned(), dot)
        }
        None if aura.lamp.is_some() => ("Desk lamp only".to_owned(), None),
        None => ("Left as it is".to_owned(), None),
    }
}

/// The profile at a glance: a tile per page with what it does; a click
/// opens the page.
#[allow(clippy::too_many_arguments)]
fn overview_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    active: Option<&Profile>,
    shown: &PageShown,
    headers: &BTreeMap<String, FanHeaderConfig>,
    cpu: &AmdCpuLimit,
    co: &CurveOptPart,
    gpu: &BTreeMap<String, i32>,
    aura: Option<&AuraPart>,
    ui_state: &mut ControlUi,
    rules: &[rigstats_backend::profile_rules::AppRule],
) {
    let Some(profile) = active else {
        ui_kit::page_header(ui, dc, "Overview", "");
        // Refused or another version is explained above; only a real wait
        // says "waiting".
        if connection_problem(control).is_none() {
            ui.label(
                egui::RichText::new(crate::problems::WAITING_FOR_SERVICE)
                    .size(12.0)
                    .color(dc.muted),
            );
        }
        return;
    };
    ui_kit::page_header(
        ui,
        dc,
        &profile.name,
        "Everything this profile changes. Click a tile to change it.",
    );
    let Some(look) = ui_state.look.current().cloned() else {
        return;
    };
    let warn = |key: &str| {
        look.thresholds
            .get(key)
            .and_then(|t| t.warn)
            .map_or_else(|| "off".to_owned(), |v| format!("{v} °C"))
    };
    let lighting = lighting_summary(aura);
    let power = active
        .and_then(|p| p.part.power_plan.as_deref())
        .map_or_else(
            || "Power plan left as it is".to_owned(),
            |id| scheme_name(id, &control.power_schemes()),
        );
    let look_tiles = [
        (
            Page::Dashboard,
            Icon::Dashboard,
            "Dashboard",
            format!(
                "{} · {} panels",
                profile_look::theme_name(&look.theme),
                look.visible_panels.len()
            ),
            Some(theme::AppTheme::from_key(&look.theme).accent),
        ),
        (
            Page::Overlay,
            Icon::Overlay,
            "Overlay",
            if look.overlay_enabled {
                format!("Shown · {} metrics", look.overlay_metrics.len())
            } else {
                "Hidden".to_owned()
            },
            None,
        ),
        (
            Page::Alerts,
            Icon::Alerts,
            "Alerts",
            format!("CPU {} · GPU {}", warn("cpu"), warn("gpu")),
            None,
        ),
    ];
    let windows_tiles = [(
        Page::Desktop,
        Icon::Desktop,
        "Desktop",
        profile_look::desktop_summary(
            look.desktop_background.as_ref(),
            look.wallpaper_engine.as_ref(),
        ),
        None,
    )];
    let hardware_tiles = [
        (Page::Power, Icon::Power, "Power", power, None),
        {
            let (summary, dot) =
                tile_summary(control.domain_problem("fan"), dc, fans_summary(headers));
            (Page::Fans, Icon::Fans, "Fans", summary, dot)
        },
        {
            let problem = control
                .domain_problem("cpu_limit")
                .or_else(|| control.domain_problem("curve_opt"));
            let (summary, dot) = tile_summary(problem, dc, cpu_summary(cpu, co));
            (Page::Cpu, Icon::Cpu, "CPU", summary, dot)
        },
        {
            let (summary, dot) = tile_summary(control.domain_problem("gpu"), dc, gpu_summary(gpu));
            (Page::Gpu, Icon::Gpu, "Graphics card", summary, dot)
        },
        match lighting_problem(control) {
            Some(problem) => {
                let (summary, dot) = tile_summary(Some(&problem), dc, String::new());
                (Page::Lighting, Icon::Lighting, "Lighting", summary, dot)
            }
            None => (
                Page::Lighting,
                Icon::Lighting,
                "Lighting",
                lighting.0,
                lighting.1,
            ),
        },
        {
            // From the profile only — the Overview never asks the mouse.
            let chosen = profile
                .part
                .mouse
                .as_ref()
                .and_then(|m| m.profile)
                .map_or_else(
                    || "Left as it is".to_owned(),
                    |p| format!("Mouse profile {p}"),
                );
            let (summary, dot) = tile_summary(control.domain_problem("mouse"), dc, chosen);
            (Page::Mouse, Icon::Mouse, "Mouse", summary, dot)
        },
        {
            let chosen = profile
                .part
                .keyboard
                .as_ref()
                .and_then(|k| k.profile)
                .map_or_else(
                    || "Left as it is".to_owned(),
                    |p| format!("Keyboard profile {p}"),
                );
            let (summary, dot) = tile_summary(control.domain_problem("keyboard"), dc, chosen);
            (Page::Keyboard, Icon::Keyboard, "Keyboard", summary, dot)
        },
    ];
    let starts_with: Vec<&str> = rules
        .iter()
        .filter(|r| r.profile_id == profile.id)
        .map(|r| r.exe.as_str())
        .collect();
    let automation_tiles = [(
        Page::Rules,
        Icon::Rules,
        "Starts automatically with",
        if starts_with.is_empty() {
            "No app rules".to_owned()
        } else {
            starts_with.join(", ")
        },
        None,
    )];
    let gap = 12.0;
    let tile_w = ((ui.available_width() - 2.0 * gap) / 3.0).floor();
    let size = egui::vec2(tile_w, ui_kit::TILE_H);
    // Sections side by side when they fit in one row of three tiles:
    // Windows and Automation hold a tile each, so they share a row.
    let lines: [&[(&str, &[OverviewTile])]; 3] = [
        &[("RIGStats", &look_tiles[..])],
        &[
            ("Windows", &windows_tiles[..]),
            ("Automation", &automation_tiles[..]),
        ],
        &[("Hardware", &hardware_tiles[..])],
    ];
    let heading = |ui: &mut egui::Ui, title: &str| {
        ui.label(
            egui::RichText::new(title)
                .size(12.0)
                .strong()
                .color(dc.label),
        );
        ui.add_space(6.0);
    };
    let mut clicked = None;
    for line in lines {
        if let [(title, tiles)] = line {
            heading(ui, title);
            let visible: Vec<_> = tiles.iter().filter(|t| shown.has(t.0)).collect();
            for row in visible.chunks(3) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for t in row {
                        if overview_tile(ui, dc, size, t) {
                            clicked = Some(t.0);
                        }
                    }
                });
                ui.add_space(gap);
            }
        } else {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (title, tiles) in line {
                    ui.vertical(|ui| {
                        ui.set_width(tile_w);
                        heading(ui, title);
                        for t in tiles.iter().filter(|t| shown.has(t.0)) {
                            if overview_tile(ui, dc, size, t) {
                                clicked = Some(t.0);
                            }
                        }
                    });
                }
            });
            ui.add_space(gap);
        }
        ui.add_space(8.0);
    }
    if let Some(page) = clicked {
        ui_state.tab = page;
    }
}

/// A tile on the Overview: page, icon, title, summary, colour dot.
type OverviewTile = (Page, Icon, &'static str, String, Option<egui::Color32>);

/// One Overview tile; true when clicked.
fn overview_tile(ui: &mut egui::Ui, dc: &DialogColors, size: egui::Vec2, t: &OverviewTile) -> bool {
    let (_, icon, label, value, dot) = t;
    ui_kit::tile(ui, dc, size, *icon, label, value, *dot).clicked()
}

fn power_tab(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    open: &OpenRequests,
) -> bool {
    let mut saved = false;
    let power = control
        .capabilities
        .iter()
        .find(|c| c.domain == "power_plan");
    match power {
        // Refused or another version is explained above every page.
        None if !control.connected && connection_problem(control).is_some() => {}
        None if !control.connected => {
            ui.label(
                egui::RichText::new(crate::problems::WAITING_FOR_SERVICE)
                    .size(12.0)
                    .color(dc.muted),
            );
        }
        None => {
            ui.label(
                egui::RichText::new("No control capabilities reported.")
                    .size(12.0)
                    .color(dc.muted),
            );
        }
        Some(cap) if !cap.supported => match &cap.problem {
            Some(problem) => problem_notice(ui, dc, problem, open),
            None => {
                ui.label(
                    egui::RichText::new(
                        cap.reason
                            .as_deref()
                            .unwrap_or("Power plan control unavailable."),
                    )
                    .size(12.0)
                    .color(dc.muted),
                );
            }
        },
        Some(_) => {
            let Some(active) = control.active() else {
                return false;
            };
            // The plan the active profile sets — not the profile's own
            // name, which a "Gaming" profile would have shown here.
            let current = active.part.power_plan.clone();
            let mut chosen = current.clone();
            let schemes = control.power_schemes();
            // Which plans exist is up to the PC (Modern Standby laptops
            // often have Balanced only); the service then uses the closest one.
            let missing = chosen
                .as_deref()
                .filter(|id| !schemes.iter().any(|s| s.id == *id))
                .map(|id| {
                    format!(
                        "{} isn't available on this PC — Windows' closest plan (usually \
                         Balanced) is used instead.",
                        power_plan_name(id)
                    )
                });
            ui_kit::group(ui, dc, None, missing.as_deref(), |g| {
                g.row(
                    "Windows power plan",
                    Some("Switched to with the profile; saved at once"),
                    |ui| {
                        let combo = ui_kit::dropdown(
                            ui,
                            "control_power_plan",
                            chosen.as_deref().map_or_else(
                                || "Leave unchanged".to_owned(),
                                |id| scheme_name(id, &schemes),
                            ),
                            |ui| {
                                ui.selectable_value(&mut chosen, None, "Leave unchanged");
                                for scheme in &schemes {
                                    ui.selectable_value(
                                        &mut chosen,
                                        Some(scheme.id.clone()),
                                        &scheme.name,
                                    );
                                }
                            },
                        );
                        // Re-read the schemes on open, so a plan just created
                        // in Windows' Power Options shows up without a restart.
                        if combo.clicked() {
                            let _ = cmd_tx.try_send(ControlCmd::Refresh);
                        }
                    },
                );
                g.row(
                    "Power plans",
                    Some("Plans are created and edited in Windows"),
                    |ui| {
                        if theme::dialog_btn_secondary(ui, "Open Power Options", dc).clicked() {
                            open_power_options();
                        }
                    },
                );
            });
            if chosen != current {
                saved = true;
                let mut profile = active.clone();
                profile.part.power_plan = chosen;
                let _ = cmd_tx.try_send(ControlCmd::SaveProfile {
                    profile: Box::new(profile),
                    apply: true,
                });
            }
            dry_run_note(ui, dc, control);
        }
    }
    saved
}

/// A copy of `source` as a new custom profile, with an id and a name no
/// existing profile uses ("Gaming copy", "Gaming copy 2", ...).
fn duplicate_profile(source: &Profile, existing: &[Profile]) -> Profile {
    let taken_name = |n: &str| existing.iter().any(|p| p.name.eq_ignore_ascii_case(n));
    let base = format!("{} copy", source.name);
    let name = (1..)
        .map(|i| {
            if i == 1 {
                base.clone()
            } else {
                format!("{base} {i}")
            }
        })
        .find(|n| !taken_name(n))
        .unwrap_or(base);
    Profile {
        id: unique_profile_id(&name, existing),
        name,
        icon: source.icon.clone(),
        builtin: false,
        part: source.part.clone(),
    }
}

/// A lower-case slug of `name`, made unique against the existing ids.
fn unique_profile_id(name: &str, existing: &[Profile]) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.is_empty() {
        "profile".to_owned()
    } else {
        slug
    };
    let taken = |id: &str| existing.iter().any(|p| p.id == id);
    (1..)
        .map(|i| {
            if i == 1 {
                slug.clone()
            } else {
                format!("{slug}-{i}")
            }
        })
        .find(|id| !taken(id))
        .unwrap_or(slug)
}

/// Windows' classic Power Options (`powercfg.cpl`) — still the only place
/// plans are created and edited; the Settings app only picks a mode. Runs
/// in the user's session from the app, not the service.
fn open_power_options() {
    let _ = std::process::Command::new("control.exe")
        .arg("powercfg.cpl")
        .spawn();
}

/// The scheme's own name as Windows reports it, else the readable symbolic
/// name (a scheme that is gone still shows something sensible).
fn scheme_name(id: &str, schemes: &[PowerScheme]) -> String {
    schemes
        .iter()
        .find(|s| s.id == id)
        .map_or_else(|| power_plan_name(id), |s| s.name.clone())
}

/// Readable name for `ProfilePart.power_plan`'s symbolic scheme names (see
/// `PowerPlanProvider` in the service).
fn power_plan_name(plan: &str) -> String {
    match plan {
        "power_saver" => "Power saver",
        "balanced" => "Balanced",
        "high_performance" => "High performance",
        "ultimate_performance" => "Ultimate performance",
        other => other,
    }
    .to_owned()
}

fn dry_run_note(ui: &mut egui::Ui, dc: &DialogColors, control: &ControlState) {
    if control.dry_run {
        ui.add_space(6.0);
        ui_kit::notice(ui, dc, &crate::problems::dry_run());
    }
}

#[allow(clippy::too_many_arguments)]
fn fans_tab(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    caps: &FanCaps,
    active: Option<&Profile>,
    saved: &BTreeMap<String, FanHeaderConfig>,
    ui_state: &mut ControlUi,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
) {
    let Some(profile) = active else {
        ui.label(
            egui::RichText::new("Choose a profile to edit its fan curves.")
                .size(12.0)
                .color(dc.muted),
        );
        return;
    };
    if !ui_state
        .selected_header
        .as_ref()
        .is_some_and(|id| caps.headers.iter().any(|h| &h.id == id))
    {
        ui_state.selected_header = caps.headers.first().map(|h| h.id.clone());
    }
    let Some(header) = ui_state
        .selected_header
        .as_ref()
        .and_then(|id| caps.headers.iter().find(|h| &h.id == id))
        .cloned()
    else {
        return;
    };

    let shown = ui_state
        .draft
        .as_ref()
        .map_or(saved, |d| &d.headers)
        .clone();
    // Other headers currently running the selected header's curve.
    let group = curve_group(&shown, &header.id);

    if let Some(notice) = &ui_state.notice {
        ui_kit::row_problem(ui, dc, Severity::Attention, notice);
        ui.add_space(6.0);
    }

    // ── Header picker + identify ─────────────────────────────────────────
    ui.horizontal(|ui| {
        let mut selected = header.id.clone();
        ui_kit::combo(ui, "fan_header", 260.0, header_name(&header, shown.get(&header.id)), |ui| {
                for h in &caps.headers {
                    let mode = match control.fan_duty.get(&h.id) {
                        Some(duty) if shown.contains_key(&h.id) => format!("   {duty:.0} %"),
                        _ if shown.contains_key(&h.id) => "   curve".to_owned(),
                        _ => "   BIOS".to_owned(),
                    };
                    ui.selectable_value(
                        &mut selected,
                        h.id.clone(),
                        format!("{}{mode}", header_name(h, shown.get(&h.id))),
                    );
                }
            });
        if selected != header.id {
            ui_state.selected_header = Some(selected);
            ui_state.dragging = None;
        }

        let spinning = ui_state
            .identifying
            .as_ref()
            .filter(|(id, until)| id == &header.id && Instant::now() < *until)
            .map(|(_, until)| *until);
        if let Some(until) = spinning {
            theme::dialog_btn_secondary_disabled(ui, "Spinning…", dc);
            ui.ctx()
                .request_repaint_after(until.saturating_duration_since(Instant::now()));
        } else if theme::dialog_btn_secondary(ui, "Identify", dc)
            .on_hover_text(
                "Run this fan channel at full speed for 5 seconds so you can hear which fan it is, and see which fan speeds respond.",
            )
            .clicked()
        {
            let _ = cmd_tx.try_send(ControlCmd::IdentifyFan(header.id.clone()));
            ui_state.identifying = Some((header.id.clone(), Instant::now() + IDENTIFY_DURATION));
        }
    });

    // ── What this channel drives (measured by identify, #207) ────────────
    let spinning = ui_state
        .identifying
        .as_ref()
        .is_some_and(|(id, until)| id == &header.id && Instant::now() < *until);
    let (text, detail) = if spinning {
        ("Measuring which fans speed up…".to_owned(), None)
    } else {
        identify_summary(
            control.fan_identified.get(&header.id).map(Vec::as_slice),
            header.drives.as_deref(),
        )
    };
    let resp = ui.label(egui::RichText::new(text).size(11.0).color(dc.muted));
    if let Some(detail) = detail {
        resp.on_hover_text(detail);
    }
    ui.add_space(8.0);

    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        let controlled = shown.contains_key(&header.id);

        ui.horizontal(|ui| {
            section_label(ui, dc, "Control");
            ui.add_space(8.0);
            let mut curve = controlled;
            let picked = ui_kit::segmented(
                ui,
                dc,
                &mut curve,
                &[(false, "Automatic (BIOS)"), (true, "Custom curve")],
            );
            if picked && !curve {
                ui_state.edit(&profile.id, saved).remove(&header.id);
            }
            if picked && curve {
                let preset = PRESETS[default_preset(&profile.id)].1;
                ui_state.edit(&profile.id, saved).insert(
                    header.id.clone(),
                    FanHeaderConfig {
                        label: None,
                        source: default_source(&caps.sources),
                        curve: preset.to_vec(),
                        hysteresis_c: 3.0,
                    },
                );
            }
            if let Some(duty) = control.fan_duty.get(&header.id).filter(|_| controlled) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("Now {duty:.0} %"))
                            .size(12.0)
                            .color(dc.text),
                    );
                });
            }
        });

        let Some(config) = shown.get(&header.id) else {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "The motherboard firmware controls this fan. Choose Custom curve to set your own.",
                )
                .size(11.0)
                .color(dc.muted),
            );
            return;
        };
        let mut config = config.clone();
        let before = config.clone();

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Label").size(12.0).color(dc.text));
            let mut label = config.label.clone().unwrap_or_default();
            ui.add(
                egui::TextEdit::singleline(&mut label)
                    .vertical_align(egui::Align::Center)
                    .hint_text("e.g. CPU cooler")
                    .desired_width(150.0)
.min_size(egui::vec2(0.0, ui_kit::CONTROL_H)),
            );
            config.label = Some(label.trim().to_owned()).filter(|l| !l.is_empty());

            ui.add_space(12.0);
            ui.label(egui::RichText::new("Follows").size(12.0).color(dc.text));
            ui_kit::combo(ui, "fan_source", 170.0, source_name(&config.source), |ui| {
                    for source in &caps.sources {
                        ui.selectable_value(
                            &mut config.source,
                            source.clone(),
                            source_name(source),
                        );
                    }
                });
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Start from").size(12.0).color(dc.text));
            for (name, points) in PRESETS {
                if theme::dialog_btn_secondary(ui, name, dc)
                    .on_hover_text(
                        "Replace this curve with a ready-made shape you can adjust. \
                         Not linked to the profiles of the same name.",
                    )
                    .clicked()
                {
                    config.curve = points.to_vec();
                }
            }
        });

        ui.add_space(8.0);
        curve_editor(
            ui,
            dc,
            &mut config.curve,
            &mut ui_state.dragging,
            control.fan_duty.get(&header.id).copied(),
        );
        ui.label(
            egui::RichText::new(
                "Drag a point to move it · double-click to add one · right-click to remove one",
            )
            .size(10.5)
            .color(dc.muted),
        );

        // ── Shared curve ─────────────────────────────────────────────────
        let mut toggled: Vec<(String, bool)> = Vec::new();
        if caps.headers.len() > 1 {
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new("Same curve on")
                        .size(12.0)
                        .color(dc.text),
                )
                .on_hover_text(
                    "Ticked fans run this exact curve and follow every change to it. \
                         Unticking hands a fan back to the BIOS.",
                );
                for other in caps.headers.iter().filter(|h| h.id != header.id) {
                    let mut on = group.contains(&other.id);
                    if ui
                        .checkbox(&mut on, header_name(other, shown.get(&other.id)))
                        .changed()
                    {
                        toggled.push((other.id.clone(), on));
                    }
                }
            });
        }

        if config != before || !toggled.is_empty() {
            let edits = ui_state.edit(&profile.id, saved);
            if !same_curve(&config, &before) {
                share_curve(edits, &group, &config);
            }
            for (id, on) in toggled {
                if on {
                    share_curve(edits, std::slice::from_ref(&id), &config);
                } else {
                    edits.remove(&id);
                }
            }
            edits.insert(header.id.clone(), config);
        }
    });

    if let Some(reason) = &control.safety_tripped {
        ui.add_space(8.0);
        ui_kit::notice(ui, dc, &crate::problems::safety_tripped(reason));
    }
    dry_run_note(ui, dc, control);
}

/// Draws the curve with draggable points; edits `curve` in place.
fn curve_editor(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    curve: &mut Vec<[f64; 2]>,
    dragging: &mut Option<usize>,
    live_duty: Option<f64>,
) {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 180.0),
        egui::Sense::click_and_drag(),
    );
    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(36.0, 6.0),
        rect.max - egui::vec2(8.0, 18.0),
    );
    let to_screen = |[t, d]: [f64; 2]| {
        egui::pos2(
            plot.left() + ((t - T_MIN) / (T_MAX - T_MIN)) as f32 * plot.width(),
            plot.bottom() - (d / 100.0) as f32 * plot.height(),
        )
    };
    let from_screen = |p: egui::Pos2| {
        [
            T_MIN + f64::from((p.x - plot.left()) / plot.width()) * (T_MAX - T_MIN),
            f64::from((plot.bottom() - p.y) / plot.height()) * 100.0,
        ]
    };
    let nearest = |curve: &[[f64; 2]], p: egui::Pos2| {
        curve
            .iter()
            .enumerate()
            .map(|(i, &pt)| (i, to_screen(pt).distance(p)))
            .filter(|&(_, d)| d <= GRAB_RADIUS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };

    // ── Interaction ──────────────────────────────────────────────────────
    if resp.drag_started() {
        *dragging = resp.interact_pointer_pos().and_then(|p| nearest(curve, p));
    }
    if resp.dragged() {
        if let (Some(i), Some(p)) = (*dragging, resp.interact_pointer_pos()) {
            let [t, d] = from_screen(p);
            move_point(curve, i, t, d);
        }
    }
    if resp.drag_stopped() {
        *dragging = None;
    }
    if resp.double_clicked() {
        if let Some(p) = resp
            .interact_pointer_pos()
            .filter(|&p| nearest(curve, p).is_none())
        {
            add_point(curve, from_screen(p));
        }
    }
    if resp.secondary_clicked() {
        if let Some(i) = resp.interact_pointer_pos().and_then(|p| nearest(curve, p)) {
            remove_point(curve, i);
        }
    }
    let hovered = dragging.or_else(|| resp.hover_pos().and_then(|p| nearest(curve, p)));
    if dragging.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    // ── Drawing ──────────────────────────────────────────────────────────
    let painter = ui.painter_at(rect);
    painter.rect_filled(plot, egui::CornerRadius::same(4), dc.inset);
    let grid = egui::Stroke::new(1.0_f32, dc.card_border);
    let font = egui::FontId::proportional(10.0);
    for t in (T_MIN as i32..=T_MAX as i32).step_by(10) {
        let x = to_screen([f64::from(t), 0.0]).x;
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            grid,
        );
        if t % 20 == 0 {
            // The last label hugs the right edge instead of being clipped.
            let align = if t == T_MAX as i32 {
                egui::Align2::RIGHT_TOP
            } else {
                egui::Align2::CENTER_TOP
            };
            painter.text(
                egui::pos2(x, plot.bottom() + 3.0),
                align,
                format!("{t}°C"),
                font.clone(),
                dc.muted,
            );
        }
    }
    for d in (0..=100).step_by(20) {
        let y = to_screen([T_MIN, f64::from(d)]).y;
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            grid,
        );
        painter.text(
            egui::pos2(plot.left() - 5.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{d}%"),
            font.clone(),
            dc.muted,
        );
    }

    if let Some(duty) = live_duty {
        let y = to_screen([T_MIN, duty]).y;
        painter.extend(egui::Shape::dashed_line(
            &[egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            egui::Stroke::new(1.0_f32, dc.muted),
            4.0,
            4.0,
        ));
    }

    if let (Some(first), Some(last)) = (curve.first(), curve.last()) {
        // The service holds the end duties outside the curve's range.
        let mut line = vec![to_screen([T_MIN, first[1]])];
        line.extend(curve.iter().map(|&p| to_screen(p)));
        line.push(to_screen([T_MAX, last[1]]));
        painter.add(egui::Shape::line(
            line,
            egui::Stroke::new(2.0_f32, CURVE_COLOR),
        ));
    }
    for (i, &pt) in curve.iter().enumerate() {
        let pos = to_screen(pt);
        let radius = if hovered == Some(i) { 6.0 } else { 4.5 };
        painter.circle(pos, radius, CURVE_COLOR, egui::Stroke::new(1.5_f32, dc.bg));
    }
    if let Some(i) = hovered {
        let [t, d] = curve[i];
        let pos = to_screen(curve[i]);
        let align = if pos.x > plot.center().x {
            egui::Align2::RIGHT_BOTTOM
        } else {
            egui::Align2::LEFT_BOTTOM
        };
        painter.text(
            pos + egui::vec2(if pos.x > plot.center().x { -8.0 } else { 8.0 }, -8.0),
            align,
            format!("{t:.0}°C · {d:.0}%"),
            egui::FontId::proportional(11.0),
            dc.title,
        );
    }
}

/// Moves point `i` to (`t`, `d`), rounded to whole degrees/percent and kept
/// strictly between its neighbours so the curve stays ascending.
fn move_point(curve: &mut [[f64; 2]], i: usize, t: f64, d: f64) {
    let lo = if i == 0 { T_MIN } else { curve[i - 1][0] + 1.0 };
    let hi = if i + 1 == curve.len() {
        T_MAX
    } else {
        curve[i + 1][0] - 1.0
    };
    // Neighbours closer than 2 °C (hand-edited data): don't move sideways.
    let (lo, hi) = if lo > hi {
        (curve[i][0], curve[i][0])
    } else {
        (lo, hi)
    };
    curve[i] = [t.round().clamp(lo, hi), d.round().clamp(0.0, 100.0)];
}

/// Inserts a point at its sorted position, unless one already sits within
/// 1 °C of it.
fn add_point(curve: &mut Vec<[f64; 2]>, [t, d]: [f64; 2]) {
    let t = t.round().clamp(T_MIN, T_MAX);
    if curve.iter().any(|p| (p[0] - t).abs() < 1.0) {
        return;
    }
    let at = curve.partition_point(|p| p[0] < t);
    curve.insert(at, [t, d.round().clamp(0.0, 100.0)]);
}

/// Removes point `i`, keeping at least two so a curve always has a slope.
fn remove_point(curve: &mut Vec<[f64; 2]>, i: usize) {
    if curve.len() > 2 && i < curve.len() {
        curve.remove(i);
    }
}

/// The line under the header picker, plus an RPM breakdown for its tooltip.
/// Channel numbers and RPM sensor numbers don't always match (one channel
/// can drive several fans), so the measured result is what to trust.
/// `measured` is this session's identify (with RPM); `stored` is what the
/// service remembered from an earlier one.
fn identify_summary(
    measured: Option<&[FanResponder]>,
    stored: Option<&[String]>,
) -> (String, Option<String>) {
    let labels: Option<Vec<&str>> = measured
        .map(|rs| rs.iter().map(|r| r.label.as_str()).collect())
        .or_else(|| stored.map(|s| s.iter().map(String::as_str).collect()));
    match labels.as_deref() {
        None => (
            "Use Identify to see which fans this channel drives.".to_owned(),
            None,
        ),
        Some([]) => (
            "No fan sped up: nothing connected, already at full speed, or not controllable."
                .to_owned(),
            None,
        ),
        Some(ls) => {
            let detail = match measured {
                Some(rs) => rs
                    .iter()
                    .map(|r| format!("{}: {:.0} to {:.0} rpm", r.label, r.before_rpm, r.peak_rpm))
                    .collect::<Vec<_>>()
                    .join("\n"),
                None => {
                    "Measured by an earlier Identify. Run it again if you moved fans.".to_owned()
                }
            };
            (format!("Drives: {}", ls.join(", ")), Some(detail))
        }
    }
}

/// Two headers share a curve when everything but their label matches.
fn same_curve(a: &FanHeaderConfig, b: &FanHeaderConfig) -> bool {
    a.source == b.source && a.curve == b.curve && a.hysteresis_c == b.hysteresis_c
}

/// The other headers running exactly `id`'s curve. Groups aren't stored —
/// the service keeps one curve per header — they are whatever matches.
fn curve_group(headers: &BTreeMap<String, FanHeaderConfig>, id: &str) -> Vec<String> {
    let Some(this) = headers.get(id) else {
        return Vec::new();
    };
    headers
        .iter()
        .filter(|(other, config)| other.as_str() != id && same_curve(config, this))
        .map(|(other, _)| other.clone())
        .collect()
}

/// Gives each header in `ids` `config`'s curve and source, keeping its own
/// label (a header on BIOS control is switched to the curve).
fn share_curve(
    headers: &mut BTreeMap<String, FanHeaderConfig>,
    ids: &[String],
    config: &FanHeaderConfig,
) {
    for id in ids {
        let label = headers.get(id).and_then(|c| c.label.clone());
        headers.insert(
            id.clone(),
            FanHeaderConfig {
                label,
                ..config.clone()
            },
        );
    }
}

fn default_preset(profile_id: &str) -> usize {
    match profile_id {
        "silent" | "eco" => 0,
        "gaming" => 2,
        _ => 1,
    }
}

fn default_source(sources: &[String]) -> String {
    sources
        .iter()
        .find(|s| *s == "cpu_package")
        .or_else(|| sources.first())
        .cloned()
        .unwrap_or_else(|| "cpu_package".to_owned())
}

fn source_name(source: &str) -> String {
    match source {
        "cpu_package" => "CPU temperature".to_owned(),
        "gpu" => "GPU temperature".to_owned(),
        _ => source.strip_prefix("mb:").map_or_else(
            || source.to_owned(),
            |label| format!("Motherboard: {label}"),
        ),
    }
}

fn header_name(cap: &FanHeaderCap, config: Option<&FanHeaderConfig>) -> String {
    match config.and_then(|c| c.label.as_deref()) {
        Some(label) => format!("{} — {label}", cap.label),
        None => cap.label.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_summaries_say_what_a_profile_does() {
        assert_eq!(fans_summary(&BTreeMap::new()), "Motherboard default");
        let mut headers = BTreeMap::new();
        let curve = FanHeaderConfig {
            label: None,
            source: "cpu_package".into(),
            curve: vec![[30.0, 30.0], [80.0, 100.0]],
            hysteresis_c: 3.0,
        };
        headers.insert("a".to_owned(), curve.clone());
        assert_eq!(fans_summary(&headers), "Custom curve on 1 fan header");
        headers.insert("b".to_owned(), curve);
        assert_eq!(fans_summary(&headers), "Custom curves on 2 fan headers");

        let stock = AmdCpuLimit::default();
        assert_eq!(
            cpu_summary(&stock, &CurveOptPart::default()),
            "Stock limits"
        );
        let eco = AmdCpuLimit {
            ppt_w: Some(142.0),
            ..Default::default()
        };
        let co = CurveOptPart {
            all_core: Some(-10),
            per_core: None,
        };
        assert_eq!(
            cpu_summary(&eco, &co),
            "142 W power limit · Curve Optimizer"
        );

        assert_eq!(gpu_summary(&BTreeMap::new()), "Stock power limit");
        assert_eq!(
            gpu_summary(&BTreeMap::from([("gpu".to_owned(), -15)])),
            "Power limit -15 %"
        );
    }

    #[test]
    fn lighting_summary_names_the_effect_and_its_colour() {
        assert_eq!(lighting_summary(None), ("Left as it is".to_owned(), None));
        let red = AuraPart {
            effect: Some("static".into()),
            color: Some("#ff0000".into()),
            ..Default::default()
        };
        assert_eq!(
            lighting_summary(Some(&red)),
            (
                "Static".to_owned(),
                Some(egui::Color32::from_rgb(255, 0, 0))
            )
        );
        let cycle = AuraPart {
            effect: Some("spectrum_cycle".into()),
            ..Default::default()
        };
        assert_eq!(
            lighting_summary(Some(&cycle)),
            ("Colour cycle".to_owned(), None),
            "no single colour"
        );
    }

    #[test]
    fn device_kinds_read_as_words() {
        assert_eq!(device_kind("monitor_light_bar"), "Monitor light bar");
        assert_eq!(device_kind("keyboard"), "Keyboard");
        assert_eq!(device_kind(""), "");
    }

    fn curve() -> Vec<[f64; 2]> {
        vec![[30.0, 25.0], [50.0, 40.0], [70.0, 80.0]]
    }

    #[test]
    fn move_point_rounds_and_stays_between_neighbours() {
        let mut c = curve();
        move_point(&mut c, 1, 75.4, 55.6);
        assert_eq!(c[1], [69.0, 56.0]);
        move_point(&mut c, 1, 10.0, -5.0);
        assert_eq!(c[1], [31.0, 0.0]);
    }

    #[test]
    fn move_point_clamps_the_ends_to_the_axis_range() {
        let mut c = curve();
        move_point(&mut c, 0, 5.0, 120.0);
        assert_eq!(c[0], [T_MIN, 100.0]);
        move_point(&mut c, 2, 140.0, 50.0);
        assert_eq!(c[2], [T_MAX, 50.0]);
    }

    #[test]
    fn move_point_does_not_panic_on_crowded_neighbours() {
        let mut c = vec![[50.0, 30.0], [50.5, 40.0], [51.0, 50.0]];
        move_point(&mut c, 1, 80.0, 45.0);
        assert_eq!(c[1], [50.5, 45.0]);
    }

    #[test]
    fn add_point_inserts_sorted_and_skips_duplicates() {
        let mut c = curve();
        add_point(&mut c, [60.2, 61.0]);
        assert_eq!(c[2], [60.0, 61.0]);
        assert_eq!(c.len(), 4);
        add_point(&mut c, [60.4, 10.0]);
        assert_eq!(c.len(), 4);
    }

    #[test]
    fn remove_point_keeps_at_least_two() {
        let mut c = curve();
        remove_point(&mut c, 0);
        remove_point(&mut c, 0);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn presets_are_valid_ascending_curves() {
        for (_, points) in PRESETS {
            assert!(points.windows(2).all(|w| w[0][0] < w[1][0]));
            assert!(points.iter().all(|p| (0.0..=100.0).contains(&p[1])));
            assert!(points.iter().all(|p| p[1] >= 25.0));
        }
    }

    #[test]
    fn default_preset_follows_the_profile() {
        assert_eq!(PRESETS[default_preset("silent")].0, "Silent");
        assert_eq!(PRESETS[default_preset("gaming")].0, "Performance");
        assert_eq!(PRESETS[default_preset("my-own")].0, "Balanced");
    }

    #[test]
    fn default_source_prefers_the_cpu() {
        let sources = vec!["gpu".to_owned(), "cpu_package".to_owned()];
        assert_eq!(default_source(&sources), "cpu_package");
        assert_eq!(default_source(&["mb:System".to_owned()]), "mb:System");
        assert_eq!(default_source(&[]), "cpu_package");
    }

    #[test]
    fn source_names_are_readable() {
        assert_eq!(source_name("cpu_package"), "CPU temperature");
        assert_eq!(source_name("mb:System"), "Motherboard: System");
        assert_eq!(source_name("other"), "other");
    }

    fn header(label: Option<&str>, source: &str, curve: Vec<[f64; 2]>) -> FanHeaderConfig {
        FanHeaderConfig {
            label: label.map(str::to_owned),
            source: source.into(),
            curve,
            hysteresis_c: 3.0,
        }
    }

    #[test]
    fn curve_group_matches_curve_and_source_but_ignores_labels() {
        let mut headers = BTreeMap::new();
        headers.insert("a".to_owned(), header(Some("CPU"), "cpu_package", curve()));
        headers.insert(
            "b".to_owned(),
            header(Some("Front"), "cpu_package", curve()),
        );
        headers.insert("c".to_owned(), header(None, "gpu", curve()));
        headers.insert(
            "d".to_owned(),
            header(None, "cpu_package", vec![[40.0, 50.0], [80.0, 100.0]]),
        );

        assert_eq!(curve_group(&headers, "a"), vec!["b".to_owned()]);
        assert!(curve_group(&headers, "c").is_empty());
        assert!(curve_group(&headers, "missing").is_empty());
    }

    #[test]
    fn share_curve_copies_the_curve_but_keeps_each_label() {
        let mut headers = BTreeMap::new();
        headers.insert("b".to_owned(), header(Some("Front"), "gpu", curve()));
        let shared = header(
            Some("CPU"),
            "cpu_package",
            vec![[30.0, 30.0], [80.0, 100.0]],
        );

        share_curve(&mut headers, &["b".to_owned(), "c".to_owned()], &shared);

        assert_eq!(headers["b"].label.as_deref(), Some("Front"));
        assert!(same_curve(&headers["b"], &shared));
        assert_eq!(headers["c"].label, None); // a BIOS header joins with no label
        assert!(same_curve(&headers["c"], &shared));
    }

    #[test]
    fn power_plan_names_are_readable() {
        assert_eq!(power_plan_name("high_performance"), "High performance");
        assert_eq!(power_plan_name("power_saver"), "Power saver");
        assert_eq!(power_plan_name("custom"), "custom");
    }

    #[test]
    fn identify_summary_names_the_driven_fans() {
        assert!(identify_summary(None, None).0.starts_with("Use Identify"));
        assert!(identify_summary(Some(&[]), None)
            .0
            .starts_with("No fan sped up"));

        let rs = [
            FanResponder {
                label: "Fan #2".into(),
                before_rpm: 996.0,
                peak_rpm: 2015.0,
            },
            FanResponder {
                label: "Fan #5".into(),
                before_rpm: 1007.0,
                peak_rpm: 1988.0,
            },
        ];
        let (text, detail) = identify_summary(Some(&rs), None);
        assert_eq!(text, "Drives: Fan #2, Fan #5");
        assert_eq!(
            detail.as_deref(),
            Some("Fan #2: 996 to 2015 rpm\nFan #5: 1007 to 1988 rpm")
        );

        // After a restart only the service's stored map is known.
        let stored = ["Fan #2".to_owned(), "Fan #5".to_owned()];
        let (text, detail) = identify_summary(None, Some(&stored));
        assert_eq!(text, "Drives: Fan #2, Fan #5");
        assert!(detail
            .unwrap()
            .starts_with("Measured by an earlier Identify"));
        // A fresh measurement wins over the stored one.
        assert!(identify_summary(Some(&[]), Some(&stored))
            .0
            .starts_with("No fan sped up"));
    }

    #[test]
    fn edit_starts_a_draft_from_the_saved_headers_once() {
        let mut ui = ControlUi::default();
        let mut saved = BTreeMap::new();
        saved.insert(
            "h".to_owned(),
            FanHeaderConfig {
                label: None,
                source: "gpu".into(),
                curve: curve(),
                hysteresis_c: 3.0,
            },
        );
        ui.edit("gaming", &saved).remove("h");
        assert!(ui.edit("gaming", &saved).is_empty()); // the draft persists, not re-copied
        assert_eq!(ui.draft.as_ref().unwrap().profile_id, "gaming");
    }
    const STOCK: AmdLimitValues = AmdLimitValues {
        ppt_w: 162.0,
        tdc_a: 120.0,
        edc_a: 180.0,
    };

    #[test]
    fn a_limit_at_or_above_the_bios_value_is_stored_as_none() {
        assert_eq!(below_stock(88.4, 162.0), Some(88.0));
        assert_eq!(below_stock(162.0, 162.0), None);
        assert_eq!(below_stock(161.7, 162.0), None); // rounds to the BIOS value
    }

    #[test]
    fn presets_never_raise_a_limit_above_the_bios_value() {
        let eco = preset_limits(CPU_PRESETS[1].1, &STOCK);
        assert_eq!(eco.ppt_w, Some(88.0));
        assert_eq!(eco.edc_a, Some(150.0));

        // A 65 W CPU: "105 W Eco" is above its BIOS values entirely.
        let small = AmdLimitValues {
            ppt_w: 88.0,
            tdc_a: 75.0,
            edc_a: 150.0,
        };
        assert!(preset_limits(CPU_PRESETS[0].1, &small).is_stock());
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
    fn rx9070() -> GpuAdapterCap {
        GpuAdapterCap {
            id: "pci-9070".into(),
            name: "AMD Radeon RX 9070 XT".into(),
            min: -30,
            max: 10,
            step: 1,
            default: 0,
            original: 0,
            current: 0,
        }
    }

    #[test]
    fn gpu_percentages_read_naturally() {
        assert_eq!(pct_text(-15), "-15 %");
        assert_eq!(pct_text(5), "+5 %");
        assert_eq!(pct_text(0), "0 %");
    }

    #[test]
    fn a_gpu_limit_at_the_original_value_is_stored_as_none() {
        let mut limits = BTreeMap::new();
        set_gpu_limit(&mut limits, &rx9070(), -15);
        assert_eq!(limits.get("pci-9070"), Some(&-15));
        set_gpu_limit(&mut limits, &rx9070(), 0);
        assert!(limits.is_empty());
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
    fn curve_optimizer_summary_collapses_equal_cores() {
        assert_eq!(co_summary(&[-15; 8]), "all cores -15");
        assert_eq!(co_summary(&[0; 8]), "all cores 0");
        assert_eq!(co_summary(&[-10, -15]), "-10 · -15");
        assert_eq!(co_summary(&[]), "—");
    }

    #[test]
    fn an_empty_per_core_map_is_the_same_as_none() {
        let with_empty = CurveOptPart {
            all_core: Some(-10),
            per_core: Some(BTreeMap::new()),
        };
        assert_eq!(
            normalized_co(&with_empty),
            CurveOptPart {
                all_core: Some(-10),
                per_core: None
            }
        );
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
    fn per_core_values_are_never_hidden() {
        let per_core = CurveOptPart {
            all_core: None,
            per_core: Some(BTreeMap::from([("3".to_owned(), -10)])),
        };
        assert!(per_core_shown(false, &per_core));
        assert!(!per_core_shown(false, &CurveOptPart::default()));
        assert!(per_core_shown(true, &CurveOptPart::default()));
    }
    fn named(id: &str, name: &str) -> Profile {
        Profile {
            id: id.into(),
            name: name.into(),
            icon: Some("bolt".into()),
            builtin: true,
            part: rigstats_backend::control::ProfilePart {
                power_plan: Some("high_performance".into()),
                ..Default::default()
            },
        }
    }

    #[test]
    fn duplicates_get_a_free_name_and_id_and_are_custom() {
        let gaming = named("gaming", "Gaming");
        let mut existing = vec![gaming.clone()];

        let first = duplicate_profile(&gaming, &existing);
        assert_eq!(
            (first.id.as_str(), first.name.as_str()),
            ("gaming-copy", "Gaming copy")
        );
        assert!(!first.builtin);
        assert_eq!(first.part, gaming.part);
        assert_eq!(first.icon, gaming.icon);

        existing.push(first);
        let second = duplicate_profile(&gaming, &existing);
        assert_eq!(
            (second.id.as_str(), second.name.as_str()),
            ("gaming-copy-2", "Gaming copy 2")
        );
    }

    #[test]
    fn profile_ids_are_slugs() {
        assert_eq!(
            unique_profile_id("Late Night  Gaming!", &[]),
            "late-night-gaming"
        );
        assert_eq!(unique_profile_id("???", &[]), "profile");
        assert_eq!(
            unique_profile_id("Gaming", &[named("gaming", "Gaming")]),
            "gaming-2"
        );
    }

    #[test]
    fn scheme_names_come_from_windows_with_a_readable_fallback() {
        let schemes = vec![PowerScheme {
            id: "balanced".into(),
            name: "Balanserad".into(),
        }];
        assert_eq!(scheme_name("balanced", &schemes), "Balanserad"); // localized Windows name
        assert_eq!(scheme_name("power_saver", &schemes), "Power saver");
    }
    #[test]
    fn lighting_colours_round_trip_and_default_to_white() {
        assert_eq!(parse_hex_color(Some("#ff0033")), [255, 0, 51]);
        assert_eq!(hex_color([255, 0, 51]), "#ff0033");
        assert_eq!(parse_hex_color(Some("red")), [255, 255, 255]);
        assert_eq!(parse_hex_color(None), [255, 255, 255]);
    }

    #[test]
    fn lighting_effect_names() {
        assert_eq!(effect_name(None), "Unchanged");
        assert_eq!(effect_name(Some("spectrum_cycle")), "Colour cycle");
    }

    #[test]
    fn the_lamp_and_the_effect_are_set_and_left_independently() {
        let lamp = LampPart {
            on: false,
            brightness: None,
            temperature: None,
        };
        // A lamp alone: the RGB is left as is.
        let part = with_lamp(None, Some(lamp.clone())).unwrap();
        assert_eq!(part.effect, None);
        // Adding an effect keeps the lamp; leaving the effect again too.
        let part = with_effect(Some(part), Some("static".into())).unwrap();
        assert_eq!(part.lamp, Some(lamp.clone()));
        let part = with_effect(Some(part), None).unwrap();
        assert_eq!((part.effect, part.lamp), (None, Some(lamp)));
        // Leaving both as is is no lighting part at all.
        assert_eq!(with_lamp(Some(AuraPart::default()), None), None);
        let effect_only = with_lamp(
            Some(AuraPart {
                effect: Some("off".into()),
                ..Default::default()
            }),
            None,
        );
        assert_eq!(effect_only.and_then(|p| p.effect).as_deref(), Some("off"));
    }

    fn lamp(on: bool) -> Option<LampPart> {
        Some(LampPart {
            on,
            brightness: None,
            temperature: None,
        })
    }

    #[test]
    fn a_colour_change_never_touches_a_lamp_switched_from_the_tray() {
        // The profile says lamp off; the tray switched it on. Changing the
        // colour must not send "lamp off" (the bug).
        let saved = AuraPart {
            effect: Some("static".into()),
            color: Some("#ff0000".into()),
            lamp: lamp(false),
            ..Default::default()
        };
        let edited = AuraPart {
            color: Some("#0000ff".into()),
            ..saved.clone()
        };
        let sent = preview_delta(Some(&saved), &edited).unwrap();
        assert_eq!(sent.lamp, None);
        assert_eq!(sent.color.as_deref(), Some("#0000ff"));

        // A lamp change sends only the lamp.
        let lamp_on = AuraPart {
            lamp: lamp(true),
            ..saved.clone()
        };
        let sent = preview_delta(Some(&saved), &lamp_on).unwrap();
        assert_eq!((sent.effect, sent.lamp), (None, lamp(true)));
        assert_eq!(preview_delta(Some(&saved), &saved), None);
    }

    #[test]
    fn previews_waiting_for_the_throttle_merge() {
        let colour = AuraPart {
            effect: Some("static".into()),
            color: Some("#00ff00".into()),
            ..Default::default()
        };
        let lamp_only = AuraPart {
            lamp: lamp(true),
            ..Default::default()
        };
        let merged = merge_preview(colour, lamp_only);
        assert_eq!(merged.color.as_deref(), Some("#00ff00"));
        assert_eq!(merged.lamp, lamp(true));
    }

    #[test]
    fn a_lamp_differing_from_the_profile_is_explained_until_edited() {
        let saved = AuraPart {
            lamp: lamp(false),
            ..Default::default()
        };
        let note = lamp_state_note(Some(true), Some(&saved), Some(&saved));
        assert!(note.is_some_and(|n| n.starts_with("The lamp is on now")));
        assert_eq!(
            lamp_state_note(Some(false), Some(&saved), Some(&saved)),
            None
        );
        // Edited: the lamp already shows the edit.
        let edited = AuraPart {
            lamp: lamp(true),
            ..Default::default()
        };
        assert_eq!(
            lamp_state_note(Some(true), Some(&saved), Some(&edited)),
            None
        );
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

    #[test]
    fn a_failed_domains_tile_shows_the_problem_in_its_colour() {
        let dc = DialogColors::dark();
        let crashed = Problem {
            id: "gpu_driver_crashed".into(),
            severity: rigstats_backend::control::Severity::Error,
            title: "The graphics driver crashed".into(),
            message: "…".into(),
            ..Default::default()
        };

        assert_eq!(
            tile_summary(Some(&crashed), &dc, "Stock power limit".into()),
            ("The graphics driver crashed".into(), Some(ui_kit::C_BAD))
        );
        assert_eq!(
            tile_summary(None, &dc, "Stock power limit".into()),
            ("Stock power limit".into(), None)
        );
    }
}
