//! Win32 helpers for the WorkerW desktop-wallpaper layer.
//!
//! Reparents a borderless window into the desktop `WorkerW` — the window that
//! sits *between* the wallpaper and the desktop icons — so the dashboard becomes
//! part of the wallpaper and survives `Win+D`, fullscreen apps and screen
//! switches. This is the technique used by Wallpaper Engine and Lively.
//!
//! Used only by the standalone `rigstats-wallpaper` host process (never the main
//! app): if Explorer restarts, the WorkerW hierarchy is destroyed along with any
//! child window, so reparenting our *main* window would kill the app. The host
//! polls [`is_attached`] each tick and calls [`attach`] again if it has been
//! detached; if the host window itself is destroyed, the host process exits and
//! the main app relaunches it.
//!
//! The caller passes a cached `HWND` (as `isize`) rather than a window title:
//! once reparented, the window is a *child* of WorkerW and `FindWindow` (which
//! only searches top-level windows) can no longer locate it. See
//! `docs/architecture.md`.

#![allow(unsafe_code)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use winapi::shared::minwindef::{BOOL, LPARAM, UINT};
use winapi::shared::windef::{HWND, RECT};
use winapi::shared::winerror::WAIT_TIMEOUT;
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::OpenProcess;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winnt::SYNCHRONIZE;
use winapi::um::winuser::{
    EnumWindows, FindWindowExW, FindWindowW, GetAncestor, GetClassNameW, GetWindowRect,
    SendMessageTimeoutW, SetParent, SetWindowPos, GA_PARENT, SMTO_NORMAL, SWP_NOACTIVATE,
    SWP_NOZORDER,
};

/// The undocumented Progman message that forces creation of the split `WorkerW`
/// sibling behind the desktop icons. Sending it is a no-op if it already exists.
const WM_SPAWN_WORKERW: UINT = 0x052C;

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn as_hwnd(hwnd: isize) -> HWND {
    hwnd as HWND
}

/// Class name of `hwnd` as a Rust string (empty if the call fails).
fn window_class(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: buf is a valid fixed-size buffer; GetClassNameW returns the length.
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

/// Ask Progman to create the split `WorkerW` sibling that hosts the wallpaper
/// behind the desktop icons. Idempotent.
fn spawn_workerw(progman: HWND) {
    let mut result: usize = 0;
    // SAFETY: progman is a valid HWND; lpdwResult points at a local usize.
    unsafe {
        SendMessageTimeoutW(
            progman,
            WM_SPAWN_WORKERW,
            0,
            0,
            SMTO_NORMAL,
            1000,
            &mut result as *mut usize as *mut _,
        );
    }
}

/// Callback for the legacy `WorkerW` search: the wallpaper `WorkerW` is the
/// sibling that follows the top-level window owning the `SHELLDLL_DefView`
/// (desktop-icon) child. Used as a fallback on older Windows 10 builds where the
/// icon view is not a direct child of Progman.
unsafe extern "system" fn enum_workerw(top: HWND, lparam: LPARAM) -> BOOL {
    let defview = FindWindowExW(
        top,
        ptr::null_mut(),
        wide("SHELLDLL_DefView").as_ptr(),
        ptr::null(),
    );
    if !defview.is_null() {
        let worker = FindWindowExW(ptr::null_mut(), top, wide("WorkerW").as_ptr(), ptr::null());
        if !worker.is_null() {
            *(lparam as *mut HWND) = worker;
            return 0; // stop enumeration
        }
    }
    1 // continue
}

/// Locate the `WorkerW` that paints the wallpaper behind the desktop icons, or
/// null if not found.
///
/// Tries, in order:
/// 1. A `WorkerW` that is a *child* of Progman — the Windows 11 (and recent
///    Windows 10) layout, where `SHELLDLL_DefView` is also a direct child of
///    Progman and the wallpaper WorkerW sits behind it. Checked first, without
///    sending `WM_SPAWN_WORKERW`, because on Windows 11 that message spawns
///    useless extra top-level WorkerWs.
/// 2. Force the split via `WM_SPAWN_WORKERW`, then the top-level `WorkerW`
///    sibling right after Progman (older Windows 10 post-split layout).
/// 3. The legacy layout: the `WorkerW` sibling of the top-level window that owns
///    `SHELLDLL_DefView`.
fn find_wallpaper_workerw() -> HWND {
    let progman = unsafe { FindWindowW(wide("Progman").as_ptr(), ptr::null()) };
    if progman.is_null() {
        return ptr::null_mut();
    }

    // 1. Windows 11: the wallpaper WorkerW is a child of Progman.
    let child = unsafe {
        FindWindowExW(
            progman,
            ptr::null_mut(),
            wide("WorkerW").as_ptr(),
            ptr::null(),
        )
    };
    if !child.is_null() {
        return child;
    }

    // 2. Older Windows 10: force the split, then take the sibling after Progman.
    spawn_workerw(progman);
    let sibling = unsafe {
        FindWindowExW(
            ptr::null_mut(),
            progman,
            wide("WorkerW").as_ptr(),
            ptr::null(),
        )
    };
    if !sibling.is_null() {
        return sibling;
    }

    // 3. Legacy: WorkerW sibling of the SHELLDLL_DefView owner.
    let mut result: HWND = ptr::null_mut();
    unsafe {
        EnumWindows(Some(enum_workerw), &mut result as *mut HWND as LPARAM);
    }
    result
}

/// Reparent the window `hwnd` into the wallpaper `WorkerW`, keeping it at its
/// current screen position (translated into WorkerW-client coordinates so it does
/// not jump on multi-monitor setups). Returns `true` on success.
pub fn attach(hwnd: isize) -> bool {
    let hwnd = as_hwnd(hwnd);
    if hwnd.is_null() {
        return false;
    }
    let workerw = find_wallpaper_workerw();
    if workerw.is_null() {
        return false;
    }
    // SAFETY: hwnd and workerw are valid; RECTs are written by GetWindowRect.
    unsafe {
        let mut wnd: RECT = std::mem::zeroed();
        let mut parent: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut wnd) == 0 || GetWindowRect(workerw, &mut parent) == 0 {
            return false;
        }
        SetParent(hwnd, workerw);
        // After reparenting, child coordinates are relative to the WorkerW origin
        // (the top-left of the virtual screen). Translate our screen rect so the
        // window stays exactly where eframe placed it. All physical pixels.
        SetWindowPos(
            hwnd,
            ptr::null_mut(),
            wnd.left - parent.left,
            wnd.top - parent.top,
            wnd.right - wnd.left,
            wnd.bottom - wnd.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    true
}

/// Detach the window from WorkerW, restoring it to a normal top-level window.
pub fn detach(hwnd: isize) {
    let hwnd = as_hwnd(hwnd);
    if hwnd.is_null() {
        return;
    }
    // SAFETY: hwnd is valid; NULL parent re-parents to the desktop.
    unsafe {
        SetParent(hwnd, ptr::null_mut());
    }
}

/// True when the window is currently a child of a `WorkerW`. The host polls this
/// each tick and re-[`attach`]es if Explorer rebuilt the hierarchy underneath it.
pub fn is_attached(hwnd: isize) -> bool {
    let hwnd = as_hwnd(hwnd);
    if hwnd.is_null() {
        return false;
    }
    // GA_PARENT (unlike GetParent) returns the real parent, not the owner — which
    // matters because we don't set WS_CHILD on the reparented window.
    let parent = unsafe { GetAncestor(hwnd, GA_PARENT) };
    !parent.is_null() && window_class(parent) == "WorkerW"
}

/// True if the process with id `pid` is still running. Used by the host to exit
/// when its supervising parent (the main `rigstats` app) has gone, so it never
/// lingers as an orphan painting the wallpaper.
pub fn process_alive(pid: u32) -> bool {
    // SAFETY: OpenProcess/WaitForSingleObject/CloseHandle on a plain pid.
    unsafe {
        let handle = OpenProcess(SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            // Could not open — treat as gone (the parent has exited or we lack
            // rights; either way the host should not outlive a missing parent).
            return false;
        }
        let r = WaitForSingleObject(handle, 0);
        CloseHandle(handle);
        // WAIT_TIMEOUT means the process handle is not signalled → still running.
        r == WAIT_TIMEOUT
    }
}
