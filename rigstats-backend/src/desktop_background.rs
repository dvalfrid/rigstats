//! The Windows desktop background a profile can set (#306): a picture — the
//! same on every screen or one per screen — and how it fits. A profile
//! without one shows Windows' own background — the one that was there
//! before RIGStats first replaced it, kept in
//! `Settings::desktop_background_before`.
//!
//! Windows' `IDesktopWallpaper` does the work: a picture per monitor, one fit
//! for all of them (Windows has no fit per monitor).
//!
//! `#![allow(unsafe_code)]`: COM calls have no safe wrapper.
#![allow(unsafe_code)]

use serde::{Deserialize, Serialize};

/// A desktop background.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopBackground {
    /// The picture on every screen not listed in `screens`; empty = a plain
    /// colour (Windows' own, restored).
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub fit: Fit,
    /// A picture of its own per screen, by Windows' monitor id. Empty = the
    /// same picture everywhere.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screens: Vec<ScreenPicture>,
}

/// One screen's own picture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenPicture {
    /// Windows' monitor id (a device path, stable across restarts).
    pub screen: String,
    pub path: String,
}

impl DesktopBackground {
    /// The picture `screen` shows.
    pub fn path_for(&self, screen: &str) -> &str {
        self.screens
            .iter()
            .find(|s| s.screen == screen)
            .map_or(&self.path, |s| &s.path)
    }

    /// Gives `screen` its own picture. Keeps "the same everywhere" when
    /// every listed screen ends up with the same picture.
    pub fn set_screen(&mut self, screen: &str, path: String, all: &[String]) {
        if self.screens.is_empty() {
            self.screens = all
                .iter()
                .map(|s| ScreenPicture {
                    screen: s.clone(),
                    path: self.path.clone(),
                })
                .collect();
        }
        match self.screens.iter_mut().find(|s| s.screen == screen) {
            Some(s) => s.path = path,
            None => self.screens.push(ScreenPicture {
                screen: screen.to_owned(),
                path,
            }),
        }
        self.collapse();
    }

    /// One picture everywhere again when all screens show the same one.
    fn collapse(&mut self) {
        if let Some(first) = self.screens.first().map(|s| s.path.clone()) {
            if self.screens.iter().all(|s| s.path == first) {
                self.path = first;
                self.screens.clear();
            }
        }
    }

    /// What to hand Windows: `(None, path)` for every screen at once, else
    /// one `(Some(screen), path)` per connected screen.
    pub fn assignments<'a>(&'a self, connected: &'a [String]) -> Vec<(Option<&'a str>, &'a str)> {
        if self.screens.is_empty() {
            vec![(None, self.path.as_str())]
        } else {
            connected
                .iter()
                .map(|s| (Some(s.as_str()), self.path_for(s)))
                .collect()
        }
    }

    /// Every picture this background uses.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.path.as_str())
            .chain(self.screens.iter().map(|s| s.path.as_str()))
            .filter(|p| !p.is_empty())
    }
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

    /// `DESKTOP_WALLPAPER_POSITION`.
    fn position(self) -> u32 {
        match self {
            Fit::Center => 0,
            Fit::Tile => 1,
            Fit::Stretch => 2,
            Fit::Fit => 3,
            Fit::Fill => 4,
            Fit::Span => 5,
        }
    }

    fn from_position(p: u32) -> Self {
        match p {
            0 => Fit::Center,
            1 => Fit::Tile,
            2 => Fit::Stretch,
            3 => Fit::Fit,
            5 => Fit::Span,
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

/// A connected screen: Windows' monitor id and where it sits on the desktop
/// (left, top, right, bottom).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub id: String,
    pub rect: [i32; 4],
}

impl Screen {
    pub fn size(&self) -> (i32, i32) {
        (self.rect[2] - self.rect[0], self.rect[3] - self.rect[1])
    }

    /// The main screen sits at the desktop's origin.
    pub fn is_main(&self) -> bool {
        self.rect[0] == 0 && self.rect[1] == 0
    }
}

#[cfg(windows)]
mod win {
    use super::{DesktopBackground, Fit, Screen, ScreenPicture};
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        DesktopWallpaper, IDesktopWallpaper, DESKTOP_WALLPAPER_POSITION,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A COM-allocated string, freed.
    ///
    /// # Safety
    /// `p` is null or a NUL-terminated string the caller owns, allocated
    /// with CoTaskMemAlloc.
    unsafe fn take(p: PWSTR) -> String {
        if p.is_null() {
            return String::new();
        }
        // SAFETY: per this function's contract.
        let s = unsafe { p.to_string() }.unwrap_or_default();
        // SAFETY: the caller owns `p`, from CoTaskMemAlloc.
        unsafe { CoTaskMemFree(Some(p.0.cast())) };
        s
    }

    /// Runs `f` with Windows' desktop wallpaper object, setting COM up on
    /// this thread for the call if it isn't already.
    fn with_wallpaper<T>(f: impl FnOnce(&IDesktopWallpaper) -> T) -> Result<T, String> {
        // SAFETY: COM is set up and torn down on this thread around the
        // call; the wallpaper object is released (dropped) before
        // CoUninitialize.
        unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let out =
                match CoCreateInstance::<_, IDesktopWallpaper>(&DesktopWallpaper, None, CLSCTX_ALL)
                {
                    Ok(w) => {
                        let r = f(&w);
                        drop(w);
                        Ok(r)
                    }
                    Err(e) => Err(format!(
                        "no desktop wallpaper object (0x{:08x})",
                        e.code().0
                    )),
                };
            // S_OK / S_FALSE: this call initialized (or re-counted) COM.
            if init.is_ok() {
                CoUninitialize();
            }
            out
        }
    }

    fn connected(w: &IDesktopWallpaper) -> Vec<Screen> {
        // SAFETY: `w` is a live COM object for the whole call.
        let Ok(count) = (unsafe { w.GetMonitorDevicePathCount() }) else {
            return Vec::new();
        };
        (0..count)
            .filter_map(|i| {
                // SAFETY: `w` is live; `i` < the count it reported; the path
                // it returns is ours to free (`take`).
                let id = unsafe { take(w.GetMonitorDevicePathAt(i).ok()?) };
                let id_w = wide(&id);
                // Fails for a monitor Windows remembers but isn't attached.
                // SAFETY: `w` is live; `id_w` is NUL-terminated.
                let r = unsafe { w.GetMonitorRECT(PCWSTR(id_w.as_ptr())) }.ok()?;
                if r.right <= r.left {
                    return None;
                }
                Some(Screen {
                    id,
                    rect: [r.left, r.top, r.right, r.bottom],
                })
            })
            .collect()
    }

    pub fn screens() -> Vec<Screen> {
        let mut list = with_wallpaper(connected).unwrap_or_default();
        list.sort_by_key(|s| (s.rect[0], s.rect[1]));
        list
    }

    pub fn current() -> Option<DesktopBackground> {
        with_wallpaper(|w| {
            // SAFETY: `w` is live for the closure.
            let fit = match unsafe { w.GetPosition() } {
                Ok(pos) => Fit::from_position(pos.0 as u32),
                Err(_) => Fit::Fill,
            };
            let screens: Vec<ScreenPicture> = connected(w)
                .into_iter()
                .map(|s| {
                    let id_w = wide(&s.id);
                    // SAFETY: `w` is live; `id_w` is NUL-terminated; the
                    // path it returns is ours to free (`take`).
                    let path = match unsafe { w.GetWallpaper(PCWSTR(id_w.as_ptr())) } {
                        Ok(p) => unsafe { take(p) },
                        Err(_) => String::new(),
                    };
                    ScreenPicture { screen: s.id, path }
                })
                .collect();
            let mut bg = DesktopBackground {
                path: screens.first().map(|s| s.path.clone()).unwrap_or_default(),
                fit,
                screens,
            };
            bg.collapse();
            bg
        })
        .ok()
    }

    pub fn show(bg: &DesktopBackground) -> Result<(), String> {
        for p in bg.paths() {
            if !std::path::Path::new(p).is_file() {
                return Err(format!("{p} is gone"));
            }
        }
        with_wallpaper(|w| {
            let ids: Vec<String> = connected(w).into_iter().map(|s| s.id).collect();
            // SAFETY: `w` is live for the closure.
            if let Err(e) =
                unsafe { w.SetPosition(DESKTOP_WALLPAPER_POSITION(bg.fit.position() as i32)) }
            {
                return Err(format!("couldn't set the fit (0x{:08x})", e.code().0));
            }
            for (screen, path) in bg.assignments(&ids) {
                let screen = screen.map(wide);
                let path_w = wide(path);
                // SAFETY: `w` is live; both strings are NUL-terminated or the
                // screen is null (every screen).
                let set = unsafe {
                    w.SetWallpaper(
                        screen
                            .as_ref()
                            .map_or(PCWSTR::null(), |s| PCWSTR(s.as_ptr())),
                        PCWSTR(path_w.as_ptr()),
                    )
                };
                if let Err(e) = set {
                    return Err(format!("couldn't set {path} (0x{:08x})", e.code().0));
                }
            }
            Ok(())
        })?
    }
}

/// The connected screens, left to right.
#[cfg(windows)]
pub fn screens() -> Vec<Screen> {
    win::screens()
}

#[cfg(not(windows))]
pub fn screens() -> Vec<Screen> {
    Vec::new()
}

/// The background Windows shows now.
#[cfg(windows)]
pub fn current() -> Option<DesktopBackground> {
    win::current()
}

#[cfg(not(windows))]
pub fn current() -> Option<DesktopBackground> {
    None
}

/// Puts `bg` on the desktop. Blocks while Windows tells every window about
/// it — call it off the UI thread.
#[cfg(windows)]
pub fn show(bg: &DesktopBackground) -> Result<(), String> {
    win::show(bg)
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
            screens: Vec::new(),
        }
    }

    fn ids() -> Vec<String> {
        vec!["left".into(), "right".into()]
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
    fn one_picture_goes_to_every_screen_at_once() {
        assert_eq!(bg("a.jpg").assignments(&ids()), vec![(None, "a.jpg")]);
    }

    #[test]
    fn a_screen_of_its_own_splits_the_picture_per_screen() {
        let mut b = bg("a.jpg");
        b.set_screen("right", "b.jpg".into(), &ids());
        assert_eq!(b.path_for("left"), "a.jpg");
        assert_eq!(b.path_for("right"), "b.jpg");
        assert_eq!(
            b.assignments(&ids()),
            vec![(Some("left"), "a.jpg"), (Some("right"), "b.jpg")]
        );
        // A screen not seen before shows the common picture.
        assert_eq!(b.path_for("new"), "a.jpg");
    }

    #[test]
    fn the_same_picture_everywhere_is_one_picture_again() {
        let mut b = bg("a.jpg");
        b.set_screen("right", "b.jpg".into(), &ids());
        b.set_screen("left", "b.jpg".into(), &ids());
        assert_eq!(b, bg("b.jpg"));
    }

    #[test]
    fn every_picture_in_use_is_listed() {
        let mut b = bg("a.jpg");
        b.set_screen("right", "b.jpg".into(), &ids());
        assert_eq!(b.paths().collect::<Vec<_>>(), ["a.jpg", "a.jpg", "b.jpg"]);
        assert_eq!(bg("").paths().count(), 0, "a plain colour has none");
    }

    #[test]
    fn every_fit_reads_back_as_itself() {
        for fit in Fit::ALL {
            assert_eq!(Fit::from_position(fit.position()), fit);
        }
    }

    #[test]
    fn stored_as_lower_case_words_and_one_picture_stays_short() {
        let json = serde_json::to_string(&DesktopBackground {
            path: "a.png".into(),
            fit: Fit::Center,
            screens: Vec::new(),
        })
        .unwrap();
        assert_eq!(json, r#"{"path":"a.png","fit":"center"}"#);
        let back: DesktopBackground = serde_json::from_str(r#"{"path":"a.png"}"#).unwrap();
        assert_eq!(back.fit, Fit::Fill);
        assert!(back.screens.is_empty());
    }

    #[test]
    fn the_main_screen_sits_at_the_origin() {
        let s = Screen {
            id: "x".into(),
            rect: [0, 0, 2560, 1440],
        };
        assert!(s.is_main());
        assert_eq!(s.size(), (2560, 1440));
    }

    /// #702: the screens IDesktopWallpaper reports, against Windows' own
    /// count — read only, before and after the COM code moves off winapi.
    #[cfg(windows)]
    #[test]
    #[allow(unsafe_code)] // one FFI call, the independent count
    fn the_screens_are_windows_own_and_one_is_the_main_one() {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CMONITORS};
        let list = screens();
        // SAFETY: GetSystemMetrics takes an index and returns an int.
        let count = unsafe { GetSystemMetrics(SM_CMONITORS) } as usize;
        assert_eq!(list.len(), count);
        let mut ids: Vec<&str> = list.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.iter().all(|id| !id.is_empty()));
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "each screen once");
        assert_eq!(list.iter().filter(|s| s.is_main()).count(), 1);
    }

    /// #702: the fit IDesktopWallpaper reads is the one Windows keeps in
    /// `HKCU\Control Panel\Desktop` — read only.
    #[cfg(windows)]
    #[test]
    fn the_fit_is_the_one_windows_keeps_in_the_registry() {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        let Ok(desktop) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Control Panel\Desktop")
        else {
            return;
        };
        let style: String = desktop.get_value("WallpaperStyle").unwrap_or_default();
        let tile: String = desktop.get_value("TileWallpaper").unwrap_or_default();
        let expected = match (style.trim(), tile.trim()) {
            ("0", "1") => Fit::Tile,
            ("0", _) => Fit::Center,
            ("2", _) => Fit::Stretch,
            ("6", _) => Fit::Fit,
            ("10", _) => Fit::Fill,
            ("22", _) => Fit::Span,
            _ => return, // not set: nothing to compare with
        };
        assert_eq!(current().expect("the background reads").fit, expected);
    }

    #[cfg(windows)]
    #[test]
    fn windows_reports_its_screens_and_background() {
        assert!(current().is_some());
        // CI runners have a screen too, though a virtual one.
        let _ = screens();
    }
}
