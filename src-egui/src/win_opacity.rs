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
        FindWindowW, GetWindowLongW, IsIconic, SetForegroundWindow, SetLayeredWindowAttributes,
        SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE, LWA_ALPHA, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_RESTORE, WS_EX_LAYERED,
        WS_EX_NOREDIRECTIONBITMAP,
    },
};

/// Move a window to physical screen coordinates — no resize, no Z-order
/// change, no activation. For placing a window exactly (the tray hover card)
/// regardless of which monitor's DPI egui would convert a logical position
/// with. No-op if hwnd is 0.
pub fn move_window(hwnd: isize, x: i32, y: i32) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Hide a window without waiting for its thread — safe from a background
/// thread while the owner is inside a modal loop (the tray menu's
/// `TrackPopupMenu`). No-op if hwnd is 0.
pub fn hide_window_async(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        winapi::um::winuser::ShowWindowAsync(hwnd as HWND, winapi::um::winuser::SW_HIDE);
    }
}

/// Whether one of this process's popup menus is open — the tray context
/// menu (`TrackPopupMenu` windows have the system class `#32768`). Other
/// apps' menus don't count.
pub fn own_popup_menu_open() -> bool {
    let class: Vec<u16> = "#32768\0".encode_utf16().collect();
    let own = unsafe { winapi::um::processthreadsapi::GetCurrentProcessId() };
    let mut after: HWND = std::ptr::null_mut();
    loop {
        let hwnd = unsafe {
            winapi::um::winuser::FindWindowExW(
                std::ptr::null_mut(),
                after,
                class.as_ptr(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return false;
        }
        let mut pid = 0u32;
        unsafe { winapi::um::winuser::GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid == own && unsafe { winapi::um::winuser::IsWindowVisible(hwnd) } != 0 {
            return true;
        }
        after = hwnd;
    }
}

/// The mouse pointer's position in physical screen pixels (the process is
/// per-monitor DPI aware), or None if the call failed.
pub fn cursor_position() -> Option<[i32; 2]> {
    let mut point = winapi::shared::windef::POINT { x: 0, y: 0 };
    let ok = unsafe { winapi::um::winuser::GetCursorPos(&mut point) };
    (ok != 0).then_some([point.x, point.y])
}

/// A window's outer size in physical pixels, or None (hwnd 0 or the call
/// failed).
pub fn window_size(hwnd: isize) -> Option<[i32; 2]> {
    if hwnd == 0 {
        return None;
    }
    let mut rect = winapi::shared::windef::RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let ok = unsafe { winapi::um::winuser::GetWindowRect(hwnd as HWND, &mut rect) };
    (ok != 0).then_some([rect.right - rect.left, rect.bottom - rect.top])
}

/// Find the HWND for the top-level window with the given title.
/// Returns 0 if not found.
pub fn find_hwnd(title: &str) -> isize {
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) as isize }
}

/// Post WM_PAINT to the window so its render loop runs on the next event loop tick,
/// bypassing egui's request_repaint/request_repaint_of — which don't work reliably
/// on Windows in some contexts (originally: non-focused deferred viewports; also
/// used from `main.rs`'s tray `TrayIconEvent` handler, where the same unreliability
/// showed up around Windows' modal `TrackPopupMenu` loop, see #177). The
/// `TrackPopupMenu` case is a confirmed upstream winit bug, not app-specific —
/// <https://github.com/rust-windowing/winit/issues/4608> — watched weekly (see
/// the comment at the `main.rs` call site for the routine ID); revisit once
/// that ships a fix, though this stays harmless either way.
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

/// Turn off DWM's show/hide/close animations for `hwnd`
/// (`DWMWA_TRANSITIONS_FORCEDISABLED`). DWM animates those from a snapshot
/// of the window, which for our flip-model swap chains is often the blank
/// (white) redirection surface rather than what we rendered — a white flash
/// when a dialog closes, most visible on a slow (power-saving) GPU where the
/// animation runs longer. Idempotent; no-op when `hwnd` is 0.
pub fn disable_dwm_transitions(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    use winapi::um::dwmapi::DwmSetWindowAttribute;
    const DWMWA_TRANSITIONS_FORCEDISABLED: u32 = 3;
    let disabled: i32 = 1; // BOOL TRUE
    unsafe {
        DwmSetWindowAttribute(
            hwnd as HWND,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &disabled as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// Bring the window to the foreground using Win32 SetForegroundWindow,
/// restoring it first if it's minimized. Used after show_viewport_immediate to
/// ensure newly opened dialogs get focus. Requires AllowSetForegroundWindow to
/// have been called previously in the tray thread.
///
/// The restore matters because dialogs have no taskbar button: once minimized
/// (e.g. by Win+D / "Show desktop" — common in Desktop Wallpaper mode) the
/// tray entry is the only way back, and SetForegroundWindow alone leaves a
/// minimized window parked off-screen, so the dialog looked impossible to open.
pub fn bring_to_foreground(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        if IsIconic(hwnd as HWND) != 0 {
            ShowWindow(hwnd as HWND, SW_RESTORE);
        }
        SetForegroundWindow(hwnd as HWND);
    }
}
