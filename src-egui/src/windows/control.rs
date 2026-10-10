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

use crate::theme::{self, DialogColors};
use crate::windows::profile_look::{self, LookEditor, LookLink};
use crate::windows::ui_kit::{self, Icon};
use rigstats_backend::control::{
    AmdCpuLimit, AmdLimitValues, ApplyResult, AuraCaps, AuraPart, ControlCmd, ControlState,
    CpuLimitCaps, CpuLimitPart, CurveOptCaps, CurveOptPart, FanCaps, FanHeaderCap, FanHeaderConfig,
    FanPart, FanResponder, GpuAdapterCap, GpuAdapterConfig, GpuCaps, GpuPart, HueCaps, LampPart,
    PowerScheme, Profile,
};
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
    Power,
    Fans,
    Cpu,
    Gpu,
    Lighting,
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

    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "CPU power limits");
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!(
                "AMD Ryzen ({}). Lower limits run cooler and quieter; the maximum is what \
                 the BIOS set. New limits are tried first and revert by themselves unless \
                 you keep them.",
                caps.generation
            ))
            .size(11.0)
            .color(dc.muted),
        );
        ui.add_space(10.0);

        // No edits while a try is running: Keep/Undo in the footer first.
        ui.add_enabled_ui(control.preview.is_none(), |ui| {
            ui.horizontal(|ui| {
                if theme::dialog_btn_secondary(ui, "BIOS", dc).clicked() {
                    limits = AmdCpuLimit::default();
                }
                for (name, values) in CPU_PRESETS {
                    if theme::dialog_btn_secondary(ui, name, dc).clicked() {
                        limits = preset_limits(values, &caps.stock);
                    }
                }
            });
            ui.add_space(8.0);
            limit_row(
                ui,
                dc,
                "PPT (package power)",
                "W",
                &mut limits.ppt_w,
                caps.min.ppt_w,
                caps.stock.ppt_w,
            );
            limit_row(
                ui,
                dc,
                "TDC (sustained current)",
                "A",
                &mut limits.tdc_a,
                caps.min.tdc_a,
                caps.stock.tdc_a,
            );
            limit_row(
                ui,
                dc,
                "EDC (peak current)",
                "A",
                &mut limits.edc_a,
                caps.min.edc_a,
                caps.stock.edc_a,
            );
        });

        ui.add_space(8.0);
        let now = caps.current;
        ui.label(
            egui::RichText::new(format!(
                "In force now: PPT {:.0} W · TDC {:.0} A · EDC {:.0} A",
                now.ppt_w, now.tdc_a, now.edc_a
            ))
            .size(11.0)
            .color(dc.muted),
        );
        dry_run_note(ui, dc, control);
    });

    if limits != before {
        ui_state.cpu_draft = Some(CpuDraft { profile_id, limits });
    }
}

/// The boot-crash guard's notice, on its own above the CPU tab's cards — so
/// it shows whichever of CPU limits / Curve Optimizer this CPU supports, and
/// can't be mistaken for the Curve Optimizer's standing risk warning.
fn crash_guard_banner(ui: &mut egui::Ui, control: &ControlState) {
    let Some(notice) = &control.crash_guard_notice else {
        return;
    };
    let red = egui::Color32::from_rgb(0xff, 0x77, 0x77);
    egui::Frame::new()
        .fill(red.gamma_multiply(0.12))
        .stroke(egui::Stroke::new(1.0_f32, red))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new("Reverted after a restart")
                    .size(12.0)
                    .strong()
                    .color(red),
            );
            ui.add_space(2.0);
            ui.label(egui::RichText::new(notice).size(11.0).color(red));
        });
    ui.add_space(10.0);
}

/// The service couldn't read profiles.json and recovered it (#221) — above
/// every tab, until profiles are saved again.
fn profiles_notice_banner(ui: &mut egui::Ui, control: &ControlState) {
    let Some(notice) = &control.profiles_notice else {
        return;
    };
    let amber = egui::Color32::from_rgb(0xff, 0xb3, 0x47);
    egui::Frame::new()
        .fill(amber.gamma_multiply(0.12))
        .stroke(egui::Stroke::new(1.0_f32, amber))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new("Profiles recovered")
                    .size(12.0)
                    .strong()
                    .color(amber),
            );
            ui.add_space(2.0);
            ui.label(egui::RichText::new(notice).size(11.0).color(amber));
        });
    ui.add_space(10.0);
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

/// Curve Optimizer (#191) in the CPU tab: all-core, optionally per core.
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

    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "Curve Optimizer (undervolt)");
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "Negative offsets lower the CPU voltage: cooler, and often faster under \
                 load. Too far and the PC becomes unstable or crashes, so start small \
                 (−5 … −15) and test under load. A change runs for 15 s unless you keep \
                 it; if the PC restarts within 3 minutes of a change, it is not applied \
                 again at start-up.",
            )
            .size(11.0)
            .color(egui::Color32::from_rgb(0xff, 0xb3, 0x47)),
        );
        ui.add_space(10.0);
        ui.add_enabled_ui(control.preview.is_none(), |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("All cores").size(12.0).color(dc.muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let text = match part.all_core {
                        Some(v) => co_text(v),
                        None => "BIOS".to_owned(),
                    };
                    ui.add_sized(
                        [86.0, 18.0],
                        egui::Label::new(egui::RichText::new(text).size(12.0).color(dc.text)),
                    );
                    let mut v = part.all_core.unwrap_or(0);
                    let slider = egui::Slider::new(&mut v, caps.min..=caps.max)
                        .show_value(false)
                        .trailing_fill(true);
                    if ui.add(slider).changed() {
                        part.all_core = Some(v);
                    }
                    if theme::dialog_btn_secondary(ui, "BIOS", dc).clicked() {
                        part = CurveOptPart::default();
                    }
                });
            });
            if caps.per_core {
                ui.add_space(4.0);
                // Per-core values in the profile always show their rows;
                // unticking removes them (a draft, tried like any change).
                let mut shown = per_core_shown(ui_state.co_per_core, &part);
                if ui
                    .checkbox(
                        &mut shown,
                        egui::RichText::new("Per core").size(12.0).color(dc.muted),
                    )
                    .changed()
                {
                    ui_state.co_per_core = shown;
                    if !shown {
                        part.per_core = None;
                    }
                }
                if shown {
                    for core in 0..caps.cores {
                        co_core_row(ui, dc, core, &mut part, bios(core), caps.min..=caps.max);
                    }
                }
            }
        });
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(format!("In force now: {}", co_summary(&caps.current)))
                .size(11.0)
                .color(dc.muted),
        );
        dry_run_note(ui, dc, control);
    });

    let part = normalized_co(&part);
    if part != normalized_co(&before) {
        ui_state.co_draft = Some(CoDraft { profile_id, part });
    }
}

/// One core: its own offset, or "(all)" when it follows the all-core value.
fn co_core_row(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    core: usize,
    part: &mut CurveOptPart,
    bios: i32,
    range: std::ops::RangeInclusive<i32>,
) {
    let key = core.to_string();
    let own = part.per_core.as_ref().and_then(|m| m.get(&key).copied());
    let inherited = part.all_core.unwrap_or(bios);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("Core {core}"))
                .size(12.0)
                .color(dc.muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let text = match own {
                Some(v) => co_text(v),
                None => format!("{} (all)", co_text(inherited)),
            };
            ui.add_sized(
                [86.0, 18.0],
                egui::Label::new(egui::RichText::new(text).size(12.0).color(dc.text)),
            );
            let mut v = own.unwrap_or(inherited);
            let slider = egui::Slider::new(&mut v, range)
                .show_value(false)
                .trailing_fill(true);
            if ui.add(slider).changed() {
                part.per_core
                    .get_or_insert_with(BTreeMap::new)
                    .insert(key.clone(), v);
            }
            if own.is_some() && theme::dialog_btn_secondary(ui, "All", dc).clicked() {
                if let Some(map) = part.per_core.as_mut() {
                    map.remove(&key);
                }
            }
        });
    });
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
fn lighting_tab(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    caps: Option<&AuraCaps>,
    saved: Option<&AuraPart>,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ui_state: &mut ControlUi,
) {
    let hue = control.hue_caps();
    let Some(caps) = caps else {
        // Present but taken by another app — explained, not hidden.
        let reason = match (control.aura_unavailable().as_deref(), &hue) {
            (Some(blocked), _) => blocked.to_owned(),
            (None, Some(_)) => {
                "No RGB device found. Pair a Hue Bridge below to light the room.".to_owned()
            }
            (None, None) => "Lighting is unavailable.".to_owned(),
        };
        ui_kit::group(ui, dc, None, None, |g| {
            g.block(|ui| {
                ui.label(egui::RichText::new(reason).size(12.0).color(WARN_TEXT));
            });
        });
        if let Some(hue) = &hue {
            hue_card(ui, dc, control, hue, cmd_tx, &mut ui_state.hue_ip);
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
            g.row(&device.name, Some(&zones), |ui| {
                let (text, color) = if device.blocked.is_some() || device.problem.is_some() {
                    ("Needs attention", WARN_TEXT)
                } else {
                    (kind.as_str(), dc.muted)
                };
                ui.label(egui::RichText::new(text).size(12.0).color(color))
                    .on_hover_text(if device.firmware.is_empty() {
                        kind.clone()
                    } else {
                        format!("{kind}, firmware {}", device.firmware)
                    });
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
                    ui.label(egui::RichText::new(*w).size(11.0).color(WARN_TEXT));
                }
                if wdl_off {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(
                                "Ignores lighting changes: its Cross-device Lighting Toggle is \
                                 set to \"Device Lighting\".",
                            )
                            .size(11.0)
                            .color(WARN_TEXT),
                        );
                        if theme::dialog_btn_secondary(ui, "Let RIGStats control it", dc)
                            .on_hover_text(
                                "Switches the device to \"Aura Sync & Windows Dynamic Lighting\" \
                                 — the same setting as in Gear Link or Armoury Crate, saved in \
                                 the device.",
                            )
                            .clicked()
                        {
                            let _ = cmd_tx.try_send(ControlCmd::EnableWdl(device.id.clone()));
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
        hue_card(ui, dc, control, hue, cmd_tx, &mut ui_state.hue_ip);
    }
}

/// Amber text for something that needs the user (a device taken by another
/// app, a setting on the device that blocks us).
const WARN_TEXT: egui::Color32 = egui::Color32::from_rgb(0xff, 0xb3, 0x47);

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
                                    .desired_width(130.0)
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
                ui.label(egui::RichText::new(message).size(11.0).color(WARN_TEXT));
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

    card_frame(dc).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        section_label(ui, dc, "GPU power limit");
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "The same setting as the Power Limit in AMD Adrenalin or MSI Afterburner, \
                 relative to the driver default. Lower runs cooler and quieter; higher allows more boost. \
                 New limits are tried first and revert by themselves unless you keep them.",
            )
            .size(11.0)
            .color(dc.muted),
        );
        ui.add_space(10.0);
        ui.add_enabled_ui(control.preview.is_none(), |ui| {
            for adapter in &caps.adapters {
                gpu_limit_row(ui, dc, adapter, &mut limits);
            }
        });
        ui.add_space(8.0);
        let now = caps
            .adapters
            .iter()
            .map(|a| format!("{} {}", a.name, pct_text(a.current)))
            .collect::<Vec<_>>()
            .join(" · ");
        ui.label(
            egui::RichText::new(format!("In force now: {now}"))
                .size(11.0)
                .color(dc.muted),
        );
        dry_run_note(ui, dc, control);
    });

    if limits != before {
        ui_state.gpu_draft = Some(GpuDraft { profile_id, limits });
    }
}

/// One adapter: name, an "Original" reset and a slider over the driver's
/// range. A value equal to the original is stored as no value at all.
fn gpu_limit_row(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    adapter: &GpuAdapterCap,
    limits: &mut BTreeMap<String, i32>,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(&adapter.name)
                .size(12.0)
                .color(dc.muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let text = match limits.get(&adapter.id) {
                Some(&v) => pct_text(v),
                None => format!("{} (original)", pct_text(adapter.original)),
            };
            ui.add_sized(
                [86.0, 18.0],
                egui::Label::new(egui::RichText::new(text).size(12.0).color(dc.text)),
            );
            let mut v = limits.get(&adapter.id).copied().unwrap_or(adapter.original);
            let slider = egui::Slider::new(&mut v, adapter.min..=adapter.max)
                .step_by(f64::from(adapter.step.max(1)))
                .show_value(false)
                .trailing_fill(true);
            if ui.add(slider).changed() {
                set_gpu_limit(limits, adapter, v);
            }
            if theme::dialog_btn_secondary(ui, "Original", dc).clicked() {
                limits.remove(&adapter.id);
            }
        });
    });
}

fn set_gpu_limit(limits: &mut BTreeMap<String, i32>, adapter: &GpuAdapterCap, value: i32) {
    if value == adapter.original {
        limits.remove(&adapter.id);
    } else {
        limits.insert(adapter.id.clone(), value);
    }
}

fn limit_row(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    label: &str,
    unit: &str,
    value: &mut Option<f64>,
    min: f64,
    stock: f64,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(12.0).color(dc.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let text = match value {
                Some(v) => format!("{v:.0} {unit}"),
                None => format!("{stock:.0} {unit} (BIOS)"),
            };
            ui.add_sized(
                [86.0, 18.0],
                egui::Label::new(egui::RichText::new(text).size(12.0).color(dc.text)),
            );
            let mut v = value.unwrap_or(stock) as f32;
            let slider = egui::Slider::new(&mut v, min as f32..=stock as f32)
                .step_by(1.0)
                .show_value(false)
                .trailing_fill(true);
            if ui.add(slider).changed() {
                *value = below_stock(f64::from(v), stock);
            }
        });
    });
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
    let lighting_shown = aura_caps.is_some() || aura_blocked.is_some() || hue_caps.is_some();
    let gpu_caps = control.gpu_caps();
    match ui_state.tab {
        Page::Fans if fan_caps.is_none() => ui_state.tab = Page::Overview,
        Page::Cpu if cpu_caps.is_none() && co_caps.is_none() => ui_state.tab = Page::Overview,
        Page::Gpu if gpu_caps.is_none() => ui_state.tab = Page::Overview,
        Page::Lighting if !lighting_shown => ui_state.tab = Page::Overview,
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
        let (dot, text) = if control.protocol_mismatch.is_some() {
            (
                egui::Color32::from_rgb(0xff, 0x55, 0x55),
                "Version mismatch",
            )
        } else if control.connected {
            (egui::Color32::from_rgb(0x30, 0xd1, 0x58), "Connected")
        } else if control.refused {
            (
                egui::Color32::from_rgb(0xff, 0x55, 0x55),
                "Refused by the service",
            )
        } else {
            (dc.muted, "Connecting…")
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
            if let Some(result) = &control.last_apply_result {
                if !result.ok {
                    ui.label(
                        egui::RichText::new(result.message.as_deref().unwrap_or("Apply failed."))
                            .size(11.0)
                            .color(egui::Color32::from_rgb(0xff, 0x77, 0x77)),
                    );
                    ui.add_space(6.0);
                }
            }
            // A request the service turned down (save, delete, reset, ...) —
            // shown until dismissed; the Hue card shows its own (#219).
            if let Some(error) = control.last_error.as_ref().filter(|e| {
                control.errors != ui_state.dismissed_errors
                    && control.hue_message.as_ref() != Some(*e)
            }) {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("The service reported an error: {error}"))
                            .size(11.0)
                            .color(egui::Color32::from_rgb(0xff, 0x77, 0x77)),
                    );
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
    let cpu_tab_shown = cpu_caps.is_some() || co_caps.is_some();
    let shown = PageShown {
        fans: fan_caps.is_some(),
        cpu: cpu_tab_shown,
        gpu: gpu_caps.is_some(),
        lighting: lighting_shown,
    };
    egui::SidePanel::left("control_nav")
        .resizable(false)
        .exact_width(ui_kit::SIDEBAR_W)
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin::same(10)))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                for (i, &(page, icon, label)) in PAGES.iter().enumerate() {
                    if !shown.has(page) {
                        continue;
                    }
                    match i {
                        1 => ui_kit::nav_header(ui, dc, "RIGStats"),
                        4 => ui_kit::nav_header(ui, dc, "Hardware"),
                        _ => {}
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
            profiles_notice_banner(ui, control);
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
                        crash_guard_banner(ui, control);
                        if let Some(caps) = &cpu_caps {
                            cpu_tab(ui, dc, control, caps, &saved_cpu, ui_state);
                            ui.add_space(10.0);
                        }
                        if let Some(caps) = &co_caps {
                            curve_opt_card(ui, dc, control, caps, &saved_co, ui_state);
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
                        );
                    }
                    Page::Gpu => {
                        ui_kit::page_header(
                            ui,
                            dc,
                            "Graphics card",
                            &format!("The graphics card's power limit in {profile_name}."),
                        );
                        if let Some(caps) = &gpu_caps {
                            gpu_tab(ui, dc, control, caps, &saved_gpu, ui_state);
                        }
                    }
                    Page::Power | Page::Fans => {
                        ui_kit::page_header(
                            ui,
                            dc,
                            "Power",
                            &format!("The Windows power plan in {profile_name}."),
                        );
                        power_tab(ui, dc, control, cmd_tx);
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
/// profile's RIGStats look, then its hardware.
const PAGES: &[(Page, Icon, &str)] = &[
    (Page::Overview, Icon::Overview, "Overview"),
    (Page::Dashboard, Icon::Dashboard, "Dashboard"),
    (Page::Overlay, Icon::Overlay, "Overlay"),
    (Page::Alerts, Icon::Alerts, "Alerts"),
    (Page::Power, Icon::Power, "Power"),
    (Page::Fans, Icon::Fans, "Fans"),
    (Page::Cpu, Icon::Cpu, "CPU"),
    (Page::Gpu, Icon::Gpu, "Graphics card"),
    (Page::Lighting, Icon::Lighting, "Lighting"),
];

/// Which hardware pages this PC has (the service's capabilities);
/// unsupported features are hidden, not greyed out.
struct PageShown {
    fans: bool,
    cpu: bool,
    gpu: bool,
    lighting: bool,
}

impl PageShown {
    fn has(&self, page: Page) -> bool {
        match page {
            Page::Fans => self.fans,
            Page::Cpu => self.cpu,
            Page::Gpu => self.gpu,
            Page::Lighting => self.lighting,
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
                        .desired_width(200.0)
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
                for profile in &control.profiles {
                    let is_active = active.map(|a| &a.id) == Some(&profile.id);
                    let resp = ui_kit::chip(ui, dc, &profile.name, is_active);
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
) {
    let Some(profile) = active else {
        ui_kit::page_header(ui, dc, "Overview", "");
        ui.label(
            egui::RichText::new("Waiting for the RIGStats service…")
                .size(12.0)
                .color(dc.muted),
        );
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
    let hardware_tiles = [
        (Page::Power, Icon::Power, "Power", power, None),
        (Page::Fans, Icon::Fans, "Fans", fans_summary(headers), None),
        (Page::Cpu, Icon::Cpu, "CPU", cpu_summary(cpu, co), None),
        (
            Page::Gpu,
            Icon::Gpu,
            "Graphics card",
            gpu_summary(gpu),
            None,
        ),
        (
            Page::Lighting,
            Icon::Lighting,
            "Lighting",
            lighting.0,
            lighting.1,
        ),
    ];
    let gap = 12.0;
    let tile_w = ((ui.available_width() - 2.0 * gap) / 3.0).floor();
    for (title, tiles) in [
        ("RIGStats", &look_tiles[..]),
        ("Hardware", &hardware_tiles[..]),
    ] {
        ui.label(
            egui::RichText::new(title)
                .size(12.0)
                .strong()
                .color(dc.label),
        );
        ui.add_space(6.0);
        let visible: Vec<_> = tiles.iter().filter(|t| shown.has(t.0)).collect();
        for row in visible.chunks(3) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (page, icon, label, value, dot) in row {
                    let resp =
                        ui_kit::tile(ui, dc, egui::vec2(tile_w, 92.0), *icon, label, value, *dot);
                    if resp.clicked() {
                        ui_state.tab = *page;
                    }
                }
            });
            ui.add_space(gap);
        }
        ui.add_space(8.0);
    }
}

fn power_tab(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
) {
    let power = control
        .capabilities
        .iter()
        .find(|c| c.domain == "power_plan");
    match power {
        None if !control.connected => {
            let text = if control.refused {
                "The sensor service refused this app (see the service log)."
            } else {
                "Waiting for the RIGStats service…"
            };
            ui.label(egui::RichText::new(text).size(12.0).color(dc.muted));
        }
        None => {
            ui.label(
                egui::RichText::new("No control capabilities reported.")
                    .size(12.0)
                    .color(dc.muted),
            );
        }
        Some(cap) if !cap.supported => {
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
        Some(_) => {
            let Some(active) = control.active() else {
                return;
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
        ui.label(
            egui::RichText::new(
                "Dry-run mode — the service is logging changes instead of applying them.",
            )
            .size(11.0)
            .color(dc.muted),
        );
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
        ui.label(
            egui::RichText::new(notice)
                .size(11.0)
                .color(egui::Color32::from_rgb(0xff, 0x99, 0x55)),
        );
        ui.add_space(6.0);
    }

    // ── Header picker + identify ─────────────────────────────────────────
    ui.horizontal(|ui| {
        let mut selected = header.id.clone();
        egui::ComboBox::from_id_salt("fan_header")
            .width(260.0)
            .selected_text(header_name(&header, shown.get(&header.id)))
            .show_ui(ui, |ui| {
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
                    "The motherboard firmware controls this fan. Choose Curve to set your own.",
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
                    .hint_text("e.g. CPU cooler")
                    .desired_width(150.0),
            );
            config.label = Some(label.trim().to_owned()).filter(|l| !l.is_empty());

            ui.add_space(12.0);
            ui.label(egui::RichText::new("Follows").size(12.0).color(dc.text));
            egui::ComboBox::from_id_salt("fan_source")
                .width(170.0)
                .selected_text(source_name(&config.source))
                .show_ui(ui, |ui| {
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
        ui.label(
            egui::RichText::new(format!(
                "Last safety override: {reason} — all fans ran at 100 %."
            ))
            .size(11.0)
            .color(egui::Color32::from_rgb(0xff, 0x99, 0x55)),
        );
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
}
