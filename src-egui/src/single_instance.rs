//! Prevents launching a second `rigstats` process when one is already running.
//!
//! Two layers, in order:
//! 1. A named kernel mutex, created atomically before anything else happens
//!    in `main()`. This is the authoritative check — `CreateMutexW` +
//!    `ERROR_ALREADY_EXISTS` closes the race a window-title lookup alone
//!    can't: the first instance takes time (settings load, GPU adapter
//!    probe) before its window exists, and two near-simultaneous launches
//!    could otherwise both slip past a `FindWindowW` check.
//! 2. `FindWindowW` on the main window's title (`"RIGStats"`, set in
//!    `main.rs`, unchanged even when hidden off-screen in wallpaper mode or
//!    behind other windows in "Always Behind" mode) and winit's window class
//!    to actually bring the running instance to the foreground. The class
//!    matters: the title match ignores case, so an editor or Explorer window
//!    showing a "rigstats" folder matched too (#366).

#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowLongPtrW, IsIconic, SetForegroundWindow, SetWindowLongPtrW, ShowWindow,
    GWL_EXSTYLE, SW_RESTORE, WS_EX_NOACTIVATE,
};

const MUTEX_NAME: &str = "Local\\se.codeby.rigstats.SingleInstance";
const MAIN_WINDOW_TITLE: &str = "RIGStats";
/// winit's window class, which every RigStats window has.
const MAIN_WINDOW_CLASS: &str = "Window Class";

/// Enforces a single running `rigstats` instance. If another instance is
/// already running, brings its window to the foreground and returns `true`
/// — the caller should exit immediately. Returns `false` when this process
/// is the first/only instance and should continue starting up normally.
pub fn ensure_single_instance() -> bool {
    if acquire_instance_mutex() {
        return false;
    }
    focus_existing_instance();
    true
}

/// Atomically creates (or opens) the named mutex and reports whether this
/// process was first to do so. The handle is intentionally never closed —
/// Windows releases it when the process exits, keeping the mutex held for
/// the process's whole lifetime.
fn acquire_instance_mutex() -> bool {
    acquire_named_mutex(MUTEX_NAME)
}

/// [`acquire_instance_mutex`] for any name — a seam for the tests (#702).
fn acquire_named_mutex(name: &str) -> bool {
    let name_w: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
    // SAFETY: name_w is null-terminated; no security attributes and
    // initial-owner false are valid per CreateMutexW's contract. The
    // handle is never closed (see above); GetLastError is read right after.
    unsafe {
        if CreateMutexW(None, false, PCWSTR(name_w.as_ptr())).is_err() {
            // Creation failed outright (rare) — don't block startup on it.
            return true;
        }
        GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Best-effort: brings the running instance's main window to the foreground.
/// A no-op if the window hasn't been created yet (the other process is still
/// early in startup) — that instance will simply finish starting on its own.
fn focus_existing_instance() {
    let wide = |s: &str| -> Vec<u16> { OsStr::new(s).encode_wide().chain(Some(0)).collect() };
    let class_w = wide(MAIN_WINDOW_CLASS);
    let title_w = wide(MAIN_WINDOW_TITLE);
    // SAFETY: both strings are null-terminated.
    let Ok(hwnd) = (unsafe { FindWindowW(PCWSTR(class_w.as_ptr()), PCWSTR(title_w.as_ptr())) })
    else {
        return;
    };
    // SAFETY: `hwnd` was just found; user32 checks a handle that went stale.
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // "Always Behind" mode sets WS_EX_NOACTIVATE, which blocks
        // SetForegroundWindow — strip it so the existing window can activate.
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let desired = ex_style & !(WS_EX_NOACTIVATE.0 as isize);
        if ex_style != desired {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #702: the named mutex — the first caller gets it, a second one sees
    /// it taken. (Focusing the other instance isn't tested: it would bring
    /// a real RIGStats window to the front.)
    #[test]
    fn the_first_takes_the_mutex_and_a_second_sees_it_taken() {
        let name = format!("Local\\se.codeby.rigstats.test-702-{}", std::process::id());
        assert!(acquire_named_mutex(&name), "the first caller is first");
        assert!(!acquire_named_mutex(&name), "a second caller sees it taken");
    }
}
