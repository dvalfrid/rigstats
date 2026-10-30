//! Test-only: real top-level windows for the Win32 window helpers' tests
//! (#702), and the windows-crate checks those tests compare with — an
//! independent binding, so the checks don't change when the code under test
//! moves off `winapi`.

#![allow(unsafe_code)]

use ::windows::core::{w, PCWSTR};
use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use ::windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, CreateWindowExW, DestroyWindow, DispatchMessageW, GetWindowLongPtrW,
    GetWindowRect, IsWindowVisible, PeekMessageW, SetWindowLongPtrW, TranslateMessage,
    GWLP_WNDPROC, GWL_EXSTYLE, MSG, PM_REMOVE, STYLESTRUCT, WINDOW_EX_STYLE, WM_STYLECHANGING,
    WNDPROC, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
};
use std::cell::{Cell, RefCell};

thread_local! {
    /// The extended styles asked for (`WM_STYLECHANGING`'s new style) since
    /// [`TestWindow::record_ex_style_requests`], and the window procedure it
    /// replaced.
    static EX_STYLE_REQUESTS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    static ORIGINAL_PROC: Cell<isize> = const { Cell::new(0) };
}

unsafe extern "system" fn recording_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_STYLECHANGING && wparam.0 as i32 == GWL_EXSTYLE.0 {
        // SAFETY: for WM_STYLECHANGING, lparam points at a STYLESTRUCT.
        let new = unsafe { (*(lparam.0 as *const STYLESTRUCT)).styleNew };
        EX_STYLE_REQUESTS.with(|r| r.borrow_mut().push(new));
    }
    // SAFETY: ORIGINAL_PROC is the window procedure SetWindowLongPtrW
    // returned for this window; WNDPROC is that pointer's type.
    unsafe {
        let original: WNDPROC = std::mem::transmute(ORIGINAL_PROC.with(Cell::get));
        CallWindowProcW(original, hwnd, msg, wparam, lparam)
    }
}

/// A popup window owned by the test thread, off every screen, with no
/// taskbar button (`WS_EX_TOOLWINDOW`); destroyed when dropped. Use it on
/// the thread that made it.
pub struct TestWindow {
    pub hwnd: HWND,
    pub title: String,
}

impl TestWindow {
    /// A hidden window with a title unique to this process and `name`.
    pub fn hidden(name: &str) -> Self {
        Self::make(name, false)
    }

    /// The same, shown (still off every screen).
    pub fn visible(name: &str) -> Self {
        Self::make(name, true)
    }

    fn make(name: &str, visible: bool) -> Self {
        let title = format!("rigstats-test-702-{name}-{}", std::process::id());
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let style = if visible {
            WS_POPUP | WS_VISIBLE
        } else {
            WS_POPUP
        };
        // SAFETY: "STATIC" is a system class (nothing to register); `wide` is
        // NUL-terminated and outlives the call; no parent, menu, instance or
        // parameter.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("STATIC"),
                PCWSTR(wide.as_ptr()),
                style,
                -32000,
                -32000,
                120,
                80,
                None,
                None,
                None,
                None,
            )
        }
        .expect("a test window");
        Self { hwnd, title }
    }

    /// The handle as the helpers take it.
    pub fn raw(&self) -> isize {
        self.hwnd.0 as isize
    }

    pub fn ex_style(&self) -> isize {
        // SAFETY: a read-only query on our own window.
        unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) }
    }

    pub fn has_ex_style(&self, flag: WINDOW_EX_STYLE) -> bool {
        self.ex_style() & flag.0 as isize != 0
    }

    pub fn rect(&self) -> RECT {
        let mut r = RECT::default();
        // SAFETY: `r` is a valid, writable RECT; our own window.
        unsafe { GetWindowRect(self.hwnd, &mut r) }.expect("GetWindowRect");
        r
    }

    /// From now on, records every extended style the window is asked to
    /// take — what a `SetWindowLong*` call requested, even when Windows then
    /// keeps a flag out (`WS_EX_NOREDIRECTIONBITMAP` after creation).
    pub fn record_ex_style_requests(&self) {
        EX_STYLE_REQUESTS.with(|r| r.borrow_mut().clear());
        // SAFETY: our own window, on its thread; `recording_proc` passes
        // every message on to the procedure it replaces.
        let original = unsafe {
            SetWindowLongPtrW(
                self.hwnd,
                GWLP_WNDPROC,
                recording_proc as *const () as isize,
            )
        };
        ORIGINAL_PROC.with(|p| p.set(original));
    }

    /// The extended styles requested since [`Self::record_ex_style_requests`].
    pub fn ex_style_requests(&self) -> Vec<u32> {
        EX_STYLE_REQUESTS.with(|r| r.borrow().clone())
    }

    pub fn is_visible(&self) -> bool {
        // SAFETY: a read-only query on our own window.
        unsafe { IsWindowVisible(self.hwnd) }.as_bool()
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: the window this value made, on the same thread.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

/// Runs this thread's pending messages, so a posted change (ShowWindowAsync)
/// takes effect.
pub fn pump() {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid, writable MSG; any window of this thread.
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        // SAFETY: a message PeekMessageW just returned.
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
