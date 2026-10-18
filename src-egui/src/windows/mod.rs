pub mod about;
pub mod control;
pub mod history;
pub mod mouse_drawing;
pub mod profile_look;
pub mod rules_page;
pub mod settings;
pub mod status;
pub mod ui_kit;
pub mod updater;

use std::sync::{Arc, Mutex};

use crate::lock_ext::LockSafe;
use rigstats_backend::control::{ProblemAction, ProblemActionKind};

/// A window asking for another one to open on a given page — Settings and
/// the Control Center link to each other where a setting lives in the
/// other one. `RigStatsApp` picks it up next frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRequest {
    Settings(settings::Page),
    Control(control::Page),
    About,
    Status,
    Updates,
    History,
    /// Open Status and collect a diagnostics ZIP — a problem's
    /// "Collect diagnostics…" button, from any window (#370).
    Diagnostics,
}

pub type OpenRequests = Arc<Mutex<Option<OpenRequest>>>;

/// What a problem's button does (#370) — decided here once for every window,
/// so no window can miss a kind (Status had its own copy and ignored "Open
/// Windows settings", #449).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionEffect {
    /// An https link — a problem's text comes from the service, and the app
    /// must not be made to open anything else.
    OpenLink(String),
    /// Windows' notification settings (a fixed page).
    OpenNotificationSettings,
    /// Another window (or Status collecting diagnostics).
    Open(OpenRequest),
    /// The showing window's own (`Retry`), or nothing (an unsafe link, a
    /// kind from a newer service).
    None,
}

pub fn action_effect(action: &ProblemAction) -> ActionEffect {
    match action.kind {
        ProblemActionKind::OpenUrl => action
            .target
            .as_deref()
            .filter(|u| is_safe_link(u))
            .map_or(ActionEffect::None, |u| ActionEffect::OpenLink(u.to_owned())),
        ProblemActionKind::CollectDiagnostics => ActionEffect::Open(OpenRequest::Diagnostics),
        // The debug log is in Status.
        ProblemActionKind::OpenLog => ActionEffect::Open(OpenRequest::Status),
        ProblemActionKind::OpenNotificationSettings => ActionEffect::OpenNotificationSettings,
        // Only the window showing the problem knows what to try again.
        ProblemActionKind::Retry | ProblemActionKind::Unknown => ActionEffect::None,
    }
}

/// Carries out a problem's button from a window that can open others.
pub fn run_problem_action(ctx: &egui::Context, action: &ProblemAction, open: &OpenRequests) {
    match action_effect(action) {
        ActionEffect::Open(request) => *open.lock_safe() = Some(request),
        effect => perform_local(ctx, &effect),
    }
}

/// Carries out what needs no other window — a link, Windows' settings — and
/// nothing else. For windows without [`OpenRequests`] (Status, Updates),
/// after handling the [`ActionEffect::Open`] they can do themselves.
pub fn perform_local(ctx: &egui::Context, effect: &ActionEffect) {
    match effect {
        ActionEffect::OpenLink(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        #[cfg(windows)]
        ActionEffect::OpenNotificationSettings => crate::notify::open_windows_settings(),
        _ => {}
    }
}

/// An https link with a host — nothing local, no other scheme.
fn is_safe_link(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        !rest.is_empty() && !rest.starts_with('/') && !rest.contains(char::is_whitespace)
    })
}

/// Debug builds only: `RIGSTATS_OPEN=control:<page>`, `settings:<page>`,
/// `about`, `status`, `updates` or `history` opens that dialog at start-up, so a page can be screenshotted without
/// clicking through the tray (`/verifier-gui`). Pages are the sidebar's
/// names in lower case, e.g. `control:overview`, `settings:display`.
pub fn dev_open_request() -> Option<OpenRequest> {
    if !cfg!(debug_assertions) {
        return None;
    }
    parse_open_request(&std::env::var("RIGSTATS_OPEN").ok()?)
}

fn parse_open_request(value: &str) -> Option<OpenRequest> {
    let (window, page) = value.split_once(':').unwrap_or((value, ""));
    match window {
        "settings" => Some(OpenRequest::Settings(match page {
            "display" => settings::Page::Display,
            "overlay" => settings::Page::Overlay,
            "notifications" => settings::Page::Notifications,
            _ => settings::Page::General,
        })),
        "control" => Some(OpenRequest::Control(match page {
            "dashboard" => control::Page::Dashboard,
            "overlay" => control::Page::Overlay,
            "alerts" => control::Page::Alerts,
            "desktop" => control::Page::Desktop,
            "power" => control::Page::Power,
            "fans" => control::Page::Fans,
            "cpu" => control::Page::Cpu,
            "gpu" => control::Page::Gpu,
            "lighting" => control::Page::Lighting,
            "headset" => control::Page::Headset,
            "mouse" => control::Page::Mouse,
            "rules" => control::Page::Rules,
            _ => control::Page::Overview,
        })),
        "about" => Some(OpenRequest::About),
        "status" => Some(OpenRequest::Status),
        "updates" => Some(OpenRequest::Updates),
        "history" => Some(OpenRequest::History),
        "diagnostics" => Some(OpenRequest::Diagnostics),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_requests_parse() {
        assert_eq!(
            parse_open_request("control:alerts"),
            Some(OpenRequest::Control(control::Page::Alerts))
        );
        assert_eq!(
            parse_open_request("settings"),
            Some(OpenRequest::Settings(settings::Page::General))
        );
        assert_eq!(parse_open_request("history"), Some(OpenRequest::History));
        assert_eq!(
            parse_open_request("diagnostics"),
            Some(OpenRequest::Diagnostics)
        );
        assert_eq!(parse_open_request("nope:x"), None);
    }

    #[test]
    fn only_https_links_with_a_host_are_opened() {
        assert!(is_safe_link(
            "https://www.amd.com/en/support/download/drivers.html"
        ));
        assert!(is_safe_link("https://rigstats.app/#faq-amd-driver"));
        assert!(!is_safe_link("http://example.com"));
        assert!(!is_safe_link("file:///C:/Windows/System32/cmd.exe"));
        assert!(!is_safe_link("C:\\Windows\\System32\\cmd.exe"));
        assert!(!is_safe_link("https://"));
        assert!(!is_safe_link("https:///etc"));
        assert!(!is_safe_link("https://a b"));
        assert!(!is_safe_link("javascript:alert(1)"));
    }

    #[test]
    fn every_button_kind_does_something_except_retry_and_unknown() {
        let action = |kind, target: Option<&str>| ProblemAction {
            kind,
            label: "x".into(),
            target: target.map(str::to_owned),
        };
        use ProblemActionKind as K;
        assert_eq!(
            action_effect(&action(K::OpenUrl, Some("https://rigstats.app/"))),
            ActionEffect::OpenLink("https://rigstats.app/".into())
        );
        assert_eq!(
            action_effect(&action(K::CollectDiagnostics, None)),
            ActionEffect::Open(OpenRequest::Diagnostics)
        );
        assert_eq!(
            action_effect(&action(K::OpenLog, None)),
            ActionEffect::Open(OpenRequest::Status)
        );
        // A fixed page: a target from the service is ignored.
        assert_eq!(
            action_effect(&action(
                K::OpenNotificationSettings,
                Some("file:///C:/x.exe")
            )),
            ActionEffect::OpenNotificationSettings
        );
        assert_eq!(action_effect(&action(K::Retry, None)), ActionEffect::None);
        assert_eq!(action_effect(&action(K::Unknown, None)), ActionEffect::None);
    }

    #[test]
    fn collect_diagnostics_asks_for_the_status_window() {
        let open: OpenRequests = Arc::new(Mutex::new(None));
        let ctx = egui::Context::default();
        let action = ProblemAction {
            kind: ProblemActionKind::CollectDiagnostics,
            label: "Collect diagnostics…".into(),
            target: None,
        };

        run_problem_action(&ctx, &action, &open);

        assert_eq!(*open.lock_safe(), Some(OpenRequest::Diagnostics));
    }

    #[test]
    fn an_unsafe_or_unknown_action_does_nothing() {
        let open: OpenRequests = Arc::new(Mutex::new(None));
        let ctx = egui::Context::default();
        for action in [
            ProblemAction {
                kind: ProblemActionKind::OpenUrl,
                label: "x".into(),
                target: Some("file:///C:/x.exe".into()),
            },
            ProblemAction {
                kind: ProblemActionKind::Unknown,
                label: "x".into(),
                target: None,
            },
        ] {
            run_problem_action(&ctx, &action, &open);
        }

        assert_eq!(*open.lock_safe(), None);
        assert!(ctx.output(|o| o.commands.is_empty()));
    }
}
