//! Windows notifications sent by RIGStats itself (#447): a toast under the
//! app's own app id, so Windows shows "RIGStats" and its icon as the sender
//! and lists RIGStats in its notification settings. They used to come from a
//! hidden PowerShell balloon, which showed as "Windows PowerShell".
//!
//! An unpackaged app gets a name and icon for its app id from
//! `HKCU\Software\Classes\AppUserModelId\<id>` ([`register`], at start-up);
//! the uninstaller removes the key.

#![allow(unsafe_code)]

use std::path::Path;

/// The app id toasts are sent under. The uninstaller (`build/installer.nsi`)
/// deletes its registry key — keep the two in step.
pub const APP_ID: &str = "se.codeby.rigstats";

const ICON_PNG: &[u8] = include_bytes!("../../assets/icon-256.png");
const ICON_FILE: &str = "rigstats-notification-icon.png";

/// Gives the app id its name and icon: writes the icon next to the settings
/// in `dir` and the registry key. Cheap — run once at start-up. Errors only
/// cost the name and icon (the toast still shows), so they're returned for
/// the log, not raised.
pub fn register(dir: &Path) -> Result<(), String> {
    let icon = dir.join(ICON_FILE);
    if std::fs::read(&icon).ok().as_deref() != Some(ICON_PNG) {
        std::fs::write(&icon, ICON_PNG).map_err(|e| format!("icon: {e}"))?;
    }
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))
        .map_err(|e| format!("registry: {e}"))?;
    key.set_value("DisplayName", &"RIGStats")
        .and_then(|()| key.set_value("IconUri", &icon.to_string_lossy().into_owned()))
        .map_err(|e| format!("registry: {e}"))
}

/// Shows a toast. Quick (no process start), but call it off the UI thread
/// anyway: the first WinRT call loads the notification platform.
pub fn send(title: &str, message: &str) -> Result<(), String> {
    use ::windows::core::HSTRING;
    use ::windows::Data::Xml::Dom::XmlDocument;
    use ::windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    // WinRT needs an apartment on this thread; "already initialised" is fine.
    let _ = unsafe {
        ::windows::Win32::System::WinRT::RoInitialize(
            ::windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        )
    };
    let show = || -> ::windows::core::Result<()> {
        let xml = XmlDocument::new()?;
        xml.LoadXml(&HSTRING::from(toast_xml(title, message)))?;
        let toast = ToastNotification::CreateToastNotification(&xml)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
    };
    show().map_err(|e| e.to_string())
}

/// The toast's XML: a title line and a message line.
fn toast_xml(title: &str, message: &str) -> String {
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escape(title),
        escape(message)
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_and_message_are_escaped_into_the_toast() {
        assert_eq!(
            toast_xml("Fan & <safety>", "CPU \"hot\" — it's 95 °C"),
            "<toast><visual><binding template=\"ToastGeneric\">\
             <text>Fan &amp; &lt;safety&gt;</text>\
             <text>CPU &quot;hot&quot; — it&apos;s 95 °C</text>\
             </binding></visual></toast>"
        );
    }
}
