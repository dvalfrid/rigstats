//! Every problem the app itself reports, in one place and one wording
//! (#370, #373). A state that shows in several windows — the service not
//! answering, this app refused, a profile that didn't apply — is built here
//! once, so the Control Center, Status and notifications say the same thing.
//! The service's own problems (a crashed driver) arrive as
//! `CapabilitySet::problem`; [`active_problems`] lists them all for Status.
//! Shown with `ui_kit::notice`; written for someone who has never seen the
//! code: what happened, what still works, what to do.

use rigstats_backend::control::{
    ControlState, Problem, ProblemAction, ProblemActionKind, ProtocolMismatch, Severity,
};

pub const TROUBLESHOOTING_URL: &str =
    "https://github.com/dvalfrid/rigstats/blob/main/docs/troubleshooting.md";
pub const DOWNLOAD_URL: &str = "https://rigstats.app/#download";

/// The one wording for "no answer from the service yet", wherever a value or
/// a page waits for it.
pub const WAITING_FOR_SERVICE: &str = "Waiting for the RIGStats service…";

/// The control connection's state in a word — the Control Center's title bar
/// and Status's Health row say the same.
pub const STATE_CONNECTED: &str = "Connected";
pub const STATE_CONNECTING: &str = "Connecting…";
pub const STATE_REFUSED: &str = "Refused by the service";
pub const STATE_MISMATCH: &str = "Version mismatch";

fn action(kind: ProblemActionKind, label: &str, target: Option<&str>) -> ProblemAction {
    ProblemAction {
        kind,
        label: label.to_owned(),
        target: target.map(str::to_owned),
    }
}

fn diagnostics() -> ProblemAction {
    action(
        ProblemActionKind::CollectDiagnostics,
        "Collect diagnostics…",
        None,
    )
}

fn read_more(anchor: &str) -> ProblemAction {
    action(
        ProblemActionKind::OpenUrl,
        "Read more",
        Some(&format!("{TROUBLESHOOTING_URL}{anchor}")),
    )
}

fn problem(
    id: &str,
    severity: Severity,
    title: &str,
    message: String,
    actions: Vec<ProblemAction>,
    detail: Option<String>,
) -> Problem {
    Problem {
        id: id.to_owned(),
        severity,
        title: title.to_owned(),
        message,
        actions,
        detail,
    }
}

/// The sensor service isn't running or doesn't answer.
pub fn service_unreachable() -> Problem {
    problem(
        "service_unreachable",
        Severity::Error,
        "The RIGStats service isn't answering",
        "Sensor readings, fan control and lighting come from the RIGStats sensor service, \
         which starts with Windows. It isn't answering right now, so values show as -- and the \
         Control Center can't change anything. Restart Windows; if it stays away, reinstall \
         RIGStats."
            .to_owned(),
        vec![
            diagnostics(),
            read_more("#cpu-temperature--package-power-missing-or-defender-alert-on-service-start"),
        ],
        None,
    )
}

/// The service runs, but its sensor data stopped coming.
pub fn sensor_data_lost() -> Problem {
    problem(
        "sensor_data_lost",
        Severity::Attention,
        "Sensor readings stopped",
        "The RIGStats service is running, but its sensor readings stopped coming. RIGStats keeps \
         trying to reconnect; values show as -- until then."
            .to_owned(),
        vec![diagnostics()],
        None,
    )
}

/// The service closes the control connection: this isn't the installed,
/// signed RIGStats.
pub fn service_refused() -> Problem {
    problem(
        "service_refused",
        Severity::Error,
        "The RIGStats service refused this app",
        "Only the installed, signed RIGStats may control fans, power and lighting — a copied or \
         self-built rigstats.exe is refused on purpose. Start RIGStats from the Start menu, or \
         reinstall it."
            .to_owned(),
        vec![
            action(
                ProblemActionKind::OpenUrl,
                "Download RIGStats",
                Some(DOWNLOAD_URL),
            ),
            read_more("#control-center"),
        ],
        None,
    )
}

/// The app and the service speak different protocol versions.
pub fn version_mismatch(mismatch: &ProtocolMismatch) -> Problem {
    problem(
        "version_mismatch",
        Severity::Error,
        "The app and the service don't match",
        "This RIGStats and its sensor service are from different versions, so the Control Center \
         is off. Install the latest RIGStats; it updates both."
            .to_owned(),
        vec![action(
            ProblemActionKind::OpenUrl,
            "Download RIGStats",
            Some(DOWNLOAD_URL),
        )],
        Some(format!(
            "Control protocol: the app speaks v{}, the service v{}.",
            mismatch.expected, mismatch.got
        )),
    )
}

/// A profile that wasn't applied (everything was put back).
pub fn apply_failed(message: &str) -> Problem {
    problem(
        "apply_failed",
        Severity::Error,
        "The profile wasn't applied",
        "Nothing was changed: RIGStats put everything back as it was. Try again, or choose \
         another profile; if it keeps failing, send diagnostics."
            .to_owned(),
        vec![diagnostics()],
        Some(message.to_owned()),
    )
}

/// A profile applied except for a part that is down (the graphics driver
/// crashed): the rest is in force.
pub fn apply_partial(message: &str) -> Problem {
    problem(
        "apply_partial",
        Severity::Attention,
        "The profile was applied in part",
        "Everything else in the profile is in force; the part that couldn't be reached was left \
         as it was."
            .to_owned(),
        Vec::new(),
        Some(message.to_owned()),
    )
}

/// The critical-temperature override ran the fans at 100 %.
pub fn safety_tripped(reason: &str) -> Problem {
    problem(
        "safety_tripped",
        Severity::Attention,
        "Fans ran at full speed to protect the hardware",
        format!(
            "{reason}. RIGStats set every fan it controls to 100 % until the temperature came \
             down. If this happens often, check the cooler and the fan curves."
        ),
        Vec::new(),
        None,
    )
}

/// A request the service answered with an error.
pub fn service_error(message: &str) -> Problem {
    problem(
        "service_error",
        Severity::Attention,
        "The RIGStats service reported an error",
        "The last request didn't go through. Try again; if it keeps happening, send diagnostics."
            .to_owned(),
        vec![diagnostics()],
        Some(message.to_owned()),
    )
}

/// An update check or download failed.
pub fn update_failed(error: &str) -> Problem {
    problem(
        "update_failed",
        Severity::Error,
        "Couldn't update",
        "RIGStats couldn't check for or download the update. It tries again by itself; you can \
         also try again now, or download the latest version from rigstats.app."
            .to_owned(),
        // "Try Again" is the Updates window's own footer button.
        vec![action(
            ProblemActionKind::OpenUrl,
            "Download RIGStats",
            Some(DOWNLOAD_URL),
        )],
        Some(error.to_owned()),
    )
}

/// Wallpaper Engine's last command failed.
pub fn wallpaper_engine_failed(error: &str) -> Problem {
    problem(
        "wallpaper_engine_failed",
        Severity::Attention,
        "Wallpaper Engine didn't do what was asked",
        "The last command to Wallpaper Engine failed. The details are in the log.".to_owned(),
        vec![action(ProblemActionKind::OpenLog, "Open log", None)],
        Some(error.to_owned()),
    )
}

/// Settings while the dashboard is in the desktop wallpaper.
pub fn wallpaper_saves_only() -> Problem {
    problem(
        "wallpaper_saves_only",
        Severity::Info,
        "Changes show when you click Save",
        "The dashboard is in the desktop wallpaper, so it picks up a change once it is saved."
            .to_owned(),
        Vec::new(),
        None,
    )
}

/// The service runs in dry-run mode (a developer build).
pub fn dry_run() -> Problem {
    problem(
        "dry_run",
        Severity::Info,
        "Dry-run mode",
        "The service only logs what it would change; nothing is written to the hardware."
            .to_owned(),
        Vec::new(),
        None,
    )
}

/// Every problem right now, most serious first — for Status. `service_running`
/// is the service's state (`sc query`); `sensor_connected` whether its
/// readings arrive.
pub fn active_problems(
    control: &ControlState,
    sensor_connected: bool,
    service_running: bool,
) -> Vec<Problem> {
    if !service_running {
        // Everything else follows from it.
        return vec![service_unreachable()];
    }
    let mut out = Vec::new();
    if let Some(mismatch) = &control.protocol_mismatch {
        out.push(version_mismatch(mismatch));
    } else if control.refused {
        out.push(service_refused());
    } else if !control.connected {
        out.push(service_unreachable());
    }
    if !sensor_connected {
        out.push(sensor_data_lost());
    }
    out.extend(
        control
            .capabilities
            .iter()
            .filter(|c| !c.supported)
            .filter_map(|c| c.problem.clone()),
    );
    out.extend(control.crash_guard_problem());
    out.extend(control.profiles_problem());
    if let Some(reason) = &control.safety_tripped {
        out.push(safety_tripped(reason));
    }
    if let Some(result) = &control.last_apply_result {
        match (result.ok, result.message.as_deref()) {
            (false, message) => out.push(apply_failed(message.unwrap_or("Apply failed."))),
            (true, Some(message)) => out.push(apply_partial(message)),
            (true, None) => {}
        }
    }
    if let Some(error) = &control.last_error {
        out.push(service_error(error));
    }
    out.sort_by_key(|p| match p.severity {
        Severity::Error => 0,
        Severity::Attention => 1,
        Severity::Info => 2,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rigstats_backend::control::{ApplyResult, CapabilitySet};

    fn connected() -> ControlState {
        ControlState {
            connected: true,
            ..Default::default()
        }
    }

    fn ids(problems: &[Problem]) -> Vec<&str> {
        problems.iter().map(|p| p.id.as_str()).collect()
    }

    #[test]
    fn all_well_is_no_problems() {
        assert!(active_problems(&connected(), true, true).is_empty());
    }

    #[test]
    fn a_stopped_service_says_only_that() {
        let mut control = connected();
        control.last_error = Some("x".into());
        assert_eq!(
            ids(&active_problems(&control, false, false)),
            ["service_unreachable"]
        );
    }

    #[test]
    fn connection_problems_come_first_and_only_the_most_specific_one() {
        let mut control = ControlState {
            refused: true,
            ..Default::default()
        };
        assert_eq!(
            ids(&active_problems(&control, true, true)),
            ["service_refused"]
        );

        control.protocol_mismatch = Some(ProtocolMismatch {
            expected: 1,
            got: 2,
        });
        let problems = active_problems(&control, true, true);
        assert_eq!(ids(&problems), ["version_mismatch"]);
        assert!(problems[0].detail.as_deref().unwrap().contains("v1"));

        let waiting = ControlState::default();
        assert_eq!(
            ids(&active_problems(&waiting, true, true)),
            ["service_unreachable"]
        );
    }

    #[test]
    fn the_services_problems_and_the_apps_own_are_listed_errors_first() {
        let mut control = connected();
        control.capabilities = vec![CapabilitySet {
            domain: "gpu".into(),
            supported: false,
            problem: Some(Problem {
                id: "gpu_driver_crashed".into(),
                severity: Severity::Error,
                ..Default::default()
            }),
            ..Default::default()
        }];
        control.crash_guard_notice = Some("reverted".into());
        control.safety_tripped = Some("CPU 97°C above the critical threshold (95°C)".into());
        control.last_apply_result = Some(ApplyResult {
            ok: false,
            message: Some("Profile Gaming not applied: …".into()),
            profile_id: Some("gaming".into()),
        });

        let problems = active_problems(&control, false, true);

        assert_eq!(
            ids(&problems),
            [
                "gpu_driver_crashed",
                "apply_failed",
                "sensor_data_lost",
                "crash_guard",
                "safety_tripped"
            ]
        );
    }

    #[test]
    fn a_partly_applied_profile_is_attention_with_what_was_skipped() {
        let mut control = connected();
        control.last_apply_result = Some(ApplyResult {
            ok: true,
            message: Some(
                "Profile Gaming applied; gpu skipped: the graphics driver crashed".into(),
            ),
            profile_id: None,
        });

        let problems = active_problems(&control, true, true);

        assert_eq!(ids(&problems), ["apply_partial"]);
        assert_eq!(problems[0].severity, Severity::Attention);
        assert!(problems[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("gpu skipped"));
    }

    #[test]
    fn a_supported_domain_with_a_stale_problem_is_not_listed() {
        let mut control = connected();
        control.capabilities = vec![CapabilitySet {
            domain: "gpu".into(),
            supported: true,
            problem: Some(Problem::default()),
            ..Default::default()
        }];
        assert!(active_problems(&control, true, true).is_empty());
    }

    #[test]
    fn every_problem_says_what_to_do_in_plain_words() {
        let all = [
            service_unreachable(),
            sensor_data_lost(),
            service_refused(),
            version_mismatch(&ProtocolMismatch {
                expected: 1,
                got: 2,
            }),
            apply_failed("m"),
            apply_partial("m"),
            safety_tripped("CPU 97°C above the critical threshold (95°C)"),
            service_error("m"),
            dry_run(),
            update_failed("m"),
            wallpaper_engine_failed("m"),
            wallpaper_saves_only(),
        ];
        for p in &all {
            assert!(!p.id.is_empty() && !p.title.is_empty(), "{p:?}");
            assert!(p.message.ends_with('.'), "a full sentence: {}", p.message);
            assert!(
                !p.title.ends_with('.'),
                "a title, not a sentence: {}",
                p.title
            );
            for a in &p.actions {
                if a.kind == ProblemActionKind::OpenUrl {
                    assert!(
                        a.target.as_deref().unwrap().starts_with("https://"),
                        "{a:?}"
                    );
                }
            }
        }
        let unique: std::collections::BTreeSet<_> = all.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(unique.len(), all.len(), "ids are unique");
    }
}
