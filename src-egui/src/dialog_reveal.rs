//! Keeps a freshly opened dialog window hidden until it has rendered.
//!
//! A dialog's OS window used to be created visible straight away, so Windows
//! showed its default (white) background until the first wgpu frame was
//! presented — a visible white flash on every dialog open, lasting longer the
//! slower the GPU (very noticeable on a laptop in power-saving mode). The
//! light-to-dark title bar switch (`win32_dark_mode::apply_titlebar_theme`,
//! only possible once the HWND exists) showed during that window too.
//!
//! Instead each dialog is created hidden (`ViewportBuilder::with_visible`),
//! renders [`DialogReveal::FRAMES`] frames while hidden — the swap chain then
//! already holds a real frame and the title bar is already dark — and is only
//! then made visible. Closing mirrors it: the window is hidden for one frame
//! before it's torn down (see [`DialogReveal::take_closing`]). Pure state;
//! `main.rs` owns the viewport/Win32 calls.

use std::collections::HashMap;

/// Per-dialog reveal state, keyed by the dialog's viewport id string.
#[derive(Default)]
pub struct DialogReveal {
    /// Frames rendered since the dialog was (re)opened.
    rendered: HashMap<&'static str, u8>,
    /// Dialogs closed this frame whose window still needs hiding first.
    closing: Vec<&'static str>,
}

impl DialogReveal {
    /// Hidden frames before revealing. One presented frame is enough for the
    /// swap chain to have content; the second is margin for a first frame
    /// that is still settling (fonts/layout). Each hidden frame costs only a
    /// frame of latency since the caller keeps repaints coming.
    pub const FRAMES: u8 = 2;

    /// Call every frame for every dialog. Forgets a closed dialog, so its
    /// next opening (a brand-new OS window) starts hidden again — and, on
    /// the frame it closes, queues it for [`Self::take_closing`] if its window
    /// was ever rendered.
    pub fn track(&mut self, id: &'static str, open: bool) {
        if !open && self.rendered.remove(id).is_some() {
            self.closing.push(id);
        }
    }

    /// Dialogs that just closed. The caller must show each one for one more
    /// frame with `with_visible(false)` and no content: when a dialog simply
    /// stops being shown, eframe drops its GPU surface while the window is
    /// still visible, flashing white until the window is destroyed. Hiding
    /// it first means the surface is dropped from an already-hidden window.
    pub fn take_closing(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.closing)
    }

    /// Whether the dialog's window should be visible this frame — feed into
    /// `ViewportBuilder::with_visible`.
    pub fn visible(&self, id: &'static str) -> bool {
        self.rendered.get(id).is_some_and(|&n| n >= Self::FRAMES)
    }

    /// Record that the dialog rendered a frame.
    pub fn rendered(&mut self, id: &'static str) {
        let n = self.rendered.entry(id).or_insert(0);
        *n = n.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_dialog_starts_hidden_and_reveals_after_frames() {
        let mut r = DialogReveal::default();
        r.track("settings", true);
        for _ in 0..DialogReveal::FRAMES {
            assert!(!r.visible("settings"));
            r.rendered("settings");
        }
        assert!(r.visible("settings"));
        // Stays visible while open.
        r.rendered("settings");
        r.track("settings", true);
        assert!(r.visible("settings"));
    }

    #[test]
    fn closing_resets_so_reopen_starts_hidden() {
        let mut r = DialogReveal::default();
        for _ in 0..DialogReveal::FRAMES {
            r.rendered("about");
        }
        assert!(r.visible("about"));
        r.track("about", false);
        assert!(!r.visible("about"));
    }

    #[test]
    fn closing_is_reported_once_per_close() {
        let mut r = DialogReveal::default();
        r.track("settings", true);
        r.rendered("settings");
        r.track("settings", false);
        assert_eq!(r.take_closing(), vec!["settings"]);
        // Still closed next frame: nothing more to hide.
        r.track("settings", false);
        assert!(r.take_closing().is_empty());
    }

    #[test]
    fn never_rendered_dialog_needs_no_hide_on_close() {
        let mut r = DialogReveal::default();
        r.track("about", false);
        assert!(r.take_closing().is_empty());
    }

    #[test]
    fn dialogs_are_independent() {
        let mut r = DialogReveal::default();
        for _ in 0..DialogReveal::FRAMES {
            r.rendered("status");
        }
        assert!(r.visible("status"));
        assert!(!r.visible("history"));
    }
}
