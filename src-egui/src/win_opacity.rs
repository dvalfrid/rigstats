//! Window-level opacity via Win32 SetLayeredWindowAttributes.
//!
//! Uses WS_EX_LAYERED + LWA_ALPHA, which works with DXGI flip-mode swap chains
//! on Windows 10 1803+ and Windows 11. Applies a uniform alpha to the whole
//! window (frame + content), matching the dashboard's configurable opacity.

#![allow(unsafe_code)]

use winapi::{
    shared::windef::HWND,
    um::winuser::{
        FindWindowW, GetWindowLongW, SetForegroundWindow, SetLayeredWindowAttributes,
        SetWindowLongW, GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED,
    },
};

/// Find the HWND for the top-level window with the given title.
/// Returns 0 if not found.
pub fn find_hwnd(title: &str) -> isize {
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) as isize }
}

/// Post WM_PAINT to the window so its render loop runs on the next event loop tick.
/// Used by the heartbeat thread to drive deferred viewport repaints at ~1 fps
/// without going through egui's request_repaint_of (which doesn't work reliably
/// for non-focused deferred viewports on Windows).
#[allow(dead_code)]
pub fn force_repaint(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        winapi::um::winuser::InvalidateRect(hwnd as HWND, std::ptr::null(), 0);
    }
}

/// Apply window-level opacity (0.0 = invisible, 1.0 = fully opaque) via
/// WS_EX_LAYERED + LWA_ALPHA. No-op if hwnd is 0.
pub fn set_opacity(hwnd: isize, opacity: f32) {
    if hwnd == 0 {
        return;
    }
    let hwnd = hwnd as HWND;
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        if style & WS_EX_LAYERED as i32 == 0 {
            SetWindowLongW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED as i32);
        }
        SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
    }
}

/// Bring the window to the foreground using Win32 SetForegroundWindow.
/// Used after show_viewport_immediate to ensure newly opened dialogs get focus.
/// Requires AllowSetForegroundWindow to have been called previously in the tray thread.
pub fn bring_to_foreground(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        SetForegroundWindow(hwnd as HWND);
    }
}
