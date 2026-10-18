//! Windows notifications sent by RIGStats itself (#447): a toast under the
//! app's own app id, so Windows shows "RIGStats" and its icon as the sender
//! and lists RIGStats in its notification settings. They used to come from a
//! hidden PowerShell balloon, which showed as "Windows PowerShell".
//!
//! An unpackaged app gets a name and icon for its app id from
//! `HKCU\Software\Classes\AppUserModelId\<id>` ([`register`], at start-up);
//! the uninstaller removes the key.
//!
//! Windows can turn the notifications off (#449): [`watch_setting`] reads
//! the setting in the background and [`off`] says why, so Settings and Status
//! can tell the user instead of alerts silently not showing.

#![allow(unsafe_code)]

use crate::problems::NotificationsOff;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

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

/// Windows' notification setting for RIGStats, as last read by
/// [`watch_setting`]: 0 on (or not read yet), else [`NotificationsOff`] + 1.
static SETTING: AtomicU8 = AtomicU8::new(0);

/// Why Windows doesn't show RIGStats' notifications, if it doesn't.
pub fn off() -> Option<NotificationsOff> {
    match SETTING.load(Ordering::Relaxed) {
        1 => Some(NotificationsOff::ForRigStats),
        2 => Some(NotificationsOff::ForAllApps),
        3 => Some(NotificationsOff::ByPolicy),
        _ => None,
    }
}

/// Reads Windows' notification setting for RIGStats every few seconds on a
/// thread of its own (a WinRT call; the setting changes in Windows' own
/// window). Start once.
pub fn watch_setting() {
    std::thread::spawn(|| loop {
        let value = match read_setting() {
            Some(NotificationsOff::ForRigStats) => 1,
            Some(NotificationsOff::ForAllApps) => 2,
            Some(NotificationsOff::ByPolicy) => 3,
            None => 0,
        };
        SETTING.store(value, Ordering::Relaxed);
        std::thread::sleep(std::time::Duration::from_secs(3));
    });
}

/// `None` when notifications show, or when the setting can't be read
/// (nothing to tell the user then).
fn read_setting() -> Option<NotificationsOff> {
    use ::windows::core::HSTRING;
    use ::windows::UI::Notifications::ToastNotificationManager;
    let _ = unsafe {
        ::windows::Win32::System::WinRT::RoInitialize(
            ::windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        )
    };
    let setting = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))
        .and_then(|notifier| notifier.Setting())
        .ok()?;
    off_for(setting)
}

fn off_for(setting: ::windows::UI::Notifications::NotificationSetting) -> Option<NotificationsOff> {
    use ::windows::UI::Notifications::NotificationSetting as S;
    match setting {
        S::DisabledForApplication => Some(NotificationsOff::ForRigStats),
        S::DisabledForUser => Some(NotificationsOff::ForAllApps),
        S::DisabledByGroupPolicy => Some(NotificationsOff::ByPolicy),
        _ => None,
    }
}

/// Opens Windows' notification settings, where RIGStats is listed.
pub fn open_windows_settings() {
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let (verb, target) = (wide("open"), wide("ms-settings:notifications"));
    unsafe {
        winapi::um::shellapi::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            winapi::um::winuser::SW_SHOWNORMAL,
        );
    }
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
    fn windows_settings_map_to_the_reason_notifications_are_off() {
        use ::windows::UI::Notifications::NotificationSetting as S;
        assert_eq!(off_for(S::Enabled), None);
        assert_eq!(
            off_for(S::DisabledForApplication),
            Some(NotificationsOff::ForRigStats)
        );
        assert_eq!(
            off_for(S::DisabledForUser),
            Some(NotificationsOff::ForAllApps)
        );
        assert_eq!(
            off_for(S::DisabledByGroupPolicy),
            Some(NotificationsOff::ByPolicy)
        );
        // Packaged apps only; not something a user can change.
        assert_eq!(off_for(S::DisabledByManifest), None);
    }

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
