//! Automatic profile switching (#307), like Armoury Crate's scenario
//! profiles: when a program from the user's rules runs, switch to its
//! Control Center profile; when it closes, switch back to the profile that
//! was active before.
//!
//! [`RuleEngine::tick`] is pure — it gets a snapshot of the running
//! programs, the active profile and the time, and answers which profile to
//! apply, if any — so every case is unit-tested. The app feeds it from a
//! background thread (`src-egui/src/app/rules_watcher.rs`) and applies its
//! answer through the control pipe; the service stays machine-wide and
//! user-agnostic.
//!
//! Rules of thumb it follows:
//! - A program counts once it has run for [`SETTLE_SECS`] (launchers and
//!   updaters that flash past don't switch anything).
//! - Several matching programs: the one started last wins.
//! - The profile goes back [`RELEASE_SECS`] after the program closes (or,
//!   for a foreground-only rule, loses focus), so a quick restart or an
//!   Alt+Tab doesn't flip profiles back and forth.
//! - A profile picked by hand while a rule is active pauses rules until
//!   that program is gone — the user's choice wins, and nothing is
//!   switched back behind their back.

use serde::{Deserialize, Serialize};

/// How long a program must have been running before its rule applies.
pub const SETTLE_SECS: u64 = 5;
/// How long a rule's program must be gone before the profile goes back.
pub const RELEASE_SECS: u64 = 5;
/// How long to wait for the service to confirm a switch before trying
/// again.
const CONFIRM_SECS: u64 = 15;

/// One rule: while `exe` runs, use profile `profile_id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppRule {
    /// The program's file name, e.g. "cs2.exe" (compared case-insensitively).
    pub exe: String,
    pub profile_id: String,
    /// Only while the program's window is in the foreground.
    #[serde(default)]
    pub foreground_only: bool,
}

/// A running program: its file name in lower case and when it started.
#[derive(Debug, Clone, PartialEq)]
pub struct RunningProgram {
    pub exe: String,
    pub started_unix: u64,
}

/// What is running right now.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub now_unix: u64,
    pub running: Vec<RunningProgram>,
    /// File name (lower case) of the program that owns the foreground window.
    pub foreground: Option<String>,
}

/// What the rules are doing, for the Control Center.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum RuleStatus {
    /// No rule's program is running.
    #[default]
    Idle,
    /// `exe` runs, so `profile_id` is in use.
    Active { exe: String, profile_id: String },
    /// A profile was picked by hand while `exe` ran: rules wait until it's gone.
    Paused { exe: String },
}

#[derive(Debug, Clone)]
struct Engaged {
    exe: String,
    profile_id: String,
    /// The profile from before the first rule applied.
    return_to: Option<String>,
    /// When the program stopped matching, while waiting to switch back.
    released_at: Option<u64>,
}

/// The rules' memory between snapshots.
#[derive(Debug, Default)]
pub struct RuleEngine {
    engaged: Option<Engaged>,
    paused: Option<String>,
    /// A switch we asked for and haven't seen confirmed: (profile, asked at).
    pending: Option<(String, u64)>,
}

impl RuleEngine {
    pub fn status(&self) -> RuleStatus {
        if let Some(exe) = &self.paused {
            return RuleStatus::Paused { exe: exe.clone() };
        }
        match &self.engaged {
            Some(e) => RuleStatus::Active {
                exe: e.exe.clone(),
                profile_id: e.profile_id.clone(),
            },
            None => RuleStatus::Idle,
        }
    }

    /// Forgets everything (all rules removed).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The profile to apply now, if any. `profiles` are the ids that exist
    /// (a rule for a deleted profile is ignored); `active` is the profile
    /// the service reports.
    pub fn tick(
        &mut self,
        rules: &[AppRule],
        profiles: &[&str],
        active: Option<&str>,
        snap: &Snapshot,
    ) -> Option<String> {
        let now = snap.now_unix;
        // Wait for a switch we asked for to land before judging anything.
        if let Some((target, asked)) = &self.pending {
            if active == Some(target.as_str()) || now.saturating_sub(*asked) >= CONFIRM_SECS {
                self.pending = None;
            } else {
                return None;
            }
        }
        // Someone picked another profile while a rule was active: pause.
        if let (Some(e), Some(active)) = (&self.engaged, active) {
            if active != e.profile_id {
                self.paused = Some(e.exe.clone());
                self.engaged = None;
            }
        }
        let best = best_match(rules, profiles, snap);
        if let Some(exe) = &self.paused {
            if matching(rules, profiles, snap).any(|(r, _)| r.exe.eq_ignore_ascii_case(exe)) {
                return None;
            }
            self.paused = None;
        }
        match (best, self.engaged.as_mut()) {
            (Some(rule), Some(e)) if rule.exe.eq_ignore_ascii_case(&e.exe) => {
                e.released_at = None;
                None
            }
            (Some(rule), engaged) => {
                let return_to = match engaged {
                    Some(e) => e.return_to.clone(),
                    None => active.map(str::to_owned),
                };
                self.engaged = Some(Engaged {
                    exe: rule.exe.to_ascii_lowercase(),
                    profile_id: rule.profile_id.clone(),
                    return_to,
                    released_at: None,
                });
                self.request(&rule.profile_id, active, now)
            }
            (None, Some(e)) => {
                let released = *e.released_at.get_or_insert(now);
                if now.saturating_sub(released) < RELEASE_SECS {
                    return None;
                }
                let back = e.return_to.clone();
                self.engaged = None;
                match back {
                    Some(id) if profiles.contains(&id.as_str()) => self.request(&id, active, now),
                    _ => None,
                }
            }
            (None, None) => None,
        }
    }

    fn request(&mut self, profile: &str, active: Option<&str>, now: u64) -> Option<String> {
        if active == Some(profile) {
            return None;
        }
        self.pending = Some((profile.to_owned(), now));
        Some(profile.to_owned())
    }
}

/// Every rule whose program counts right now, with when it started (its
/// earliest running instance).
fn matching<'a>(
    rules: &'a [AppRule],
    profiles: &'a [&str],
    snap: &'a Snapshot,
) -> impl Iterator<Item = (&'a AppRule, u64)> + 'a {
    rules.iter().filter_map(move |rule| {
        if !profiles.contains(&rule.profile_id.as_str()) {
            return None;
        }
        let started = snap
            .running
            .iter()
            .filter(|p| p.exe.eq_ignore_ascii_case(&rule.exe))
            .map(|p| p.started_unix)
            .min()?;
        if snap.now_unix.saturating_sub(started) < SETTLE_SECS {
            return None;
        }
        if rule.foreground_only
            && !snap
                .foreground
                .as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case(&rule.exe))
        {
            return None;
        }
        Some((rule, started))
    })
}

/// The rule whose program started last.
fn best_match<'a>(
    rules: &'a [AppRule],
    profiles: &'a [&str],
    snap: &'a Snapshot,
) -> Option<&'a AppRule> {
    matching(rules, profiles, snap)
        .max_by_key(|(_, started)| *started)
        .map(|(rule, _)| rule)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILES: &[&str] = &["balanced", "gaming", "silent"];

    fn rule(exe: &str, profile: &str) -> AppRule {
        AppRule {
            exe: exe.into(),
            profile_id: profile.into(),
            foreground_only: false,
        }
    }

    fn snap(now: u64, running: &[(&str, u64)], foreground: Option<&str>) -> Snapshot {
        Snapshot {
            now_unix: now,
            running: running
                .iter()
                .map(|(exe, started)| RunningProgram {
                    exe: exe.to_string(),
                    started_unix: *started,
                })
                .collect(),
            foreground: foreground.map(str::to_owned),
        }
    }

    #[test]
    fn a_program_switches_after_it_settles_and_back_after_it_closes() {
        let rules = [rule("cs2.exe", "gaming")];
        let mut e = RuleEngine::default();
        // Just started: not yet.
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("balanced"),
                &snap(102, &[("cs2.exe", 100)], None)
            ),
            None
        );
        // Settled: switch to Gaming.
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("balanced"),
                &snap(106, &[("cs2.exe", 100)], None)
            ),
            Some("gaming".into())
        );
        assert_eq!(
            e.status(),
            RuleStatus::Active {
                exe: "cs2.exe".into(),
                profile_id: "gaming".into()
            }
        );
        // Confirmed, still running: nothing to do.
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("gaming"),
                &snap(110, &[("cs2.exe", 100)], None)
            ),
            None
        );
        // Closed: wait out the release time, then back to Balanced.
        assert_eq!(
            e.tick(&rules, PROFILES, Some("gaming"), &snap(200, &[], None)),
            None
        );
        assert_eq!(
            e.tick(&rules, PROFILES, Some("gaming"), &snap(206, &[], None)),
            Some("balanced".into())
        );
        assert_eq!(e.status(), RuleStatus::Idle);
    }

    #[test]
    fn a_launcher_that_flashes_past_switches_nothing() {
        let rules = [rule("launcher.exe", "gaming")];
        let mut e = RuleEngine::default();
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("balanced"),
                &snap(102, &[("launcher.exe", 100)], None)
            ),
            None
        );
        assert_eq!(
            e.tick(&rules, PROFILES, Some("balanced"), &snap(104, &[], None)),
            None
        );
        assert_eq!(e.status(), RuleStatus::Idle);
    }

    #[test]
    fn a_quick_restart_keeps_the_profile() {
        let rules = [rule("game.exe", "gaming")];
        let mut e = RuleEngine::default();
        e.tick(
            &rules,
            PROFILES,
            Some("balanced"),
            &snap(110, &[("game.exe", 100)], None),
        );
        e.tick(
            &rules,
            PROFILES,
            Some("gaming"),
            &snap(111, &[("game.exe", 100)], None),
        );
        // Gone for two seconds, then running again (and settled).
        assert_eq!(
            e.tick(&rules, PROFILES, Some("gaming"), &snap(120, &[], None)),
            None
        );
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("gaming"),
                &snap(122, &[("game.exe", 100)], None)
            ),
            None
        );
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("gaming"),
                &snap(130, &[("game.exe", 100)], None)
            ),
            None
        );
        assert!(matches!(e.status(), RuleStatus::Active { .. }));
    }

    #[test]
    fn the_program_started_last_wins_and_the_first_profile_is_restored_at_the_end() {
        let rules = [rule("editor.exe", "silent"), rule("game.exe", "gaming")];
        let mut e = RuleEngine::default();
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("balanced"),
                &snap(110, &[("editor.exe", 100)], None)
            ),
            Some("silent".into())
        );
        // The game starts later: it wins.
        let both = [("editor.exe", 100), ("game.exe", 200)];
        assert_eq!(
            e.tick(&rules, PROFILES, Some("silent"), &snap(206, &both, None)),
            Some("gaming".into())
        );
        // Both close: back to the profile from before any rule — Balanced.
        e.tick(&rules, PROFILES, Some("gaming"), &snap(300, &[], None));
        assert_eq!(
            e.tick(&rules, PROFILES, Some("gaming"), &snap(306, &[], None)),
            Some("balanced".into())
        );
    }

    #[test]
    fn a_profile_picked_by_hand_pauses_rules_until_the_program_is_gone() {
        let rules = [rule("game.exe", "gaming")];
        let mut e = RuleEngine::default();
        e.tick(
            &rules,
            PROFILES,
            Some("balanced"),
            &snap(110, &[("game.exe", 100)], None),
        );
        e.tick(
            &rules,
            PROFILES,
            Some("gaming"),
            &snap(111, &[("game.exe", 100)], None),
        );
        // The user picks Silent from the tray while the game runs.
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("silent"),
                &snap(120, &[("game.exe", 100)], None)
            ),
            None
        );
        assert_eq!(
            e.status(),
            RuleStatus::Paused {
                exe: "game.exe".into()
            }
        );
        // The game closes: nothing is switched back behind their back.
        assert_eq!(
            e.tick(&rules, PROFILES, Some("silent"), &snap(130, &[], None)),
            None
        );
        assert_eq!(
            e.tick(&rules, PROFILES, Some("silent"), &snap(140, &[], None)),
            None
        );
        assert_eq!(e.status(), RuleStatus::Idle);
        // Next time the game starts, the rule works again.
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("silent"),
                &snap(210, &[("game.exe", 200)], None)
            ),
            Some("gaming".into())
        );
    }

    #[test]
    fn a_foreground_only_rule_follows_the_focus_with_a_grace_period() {
        let rules = [AppRule {
            foreground_only: true,
            ..rule("game.exe", "gaming")
        }];
        let mut e = RuleEngine::default();
        let running = [("game.exe", 100)];
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("balanced"),
                &snap(110, &running, Some("explorer.exe"))
            ),
            None
        );
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("balanced"),
                &snap(111, &running, Some("game.exe"))
            ),
            Some("gaming".into())
        );
        // A quick Alt+Tab: no flip.
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("gaming"),
                &snap(115, &running, Some("discord.exe"))
            ),
            None
        );
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("gaming"),
                &snap(117, &running, Some("game.exe"))
            ),
            None
        );
        // Away for longer: back to Balanced.
        e.tick(
            &rules,
            PROFILES,
            Some("gaming"),
            &snap(120, &running, Some("discord.exe")),
        );
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("gaming"),
                &snap(126, &running, Some("discord.exe"))
            ),
            Some("balanced".into())
        );
    }

    #[test]
    fn rules_for_deleted_profiles_and_already_active_profiles_switch_nothing() {
        let mut e = RuleEngine::default();
        let gone = [rule("game.exe", "deleted")];
        assert_eq!(
            e.tick(
                &gone,
                PROFILES,
                Some("balanced"),
                &snap(110, &[("game.exe", 100)], None)
            ),
            None
        );
        assert_eq!(e.status(), RuleStatus::Idle);
        // Already on the rule's profile: engaged, but nothing to send.
        let rules = [rule("game.exe", "gaming")];
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("gaming"),
                &snap(110, &[("game.exe", 100)], None)
            ),
            None
        );
        assert!(matches!(e.status(), RuleStatus::Active { .. }));
    }

    #[test]
    fn names_match_case_insensitively_and_an_unconfirmed_switch_is_not_repeated() {
        let rules = [rule("CS2.EXE", "gaming")];
        let mut e = RuleEngine::default();
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("balanced"),
                &snap(110, &[("cs2.exe", 100)], None)
            ),
            Some("gaming".into())
        );
        // The service never confirms (busy, disconnected): wait, then stop — no
        // repeated switching against a service that does not answer.
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("balanced"),
                &snap(115, &[("cs2.exe", 100)], None)
            ),
            None
        );
        assert_eq!(
            e.tick(
                &rules,
                PROFILES,
                Some("balanced"),
                &snap(126, &[("cs2.exe", 100)], None)
            ),
            None,
            "after the timeout the stale active profile reads as a manual pick"
        );
        assert_eq!(
            e.status(),
            RuleStatus::Paused {
                exe: "cs2.exe".into()
            }
        );
    }

    #[test]
    fn rules_round_trip_through_json() {
        let rules = vec![AppRule {
            foreground_only: true,
            ..rule("cs2.exe", "gaming")
        }];
        let json = serde_json::to_string(&rules).unwrap();
        assert!(json.contains("\"profileId\":\"gaming\""));
        let back: Vec<AppRule> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, rules);
        let old: AppRule = serde_json::from_str(r#"{"exe":"a.exe","profileId":"x"}"#).unwrap();
        assert!(!old.foreground_only);
    }
}
