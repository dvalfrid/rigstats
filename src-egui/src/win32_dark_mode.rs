//! Enable dark-mode rendering for OS-drawn UI elements (tray context menu, title bars).
//!
//! Calls the undocumented `SetPreferredAppMode` (uxtheme.dll ordinal 135) with
//! `AllowDark = 1`.  Also calls `RefreshImmersiveColorPolicyState` (ordinal 104)
//! to apply the change immediately.  Only works on Windows 10 1903 (build 18362)
//! and later; silently does nothing on older versions where the ordinals are absent.
//!
//! `apply_dark_titlebar` forces a dark DWM title bar on a specific HWND via
//! `DwmSetWindowAttribute(DWMWA_USE_IMMERSIVE_DARK_MODE)`.  Call this for every
//! dialog window after finding its HWND so that the title bar matches the dark
//! dialog content even when the OS system theme is set to light mode.
//!
//! `is_system_dark_mode` queries uxtheme.dll ordinal 132 (`ShouldSystemUseDarkMode`)
//! to detect the current OS light/dark preference.  Returns `true` for dark, `false`
//! for light.  Fallback is `true` when the ordinal is unavailable.

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

/// Returns `true` if the OS system theme is dark, `false` if light.
/// Queries uxtheme.dll ordinal 132 (`ShouldSystemUseDarkMode`).
/// Fallback is `true` (dark) when the ordinal is unavailable.
/// LoadLibrary is cached by the OS loader so calling this once per second is fine.
pub fn is_system_dark_mode() -> bool {
    let lib_name = b"uxtheme.dll\0";
    let lib = unsafe { LoadLibraryA(lib_name.as_ptr().cast()) };
    if lib.is_null() {
        return true;
    }
    // Ordinal 132: ShouldSystemUseDarkMode() → BOOL (non-zero = dark)
    let result = unsafe {
        let f_ptr = GetProcAddress(lib, 132usize as *const i8);
        if f_ptr.is_null() {
            true
        } else {
            let f: unsafe extern "system" fn() -> i32 = std::mem::transmute(f_ptr);
            f() != 0
        }
    };
    unsafe { FreeLibrary(lib) };
    result
}

/// Set the DWM title bar theme for the given HWND.
/// Pass `dark = true` for a dark title bar, `false` for the OS-default light bar.
/// Idempotent — safe to call every frame; no-op when `hwnd` is 0.
pub fn apply_titlebar_theme(hwnd: isize, dark: bool) {
    if hwnd == 0 {
        return;
    }
    use winapi::shared::windef::HWND;
    use winapi::um::dwmapi::DwmSetWindowAttribute;
    let value: u32 = if dark { 1 } else { 0 };
    unsafe {
        DwmSetWindowAttribute(
            hwnd as HWND,
            20, // DWMWA_USE_IMMERSIVE_DARK_MODE
            &value as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}
