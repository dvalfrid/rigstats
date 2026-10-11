//! The Control Center's "App rules" page (#307): which programs switch the
//! profile while they run. Rules apply across profiles, so the page sits
//! in its own sidebar section; they are saved at once in settings.json
//! (`Settings.app_rules`) and picked up by the watcher thread
//! (`app_rules.rs`) on its next tick. Programs show with their icon and
//! own name (`app_icons.rs`); one can be picked from the running programs,
//! chosen with the Open dialog, or typed.

use crate::app_icons::{pick_program, AppInfoCache};
use crate::lock_ext::LockSafe;
use crate::theme::{self, DialogColors};
use crate::windows::profile_look::LookLink;
use crate::windows::ui_kit;
use rigstats_backend::control::Profile;
use rigstats_backend::profile_rules::{exe_stem, AppRule, RuleStatus, RELEASE_SECS, SETTLE_SECS};
use rigstats_backend::settings;
use std::sync::mpsc::Receiver;
use std::time::Instant;

const OWNER_TITLE: &str = "RigStats \u{2014} Control Center";

/// The page's state: the "Add a rule" form, the open dialog's answer to
/// come, and the program names and icons loaded so far.
#[derive(Default)]
pub struct RulesPage {
    exe: String,
    path: Option<String>,
    name: Option<String>,
    profile_id: Option<String>,
    picking: Option<Receiver<Option<String>>>,
    apps: AppInfoCache,
}

impl std::fmt::Debug for RulesPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RulesPage")
            .field("exe", &self.exe)
            .field("path", &self.path)
            .field("profile_id", &self.profile_id)
            .field("picking", &self.picking.is_some())
            .finish_non_exhaustive()
    }
}

impl RulesPage {
    /// Fills the form with the program at `path`.
    fn choose(&mut self, ctx: &egui::Context, exe: String, path: Option<String>) {
        self.name = path.as_deref().and_then(|p| self.apps.get(ctx, p).0);
        self.exe = exe;
        self.path = path;
    }

    fn clear(&mut self) {
        self.exe.clear();
        self.path = None;
        self.name = None;
        self.profile_id = None;
    }
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
    page: &mut RulesPage,
) -> bool {
    let ctx = ui.ctx().clone();
    ui_kit::page_header(
        ui,
        dc,
        "App rules",
        "Switch profile automatically while a program runs — for example Gaming while your game is open. Changes are saved at once.",
    );
    // Keep the running-programs list fresh while this page is open.
    let (status, running) = {
        let mut r = link.rules.lock_safe();
        r.list_wanted_at = Some(Instant::now());
        (r.status.clone(), r.running_apps.clone())
    };
    // A program chosen in the Open dialog.
    if let Some(rx) = &page.picking {
        if let Ok(answer) = rx.try_recv() {
            page.picking = None;
            if let Some(path) = answer {
                let exe = normalize_exe(&path).unwrap_or_default();
                page.choose(&ctx, exe, Some(path));
            }
        }
    }
    let mut rules = link.settings.lock_safe().app_rules.clone();
    let before = rules.clone();
    // A path for a rule's icon: its own, else the running program's.
    let path_for = |exe: &str, own: &Option<String>| -> Option<String> {
        own.clone().or_else(|| {
            running
                .iter()
                .find(|a| a.exe.eq_ignore_ascii_case(exe))
                .and_then(|a| a.path.clone())
        })
    };
    let name_for = |rule: &AppRule, apps: &mut AppInfoCache| -> String {
        rule.name.clone().unwrap_or_else(|| {
            path_for(&rule.exe, &rule.path)
                .and_then(|p| apps.get(&ctx, &p).0)
                .unwrap_or_else(|| rule.display_name())
        })
    };

    if !rules.is_empty() {
        let now_text = |exe: &str, apps: &mut AppInfoCache| -> String {
            rules
                .iter()
                .find(|r| r.exe.eq_ignore_ascii_case(exe))
                .map_or_else(|| exe_stem(exe).to_owned(), |r| name_for(r, apps))
        };
        ui_kit::group(ui, dc, None, None, |g| {
            g.row("Now", None, |ui| match &status {
                RuleStatus::Active { exe, profile_id } => {
                    let who = now_text(exe, &mut page.apps);
                    ui_kit::status(
                        ui,
                        ui_kit::C_GOOD,
                        &format!("{} — {who} is running", profile_name(profiles, profile_id)),
                    );
                }
                RuleStatus::Paused { .. } => ui_kit::status(
                    ui,
                    ui_kit::C_ATTENTION,
                    "Paused — you picked a profile; rules wait until their programs close",
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
            let when = if missing {
                "its profile was deleted — pick another"
            } else if rule.foreground_only {
                "only while its window is in front"
            } else {
                "while it runs"
            };
            let title = name_for(rule, &mut page.apps);
            let icon = path_for(&rule.exe, &rule.path).and_then(|p| page.apps.get(&ctx, &p).1);
            let sub = format!("{} · {when}", rule.exe);
            g.row_icon(icon.as_ref(), &title, Some(&sub), |ui| {
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
                ui_kit::combo(ui, ("rule_profile", i), 150.0, current, |ui| {
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
        "A program counts once it has run for {SETTLE_SECS} seconds. If several have rules, the one \
         in front decides, otherwise the one started last; when it closes, the next one's profile \
         takes over, and {RELEASE_SECS} seconds after the last one closes, the profile from before \
         comes back. Picking a profile yourself pauses the rules until none of their programs runs."
    );
    let exe = normalize_exe(&page.exe);
    let chosen_icon = page.path.clone().and_then(|p| page.apps.get(&ctx, &p).1);
    ui_kit::group(ui, dc, Some("Add a rule"), Some(&footer), |g| {
        let title = match (&page.name, &exe) {
            (Some(name), _) => name.clone(),
            (None, Some(exe)) => exe_stem(exe).to_owned(),
            (None, None) => "Program".to_owned(),
        };
        let sub = match &exe {
            Some(exe) => exe.clone(),
            None => "Choose one, pick one that's running, or type its file name".to_owned(),
        };
        g.row_icon(chosen_icon.as_ref(), &title, Some(&sub), |ui| {
            let picking = page.picking.is_some();
            ui.add_enabled_ui(!picking, |ui| {
                if theme::dialog_btn_secondary(
                    ui,
                    if picking { "Choosing…" } else { "Choose…" },
                    dc,
                )
                .on_hover_text("Browse for a program, even one that isn't running")
                .clicked()
                {
                    page.picking = Some(pick_program(OWNER_TITLE, ctx.clone()));
                }
            });
            let mut picked = None;
            ui_kit::combo(ui, "rule_running", 170.0, "Running apps", |ui| {
                if running.is_empty() {
                    ui.label(egui::RichText::new("Looking…").color(dc.muted));
                }
                // By the name people see, not the file name.
                let mut entries: Vec<_> = running
                    .iter()
                    .map(|app| {
                        let (name, icon) = app
                            .path
                            .as_deref()
                            .map_or((None, None), |p| page.apps.get(&ctx, p));
                        (app, name, icon)
                    })
                    .collect();
                entries.sort_by_key(|(app, name, _)| {
                    name.clone()
                        .unwrap_or_else(|| exe_stem(&app.exe).to_owned())
                        .to_lowercase()
                });
                for (app, name, icon) in entries {
                    ui.horizontal(|ui| {
                        match &icon {
                            Some(t) => {
                                ui.add(
                                    egui::Image::new(t).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                                );
                            }
                            None => {
                                ui.add_space(20.0);
                            }
                        }
                        let label = name.unwrap_or_else(|| exe_stem(&app.exe).to_owned());
                        if ui
                            .selectable_label(page.exe == app.exe, label)
                            .on_hover_text(&app.exe)
                            .clicked()
                        {
                            picked = Some(app.clone());
                        }
                    });
                }
            });
            if let Some(app) = picked {
                page.choose(&ctx, app.exe, app.path);
            }
        });
        g.row("Or type its file name", None, |ui| {
            let before = page.exe.clone();
            ui.add_sized(
                [ui_kit::CONTROL_W, ui_kit::CONTROL_H],
                egui::TextEdit::singleline(&mut page.exe)
                    .vertical_align(egui::Align::Center)
                    .hint_text("game.exe"),
            );
            // Typing a different program forgets the chosen one's path.
            if page.exe != before {
                page.path = None;
                page.name = None;
            }
        });
        g.row("Profile", None, |ui| {
            let current = page
                .profile_id
                .as_deref()
                .map_or("Pick a profile", |id| profile_name(profiles, id))
                .to_owned();
            ui_kit::dropdown(ui, "rule_new_profile", current, |ui| {
                for p in profiles {
                    ui.selectable_value(&mut page.profile_id, Some(p.id.clone()), &p.name);
                }
            });
        });
        let duplicate = exe
            .as_deref()
            .is_some_and(|e| rules.iter().any(|r| r.exe.eq_ignore_ascii_case(e)));
        let sub = if duplicate {
            "That program already has a rule"
        } else {
            "Applies from the next time the program starts or is in front"
        };
        g.row("", Some(sub), |ui| {
            let ready = exe.is_some() && page.profile_id.is_some() && !duplicate;
            if ready {
                if theme::dialog_btn_primary(ui, "Add Rule").clicked() {
                    if let (Some(exe), Some(profile_id)) = (exe.clone(), page.profile_id.clone()) {
                        rules.push(AppRule {
                            exe,
                            profile_id,
                            foreground_only: false,
                            path: page.path.clone(),
                            name: page.name.clone(),
                        });
                        page.clear();
                    }
                }
            } else {
                theme::dialog_btn_secondary_disabled(ui, "Add Rule", dc);
            }
        });
    });

    if rules != before {
        save(link, rules);
        return true;
    }
    false
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
