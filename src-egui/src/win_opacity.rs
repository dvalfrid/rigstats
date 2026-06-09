//! Window-level opacity via Win32 SetLayeredWindowAttributes.
//!
//! Uses WS_EX_LAYERED + LWA_ALPHA, which works with DXGI flip-mode swap chains
//! on Windows 10 1803+ and Windows 11. This replicates the same visual effect
//! as CSS `opacity` on the root element in the Tauri version.

#![allow(unsafe_code)]

use winapi::{
    shared::windef::HWND,
    um::winuser::{
        FindWindowW, GetWindowLongW, SetLayeredWindowAttributes, SetWindowLongW, GWL_EXSTYLE,
        LWA_ALPHA, WS_EX_LAYERED,
    },
};

/// Find the HWND for the top-level window with the given title.
/// Returns 0 if not found.
pub fn find_hwnd(title: &str) -> isize {
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) as isize }
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
