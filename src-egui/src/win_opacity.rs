//! Window-level opacity and compositor-visibility styles via raw Win32 calls.
//!
//! Uniform (whole-window) opacity uses WS_EX_LAYERED + LWA_ALPHA, which works
//! with DXGI flip-mode swap chains on Windows 10 1803+ and Windows 11. This
//! module also applies WS_EX_NOREDIRECTIONBITMAP, needed for per-pixel-alpha
//! DirectComposition swap chains (see [`set_no_redirection_bitmap`]).

#![allow(unsafe_code)]

use winapi::{
    shared::windef::HWND,
    um::winuser::{
        FindWindowW, GetWindowLongW, SetForegroundWindow, SetLayeredWindowAttributes,
        SetWindowLongW, SetWindowPos, GWL_EXSTYLE, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP,
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

/// Mark the window as not needing a DWM redirection bitmap, required for a
/// DirectComposition-backed (per-pixel-alpha) swap chain to actually composite
/// transparently — without it, DWM's own opaque redirection surface for the
/// window sits over the DComp visual tree and the window renders solid
/// regardless of the swap chain's alpha mode. Unlike WS_EX_LAYERED, eframe/
/// egui-winit has no way to request this at window-creation time, but (verified
/// empirically for issue #131) it still takes effect when applied after the
/// wgpu surface already exists, as long as `SetWindowPos(SWP_FRAMECHANGED)`
/// follows the style change. No-op if hwnd is 0.
pub fn set_no_redirection_bitmap(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    let hwnd = hwnd as HWND;
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        SetWindowLongW(hwnd, GWL_EXSTYLE, style | WS_EX_NOREDIRECTIONBITMAP as i32);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
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
