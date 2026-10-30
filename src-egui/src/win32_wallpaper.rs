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

use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT, WAIT_TIMEOUT, WPARAM};
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetAncestor, GetClassNameW, GetWindowRect,
    SendMessageTimeoutW, SetParent, SetWindowPos, GA_PARENT, SMTO_NORMAL, SWP_NOACTIVATE,
    SWP_NOSIZE, SWP_NOZORDER,
};

/// The undocumented Progman message that forces creation of the split `WorkerW`
/// sibling behind the desktop icons. Sending it is a no-op if it already exists.
const WM_SPAWN_WORKERW: u32 = 0x052C;

/// `None` for 0, else the window handle.
fn as_hwnd(hwnd: isize) -> Option<HWND> {
    (hwnd != 0).then_some(HWND(hwnd as *mut _))
}

/// Class name of `hwnd` as a Rust string (empty if the call fails).
fn window_class(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: buf is a valid fixed-size buffer; GetClassNameW returns the length.
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

/// The first window after `after` (or the first) under `parent` (`None` =
/// top level) with this class; `None` if there is none.
fn find_child(parent: Option<HWND>, after: Option<HWND>, class: PCWSTR) -> Option<HWND> {
    // SAFETY: `class` is a static NUL-terminated string; the handles are
    // None or windows Windows returned (validated by user32).
    unsafe { FindWindowExW(parent, after, class, PCWSTR::null()) }.ok()
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
            WPARAM(0),
            LPARAM(0),
            SMTO_NORMAL,
            1000,
            Some(&mut result),
        );
    }
}

/// Callback for the legacy `WorkerW` search: the wallpaper `WorkerW` is the
/// sibling that follows the top-level window owning the `SHELLDLL_DefView`
/// (desktop-icon) child. Used as a fallback on older Windows 10 builds where the
/// icon view is not a direct child of Progman.
unsafe extern "system" fn enum_workerw(top: HWND, lparam: LPARAM) -> BOOL {
    if find_child(Some(top), None, w!("SHELLDLL_DefView")).is_some() {
        if let Some(worker) = find_child(None, Some(top), w!("WorkerW")) {
            // SAFETY: lparam is the `&mut Option<HWND>` find_wallpaper_workerw
            // passed, alive for the whole enumeration.
            unsafe { *(lparam.0 as *mut Option<HWND>) = Some(worker) };
            return BOOL(0); // stop enumeration
        }
    }
    BOOL(1) // continue
}

/// Locate the `WorkerW` that paints the wallpaper behind the desktop icons, or
/// `None` if not found.
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
fn find_wallpaper_workerw() -> Option<HWND> {
    let progman = unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }.ok()?;

    // 1. Windows 11: the wallpaper WorkerW is a child of Progman.
    if let Some(child) = find_child(Some(progman), None, w!("WorkerW")) {
        return Some(child);
    }

    // 2. Older Windows 10: force the split, then take the sibling after Progman.
    spawn_workerw(progman);
    if let Some(sibling) = find_child(None, Some(progman), w!("WorkerW")) {
        return Some(sibling);
    }

    // 3. Legacy: WorkerW sibling of the SHELLDLL_DefView owner.
    let mut result: Option<HWND> = None;
    unsafe {
        let _ = EnumWindows(
            Some(enum_workerw),
            LPARAM(&mut result as *mut Option<HWND> as isize),
        );
    }
    result
}

/// Reparent the window `hwnd` into the wallpaper `WorkerW`, keeping it at its
/// current screen position (translated into WorkerW-client coordinates so it does
/// not jump on multi-monitor setups). Returns `true` on success.
pub fn attach(hwnd: isize) -> bool {
    let Some(hwnd) = as_hwnd(hwnd) else {
        return false;
    };
    let Some(workerw) = find_wallpaper_workerw() else {
        return false;
    };
    // SAFETY: hwnd and workerw are valid; RECTs are written by GetWindowRect.
    unsafe {
        let mut wnd = RECT::default();
        let mut parent = RECT::default();
        if GetWindowRect(hwnd, &mut wnd).is_err() || GetWindowRect(workerw, &mut parent).is_err() {
            return false;
        }
        let _ = SetParent(hwnd, Some(workerw));
        // After reparenting, child coordinates are relative to the WorkerW origin
        // (the top-left of the virtual screen). Translate our screen rect so the
        // window stays exactly where eframe placed it. All physical pixels.
        let _ = SetWindowPos(
            hwnd,
            None,
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
    let Some(hwnd) = as_hwnd(hwnd) else {
        return;
    };
    // SAFETY: hwnd is valid; no parent re-parents to the desktop.
    unsafe {
        let _ = SetParent(hwnd, None);
    }
}

/// True when the window is currently a child of a `WorkerW`. The host polls this
/// each tick and re-[`attach`]es if Explorer rebuilt the hierarchy underneath it.
pub fn is_attached(hwnd: isize) -> bool {
    let Some(hwnd) = as_hwnd(hwnd) else {
        return false;
    };
    // GA_PARENT (unlike GetParent) returns the real parent, not the owner — which
    // matters because we don't set WS_CHILD on the reparented window.
    let parent = unsafe { GetAncestor(hwnd, GA_PARENT) };
    !parent.is_invalid() && window_class(parent) == "WorkerW"
}

/// The window's top-left on the screen, in physical pixels — for a window in
/// WorkerW too (#690).
pub fn screen_pos(hwnd: isize) -> Option<[f32; 2]> {
    let hwnd = as_hwnd(hwnd)?;
    // SAFETY: hwnd is a window handle; the RECT is written by GetWindowRect.
    unsafe {
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r)
            .is_ok()
            .then_some([r.left as f32, r.top as f32])
    }
}

/// Moves a window in WorkerW so its top-left is at screen position `pos`
/// (physical pixels): a child's position is relative to the WorkerW, which
/// spans the virtual screen and moves with it (#690).
pub fn move_to_screen(hwnd: isize, pos: [f32; 2]) -> bool {
    let Some(hwnd) = as_hwnd(hwnd) else {
        return false;
    };
    // SAFETY: hwnd is a window handle; its parent comes from GetAncestor and
    // its RECT from GetWindowRect.
    unsafe {
        let parent = GetAncestor(hwnd, GA_PARENT);
        let mut p = RECT::default();
        if parent.is_invalid() || GetWindowRect(parent, &mut p).is_err() {
            return false;
        }
        SetWindowPos(
            hwnd,
            None,
            pos[0].round() as i32 - p.left,
            pos[1].round() as i32 - p.top,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .is_ok()
    }
}

/// True if the process with id `pid` is still running. Used by the host to exit
/// when its supervising parent (the main `rigstats` app) has gone, so it never
/// lingers as an orphan painting the wallpaper.
pub fn process_alive(pid: u32) -> bool {
    // SAFETY: OpenProcess/WaitForSingleObject/CloseHandle on a plain pid.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else {
            // Could not open — treat as gone (the parent has exited or we lack
            // rights; either way the host should not outlive a missing parent).
            return false;
        };
        let r = WaitForSingleObject(handle, 0);
        let _ = CloseHandle(handle);
        // WAIT_TIMEOUT means the process handle is not signalled → still running.
        r == WAIT_TIMEOUT
    }
}

#[cfg(all(test, windows))]
mod tests {
    //! #702: against a real (hidden) window, checked through the windows
    //! crate.
    use super::*;
    use crate::test_win::TestWindow;
    use ::windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetClassNameW, GetDesktopWindow, GA_PARENT as PARENT,
    };

    /// The window's real parent and its class.
    fn parent_of(w: &TestWindow) -> (isize, String) {
        // SAFETY: a read-only query on the test's own window.
        let parent = unsafe { GetAncestor(w.hwnd, PARENT) };
        let mut buf = [0u16; 256];
        // SAFETY: `buf` is a writable buffer; its length is passed.
        let n = unsafe { GetClassNameW(parent, &mut buf) };
        (
            parent.0 as isize,
            String::from_utf16_lossy(&buf[..n.max(0) as usize]),
        )
    }

    #[test]
    fn a_window_goes_into_the_wallpaper_and_back_out_where_it_was() {
        let w = TestWindow::hidden("workerw");
        let before = w.rect();
        assert!(!is_attached(w.raw()));
        if !attach(w.raw()) {
            // No desktop wallpaper window (a session without Explorer).
            eprintln!("no WorkerW here: attach not tested");
            return;
        }
        assert_eq!(parent_of(&w).1, "WorkerW");
        assert!(is_attached(w.raw()));
        // Same place on the screen as before.
        assert_eq!(w.rect(), before);
        assert_eq!(
            screen_pos(w.raw()),
            Some([before.left as f32, before.top as f32])
        );

        // Moved to a screen position, from inside the WorkerW.
        assert!(move_to_screen(w.raw(), [-31000.6, -31500.4]));
        let moved = w.rect();
        assert_eq!((moved.left, moved.top), (-31001, -31500));
        assert_eq!(
            (moved.right - moved.left, moved.bottom - moved.top),
            (before.right - before.left, before.bottom - before.top)
        );
        assert_eq!(screen_pos(w.raw()), Some([-31001.0, -31500.0]));

        detach(w.raw());
        // SAFETY: no arguments.
        let desktop = unsafe { GetDesktopWindow() }.0 as isize;
        assert_eq!(parent_of(&w).0, desktop);
        assert!(!is_attached(w.raw()));
    }

    #[test]
    fn a_zero_handle_is_never_attached() {
        assert!(!attach(0));
        assert!(!is_attached(0));
        assert_eq!(screen_pos(0), None);
        assert!(!move_to_screen(0, [0.0, 0.0]));
        detach(0);
    }

    #[test]
    fn screen_pos_of_a_top_level_window() {
        let w = TestWindow::hidden("pos");
        let r = w.rect();
        assert_eq!(screen_pos(w.raw()), Some([r.left as f32, r.top as f32]));
    }

    #[test]
    fn a_process_is_alive_until_it_has_exited() {
        assert!(process_alive(std::process::id()));
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit 0"])
            .spawn()
            .expect("cmd starts");
        let pid = child.id();
        child.wait().unwrap();
        // The handle `child` holds keeps the pid from being reused yet.
        assert!(!process_alive(pid));
        drop(child);
        // The idle process can't be opened: counted as gone.
        assert!(!process_alive(0));
    }
}
