//! Every problem the app itself reports, in one place and one wording
//! (#370, #373). A state that shows in several windows — the service not
//! answering, this app refused, a profile that didn't apply — is built here
//! once, so the Control Center, Status and notifications say the same thing.
//! The service's own problems (a crashed driver) arrive as
//! `CapabilitySet::problem`; [`active_problems`] lists them all for Status.
//! Shown with `ui_kit::notice`; written for someone who has never seen the
//! code: what happened, what still works, what to do.

use rigstats_backend::control::{
    ApplyResult, ControlState, Problem, ProblemAction, ProblemActionKind, ProtocolMismatch,
    Severity,
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
            read_more("#the-rigstats-service-isnt-answering-or-values-show---"),
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

/// Why Windows doesn't show RIGStats' notifications (#449).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationsOff {
    /// Turned off for RIGStats in Windows' notification settings.
    ForRigStats,
    /// Turned off for every app.
    ForAllApps,
    /// Turned off by the organisation's policy.
    ByPolicy,
}

/// Windows turned the notifications off: alerts won't show.
pub fn notifications_off(why: NotificationsOff) -> Problem {
    let open_settings = || {
        action(
            ProblemActionKind::OpenNotificationSettings,
            "Open Windows settings",
            None,
        )
    };
    let (message, actions) = match why {
        NotificationsOff::ForRigStats => (
            "Notifications from RIGStats are turned off in Windows, so alerts won't show. \
             Everything else keeps working. Turn RIGStats back on under Notifications in Windows \
             settings.",
            vec![open_settings()],
        ),
        NotificationsOff::ForAllApps => (
            "Notifications are turned off for all apps in Windows, so RIGStats' alerts won't show \
             either. Everything else keeps working. Turn notifications back on in Windows settings.",
            vec![open_settings()],
        ),
        NotificationsOff::ByPolicy => (
            "Your organisation's policy turns notifications off on this PC, so RIGStats' alerts \
             won't show. Everything else keeps working. Ask whoever manages this PC.",
            Vec::new(),
        ),
    };
    problem(
        "notifications_off",
        Severity::Attention,
        "Notifications are off in Windows",
        message.to_owned(),
        actions,
        None,
    )
}

/// The notification for a profile switch that failed or only partly applied
/// (#373) — a new result since `previous`, while the Control Center is closed
/// (its footer shows it otherwise). Hotkey, app rules and the tray switch
/// profiles without a window, so this is how a failure gets seen.
pub fn apply_notification(
    previous: Option<&ApplyResult>,
    current: Option<&ApplyResult>,
    control_open: bool,
) -> Option<Problem> {
    let current = current?;
    if control_open || previous == Some(current) {
        return None;
    }
    match (current.ok, current.message.as_deref()) {
        (false, message) => Some(apply_failed(message.unwrap_or("Apply failed."))),
        (true, Some(message)) => Some(apply_partial(message)),
        (true, None) => None,
    }
}

/// Every problem right now, most serious first — for Status. `service_running`
/// is the service's state (`sc query`); `sensor_connected` whether its
/// readings arrive; `hardware` the sensor service's hardware problems and the
/// app's own that aren't in `control` (Windows' notification setting, #449).
pub fn active_problems(
    control: &ControlState,
    sensor_connected: bool,
    service_running: bool,
    hardware: &[Problem],
) -> Vec<Problem> {
    // Readings arriving (once a second) are the service answering, whatever
    // Windows says about it — e.g. the development sidecar running in its
    // place (#374, seen live: "isn't answering" hid every other problem).
    if !service_running && !sensor_connected {
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
    // Hardware the sensor service stopped reading (#374): on the telemetry
    // line, so only while readings arrive.
    out.extend(hardware.iter().cloned());
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

    /// The anchors `docs/troubleshooting.md` offers: GitHub's heading slugs
    /// (lower case; letters, digits, spaces, `-` and `_` kept; spaces to `-`)
    /// and the `<a id="…">` kept for old links.
    fn troubleshooting_anchors() -> Vec<String> {
        let doc = include_str!("../../docs/troubleshooting.md");
        let mut anchors = Vec::new();
        for line in doc.lines() {
            if let Some(heading) = line.strip_prefix("## ") {
                let slug: String = heading
                    .to_lowercase()
                    .chars()
                    .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
                    .map(|c| if c == ' ' { '-' } else { c })
                    .collect();
                anchors.push(slug);
            }
            for part in line.split("<a id=\"").skip(1) {
                if let Some((id, _)) = part.split_once('"') {
                    anchors.push(id.to_owned());
                }
            }
        }
        anchors
    }

    #[test]
    fn every_read_more_anchor_exists_in_the_troubleshooting_guide() {
        let anchors = troubleshooting_anchors();
        let app: Vec<String> = [service_unreachable(), service_refused()]
            .iter()
            .flat_map(|p| p.actions.iter())
            .filter_map(|a| a.target.as_deref()?.strip_prefix(TROUBLESHOOTING_URL))
            .map(|a| a.trim_start_matches('#').to_owned())
            .collect();
        assert!(!app.is_empty());
        // The service's links (sensor-sidecar: ControlProtocol.cs ReadMore,
        // GpuWorker.HelpUrl, HardwareWorkerClient.ReadMoreAnchor) and older
        // app versions'. Changing a heading must keep these working.
        let kept = [
            "control-center",
            "gpu-sensors-missing-temp--clock--power--fan-or-sidecar-crashes",
            "cpu-temperature--package-power-missing-or-defender-alert-on-service-start",
            "desktop-wallpaper-mode",
            "wallpaper-engine",
        ];
        for anchor in app.iter().map(String::as_str).chain(kept) {
            assert!(anchors.iter().any(|a| a == anchor), "#{anchor} missing");
        }
    }

    fn connected() -> ControlState {
        ControlState {
            connected: true,
            ..Default::default()
        }
    }

    fn ids(problems: &[Problem]) -> Vec<&str> {
        problems.iter().map(|p| p.id.as_str()).collect()
    }

    fn result(ok: bool, message: Option<&str>) -> ApplyResult {
        ApplyResult {
            ok,
            message: message.map(str::to_owned),
            profile_id: Some("gaming".into()),
        }
    }

    #[test]
    fn a_failed_or_partial_switch_is_notified_once_while_the_control_center_is_closed() {
        let failed = result(
            false,
            Some("Profile Gaming not applied: fan header control/1 did not accept 40 %"),
        );
        let partial = result(
            true,
            Some("Profile Gaming applied; gpu skipped: the graphics driver crashed"),
        );
        let fine = result(true, None);

        let n = apply_notification(None, Some(&failed), false).unwrap();
        assert_eq!(
            (n.id.as_str(), n.severity),
            ("apply_failed", Severity::Error)
        );
        assert!(n.detail.unwrap().contains("did not accept 40 %"));
        assert_eq!(
            apply_notification(None, Some(&partial), false).map(|n| n.severity),
            Some(Severity::Attention)
        );

        assert_eq!(
            apply_notification(Some(&failed), Some(&failed), false),
            None
        ); // already told
        assert_eq!(apply_notification(None, Some(&failed), true), None); // the footer shows it
        assert_eq!(apply_notification(None, Some(&fine), false), None);
        assert_eq!(apply_notification(Some(&failed), None, false), None);
    }

    #[test]
    fn all_well_is_no_problems() {
        assert!(active_problems(&connected(), true, true, &[]).is_empty());
    }

    #[test]
    fn a_stopped_service_says_only_that() {
        let mut control = connected();
        control.last_error = Some("x".into());
        assert_eq!(
            ids(&active_problems(&control, false, false, &[])),
            ["service_unreachable"]
        );
    }

    #[test]
    fn readings_arriving_mean_the_service_answers_whatever_windows_says() {
        // #374: the sensor service's hardware problems must show, not a
        // "isn't answering" that contradicts the live readings.
        let hardware = Problem {
            id: "hardware_off".into(),
            ..Default::default()
        };
        assert_eq!(
            ids(&active_problems(
                &connected(),
                true,
                false,
                std::slice::from_ref(&hardware)
            )),
            ["hardware_off"]
        );
    }

    #[test]
    fn connection_problems_come_first_and_only_the_most_specific_one() {
        let mut control = ControlState {
            refused: true,
            ..Default::default()
        };
        assert_eq!(
            ids(&active_problems(&control, true, true, &[])),
            ["service_refused"]
        );

        control.protocol_mismatch = Some(ProtocolMismatch {
            expected: 1,
            got: 2,
        });
        let problems = active_problems(&control, true, true, &[]);
        assert_eq!(ids(&problems), ["version_mismatch"]);
        assert!(problems[0].detail.as_deref().unwrap().contains("v1"));

        let waiting = ControlState::default();
        assert_eq!(
            ids(&active_problems(&waiting, true, true, &[])),
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

        let problems = active_problems(&control, false, true, &[]);

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

        let problems = active_problems(&control, true, true, &[]);

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
        assert!(active_problems(&control, true, true, &[]).is_empty());
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

    #[test]
    fn notifications_off_offers_windows_settings_unless_a_policy_decides() {
        for why in [NotificationsOff::ForRigStats, NotificationsOff::ForAllApps] {
            let p = notifications_off(why);
            assert_eq!(p.severity, Severity::Attention);
            assert_eq!(
                p.actions.iter().map(|a| a.kind).collect::<Vec<_>>(),
                [ProblemActionKind::OpenNotificationSettings]
            );
        }
        assert!(notifications_off(NotificationsOff::ByPolicy)
            .actions
            .is_empty());
        // The source's line breaks mustn't leave runs of spaces in the text.
        for why in [
            NotificationsOff::ForRigStats,
            NotificationsOff::ForAllApps,
            NotificationsOff::ByPolicy,
        ] {
            assert!(!notifications_off(why).message.contains("  "), "{why:?}");
        }
    }
}
