//! Win32 helpers for "Always Behind" floating-panel mode.
//!
//! On Windows, `WindowLevel::AlwaysOnBottom` only sets the Z-order at window
//! creation time.  A mouse click still sends WM_ACTIVATE, which brings the
//! window to the foreground.  The correct fix is two-fold:
//!
//! 1. `WS_EX_NOACTIVATE` — prevents the window from being activated on click,
//!    so it never jumps to the foreground.
//! 2. Periodic `SetWindowPos(HWND_BOTTOM)` — keeps the Z-order correct even
//!    when other windows are manipulated.
//!
//! Both operations are idempotent and cheap; calling `apply_behind` every
//! frame (60 Hz) adds negligible overhead.

#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use winapi::um::winuser::{
    FindWindowW, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_BOTTOM,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_NOACTIVATE,
};

fn to_wide_nul(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

/// Enforce "always behind" on the window whose title matches `title`.
///
/// * Sets `WS_EX_NOACTIVATE` so clicking the window does not activate it.
/// * Calls `SetWindowPos(HWND_BOTTOM)` to keep it behind all normal windows.
///
/// Uses `FindWindowW` to look up the HWND from the title each call; returns
/// silently if the window does not exist yet.
pub fn apply_behind(title: &str) {
    let title_w = to_wide_nul(title);
    // SAFETY: title_w is null-terminated, first arg NULL means any class.
    let hwnd = unsafe { FindWindowW(std::ptr::null(), title_w.as_ptr()) };
    if hwnd.is_null() {
        return;
    }
    unsafe {
        // Add WS_EX_NOACTIVATE if not already present.
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let desired = ex_style | WS_EX_NOACTIVATE as isize;
        if ex_style != desired {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired);
        }
        // Push to bottom of Z-order without activating.
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

/// Remove `WS_EX_NOACTIVATE` when switching away from "behind" mode so the
/// window can be activated normally again.
#[allow(dead_code)]
pub fn remove_no_activate(title: &str) {
    let title_w = to_wide_nul(title);
    let hwnd = unsafe { FindWindowW(std::ptr::null(), title_w.as_ptr()) };
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let desired = ex_style & !(WS_EX_NOACTIVATE as isize);
        if ex_style != desired {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired);
        }
    }
}
