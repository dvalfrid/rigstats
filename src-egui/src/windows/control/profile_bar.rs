//! Control Center: the profile bar — chips, + copy, ⋯ rename/delete/reset.

use super::{ControlUi, ProfileEdit};
use crate::theme::{self, DialogColors};
use crate::windows::ui_kit::{self, Icon};
use rigstats_backend::control::{ControlCmd, ControlState, Profile};
use std::time::Instant;

/// The profile chips, an add button and a menu with the profile's actions.
/// Renaming and deleting replace the chips with an inline editor.
pub(super) fn profile_bar(
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
    // Wraps to a second row when the chips don't fit, rather than running
    // off the window's edge — there is no limit on how many profiles there
    // are (Ctrl+Alt+F1…F9 cover the first nine).
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
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
                        .align(egui::Align2::LEFT_CENTER)
                        .desired_width(200.0)
                        .min_size(egui::vec2(0.0, ui_kit::CONTROL_H))
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
                    ui_state.send(
                        cmd_tx,
                        ControlCmd::SaveProfile {
                            profile: Box::new(profile),
                            apply: false,
                        },
                    );
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
                    ui_state.send(cmd_tx, ControlCmd::DeleteProfile(id));
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
                for (index, profile) in control.profiles.iter().enumerate() {
                    let is_active = active.map(|a| &a.id) == Some(&profile.id);
                    let mut resp = ui_kit::chip(ui, dc, &profile.name, is_active);
                    // Its hotkey (#456), for binding a macro key to it.
                    if let Some(chord) = crate::hotkey::profile_chord(index) {
                        resp = resp.on_hover_text(format!("{chord} switches to {}", profile.name));
                    }
                    if resp.clicked() && !is_active {
                        ui_state.send(cmd_tx, ControlCmd::ApplyProfile(profile.id.clone()));
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
                    ui_state.send(
                        cmd_tx,
                        ControlCmd::SaveProfile {
                            profile: Box::new(copy),
                            apply: true,
                        },
                    );
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
                        ui_state.send(
                            cmd_tx,
                            ControlCmd::SaveProfile {
                                profile: Box::new(copy),
                                apply: true,
                            },
                        );
                    }
                    ui.separator();
                    if active.builtin {
                        // Built-ins can't be deleted, only put back.
                        if ui
                            .button("Reset to defaults")
                            .on_hover_text("Back to this built-in profile's hardware defaults")
                            .clicked()
                            && ui_state.send(cmd_tx, ControlCmd::ResetProfile(active.id.clone()))
                        {
                            ui_state.saved_at = Some((Instant::now(), "Reset to its defaults"));
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
