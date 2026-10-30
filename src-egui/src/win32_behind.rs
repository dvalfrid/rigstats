//! Win32 helpers for "Always Behind" floating-panel mode.
//!
//! `SC_MOVE` (the Win32 move-drag command egui uses for `StartDrag`) requires
//! the target window to be active.  `WS_EX_NOACTIVATE` prevents activation, so
//! dragging a "behind" panel is a two-step process:
//!
//! 1. `prepare_for_drag` — removes `WS_EX_NOACTIVATE` and activates the window
//!    momentarily so `SC_MOVE` can run its modal loop.
//! 2. `apply_behind` — called on the first frame after the drag finishes
//!    (primary button released) to re-add `WS_EX_NOACTIVATE` and push the
//!    window back to `HWND_BOTTOM`.
//!
//! All operations are idempotent; the window lookup is O(windows) but
//! harmless at the frequency we call it (once on drag-start, then once per
//! frame while idle in behind-mode).

#![allow(unsafe_code)]

use winapi::um::winuser::{
    GetWindowLongPtrW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE,
    HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_NOACTIVATE,
};

/// This process's window with the title — never another program's (#366).
fn find_hwnd(title: &str) -> winapi::shared::windef::HWND {
    crate::win_opacity::find_hwnd(title) as winapi::shared::windef::HWND
}

/// Enforce "always behind" on the window whose title matches `title`.
///
/// * Sets `WS_EX_NOACTIVATE` so clicking the window does not activate it.
/// * Calls `SetWindowPos(HWND_BOTTOM)` to keep it behind all normal windows.
///
/// Call this every frame while the panel is idle (not being dragged).
/// **Only use for floating panels** — for the main non-floating window use
/// `keep_behind` instead, which omits `WS_EX_NOACTIVATE` so dragging works.
pub fn apply_behind(title: &str) {
    let hwnd = find_hwnd(title);
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let desired = ex_style | WS_EX_NOACTIVATE as isize;
        if ex_style != desired {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired);
        }
        SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Prepare the window for a user-initiated drag.
///
/// `SC_MOVE` (used by `ViewportCommand::StartDrag`) requires the window to be
/// active.  `WS_EX_NOACTIVATE` prevents that, so we must:
///  1. Strip `WS_EX_NOACTIVATE` so the window can be activated.
///  2. Call `SetForegroundWindow` to activate it immediately.
///
/// `apply_behind` will re-add `WS_EX_NOACTIVATE` and push to `HWND_BOTTOM`
/// once the drag finishes (primary button released).
pub fn prepare_for_drag(title: &str) {
    let hwnd = find_hwnd(title);
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let desired = ex_style & !(WS_EX_NOACTIVATE as isize);
        if ex_style != desired {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired);
        }
        SetForegroundWindow(hwnd);
    }
}

/// Keep the main non-floating window behind all normal windows without setting
/// `WS_EX_NOACTIVATE`.  Unlike `apply_behind`, this preserves the window's
/// ability to be activated — which is required for `SC_MOVE` (StartDrag) to work.
pub fn keep_behind(title: &str) {
    let hwnd = find_hwnd(title);
    if hwnd.is_null() {
        return;
    }
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}
#[cfg(all(test, windows))]
mod tests {
    //! #702: against real windows, checked through the windows crate.
    use super::*;
    use crate::test_win::TestWindow;
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        GetWindow, GW_HWNDNEXT, WS_EX_NOACTIVATE as NOACTIVATE,
    };

    /// Whether `lower` comes after `upper` in the Z order.
    fn is_below(lower: &TestWindow, upper: &TestWindow) -> bool {
        let mut next: HWND = upper.hwnd;
        loop {
            // SAFETY: a read-only query; a window this thread made or one
            // the previous call returned.
            match unsafe { GetWindow(next, GW_HWNDNEXT) } {
                Ok(h) if h == lower.hwnd => return true,
                Ok(h) if !h.is_invalid() => next = h,
                _ => return false,
            }
        }
    }

    /// Two windows, the first on top of the second (the newer is on top).
    fn pair(name: &str) -> (TestWindow, TestWindow) {
        let other = TestWindow::visible(&format!("{name}-other"));
        let w = TestWindow::visible(name);
        assert!(is_below(&other, &w), "a new window starts on top");
        (w, other)
    }

    #[test]
    fn apply_behind_sends_it_down_and_makes_it_not_activate() {
        let (w, other) = pair("behind");
        apply_behind(&w.title);
        assert!(is_below(&w, &other));
        assert!(w.has_ex_style(NOACTIVATE));
    }

    #[test]
    fn prepare_for_drag_lets_it_activate_again() {
        let (w, _other) = pair("drag");
        apply_behind(&w.title);
        assert!(w.has_ex_style(NOACTIVATE));
        let before = w.ex_style();
        prepare_for_drag(&w.title);
        assert_eq!(w.ex_style(), before & !(NOACTIVATE.0 as isize));
    }

    #[test]
    fn keep_behind_sends_it_down_without_touching_its_style() {
        let (w, other) = pair("keep");
        let before = w.ex_style();
        keep_behind(&w.title);
        assert!(is_below(&w, &other));
        assert_eq!(w.ex_style(), before);
    }

    #[test]
    fn a_title_with_no_window_of_ours_is_a_no_op() {
        apply_behind("rigstats-test-702-no-such-window");
        prepare_for_drag("rigstats-test-702-no-such-window");
        keep_behind("rigstats-test-702-no-such-window");
    }
}
