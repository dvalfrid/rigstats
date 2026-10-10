pub mod about;
pub mod control;
pub mod history;
pub mod profile_look;
pub mod settings;
pub mod status;
pub mod ui_kit;
pub mod updater;

use std::sync::{Arc, Mutex};

/// A window asking for another one to open on a given page — Settings and
/// the Control Center link to each other where a setting lives in the
/// other one. `RigStatsApp` picks it up next frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRequest {
    Settings(settings::Page),
    Control(control::Page),
}

pub type OpenRequests = Arc<Mutex<Option<OpenRequest>>>;

/// Debug builds only: `RIGSTATS_OPEN=control:<page>` or `settings:<page>`
/// opens that dialog at start-up, so a page can be screenshotted without
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
            "power" => control::Page::Power,
            "fans" => control::Page::Fans,
            "cpu" => control::Page::Cpu,
            "gpu" => control::Page::Gpu,
            "lighting" => control::Page::Lighting,
            _ => control::Page::Overview,
        })),
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
        assert_eq!(parse_open_request("nope:x"), None);
    }
}
