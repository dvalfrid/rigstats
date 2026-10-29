//! Control Center: the Lighting page — effect, colours, desk lamp, devices, Philips Hue (#215).

use super::{
    battery_and_link, battery_levels, dry_run_note, lighting_problem, problem_notice, AuraDraft,
    ControlUi,
};
use crate::theme::{self, DialogColors};
use crate::windows::ui_kit::{self};
use crate::windows::OpenRequests;
use rigstats_backend::control::{
    AuraCaps, AuraPart, ControlCmd, ControlState, HueCaps, LampPart, Severity,
};
use rigstats_backend::lhm::Peripheral;
use std::time::Instant;

/// Lighting effects in the order the Lighting tab offers them.
const AURA_EFFECTS: [(&str, &str); 4] = [
    ("off", "Off"),
    ("static", "Static"),
    ("breathing", "Breathing"),
    ("spectrum_cycle", "Colour cycle"),
];

pub(super) fn effect_name(effect: Option<&str>) -> &'static str {
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
pub(super) fn parse_hex_color(hex: Option<&str>) -> [u8; 3] {
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
pub(super) fn lighting_tab(
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
            hue_card(ui, dc, control, hue, cmd_tx, ui_state);
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
    let effect = part.as_ref().and_then(|p| p.effect.clone());
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
            // One effect for every device: a device that can only hold a
            // colour shows breathing and colour cycle as that colour (#659).
            let still = device.static_only
                && matches!(effect.as_deref(), Some("breathing" | "spectrum_cycle"));
            if warnings.is_empty() && !wdl_off && !wdl_hint && !still {
                continue;
            }
            g.block(|ui| {
                for w in &warnings {
                    ui_kit::row_problem(ui, dc, Severity::Attention, w);
                }
                if still {
                    ui_kit::footnote(
                        ui,
                        dc,
                        "Shows this effect as a steady colour: RIGStats can only set one colour on \
                         this device.",
                    );
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
                            && ui_state.send(cmd_tx, ControlCmd::EnableWdl(device.id.clone()))
                        {
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
        hue_card(ui, dc, control, hue, cmd_tx, ui_state);
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
    ui_state: &mut ControlUi,
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
                            ui_state.send(cmd_tx, ControlCmd::HueUnpair);
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
                if changed && ui_state.send(cmd_tx, ControlCmd::HueChoose(chosen)) {
                    ui_state.saved_at = Some((Instant::now(), "Saved"));
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
                            let ip = ui_state.hue_ip.trim().to_owned();
                            if !ip.is_empty()
                                && theme::dialog_btn_secondary(ui, "Look up", dc).clicked()
                            {
                                let _ = cmd_tx.try_send(ControlCmd::HueDiscover(Some(ip)));
                            }
                            ui.add(
                                egui::TextEdit::singleline(&mut ui_state.hue_ip)
                                    .align(egui::Align2::LEFT_CENTER)
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
                                    ui_state.send(cmd_tx, ControlCmd::HuePair(bridge.ip.clone()));
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
pub(super) fn preview_delta(before: Option<&AuraPart>, after: &AuraPart) -> Option<AuraPart> {
    let rgb_changed = before.is_none_or(|b| {
        (&b.effect, &b.color, b.brightness) != (&after.effect, &after.color, after.brightness)
    });
    let lamp_changed = before.is_none_or(|b| b.lamp != after.lamp);
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
pub(super) fn merge_preview(older: AuraPart, newer: AuraPart) -> AuraPart {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_kinds_read_as_words() {
        assert_eq!(device_kind("monitor_light_bar"), "Monitor light bar");
        assert_eq!(device_kind("keyboard"), "Keyboard");
        assert_eq!(device_kind(""), "");
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
}
