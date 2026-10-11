//! The Control Center's "App rules" page (#307): which programs switch the
//! profile while they run. Rules apply across profiles, so the page sits
//! in its own sidebar section; they are saved at once in settings.json
//! (`Settings.app_rules`) and picked up by the watcher thread
//! (`app_rules.rs`) on its next tick.

use crate::lock_ext::LockSafe;
use crate::theme::{self, DialogColors};
use crate::windows::profile_look::LookLink;
use crate::windows::ui_kit;
use rigstats_backend::control::Profile;
use rigstats_backend::profile_rules::{AppRule, RuleStatus, RELEASE_SECS, SETTLE_SECS};
use rigstats_backend::settings;
use std::time::Instant;

/// The "Add a rule" form.
#[derive(Debug, Default)]
pub struct RuleForm {
    exe: String,
    profile_id: Option<String>,
}

/// "cs2.exe" from what was typed or picked: trimmed, lower case, with
/// ".exe" added when missing and any folder dropped.
fn normalize_exe(typed: &str) -> Option<String> {
    let name = typed
        .trim()
        .trim_matches('"')
        .rsplit(['\\', '/'])
        .next()?
        .trim()
        .to_ascii_lowercase();
    if name.is_empty() {
        return None;
    }
    Some(if name.ends_with(".exe") {
        name
    } else {
        format!("{name}.exe")
    })
}

fn profile_name<'a>(profiles: &'a [Profile], id: &'a str) -> &'a str {
    profiles
        .iter()
        .find(|p| p.id == id)
        .map_or(id, |p| p.name.as_str())
}

fn save(link: &LookLink, rules: Vec<AppRule>) {
    let mut s = link.settings.lock_safe();
    s.app_rules = rules;
    if let Err(e) = settings::persist_settings(link.dir, &s) {
        rigstats_backend::debug::log_error(link.dir, &format!("settings: persist failed — {e}"));
    }
}

pub fn rules_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    profiles: &[Profile],
    link: &LookLink,
    form: &mut RuleForm,
) {
    ui_kit::page_header(
        ui,
        dc,
        "App rules",
        "Switch profile automatically while a program runs — for example Gaming while your game is open.",
    );
    // Keep the running-programs list fresh while this page is open.
    let (status, running) = {
        let mut r = link.rules.lock_safe();
        r.list_wanted_at = Some(Instant::now());
        (r.status.clone(), r.running_apps.clone())
    };
    let mut rules = link.settings.lock_safe().app_rules.clone();
    let before = rules.clone();

    if !rules.is_empty() {
        ui_kit::group(ui, dc, None, None, |g| {
            g.row("Now", None, |ui| match &status {
                RuleStatus::Active { exe, profile_id } => ui_kit::status(
                    ui,
                    ui_kit::C_GOOD,
                    &format!("{} — {exe} is running", profile_name(profiles, profile_id)),
                ),
                RuleStatus::Paused { exe } => ui_kit::status(
                    ui,
                    ui_kit::C_ATTENTION,
                    &format!("Paused — you picked a profile while {exe} runs"),
                ),
                RuleStatus::Idle => {
                    ui.label(
                        egui::RichText::new("Waiting for a program")
                            .size(12.0)
                            .color(dc.muted),
                    );
                }
            });
        });
    }

    ui_kit::group(ui, dc, Some("Rules"), None, |g| {
        if rules.is_empty() {
            g.block(|ui| {
                ui_kit::footnote(ui, dc, "No rules yet — add one below.");
            });
        }
        let mut remove = None;
        for (i, rule) in rules.iter_mut().enumerate() {
            let missing = !profiles.iter().any(|p| p.id == rule.profile_id);
            let sub = if missing {
                "Its profile was deleted — pick another"
            } else if rule.foreground_only {
                "Only while its window is in front"
            } else {
                "While it runs"
            };
            g.row(&rule.exe, Some(sub), |ui| {
                if theme::dialog_btn_secondary(ui, "Remove", dc).clicked() {
                    remove = Some(i);
                }
                ui.add_space(6.0);
                ui_kit::segmented(
                    ui,
                    dc,
                    &mut rule.foreground_only,
                    &[(false, "Runs"), (true, "In front")],
                );
                ui.add_space(6.0);
                let current = if missing {
                    "Pick a profile".to_owned()
                } else {
                    profile_name(profiles, &rule.profile_id).to_owned()
                };
                egui::ComboBox::from_id_salt(("rule_profile", i))
                    .width(150.0)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for p in profiles {
                            ui.selectable_value(&mut rule.profile_id, p.id.clone(), &p.name);
                        }
                    });
            });
        }
        if let Some(i) = remove {
            rules.remove(i);
        }
    });

    // ── Add a rule ───────────────────────────────────────────────────────
    let footer = format!(
        "A program counts once it has run for {SETTLE_SECS} seconds, and the profile goes back \
         {RELEASE_SECS} seconds after it closes. Picking a profile yourself pauses the rules \
         until that program closes."
    );
    ui_kit::group(ui, dc, Some("Add a rule"), Some(&footer), |g| {
        g.row(
            "Program",
            Some("Pick one that's running, or type its file name"),
            |ui| {
                egui::ComboBox::from_id_salt("rule_running")
                    .width(150.0)
                    .selected_text("Running apps")
                    .show_ui(ui, |ui| {
                        if running.is_empty() {
                            ui.label(egui::RichText::new("Looking…").color(dc.muted));
                        }
                        for app in &running {
                            if ui.selectable_label(form.exe == *app, app).clicked() {
                                form.exe = app.clone();
                            }
                        }
                    });
                ui.add_sized(
                    [170.0, 24.0],
                    egui::TextEdit::singleline(&mut form.exe).hint_text("game.exe"),
                );
            },
        );
        g.row("Profile", None, |ui| {
            let current = form
                .profile_id
                .as_deref()
                .map_or("Pick a profile", |id| profile_name(profiles, id))
                .to_owned();
            ui_kit::dropdown(ui, "rule_new_profile", current, |ui| {
                for p in profiles {
                    ui.selectable_value(&mut form.profile_id, Some(p.id.clone()), &p.name);
                }
            });
        });
        let exe = normalize_exe(&form.exe);
        let duplicate = exe
            .as_deref()
            .is_some_and(|e| rules.iter().any(|r| r.exe.eq_ignore_ascii_case(e)));
        let sub = if duplicate {
            "That program already has a rule"
        } else {
            "Applies from the next time the program starts or is in front"
        };
        g.row("", Some(sub), |ui| {
            let ready = exe.is_some() && form.profile_id.is_some() && !duplicate;
            if ready {
                if theme::dialog_btn_primary(ui, "Add Rule").clicked() {
                    if let (Some(exe), Some(profile_id)) = (exe.clone(), form.profile_id.clone()) {
                        rules.push(AppRule {
                            exe,
                            profile_id,
                            foreground_only: false,
                        });
                        *form = RuleForm::default();
                    }
                }
            } else {
                theme::dialog_btn_secondary_disabled(ui, "Add Rule", dc);
            }
        });
    });

    if rules != before {
        save(link, rules);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_names_become_plain_lower_case_exe_names() {
        assert_eq!(normalize_exe(" CS2.exe "), Some("cs2.exe".into()));
        assert_eq!(
            normalize_exe("Cyberpunk2077"),
            Some("cyberpunk2077.exe".into())
        );
        assert_eq!(
            normalize_exe(r#""C:\Games\Steam\steamapps\common\game\Game.EXE""#),
            Some("game.exe".into())
        );
        assert_eq!(normalize_exe("   "), None);
    }
}
