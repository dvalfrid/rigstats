//! Screen panels in a window (#737): a borderless window covering the
//! screen, drawn by the main app. Which panels and where:
//! `rigstats_egui::screen_panels`; the desktop-background ones are drawn by
//! `rigstats-wallpaper`.

use crate::RigStatsApp;
use eframe::egui;
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::panels;
use rigstats_egui::screen_panels::Screens;
use std::sync::{Arc, Mutex};

/// The connected screens and which of them the dashboard is on.
#[derive(Default)]
pub(crate) struct ScreenPanels {
    screens: Screens,
    /// The screen the fixed dashboard is on, for the Control Center: it
    /// can't be picked there, so the two never draw over each other.
    pub(crate) dashboard_screen: Arc<Mutex<Option<String>>>,
}

impl RigStatsApp {
    pub(crate) fn render_screen_panels(&mut self, ctx: &egui::Context) {
        let (list, floating, wallpaper_pos) = {
            let s = self.current_settings.lock_safe();
            (
                s.screen_panels.clone(),
                s.floating_mode,
                s.wallpaper_position,
            )
        };
        if list.is_empty() {
            return;
        }
        self.screen_panels.screens.refresh();
        let sp = &self.screen_panels;
        // The fixed dashboard's screen: where the middle of its window is —
        // in the desktop background, where the host shows it (its top left
        // corner; the main window is parked off-screen then).
        let dashboard = if floating {
            None
        } else if self.window_layer == "wallpaper" {
            wallpaper_pos
                .and_then(|[x, y]| sp.screens.at(egui::pos2(x as f32 + 8.0, y as f32 + 8.0)))
        } else {
            ctx.input(|i| i.viewport().outer_rect)
                .and_then(|r| sp.screens.at(r.center()))
        };
        *sp.dashboard_screen.lock_safe() = dashboard.clone();
        let media = self.runtime.latest.media.clone();
        let playing = media.as_ref().is_some_and(|m| m.playing);
        let th = self.runtime.app_theme;
        for (panel, rect) in sp
            .screens
            .to_show(&list, false, dashboard.as_deref(), playing)
        {
            let spectrum = panels::spectrum::SpectrumChoice::of_screen(&panel);
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
                    // A window is never transparent (`ScreenPanel::transparent`).
                    panels::media::draw_screen(ui, rect, media.as_ref(), spectrum, &th, true);
                },
            );
        }
    }
}
