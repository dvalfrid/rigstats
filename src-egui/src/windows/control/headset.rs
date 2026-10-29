//! Control Center: the Headset page — sidetone, noise reduction, equalizer (#428, #483).

use super::{battery_and_link, device_tabs, ControlUi};
use crate::theme::{self, DialogColors};
use crate::windows::ui_kit::{self};
use rigstats_backend::control::{
    ControlCmd, ControlState, HeadsetEq, HeadsetInfo, HeadsetPart, HeadsetSettings,
};
use rigstats_backend::lhm::Peripheral;
use std::time::Instant;

/// The Headset page (#428): each connected headset with settings RIGStats
/// knows — sidetone and microphone noise reduction — read from the headset
/// when the page opens and sent to it as soon as one changes (they are kept
/// in the headset, like GearLink's, not in a profile).
pub(super) fn headset_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ui_state: &mut ControlUi,
    peripherals: &[Peripheral],
    battery_levels: (u8, u8),
) {
    // Read when the page opens, and again whenever the Control Center
    // profile changes: its equalizer preset may have changed the headset
    // (#483). Showing the old values then would send them back with the
    // next change — seen live, a preset overwritten by a click.
    // Also when a switch's devices are done, after its answer (#495).
    let read_for = (control.active_profile.clone(), control.apply_seq);
    if !ui_state.headsets_asked || ui_state.headsets_read_for != read_for {
        ui_state.headsets_asked = true;
        ui_state.headsets_read_for = read_for;
        ui_state.headset_pending = None;
        ui_state.eq_pending = None;
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
    if let Some((id, pending)) = &ui_state.eq_pending {
        let landed = control
            .headsets
            .iter()
            .any(|h| &h.id == id && h.eq.as_ref() == Some(pending));
        if landed || !control.headset_busy {
            ui_state.eq_pending = None;
        }
    }
    headset_eq_profile(ui, dc, control, cmd_tx, ui_state);
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
    let tab = device_tabs(
        ui,
        dc,
        control.headsets.iter().map(|h| h.name.as_str()),
        &mut ui_state.headset_tab,
    );
    for headset in control.headsets.iter().skip(tab).take(1) {
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
        headset_eq_group(ui, dc, cmd_tx, ui_state, headset);
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
        if ui_state.send(cmd_tx, ControlCmd::SetHeadset { id, settings }) {
            ui_state.saved_at = Some((Instant::now(), "Sent to the headset"));
        }
    }
}

/// The Headset page's profile group (#483): which equalizer preset the
/// RIGStats profile switches the headsets to, saved at once in the profile.
/// Shown while the `headset` domain lists a headset with an equalizer.
fn headset_eq_profile(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ui_state: &mut ControlUi,
) {
    let headsets = control.headset_eqs();
    if headsets.is_empty() {
        return;
    }
    let Some(mut profile) = control.active().cloned() else {
        return;
    };
    // Every preset any connected headset has, in the first one's order.
    let mut names: Vec<String> = Vec::new();
    for name in headsets.iter().flat_map(|h| h.presets.iter()) {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    let chosen = profile
        .part
        .headset
        .as_ref()
        .and_then(|h| h.eq_preset.clone());
    let mut pick = chosen.clone();
    let footnote = format!(
        "Switching to {} puts the headset's equalizer on this preset. The sliders below change it now and \
         stay until a profile with a preset is switched to. A headset without the preset is left as it is.",
        profile.name
    );
    ui_kit::group(ui, dc, Some("Profile"), Some(&footnote), |g| {
        g.row(
            &format!("Equalizer in {}", profile.name),
            Some("Saved at once"),
            |ui| {
                let label = chosen
                    .clone()
                    .unwrap_or_else(|| "Leave unchanged".to_owned());
                ui_kit::dropdown(ui, "headset_eq_profile", label, |ui| {
                    ui.selectable_value(&mut pick, None, "Leave unchanged");
                    for name in &names {
                        ui.selectable_value(&mut pick, Some(name.clone()), name);
                    }
                });
            },
        );
    });
    if pick != chosen {
        profile.part.headset = pick.map(|p| HeadsetPart { eq_preset: Some(p) });
        let saved = ui_state.send(
            cmd_tx,
            ControlCmd::SaveProfile {
                profile: Box::new(profile),
                apply: true,
            },
        );
        // The control task runs commands in order, so this read comes after
        // the profile has put the preset on the headset.
        ui_state.eq_pending = None;
        let _ = cmd_tx.try_send(ControlCmd::ReadHeadsets);
        if saved {
            ui_state.saved_at = Some((Instant::now(), "Saved"));
        }
    }
}

/// One headset's equalizer (#483): on/off, a preset, a slider per band the
/// model uses (its own scale, flat in the middle) and Flat. Kept in the
/// headset; a switch, a preset or Flat is sent at once, a slider when let go.
fn headset_eq_group(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ui_state: &mut ControlUi,
    headset: &HeadsetInfo,
) {
    if headset.eq_max == 0 {
        return;
    }
    let Some(read) = headset.eq.clone() else {
        return;
    };
    let mut eq = match &ui_state.eq_pending {
        Some((id, pending)) if id == &headset.id => pending.clone(),
        _ => read,
    };
    let before = eq.clone();
    let first = usize::from(headset.eq_first_band);
    let flat = headset.eq_max / 2;
    let matching = headset
        .eq_presets
        .iter()
        .find(|p| p.gains.len() == eq.gains.len() && p.gains[first..] == eq.gains[first..]);
    let mut preset: Option<usize> = None;
    let mut flat_clicked = false;
    let mut release = false;
    let footnote = if headset.eq_verified {
        "Kept in the headset. Flat is the middle of each slider."
    } else {
        "Kept in the headset. Flat is the middle of each slider. The presets are G-Helper's, and the \
         equalizer is not yet verified on this model."
    };
    ui_kit::group(ui, dc, Some("Equalizer"), Some(footnote), |g| {
        g.row("Equalizer", None, |ui| {
            ui_kit::toggle(ui, dc, &mut eq.on);
        });
        g.row("Preset", Some("Sets every band"), |ui| {
            let label = matching.map_or("Custom", |p| p.name.as_str());
            ui_kit::dropdown(ui, ("headset_eq_preset", &headset.id), label, |ui| {
                for (i, p) in headset.eq_presets.iter().enumerate() {
                    let selected = matching.is_some_and(|m| m.name == p.name);
                    if ui.selectable_label(selected, &p.name).clicked() {
                        preset = Some(i);
                    }
                }
            });
        });
        for (band, name) in rigstats_backend::control::EQ_BANDS
            .iter()
            .enumerate()
            .skip(first)
        {
            if band >= eq.gains.len() {
                break;
            }
            g.row(name, None, |ui| {
                ui.add_enabled_ui(eq.on, |ui| {
                    let resp = ui_kit::slider_steps(ui, dc, &mut eq.gains[band], headset.eq_max);
                    release |= resp.drag_stopped() || (resp.changed() && !resp.dragged());
                });
            });
        }
        g.row("Flat", Some("Every band to the middle"), |ui| {
            if theme::dialog_btn_secondary(ui, "Flat", dc).clicked() {
                flat_clicked = true;
            }
        });
    });
    if let Some(i) = preset {
        eq.gains = headset.eq_presets[i].gains.clone();
        eq.on = true;
    }
    if flat_clicked {
        for gain in eq.gains.iter_mut().skip(first) {
            *gain = flat;
        }
    }
    if eq != before {
        // Only a band slider held down waits; everything else goes at once.
        let dragging = eq.on == before.on && preset.is_none() && !flat_clicked && !release;
        ui_state.eq_pending = Some((headset.id.clone(), eq.clone()));
        if !dragging {
            send_eq(cmd_tx, ui_state, &headset.id, eq);
        }
    } else if release {
        send_eq(cmd_tx, ui_state, &headset.id, eq);
    }
}

fn send_eq(
    cmd_tx: &tokio::sync::mpsc::Sender<ControlCmd>,
    ui_state: &mut ControlUi,
    id: &str,
    eq: HeadsetEq,
) {
    if ui_state.send(
        cmd_tx,
        ControlCmd::SetHeadsetEq {
            id: id.to_owned(),
            eq,
        },
    ) {
        ui_state.saved_at = Some((Instant::now(), "Sent to the headset"));
    }
}
