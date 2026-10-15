//! Prevents launching a second `rigstats` process when one is already running.
//!
//! Two layers, in order:
//! 1. A named kernel mutex, created atomically before anything else happens
//!    in `main()`. This is the authoritative check — `CreateMutexW` +
//!    `ERROR_ALREADY_EXISTS` closes the race a window-title lookup alone
//!    can't: the first instance takes time (settings load, GPU adapter
//!    probe) before its window exists, and two near-simultaneous launches
//!    could otherwise both slip past a `FindWindowW` check.
//! 2. `FindWindowW` on the main window's title (`"RigStats"`, set in
//!    `main.rs`, unchanged even when hidden off-screen in wallpaper mode or
//!    behind other windows in "Always Behind" mode) and winit's window class
//!    to actually bring the running instance to the foreground. The class
//!    matters: the title match ignores case, so an editor or Explorer window
//!    showing a "rigstats" folder matched too (#366).

#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use winapi::shared::winerror::ERROR_ALREADY_EXISTS;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::synchapi::CreateMutexW;
use winapi::um::winuser::{
    FindWindowW, GetWindowLongPtrW, IsIconic, SetForegroundWindow, SetWindowLongPtrW, ShowWindow,
    GWL_EXSTYLE, SW_RESTORE, WS_EX_NOACTIVATE,
};

const MUTEX_NAME: &str = "Local\\se.codeby.rigstats.SingleInstance";
const MAIN_WINDOW_TITLE: &str = "RigStats";
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
    let name_w: Vec<u16> = OsStr::new(MUTEX_NAME)
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: name_w is null-terminated; null security attrs and
    // initial-owner false are valid per CreateMutexW's contract.
    unsafe {
        let handle = CreateMutexW(ptr::null_mut(), 0, name_w.as_ptr());
        if handle.is_null() {
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
    let hwnd = unsafe { FindWindowW(class_w.as_ptr(), title_w.as_ptr()) };
    if hwnd.is_null() {
        return;
    }
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        // "Always Behind" mode sets WS_EX_NOACTIVATE, which blocks
        // SetForegroundWindow — strip it so the existing window can activate.
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let desired = ex_style & !(WS_EX_NOACTIVATE as isize);
        if ex_style != desired {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired);
        }
        SetForegroundWindow(hwnd);
    }
}
