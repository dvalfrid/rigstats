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
//! All operations are idempotent; `FindWindowW` lookup is O(windows) but
//! harmless at the frequency we call it (once on drag-start, then once per
//! frame while idle in behind-mode).

#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use winapi::um::winuser::{
    FindWindowW, GetWindowLongPtrW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    GWL_EXSTYLE, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_NOACTIVATE,
};

fn find_hwnd(title: &str) -> winapi::shared::windef::HWND {
    let title_w: Vec<u16> = OsStr::new(title).encode_wide().chain(Some(0)).collect();
    // SAFETY: title_w is null-terminated, first arg NULL means any class.
    unsafe { FindWindowW(std::ptr::null(), title_w.as_ptr()) }
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
