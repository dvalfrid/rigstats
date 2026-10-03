//! Control Center window — profiles on the left, capability-driven tabs on
//! the right: "Power" (#187), "Fans" (#188) and "CPU" (#189) — Fans and CPU
//! only when the service reports the capability; unsupported features are
//! hidden, not greyed out. Follows the dialog contract in `src-egui/src/windows/CLAUDE.md`:
//! three panels sharing `dialog_frame`, `DialogColors`, `theme::dialog_btn_*`.
//! Actions are fire-and-forget `ControlCmd`s sent to `control_task`; results
//! come back later as `ControlEvent`s already folded into `control` by the
//! time this renders. The only local state is [`ControlUi`]: the selected
//! tab/header and unsaved drafts of the active profile's fan curves and CPU
//! limits. CPU limit changes go through `preview`: the service reverts them
//! by itself unless "Keep" arrives in time.

use crate::theme::{self, DialogColors};
use crate::windows::settings::tab_btn;
use rigstats_backend::control::{
    AmdCpuLimit, AmdLimitValues, ApplyResult, ControlCmd, ControlState, CpuLimitCaps, CpuLimitPart,
    FanCaps, FanHeaderCap, FanHeaderConfig, FanPart, FanResponder, GpuAdapterCap, GpuAdapterConfig,
    GpuCaps, GpuPart, Profile,
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
enum Tab {
    #[default]
    Power,
    Fans,
    Cpu,
    Gpu,
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

/// Unsaved GPU power limits per adapter id; a missing adapter = its original value.
#[derive(Debug, Clone, PartialEq)]
struct GpuDraft {
    profile_id: String,
    limits: BTreeMap<String, i32>,
}

/// Per-window UI state, owned by `RigStatsApp` across frames.
#[derive(Debug, Default)]
pub struct ControlUi {
    tab: Tab,
    selected_header: Option<String>,
    draft: Option<FanDraft>,
    cpu_draft: Option<CpuDraft>,
    gpu_draft: Option<GpuDraft>,
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
}

#[derive(Debug)]
struct Awaiting {
    since: Instant,
    /// `last_apply_result` at click time — a new one is an answer.
    result: Option<ApplyResult>,
}

impl ControlUi {
    /// A Motherboard-panel fan was clicked: show the Fans tab on the header
    /// that drives it, or explain how to find it when it hasn't been
    /// identified yet.
    fn select_fan(&mut self, fan: &str, control: &ControlState) {
        self.tab = Tab::Fans;
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
        }
        true
    }

    /// CPU and GPU limit edits are tried first (preview, auto-revert).
    fn has_limit_draft(&self) -> bool {
        self.cpu_draft.is_some() || self.gpu_draft.is_some()
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

fn dialog_frame(dc: &DialogColors) -> egui::Frame {
    egui::Frame::new()
        .fill(dc.bg)
        .inner_margin(egui::Margin::same(0))
}

fn card_frame(dc: &DialogColors) -> egui::Frame {
    egui::Frame::new()
        .fill(dc.card)
        .stroke(egui::Stroke::new(1.0_f32, dc.card_border))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(14, 12))
}

fn section_label(ui: &mut egui::Ui, dc: &DialogColors, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(11.0)
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
        if let Some(notice) = &control.crash_guard_notice {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(notice)
                    .size(11.0)
                    .color(egui::Color32::from_rgb(0xff, 0xb3, 0x47)),
            );
        }
        ui.add_space(10.0);

        // No edits while a try is running: Keep/Undo in the footer first.
        ui.add_enabled_ui(control.preview.is_none(), |ui| {
            ui.horizontal(|ui| {
                let size = egui::vec2(84.0, 22.0);
                if theme::dialog_btn_secondary_compact(ui, "BIOS", dc, size).clicked() {
                    limits = AmdCpuLimit::default();
                }
                for (name, values) in CPU_PRESETS {
                    if theme::dialog_btn_secondary_compact(ui, name, dc, size).clicked() {
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
                "The same setting as the Power Limit in AMD Adrenalin, relative to the \
                 driver default. Lower runs cooler and quieter; higher allows more boost. \
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
            if theme::dialog_btn_secondary_compact(ui, "Original", dc, egui::vec2(70.0, 20.0))
                .clicked()
            {
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
) {
    dc.apply_to_ctx(ctx);
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    let fan_caps = control.fan_caps();
    if let Some(fan) = ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::new(SELECT_FAN_ID))) {
        ui_state.select_fan(&fan, control);
    }
    let cpu_caps = control.cpu_caps();
    let gpu_caps = control.gpu_caps();
    match ui_state.tab {
        Tab::Fans if fan_caps.is_none() => ui_state.tab = Tab::Power,
        Tab::Cpu if cpu_caps.is_none() => ui_state.tab = Tab::Power,
        Tab::Gpu if gpu_caps.is_none() => ui_state.tab = Tab::Power,
        _ => {}
    }
    let active = control.active();
    let saved_headers = saved_fan_headers(active);
    let saved_cpu = saved_cpu_limits(active);
    let saved_gpu = saved_gpu_limits(active);
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
    // A try just started or ended: the limits in force changed, so re-read
    // them ("In force now"). Ended without Keep: drop the CPU draft so the
    // sliders show what is in force again, not the values that were undone.
    if ui_state.track_preview(control.preview.is_some(), control.preview_reverted) {
        let _ = cmd_tx.try_send(ControlCmd::Refresh);
    }
    let dirty = ui_state.draft.is_some() || ui_state.has_limit_draft();
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
    egui::TopBottomPanel::top("control_hero")
        .frame(dialog_frame(dc).inner_margin(egui::Margin {
            left: 16,
            right: 16,
            top: 14,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Control Center")
                        .size(18.0)
                        .strong()
                        .color(dc.title),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (dot, text) = if control.protocol_mismatch.is_some() {
                        (
                            egui::Color32::from_rgb(0xff, 0x55, 0x55),
                            "Version mismatch".to_string(),
                        )
                    } else if control.connected {
                        (
                            egui::Color32::from_rgb(0x39, 0xff, 0x88),
                            "Connected".to_string(),
                        )
                    } else {
                        (dc.muted, "Connecting…".to_string())
                    };
                    ui.label(egui::RichText::new(text).size(11.0).color(dc.muted));
                    // Painted dot rather than a Unicode bullet glyph, which
                    // the embedded font doesn't have (renders as a tofu box)
                    // — same fix as the "Recording" indicator in
                    // windows/history.rs.
                    let (dot_rect, _) =
                        ui.allocate_exact_size(egui::vec2(8.0, 11.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 2.5, dot);
                });
            });
        });

    // ── Footer ───────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("control_footer")
        .frame(dialog_frame(dc).inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 8,
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::dialog_btn_primary(ui, "Close").clicked() {
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
                    // CPU limits are tried first (auto-revert); fan-only
                    // edits are saved directly, as before.
                    let label = if ui_state.has_limit_draft() {
                        "Try new limits"
                    } else {
                        "Save & apply"
                    };
                    if theme::dialog_btn_primary(ui, label).clicked() {
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
                    ui.add_space(6.0);
                    if theme::dialog_btn_secondary(ui, "Revert", dc).clicked() {
                        ui_state.draft = None;
                        ui_state.cpu_draft = None;
                        ui_state.gpu_draft = None;
                        ui_state.dragging = None;
                    }
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("Unsaved changes")
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

    // ── Profiles (left) ──────────────────────────────────────────────────
    egui::SidePanel::left("control_profiles")
        .resizable(false)
        .exact_width(180.0)
        .frame(dialog_frame(dc).inner_margin(egui::Margin::same(10)))
        .show(ctx, |ui| {
            section_label(ui, dc, "Profiles");
            ui.add_space(6.0);
            if control.profiles.is_empty() {
                ui.label(
                    egui::RichText::new("No profiles yet.")
                        .size(11.0)
                        .color(dc.muted),
                );
            }
            for profile in &control.profiles {
                let is_active = control.active_profile.as_deref() == Some(profile.id.as_str());
                let resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new(&profile.name)
                            .size(13.0)
                            .color(if is_active { dc.title } else { dc.text }),
                    )
                    .fill(if is_active {
                        dc.card
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .min_size(egui::vec2(ui.available_width(), 28.0)),
                );
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() && !is_active {
                    let _ = cmd_tx.try_send(ControlCmd::ApplyProfile(profile.id.clone()));
                }
                ui.add_space(2.0);
            }
        });

    // ── Tabs (central) ───────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(dialog_frame(dc).inner_margin(egui::Margin::same(14)))
        .show(ctx, |ui| {
            if fan_caps.is_some() || cpu_caps.is_some() || gpu_caps.is_some() {
                ui.horizontal(|ui| {
                    if tab_btn(ui, dc, "Power", ui_state.tab == Tab::Power, 90.0) {
                        ui_state.tab = Tab::Power;
                    }
                    if fan_caps.is_some()
                        && tab_btn(ui, dc, "Fans", ui_state.tab == Tab::Fans, 90.0)
                    {
                        ui_state.tab = Tab::Fans;
                    }
                    if cpu_caps.is_some() && tab_btn(ui, dc, "CPU", ui_state.tab == Tab::Cpu, 90.0)
                    {
                        ui_state.tab = Tab::Cpu;
                    }
                    if gpu_caps.is_some() && tab_btn(ui, dc, "GPU", ui_state.tab == Tab::Gpu, 90.0)
                    {
                        ui_state.tab = Tab::Gpu;
                    }
                });
                ui.add_space(10.0);
            }
            if let (Tab::Cpu, Some(caps)) = (ui_state.tab, &cpu_caps) {
                cpu_tab(ui, dc, control, caps, &saved_cpu, ui_state);
                return;
            }
            if let (Tab::Gpu, Some(caps)) = (ui_state.tab, &gpu_caps) {
                gpu_tab(ui, dc, control, caps, &saved_gpu, ui_state);
                return;
            }
            match (ui_state.tab, &fan_caps) {
                (Tab::Fans, Some(caps)) => {
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
                }
                _ => power_tab(ui, dc, control),
            }
        });

    if ctx.input(|i| i.viewport().close_requested()) {
        open.store(false, Ordering::Relaxed);
        ui_state.notice = None;
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

fn power_tab(ui: &mut egui::Ui, dc: &DialogColors, control: &ControlState) {
    let power = control
        .capabilities
        .iter()
        .find(|c| c.domain == "power_plan");
    match power {
        None if !control.connected => {
            ui.label(
                egui::RichText::new("Waiting for the RIGStats service…")
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
            card_frame(dc).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                section_label(ui, dc, "Power");
                ui.add_space(8.0);
                // The plan the active profile sets — not the profile's own
                // name, which a "Gaming" profile would have shown here.
                let plan = control
                    .active()
                    .and_then(|p| p.part.power_plan.as_deref())
                    .map_or_else(|| "—".to_owned(), power_plan_name);
                ui.label(
                    egui::RichText::new(format!("Active power plan: {plan}"))
                        .size(12.0)
                        .color(dc.text),
                );
                dry_run_note(ui, dc, control);
            });
        }
    }
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

    // ── Which profile is being edited ────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Fan curves for profile")
                .size(12.0)
                .color(dc.muted),
        );
        ui.label(
            egui::RichText::new(&profile.name)
                .size(12.0)
                .strong()
                .color(dc.title),
        )
        .on_hover_text(
            "Each profile keeps its own fan curves. Choosing another profile on the \
             left switches to it, and its curves are edited here.",
        );
    });
    ui.add_space(6.0);

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
            section_label(ui, dc, "CONTROL");
            ui.add_space(8.0);
            if tab_btn(ui, dc, "BIOS", !controlled, 80.0) && controlled {
                ui_state.edit(&profile.id, saved).remove(&header.id);
            }
            if tab_btn(ui, dc, "Curve", controlled, 80.0) && !controlled {
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
                if theme::dialog_btn_secondary_compact(ui, name, dc, egui::vec2(84.0, 22.0))
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
}
