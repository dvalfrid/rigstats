//! Screen panels (#737): a panel filling a screen of its own — Now Playing
//! on an extra monitor or a TV. Which screen shows which panel is part of the
//! profile's look (`Settings::screen_panels`), set on the Control Center's
//! Desktop page, each with a spectrum of its own
//! (`SpectrumChoice::of_screen`). A panel in a window is drawn by the main app
//! (`app/screen_panels.rs`), one in the desktop background by
//! `rigstats-wallpaper` in WorkerW (`bin/wallpaper.rs`); both pick theirs
//! with [`to_show`].

use crate::geometry;
use eframe::egui;
use rigstats_backend::desktop_background::{self, Screen};
use rigstats_backend::settings::{ScreenPanel, ScreenWhen};
use std::time::{Duration, Instant};

/// How often the connected screens are looked up again: a screen plugged in
/// or out shows or hides its panel within this.
const SCREENS_EVERY: Duration = Duration::from_secs(3);

/// The connected screens, looked up every few seconds while a screen panel
/// is set.
#[derive(Default)]
pub struct Screens {
    pub screens: Vec<Screen>,
    /// Each screen's rect in physical and in logical pixels
    /// (`geometry::screen_rects`).
    pub rects: Vec<([f32; 4], [f32; 4])>,
    at: Option<Instant>,
}

impl Screens {
    pub fn refresh(&mut self) {
        if self.at.is_none_or(|t| t.elapsed() >= SCREENS_EVERY) {
            self.screens = desktop_background::screens();
            self.rects = geometry::screen_rects();
            self.at = Some(Instant::now());
        }
    }

    /// The screen that holds `point` (logical pixels), by its id.
    pub fn at(&self, point: egui::Pos2) -> Option<String> {
        screen_at(point, &self.screens, &self.rects)
    }

    /// [`to_show`] on these screens.
    pub fn to_show(
        &self,
        panels: &[ScreenPanel],
        desktop: bool,
        dashboard_screen: Option<&str>,
        playing: bool,
    ) -> Vec<(ScreenPanel, egui::Rect)> {
        to_show(
            panels,
            desktop,
            &self.screens,
            &self.rects,
            dashboard_screen,
            playing,
        )
    }
}

/// The screen panels to draw now, each with the window that covers its
/// screen (egui's logical pixels): only panels this version knows, in the
/// desktop background or not (`desktop`, the drawing process's half), on a
/// connected screen that isn't the dashboard's, and — "only while something
/// plays" — while something plays.
pub fn to_show(
    panels: &[ScreenPanel],
    desktop: bool,
    screens: &[Screen],
    rects: &[([f32; 4], [f32; 4])],
    dashboard_screen: Option<&str>,
    playing: bool,
) -> Vec<(ScreenPanel, egui::Rect)> {
    panels
        .iter()
        .filter(|p| p.known() && p.in_desktop() == desktop)
        .filter(|p| p.when == ScreenWhen::Always || playing)
        .filter(|p| dashboard_screen != Some(p.screen.as_str()))
        .filter_map(|p| {
            let screen = screens.iter().find(|s| s.id == p.screen)?;
            Some((p.clone(), geometry::screen_window(screen.rect, rects)?))
        })
        .collect()
}

/// Whether any panel is to be drawn in the desktop background: the main app
/// keeps `rigstats-wallpaper` running for it.
pub fn any_in_desktop(panels: &[ScreenPanel]) -> bool {
    panels.iter().any(|p| p.known() && p.in_desktop())
}

/// The screen that holds `point` (logical pixels), by its id.
fn screen_at(
    point: egui::Pos2,
    screens: &[Screen],
    rects: &[([f32; 4], [f32; 4])],
) -> Option<String> {
    screens
        .iter()
        .find(|s| geometry::screen_window(s.rect, rects).is_some_and(|r| r.contains(point)))
        .map(|s| s.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rigstats_backend::settings::ScreenPlace;

    fn screen(id: &str, rect: [i32; 4]) -> Screen {
        Screen {
            id: id.into(),
            rect,
        }
    }

    fn panel(screen: &str, when: ScreenWhen) -> ScreenPanel {
        ScreenPanel {
            when,
            ..ScreenPanel::new(screen, "media")
        }
    }

    fn setup() -> (Vec<Screen>, Vec<([f32; 4], [f32; 4])>) {
        let screens = vec![
            screen("main", [0, 0, 2560, 1440]),
            // A 4K TV at 150 %: its window is placed in logical pixels.
            screen("tv", [2560, 0, 6400, 2160]),
        ];
        let rects = vec![
            ([0.0, 0.0, 2560.0, 1440.0], [0.0, 0.0, 2560.0, 1440.0]),
            ([2560.0, 0.0, 6400.0, 2160.0], [1706.0, 0.0, 4266.0, 1440.0]),
        ];
        (screens, rects)
    }

    #[test]
    fn a_panel_covers_its_screen_in_logical_pixels() {
        let (screens, rects) = setup();
        let shown = to_show(
            &[panel("tv", ScreenWhen::Always)],
            false,
            &screens,
            &rects,
            None,
            false,
        );
        assert_eq!(shown.len(), 1);
        assert_eq!(
            shown[0].1,
            egui::Rect::from_min_max(egui::pos2(1706.0, 0.0), egui::pos2(4266.0, 1440.0))
        );
    }

    #[test]
    fn only_while_playing_waits_for_the_music() {
        let (screens, rects) = setup();
        let list = [panel("tv", ScreenWhen::Playing)];
        assert!(to_show(&list, false, &screens, &rects, None, false).is_empty());
        assert_eq!(to_show(&list, false, &screens, &rects, None, true).len(), 1);
    }

    #[test]
    fn a_missing_screen_or_the_dashboards_shows_nothing_but_keeps_its_place() {
        let (screens, rects) = setup();
        let list = [
            panel("gone", ScreenWhen::Always),
            panel("main", ScreenWhen::Always),
        ];
        assert!(to_show(&list, false, &screens, &rects, Some("main"), true).is_empty());
        // The dashboard moved away: the panel shows again.
        assert_eq!(to_show(&list, false, &screens, &rects, None, true).len(), 1);
    }

    #[test]
    fn a_newer_versions_panel_shows_nothing() {
        let (screens, rects) = setup();
        let mut p = panel("tv", ScreenWhen::Always);
        p.panel = "clock".into();
        assert!(!any_in_desktop(&[ScreenPanel {
            place: ScreenPlace::Desktop,
            ..p.clone()
        }]));
        assert!(to_show(&[p], false, &screens, &rects, None, true).is_empty());
    }

    #[test]
    fn each_process_draws_its_own_half() {
        // The main app the windows, the wallpaper host the desktop ones.
        let (screens, rects) = setup();
        let desktop = ScreenPanel {
            place: ScreenPlace::Desktop,
            ..panel("tv", ScreenWhen::Always)
        };
        let list = [panel("main", ScreenWhen::Always), desktop];
        let window = to_show(&list, false, &screens, &rects, None, true);
        let host = to_show(&list, true, &screens, &rects, None, true);
        assert_eq!(window.len(), 1);
        assert_eq!(window[0].0.screen, "main");
        assert_eq!(host.len(), 1);
        assert_eq!(host[0].0.screen, "tv");
        assert!(any_in_desktop(&list));
        assert!(!any_in_desktop(&list[..1]));
    }

    #[test]
    fn the_dashboards_screen_is_found_by_its_middle() {
        let (screens, rects) = setup();
        assert_eq!(
            screen_at(egui::pos2(3000.0, 700.0), &screens, &rects).as_deref(),
            Some("tv")
        );
        assert_eq!(
            screen_at(egui::pos2(100.0, 100.0), &screens, &rects).as_deref(),
            Some("main")
        );
        assert_eq!(screen_at(egui::pos2(-100.0, 100.0), &screens, &rects), None);
    }
}
