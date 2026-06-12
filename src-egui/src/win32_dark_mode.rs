//! Enable dark-mode rendering for OS-drawn UI elements (tray context menu, title bars).
//!
//! Calls the undocumented `SetPreferredAppMode` (uxtheme.dll ordinal 135) with
//! `AllowDark = 1`.  Also calls `RefreshImmersiveColorPolicyState` (ordinal 104)
//! to apply the change immediately.  Only works on Windows 10 1903 (build 18362)
//! and later; silently does nothing on older versions where the ordinals are absent.

#![allow(unsafe_code)]

use winapi::um::libloaderapi::{FreeLibrary, GetProcAddress, LoadLibraryA};

/// Call once at startup to opt the process into dark mode for OS-rendered elements.
pub fn enable() {
    let lib_name = b"uxtheme.dll\0";
    let lib = unsafe { LoadLibraryA(lib_name.as_ptr().cast()) };
    if lib.is_null() {
        return;
    }

    // Ordinal 135: SetPreferredAppMode(PreferredAppMode) → i32
    // Passing ordinal as LPCSTR: value ≤ 0xFFFF is treated as ordinal by Windows.
    let set_mode = unsafe { GetProcAddress(lib, 135usize as *const i8) };
    if !set_mode.is_null() {
        let f: unsafe extern "system" fn(i32) -> i32 = unsafe { std::mem::transmute(set_mode) };
        unsafe { f(1) }; // AllowDark = 1
    }

    // Ordinal 104: RefreshImmersiveColorPolicyState() — applies the mode change.
    let refresh = unsafe { GetProcAddress(lib, 104usize as *const i8) };
    if !refresh.is_null() {
        let f: unsafe extern "system" fn() = unsafe { std::mem::transmute(refresh) };
        unsafe { f() };
    }

    unsafe { FreeLibrary(lib) };
}
