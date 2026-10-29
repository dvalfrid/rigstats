//! Control Center: the Power page — the Windows power plan (#187).

use super::{connection_problem, dry_run_note, problem_notice, ControlUi};
use crate::theme::{self, DialogColors};
use crate::windows::ui_kit::{self};
use crate::windows::OpenRequests;
use rigstats_backend::control::{ControlCmd, ControlState, PowerScheme};

pub(super) fn power_tab(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ui_state: &mut ControlUi,
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
                let mut profile = active.clone();
                profile.part.power_plan = chosen;
                saved = ui_state.send(
                    cmd_tx,
                    ControlCmd::SaveProfile {
                        profile: Box::new(profile),
                        apply: true,
                    },
                );
            }
            dry_run_note(ui, dc, control);
        }
    }
    saved
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
pub(super) fn scheme_name(id: &str, schemes: &[PowerScheme]) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_plan_names_are_readable() {
        assert_eq!(power_plan_name("high_performance"), "High performance");
        assert_eq!(power_plan_name("power_saver"), "Power saver");
        assert_eq!(power_plan_name("custom"), "custom");
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
}
