//! Control Center: the Keyboard page — onboard profile and energy settings (#458, #460).

use super::mouse::MOUSE_POWER_OFF;
use super::{device_tabs, ControlUi};
use crate::theme::DialogColors;
use crate::windows::ui_kit::{self};
use rigstats_backend::control::{ControlCmd, ControlState, KeyboardPart, KeyboardSettings};
use std::time::Instant;

/// How often the Keyboard page asks a silent (asleep) keyboard again.
const KEYBOARD_RETRY: std::time::Duration = std::time::Duration::from_secs(4);

/// The Keyboard page (#458): which of the keyboard's own onboard profiles
/// the RIGStats profile uses, saved at once in the profile — the keyboard
/// switches with it. One choice for every keyboard (a profile has one
/// keyboard part).
pub(super) fn keyboard_page(
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
    // Every keyboard the page knows — with onboard profiles, with energy
    // settings, or both — one tab each when there is more than one (#296).
    let mut all: Vec<(&str, &str)> = keyboards
        .iter()
        .map(|k| (k.id.as_str(), k.name.as_str()))
        .collect();
    for k in &control.keyboard_settings {
        if !all.iter().any(|(id, _)| *id == k.id) {
            all.push((k.id.as_str(), k.name.as_str()));
        }
    }
    let tab = device_tabs(
        ui,
        dc,
        all.iter().map(|(_, name)| *name),
        &mut ui_state.keyboard_tab,
    );
    let shown_id = all.get(tab).map(|(id, _)| *id);
    if let Some((_, name)) = all.get(tab) {
        let subtitle = keyboards
            .iter()
            .find(|k| Some(k.id.as_str()) == shown_id)
            .map_or_else(
                || "Keyboard".to_owned(),
                |k| format!("Keyboard · {} profiles of its own", k.profiles),
            );
        ui_kit::group(ui, dc, None, None, |g| {
            g.row(name, Some(&subtitle), |_| {});
        });
    }
    let count = keyboards.iter().map(|k| k.profiles).max().unwrap_or(1);
    let chosen = profile.part.keyboard.and_then(|k| k.profile);
    let mut pick = chosen;
    let every = if all.len() > 1 {
        " The same profile number goes to every keyboard."
    } else {
        ""
    };
    let footnote = match chosen {
        // The keyboard's own switch follows back (#459).
        Some(p) => format!(
            "The keyboard keeps {count} profiles of its own — its lighting and keys. Switching to {name} \
             puts the keyboard on profile {p}, and Fn + {p} on the keyboard switches to {name}.{every}",
            name = profile.name
        ),
        None => format!(
            "The keyboard keeps {count} profiles of its own — its lighting and keys — and Fn plus a \
             number key switches between them on the keyboard. Pick one to switch the keyboard with {}, \
             and to switch to {} with Fn plus that number.{every}",
            profile.name, profile.name
        ),
    };
    if !keyboards.is_empty() {
        ui_kit::group(ui, dc, Some("Profile"), Some(&footnote), |g| {
            g.row(
                &format!("Keyboard profile in {}", profile.name),
                Some("Saved at once"),
                |ui| {
                    let label = chosen
                        .map_or_else(|| "Leave unchanged".to_owned(), |p| format!("Profile {p}"));
                    ui_kit::dropdown(ui, "keyboard_profile", label, |ui| {
                        ui.selectable_value(&mut pick, None, "Leave unchanged");
                        for p in 1..=count {
                            ui.selectable_value(&mut pick, Some(p), format!("Profile {p}"));
                        }
                    });
                },
            );
        });
    }
    if pick != chosen {
        profile.part.keyboard = pick.map(|p| KeyboardPart { profile: Some(p) });
        if ui_state.send(
            cmd_tx,
            ControlCmd::SaveProfile {
                profile: Box::new(profile),
                apply: true,
            },
        ) {
            ui_state.saved_at = Some((Instant::now(), "Saved"));
        }
    }

    // ── Energy, for every profile: the service puts it back after each of
    // the keyboard's own profile switches (#460, #497) ───────────────────
    if !ui_state.keyboards_asked {
        ui_state.keyboards_asked = true;
        ui_state.keyboard_pending = None;
        let _ = cmd_tx.try_send(ControlCmd::ReadKeyboards);
    }
    if let Some((id, pending)) = &ui_state.keyboard_pending {
        let landed = control
            .keyboard_settings
            .iter()
            .any(|k| &k.id == id && k.settings.as_ref() == Some(pending));
        if landed || !control.keyboard_busy {
            ui_state.keyboard_pending = None;
        }
    }
    let mut send: Option<(String, KeyboardSettings)> = None;
    for keyboard in control
        .keyboard_settings
        .iter()
        .filter(|k| Some(k.id.as_str()) == shown_id)
    {
        let heading = "Energy".to_owned();
        let footnote = if keyboard.verified {
            "For every profile — RIGStats sets them again whenever the keyboard switches its own profile, which loads the ones saved in it. Changes are sent at once."
        } else {
            "For every profile — RIGStats sets them again whenever the keyboard switches its own profile, which loads the ones saved in it. Changes are sent at once — not yet verified on this model."
        };
        let Some(read) = keyboard.settings else {
            // A wireless keyboard asleep behind its receiver answers nothing;
            // ask again every few seconds, so the settings appear as soon as a
            // key wakes it (the owner took the old "doesn't answer" for an error).
            let due = ui_state
                .keyboard_retry_at
                .is_none_or(|at| at.elapsed() >= KEYBOARD_RETRY);
            if due && !control.keyboard_busy {
                ui_state.keyboard_retry_at = Some(Instant::now());
                let _ = cmd_tx.try_send(ControlCmd::ReadKeyboards);
            }
            ui.ctx().request_repaint_after(KEYBOARD_RETRY);
            ui_kit::group(ui, dc, Some(&heading), None, |g| {
                g.row(
                    &format!("{} is asleep", keyboard.name),
                    Some("Press a key on it — its settings show as soon as it wakes up"),
                    |ui| {
                        ui.spinner();
                    },
                );
            });
            continue;
        };
        let mut s = match &ui_state.keyboard_pending {
            Some((id, pending)) if id == &keyboard.id => *pending,
            _ => read,
        };
        let before = s;
        let mut release = false;
        ui_kit::group(ui, dc, Some(&heading), Some(footnote), |g| {
            g.row("Sleep after", Some("Without a key press"), |ui| {
                let label = MOUSE_POWER_OFF
                    .iter()
                    .find(|(code, _)| *code == s.idle_code)
                    .map_or("—", |(_, words)| words);
                ui_kit::dropdown(ui, ("keyboard_idle", &keyboard.id), label, |ui| {
                    for (code, words) in MOUSE_POWER_OFF {
                        ui.selectable_value(&mut s.idle_code, code, words);
                    }
                });
            });
            g.row(
                "Power saving when the battery is low",
                Some("What the lighting does below the level"),
                |ui| {
                    let label = match s.power_saving {
                        1 => "Lighting off",
                        2 => "Lighting dimmed",
                        _ => "Off",
                    };
                    ui_kit::dropdown(ui, ("keyboard_saving", &keyboard.id), label, |ui| {
                        ui.selectable_value(&mut s.power_saving, 0, "Off");
                        ui.selectable_value(&mut s.power_saving, 1, "Lighting off");
                        ui.selectable_value(&mut s.power_saving, 2, "Lighting dimmed");
                    });
                },
            );
            g.row(
                "Low-battery level",
                Some("Where the lighting alert and power saving start"),
                |ui| {
                    let mut v = i32::from(s.low_battery_level);
                    let resp = ui_kit::slider_range(ui, dc, &mut v, 0..=100, 5, "%");
                    s.low_battery_level = v as u8;
                    release |= resp.drag_stopped() || (resp.changed() && !resp.dragged());
                },
            );
        });
        if s != before {
            let dragging = s.low_battery_level != before.low_battery_level && !release;
            ui_state.keyboard_pending = Some((keyboard.id.clone(), s));
            if !dragging {
                send = Some((keyboard.id.clone(), s));
            }
        } else if release {
            send = Some((keyboard.id.clone(), s));
        }
    }
    if let Some((id, settings)) = send {
        if ui_state.send(cmd_tx, ControlCmd::SetKeyboard { id, settings }) {
            ui_state.saved_at = Some((Instant::now(), "Sent to the keyboard"));
        }
    }
}
