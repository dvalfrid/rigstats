//! Control Center: the Fans page — curve editor, identify, presets (#188).

use super::{card_frame, dry_run_note, section_label, ControlUi};
use crate::theme::{self, DialogColors};
use crate::windows::ui_kit::{self};
use rigstats_backend::control::{
    ControlCmd, ControlState, FanCaps, FanHeaderCap, FanHeaderConfig, FanResponder, Profile,
    Severity,
};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Curve editor axes: temperature range shown, duty is always 0–100 %.
const T_MIN: f64 = 20.0;
const T_MAX: f64 = 100.0;
/// How close (px) the pointer must be to grab/remove a curve point.
const GRAB_RADIUS: f32 = 12.0;
const CURVE_COLOR: egui::Color32 = egui::Color32::from_rgb(0x3a, 0x9b, 0xff);
/// Mirrors `FanProvider.IdentifyDuration` in the service.
const IDENTIFY_DURATION: Duration = Duration::from_secs(5);

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

#[allow(clippy::too_many_arguments)]
pub(super) fn fans_tab(
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
            && ui_state.send(cmd_tx, ControlCmd::IdentifyFan(header.id.clone()))
        {
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
                    .align(egui::Align2::LEFT_CENTER)
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
}
