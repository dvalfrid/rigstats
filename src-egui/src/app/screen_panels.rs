//! Screen panels (#737): a panel filling a screen of its own, as a
//! borderless window covering that screen — Now Playing on an extra monitor
//! or a TV. Which screen shows which panel is part of the profile's look
//! (`Settings::screen_panels`), set on the Control Center's Dashboard page.

use crate::RigStatsApp;
use eframe::egui;
use rigstats_backend::desktop_background::{self, Screen};
use rigstats_backend::settings::{ScreenPanel, ScreenWhen};
use rigstats_egui::geometry;
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::panels;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the connected screens are looked up again: a screen plugged in
/// or out shows or hides its panel within this.
const SCREENS_EVERY: Duration = Duration::from_secs(3);

/// The connected screens, looked up every few seconds while a screen panel
/// is set, and which of them the dashboard is on.
#[derive(Default)]
pub(crate) struct ScreenPanels {
    screens: Vec<Screen>,
    rects: Vec<([f32; 4], [f32; 4])>,
    at: Option<Instant>,
    /// The screen the fixed dashboard is on, for the Control Center: it
    /// can't be picked there, so the two never draw over each other.
    pub(crate) dashboard_screen: Arc<Mutex<Option<String>>>,
}

impl ScreenPanels {
    fn refresh(&mut self) {
        if self.at.is_none_or(|t| t.elapsed() >= SCREENS_EVERY) {
            self.screens = desktop_background::screens();
            self.rects = geometry::screen_rects();
            self.at = Some(Instant::now());
        }
    }
}

/// The screen panels to draw now, each with the window that covers its
/// screen (egui's logical pixels): only panels this version knows, on a
/// connected screen that isn't the dashboard's, and — "only while
/// something plays" — while something plays.
pub(crate) fn to_show(
    panels: &[ScreenPanel],
    screens: &[Screen],
    rects: &[([f32; 4], [f32; 4])],
    dashboard_screen: Option<&str>,
    playing: bool,
) -> Vec<(ScreenPanel, egui::Rect)> {
    panels
        .iter()
        .filter(|p| p.known())
        .filter(|p| p.when == ScreenWhen::Always || playing)
        .filter(|p| dashboard_screen != Some(p.screen.as_str()))
        .filter_map(|p| {
            let screen = screens.iter().find(|s| s.id == p.screen)?;
            Some((p.clone(), geometry::screen_window(screen.rect, rects)?))
        })
        .collect()
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

impl RigStatsApp {
    pub(crate) fn render_screen_panels(&mut self, ctx: &egui::Context) {
        let (list, spectrum, floating) = {
            let s = self.current_settings.lock_safe();
            (
                s.screen_panels.clone(),
                panels::spectrum::SpectrumChoice::from_settings(&s),
                s.floating_mode,
            )
        };
        if list.is_empty() {
            return;
        }
        self.screen_panels.refresh();
        let sp = &self.screen_panels;
        // The fixed dashboard's screen: where the middle of its window is.
        let dashboard = if floating {
            None
        } else {
            ctx.input(|i| i.viewport().outer_rect)
                .and_then(|r| screen_at(r.center(), &sp.screens, &sp.rects))
        };
        *sp.dashboard_screen.lock_safe() = dashboard.clone();
        let media = self.runtime.latest.media.clone();
        let playing = media.as_ref().is_some_and(|m| m.playing);
        let th = self.runtime.app_theme;
        for (panel, rect) in to_show(&list, &sp.screens, &sp.rects, dashboard.as_deref(), playing) {
            let builder = egui::ViewportBuilder::default()
                .with_title("RIGStats \u{2014} Now Playing")
                .with_position(rect.min)
                .with_inner_size(rect.size())
                .with_decorations(false)
                .with_has_shadow(false)
                .with_resizable(false)
                .with_taskbar(false);
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of(("screen_panel", &panel.screen)),
                builder,
                |ui, _class| {
                    let rect = ui.max_rect();
                    panels::media::draw_screen(ui, rect, media.as_ref(), spectrum, &th);
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(id: &str, rect: [i32; 4]) -> Screen {
        Screen {
            id: id.into(),
            rect,
        }
    }

    fn panel(screen: &str, when: ScreenWhen) -> ScreenPanel {
        ScreenPanel {
            screen: screen.into(),
            panel: "media".into(),
            when,
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
        assert!(to_show(&list, &screens, &rects, None, false).is_empty());
        assert_eq!(to_show(&list, &screens, &rects, None, true).len(), 1);
    }

    #[test]
    fn a_missing_screen_or_the_dashboards_shows_nothing_but_keeps_its_place() {
        let (screens, rects) = setup();
        let list = [
            panel("gone", ScreenWhen::Always),
            panel("main", ScreenWhen::Always),
        ];
        assert!(to_show(&list, &screens, &rects, Some("main"), true).is_empty());
        // The dashboard moved away: the panel shows again.
        assert_eq!(to_show(&list, &screens, &rects, None, true).len(), 1);
    }

    #[test]
    fn a_newer_versions_panel_shows_nothing() {
        let (screens, rects) = setup();
        let mut p = panel("tv", ScreenWhen::Always);
        p.panel = "clock".into();
        assert!(to_show(&[p], &screens, &rects, None, true).is_empty());
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
