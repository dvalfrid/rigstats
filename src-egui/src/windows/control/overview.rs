//! Control Center: the Overview page — a tile per page summarising what the profile does (#440).

use super::lighting::{effect_name, parse_hex_color};
use super::power::scheme_name;
use super::{connection_problem, lighting_problem, ControlUi, Page, PageShown};
use crate::theme::{self, DialogColors};
use crate::windows::profile_look::{self};
use crate::windows::ui_kit::{self, Icon};
use rigstats_backend::control::{
    AmdCpuLimit, AuraPart, ControlState, CurveOptPart, FanHeaderConfig, Problem, Profile,
};
use std::collections::BTreeMap;

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
pub(super) fn overview_page(
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
                "{} · {} panels{}",
                profile_look::theme_name(&look.theme),
                look.visible_panels.len(),
                if look.floating_mode == Some(true) {
                    " · floating"
                } else {
                    ""
                }
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
        {
            // The profile's equalizer preset (#483); the rest of the
            // headset's settings are the same in every profile.
            let chosen = profile
                .part
                .headset
                .as_ref()
                .and_then(|h| h.eq_preset.clone())
                .map_or_else(|| "Left as it is".to_owned(), |p| format!("Equalizer {p}"));
            let (summary, dot) = tile_summary(control.domain_problem("headset"), dc, chosen);
            (Page::Headset, Icon::Headset, "Headset", summary, dot)
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
