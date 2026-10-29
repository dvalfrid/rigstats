//! Control Center: the CPU page — PPT/TDC/EDC limits, Eco presets and Curve Optimizer (#189).

use super::{dry_run_note, CoDraft, ControlUi, CpuDraft};
use crate::theme::{self, DialogColors};
use crate::windows::ui_kit::{self};
use rigstats_backend::control::{
    AmdCpuLimit, AmdLimitValues, ControlState, CpuLimitCaps, CurveOptCaps, CurveOptPart, Profile,
};
use std::collections::BTreeMap;

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

pub(super) fn cpu_tab(
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

/// An empty per-core map means the same as none.
pub(super) fn normalized_co(part: &CurveOptPart) -> CurveOptPart {
    CurveOptPart {
        all_core: part.all_core,
        per_core: part.per_core.clone().filter(|m| !m.is_empty()),
    }
}

pub(super) fn saved_curve_opt(profile: Option<&Profile>) -> CurveOptPart {
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
pub(super) fn curve_opt_card(
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn per_core_values_are_never_hidden() {
        let per_core = CurveOptPart {
            all_core: None,
            per_core: Some(BTreeMap::from([("3".to_owned(), -10)])),
        };
        assert!(per_core_shown(false, &per_core));
        assert!(!per_core_shown(false, &CurveOptPart::default()));
        assert!(per_core_shown(true, &CurveOptPart::default()));
    }
}
