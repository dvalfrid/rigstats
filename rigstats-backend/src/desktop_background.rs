//! The Windows desktop background a profile can set (#306): a picture and
//! how it fits the screen. A profile without one shows Windows' own
//! background — the one that was there before RIGStats first replaced it,
//! kept in `Settings::desktop_background_before`.
//!
//! `#![allow(unsafe_code)]`: `SystemParametersInfoW` has no safe wrapper.
#![allow(unsafe_code)]

use serde::{Deserialize, Serialize};

/// A picture on the desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopBackground {
    /// The image file; empty = a plain colour (Windows' own, restored).
    pub path: String,
    #[serde(default)]
    pub fit: Fit,
}

/// How the picture fills the screen — the choices Windows' own Settings
/// offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    #[default]
    Fill,
    Fit,
    Stretch,
    Tile,
    Center,
    Span,
}

impl Fit {
    pub const ALL: [Fit; 6] = [
        Fit::Fill,
        Fit::Fit,
        Fit::Stretch,
        Fit::Tile,
        Fit::Center,
        Fit::Span,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Fit::Fill => "Fill",
            Fit::Fit => "Fit",
            Fit::Stretch => "Stretch",
            Fit::Tile => "Tile",
            Fit::Center => "Centre",
            Fit::Span => "Span",
        }
    }

    /// `WallpaperStyle` and `TileWallpaper` under
    /// `HKCU\Control Panel\Desktop`.
    fn registry(self) -> (&'static str, &'static str) {
        match self {
            Fit::Fill => ("10", "0"),
            Fit::Fit => ("6", "0"),
            Fit::Stretch => ("2", "0"),
            Fit::Tile => ("0", "1"),
            Fit::Center => ("0", "0"),
            Fit::Span => ("22", "0"),
        }
    }

    fn from_registry(style: &str, tile: &str) -> Self {
        match (style.trim(), tile.trim()) {
            (_, "1") => Fit::Tile,
            ("6", _) => Fit::Fit,
            ("2", _) => Fit::Stretch,
            ("0", _) => Fit::Center,
            ("22", _) => Fit::Span,
            _ => Fit::Fill,
        }
    }
}

/// What to do when the active profile's background is `wanted`.
#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    /// Show this (`None` = leave the desktop as it is).
    pub show: Option<DesktopBackground>,
    /// Windows' own background from now on.
    pub before: Option<DesktopBackground>,
}

/// A profile with a picture shows it, first keeping Windows' own background
/// (read through `current`) if it isn't kept already; a profile without one
/// puts Windows' own back, if RIGStats replaced it.
pub fn step(
    wanted: Option<&DesktopBackground>,
    before: Option<&DesktopBackground>,
    current: impl FnOnce() -> Option<DesktopBackground>,
) -> Step {
    match wanted {
        Some(w) => Step {
            show: Some(w.clone()),
            before: before.cloned().or_else(current),
        },
        None => Step {
            show: before.cloned(),
            before: None,
        },
    }
}

const DESKTOP_KEY: &str = r"Control Panel\Desktop";

/// The background Windows shows now.
#[cfg(windows)]
pub fn current() -> Option<DesktopBackground> {
    use winapi::um::winuser::{SystemParametersInfoW, SPI_GETDESKWALLPAPER};
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let mut buf = vec![0u16; 1024];
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            buf.as_mut_ptr().cast(),
            0,
        )
    };
    if ok == 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(DESKTOP_KEY)
        .ok();
    let read = |name: &str| {
        key.as_ref()
            .and_then(|k| k.get_value::<String, _>(name).ok())
            .unwrap_or_default()
    };
    Some(DesktopBackground {
        path: String::from_utf16_lossy(&buf[..len]),
        fit: Fit::from_registry(&read("WallpaperStyle"), &read("TileWallpaper")),
    })
}

#[cfg(not(windows))]
pub fn current() -> Option<DesktopBackground> {
    None
}

/// Puts `bg` on the desktop. Blocks while Windows tells every window about
/// it — call it off the UI thread.
#[cfg(windows)]
pub fn show(bg: &DesktopBackground) -> Result<(), String> {
    use winapi::um::winuser::{
        SystemParametersInfoW, SPIF_SENDWININICHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
    };
    use winreg::{
        enums::{HKEY_CURRENT_USER, KEY_SET_VALUE},
        RegKey,
    };
    if !bg.path.is_empty() && !std::path::Path::new(&bg.path).is_file() {
        return Err(format!("{} is gone", bg.path));
    }
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(DESKTOP_KEY, KEY_SET_VALUE)
        .map_err(|e| e.to_string())?;
    let (style, tile) = bg.fit.registry();
    key.set_value("WallpaperStyle", &style)
        .and_then(|()| key.set_value("TileWallpaper", &tile))
        .map_err(|e| e.to_string())?;
    let mut path: Vec<u16> = bg.path.encode_utf16().chain(std::iter::once(0)).collect();
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            path.as_mut_ptr().cast(),
            SPIF_UPDATEINIFILE | SPIF_SENDWININICHANGE,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn show(_bg: &DesktopBackground) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bg(path: &str) -> DesktopBackground {
        DesktopBackground {
            path: path.into(),
            fit: Fit::Fill,
        }
    }

    #[test]
    fn a_picture_keeps_windows_own_background_the_first_time() {
        let s = step(Some(&bg("calm.jpg")), None, || Some(bg("own.jpg")));
        assert_eq!(s.show, Some(bg("calm.jpg")));
        assert_eq!(s.before, Some(bg("own.jpg")));
    }

    #[test]
    fn the_next_picture_keeps_the_first_one_kept() {
        let s = step(Some(&bg("scene.jpg")), Some(&bg("own.jpg")), || {
            panic!("windows' own is kept already")
        });
        assert_eq!(s.show, Some(bg("scene.jpg")));
        assert_eq!(s.before, Some(bg("own.jpg")));
    }

    #[test]
    fn no_picture_puts_windows_own_back() {
        let s = step(None, Some(&bg("own.jpg")), || None);
        assert_eq!(s.show, Some(bg("own.jpg")));
        assert_eq!(s.before, None);
    }

    #[test]
    fn no_picture_and_nothing_replaced_leaves_the_desktop_alone() {
        assert_eq!(
            step(None, None, || Some(bg("own.jpg"))),
            Step {
                show: None,
                before: None
            }
        );
    }

    #[test]
    fn every_fit_reads_back_from_the_registry_as_itself() {
        for fit in Fit::ALL {
            let (style, tile) = fit.registry();
            assert_eq!(Fit::from_registry(style, tile), fit);
        }
    }

    #[test]
    fn stored_as_lower_case_words() {
        let json = serde_json::to_string(&DesktopBackground {
            path: "a.png".into(),
            fit: Fit::Center,
        })
        .unwrap();
        assert_eq!(json, r#"{"path":"a.png","fit":"center"}"#);
        let back: DesktopBackground = serde_json::from_str(r#"{"path":"a.png"}"#).unwrap();
        assert_eq!(back.fit, Fit::Fill);
    }

    #[cfg(windows)]
    #[test]
    fn windows_reports_its_background() {
        assert!(current().is_some());
    }
}
