//! The tray icon's hover card (#290): while the pointer is over the tray
//! icon, a small borderless window above it shows "RIGStats", the recording
//! state and each wireless device's battery — name left, value right, in the
//! PERIPHERALS panel's charge colours. Replaces the native tooltip, which is
//! plain text and cut at 63 characters.
//!
//! `TrayIconEvent::Enter`/`Move` (set from `app::background`'s tray event
//! handler) store the icon's rect in [`TrayAnchor`]; a click hides the card
//! until the pointer has left the icon (the menu is opening). `Leave` is not
//! relied on — it doesn't arrive while the tray menu is open, which left the
//! card standing after the menu closed — so each frame also checks where the
//! pointer actually is ([`hover_step`]). The window is opaque (no DComp), created hidden and revealed
//! after it has rendered (`DialogReveal`, no white flash), never activated,
//! and click-through. It is placed with Win32 in physical pixels, so the
//! taskbar's monitor DPI doesn't matter.

use crate::RigStatsApp;
use eframe::egui;
use rigstats_egui::dialog_reveal::DialogReveal;
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::{panels, theme};
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use rigstats_egui::{geometry, win_opacity};

/// What the tray event handler has seen of the pointer over the icon.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct HoverState {
    /// The tray icon's rect in physical pixels (x, y, w, h) while hovered.
    pub(crate) icon: Option<[i32; 4]>,
    /// Set by a click (the menu is opening); cleared once the pointer leaves.
    pub(crate) suppressed: bool,
}

pub(crate) type TrayAnchor = Arc<Mutex<HoverState>>;

/// One decision: whether the card shows, and the state to keep. The pointer
/// outside the icon (a few pixels of slack) ends the hover and any click
/// suppression, whether or not `Leave` ever arrived; while our tray menu is
/// open the card never shows (the pointer can come back over the icon with
/// the menu still up, after a `Leave` cleared the click suppression).
pub(crate) fn hover_step(
    state: HoverState,
    cursor: Option<[i32; 2]>,
    menu_open: bool,
) -> (bool, HoverState) {
    const SLACK: i32 = 2;
    let Some([x, y, w, h]) = state.icon else {
        return (false, HoverState::default());
    };
    let inside = cursor.map_or(true, |[cx, cy]| {
        cx >= x - SLACK && cx < x + w + SLACK && cy >= y - SLACK && cy < y + h + SLACK
    });
    if !inside {
        return (false, HoverState::default());
    }
    (!state.suppressed && !menu_open, state)
}

/// Run from the tray thread every 50 ms (`app::background`): hides the card's
/// window straight away through Win32 when it shouldn't show. The UI thread's
/// own check can stall — its frames stop while the tray menu's modal loop
/// runs, and repaint requests are lost around it (winit #4608, see #177) —
/// which left the card standing after the menu closed.
#[cfg(windows)]
pub(crate) fn watchdog(anchor: &TrayAnchor) {
    let mut state = anchor.lock_safe();
    if state.icon.is_none() {
        return;
    }
    let (open, next) = hover_step(
        *state,
        win_opacity::cursor_position(),
        win_opacity::own_popup_menu_open(),
    );
    *state = next;
    drop(state);
    if !open {
        win_opacity::hide_window_async(win_opacity::find_hwnd(TITLE));
        // Let the UI thread catch up and tear the viewport down properly.
        win_opacity::force_repaint(win_opacity::find_hwnd("RigStats"));
    }
}

const ID: &str = "tray_card";
const TITLE: &str = "RigStats \u{2014} Tray card";
const CARD_W: f32 = 300.0;
const MARGIN: f32 = 10.0;
const TITLE_H: f32 = 22.0;
const ROW_H: f32 = 18.0;

/// The card's hover anchor and its reveal state.
#[derive(Default)]
pub(crate) struct TrayCard {
    pub(crate) anchor: TrayAnchor,
    reveal: DialogReveal,
}

/// Content height for `devices` rows (or the empty-state line).
fn card_height(devices: usize) -> f32 {
    MARGIN * 2.0 + TITLE_H + 4.0 + ROW_H * devices.max(1) as f32
}

impl RigStatsApp {
    /// Shows the hover card while the tray icon is hovered; hides it (one
    /// content-less hidden frame, as the dialogs do) once the pointer leaves.
    pub(crate) fn render_tray_card(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        let cursor = win_opacity::cursor_position();
        #[cfg(not(windows))]
        let cursor = None;
        #[cfg(windows)]
        let menu_open = win_opacity::own_popup_menu_open();
        #[cfg(not(windows))]
        let menu_open = false;
        let (open, icon) = {
            let mut state = self.tray_card.anchor.lock_safe();
            let (open, next) = hover_step(*state, cursor, menu_open);
            *state = next;
            (open, next.icon)
        };
        if icon.is_some() {
            // Keep checking the pointer while the icon counts as hovered —
            // `Leave` can't be relied on (see the module doc).
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.tray_card.reveal.track(ID, open);
        for id in self.tray_card.reveal.take_closing() {
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of(id),
                egui::ViewportBuilder::default().with_visible(false),
                |_, _| {},
            );
            ctx.request_repaint();
        }
        let Some(icon) = icon.filter(|_| open) else {
            return;
        };

        let visible = self.tray_card.reveal.visible(ID);
        let devices = self.runtime.latest.peripherals.clone();
        let recording = self.recording.active;
        let (warn, crit) = self.runtime.thresholds.battery;
        let builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([CARD_W, card_height(devices.len())])
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(false)
            .with_mouse_passthrough(true)
            .with_visible(visible);

        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(ID), builder, |ui, _class| {
            egui::Frame::new()
                .fill(egui::Color32::from_gray(22))
                .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_gray(64)))
                .inner_margin(MARGIN)
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("RIGStats")
                                .strong()
                                .size(14.0)
                                .color(theme::C_PANEL_TITLE),
                        );
                        if recording {
                            ui.label(
                                egui::RichText::new("\u{2014} Recording")
                                    .size(12.0)
                                    .color(egui::Color32::from_rgb(0xff, 0x55, 0x55)),
                            );
                        }
                    });
                    ui.add_space(4.0);
                    if devices.is_empty() {
                        ui.label(
                            egui::RichText::new("No wireless devices")
                                .size(12.0)
                                .color(egui::Color32::from_gray(140)),
                        );
                    }
                    let inner_w = ui.available_width();
                    for device in &devices {
                        panels::peripherals::paint_device_row(ui, inner_w, device, warn, crit, 1.0);
                    }
                });
        });
        self.tray_card.reveal.rendered(ID);

        #[cfg(windows)]
        {
            let hwnd = win_opacity::find_hwnd(TITLE);
            if let Some(size) = win_opacity::window_size(hwnd) {
                let [x, y] = geometry::tray_card_position(icon, size);
                win_opacity::move_window(hwnd, x, y);
            }
            win_opacity::disable_dwm_transitions(hwnd);
        }
        #[cfg(not(windows))]
        let _ = icon;
        if !visible {
            // Keep frames coming until it is revealed (the idle heartbeat is ~1 fps).
            ctx.request_repaint();
            #[cfg(windows)]
            win_opacity::force_repaint(self.hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICON: [i32; 4] = [2400, 1400, 24, 24];

    fn hovered(suppressed: bool) -> HoverState {
        HoverState {
            icon: Some(ICON),
            suppressed,
        }
    }

    #[test]
    fn shows_while_the_pointer_is_over_the_icon() {
        assert_eq!(
            hover_step(hovered(false), Some([2410, 1410]), false),
            (true, hovered(false))
        );
    }

    #[test]
    fn pointer_away_ends_the_hover_even_without_leave() {
        // The menu-open case: Leave never came, the pointer is elsewhere.
        assert_eq!(
            hover_step(hovered(false), Some([2410, 1200]), false),
            (false, HoverState::default())
        );
    }

    #[test]
    fn a_click_hides_it_until_the_pointer_has_left() {
        assert_eq!(
            hover_step(hovered(true), Some([2410, 1410]), false),
            (false, hovered(true))
        );
        let (_, after_leaving) = hover_step(hovered(true), Some([0, 0]), false);
        assert_eq!(after_leaving, HoverState::default());
    }

    #[test]
    fn never_over_our_open_menu_even_back_on_the_icon() {
        // Leave (pointer over another app's icon) cleared the click
        // suppression; back on our icon with our menu still open.
        assert_eq!(
            hover_step(hovered(false), Some([2410, 1410]), true),
            (false, hovered(false))
        );
    }

    #[test]
    fn card_grows_with_the_device_list_and_keeps_room_for_the_empty_line() {
        assert_eq!(card_height(0), card_height(1));
        assert_eq!(card_height(3) - card_height(1), ROW_H * 2.0);
    }
}
