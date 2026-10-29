//! Control Center: the Mouse page — onboard profile, DPI, performance, buttons, energy (#431).

use super::{battery_and_link, device_tabs, ControlUi, Page};
use crate::theme::{self, DialogColors};
use crate::windows::ui_kit::{self};
use rigstats_backend::control::{
    ControlCmd, ControlState, MouseInfo, MousePart, MouseSettings, Profile,
};
use rigstats_backend::lhm::Peripheral;
use std::time::Instant;

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
pub(super) const MOUSE_POWER_OFF: [(u8, &str); 6] = [
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
pub(super) fn mouse_page(
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
    // Also when a switch's devices are done, after its answer (#495).
    let read_for = (control.active_profile.clone(), control.apply_seq);
    if !ui_state.mice_asked || ui_state.mice_read_for != read_for {
        ui_state.mice_asked = true;
        ui_state.mice_read_for = read_for;
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
    let tab = device_tabs(
        ui,
        dc,
        control.mice.iter().map(|m| m.name.as_str()),
        &mut ui_state.mouse_tab,
    );
    for mouse in control.mice.iter().skip(tab).take(1) {
        // Each card's controls under its own id: with two mice their
        // dropdowns otherwise share one id and flicker (#296, seen live).
        ui.push_id(&mouse.id, |ui| {
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
        });
    }
    if let (Some(choice), Some(mut profile)) = (save_profile, profile) {
        profile.part.mouse = choice.map(|p| MousePart { profile: Some(p) });
        let saved = ui_state.send(
            cmd_tx,
            ControlCmd::SaveProfile {
                profile: Box::new(profile),
                apply: true,
            },
        );
        // The mouse may now use another onboard profile: show its settings.
        let _ = cmd_tx.try_send(ControlCmd::ReadMice);
        if saved {
            ui_state.saved_at = Some((Instant::now(), "Saved"));
        }
    }
    if let Some((id, settings)) = send {
        if ui_state.send(
            cmd_tx,
            ControlCmd::SetMouse {
                id,
                settings: Box::new(settings),
            },
        ) {
            ui_state.saved_at = Some((Instant::now(), "Sent to the mouse"));
        }
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
