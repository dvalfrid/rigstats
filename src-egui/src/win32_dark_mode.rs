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
//! `is_system_dark_mode` reads `AppsUseLightTheme` from the current user's HKCU
//! registry key — the authoritative per-user dark/light setting on Windows 10/11,
//! correctly handled for child/standard accounts.  Falls back to uxtheme.dll ordinal
//! 132 (`ShouldSystemUseDarkMode`) if the registry key is absent.

#![allow(unsafe_code)]
#![warn(clippy::undocumented_unsafe_blocks)]

use winapi::um::libloaderapi::{FreeLibrary, GetProcAddress, LoadLibraryA};

/// Call once at startup to opt the process into dark mode for OS-rendered elements.
pub fn enable() {
    let lib_name = b"uxtheme.dll\0";
    // SAFETY: `lib_name` is a NUL-terminated ASCII string with 'static life.
    let lib = unsafe { LoadLibraryA(lib_name.as_ptr().cast()) };
    if lib.is_null() {
        return;
    }

    // Ordinal 135: SetPreferredAppMode(PreferredAppMode) → i32
    // Passing ordinal as LPCSTR: value ≤ 0xFFFF is treated as ordinal by Windows.
    // SAFETY: `lib` is the module loaded above and still loaded; an ordinal
    // in the low word is GetProcAddress's documented MAKEINTRESOURCE form.
    let set_mode = unsafe { GetProcAddress(lib, 135usize as *const i8) };
    if !set_mode.is_null() {
        // SAFETY: ASSUMED, not checkable: ordinal 135 is undocumented. On
        // Windows 10 1903+ it is SetPreferredAppMode(int) -> int; on 1809 the
        // same ordinal is AllowDarkModeForApp(BOOL) -> BOOL, also one 32-bit
        // argument and a 32-bit return, so the call is ABI-compatible.
        let f: unsafe extern "system" fn(i32) -> i32 = unsafe { std::mem::transmute(set_mode) };
        // SAFETY: `f` has the signature above and `lib` is still loaded.
        unsafe { f(1) }; // AllowDark = 1
    }

    // Ordinal 104: RefreshImmersiveColorPolicyState() — applies the mode change.
    // SAFETY: as for ordinal 135 above.
    let refresh = unsafe { GetProcAddress(lib, 104usize as *const i8) };
    if !refresh.is_null() {
        // SAFETY: ASSUMED (undocumented): ordinal 104 takes no arguments; its
        // return value, if any, is ignored, so `fn()` is ABI-safe either way.
        let f: unsafe extern "system" fn() = unsafe { std::mem::transmute(refresh) };
        // SAFETY: `f` has the signature above and `lib` is still loaded.
        unsafe { f() };
    }

    // SAFETY: `lib` came from LoadLibraryA above and is freed once; neither
    // function pointer is used after this.
    unsafe { FreeLibrary(lib) };
}

/// Returns `true` if the current user's OS theme is dark, `false` if light.
///
/// Reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme`
/// first — this is the authoritative per-user registry value (0 = dark, 1 = light).
/// Falls back to uxtheme.dll ordinal 132 (`ShouldSystemUseDarkMode`) if the key is
/// absent, and ultimately returns `true` (dark) if neither source is available.
///
/// The registry path is correct for all Windows 10/11 accounts including child/standard
/// users where the uxtheme ordinal may reflect the system default rather than the
/// individual user's preference.
pub fn is_system_dark_mode() -> bool {
    // ── 1. Per-user registry (most reliable) ─────────────────────────────────
    if let Some(v) = read_apps_use_light_theme_hkcu() {
        return v == 0; // 0 = dark, 1 = light
    }

    // ── 2. Fallback: uxtheme.dll ordinal 132 ─────────────────────────────────
    let lib_name = b"uxtheme.dll\0";
    // SAFETY: `lib_name` is a NUL-terminated ASCII string with 'static life.
    let lib = unsafe { LoadLibraryA(lib_name.as_ptr().cast()) };
    if lib.is_null() {
        return true;
    }
    // Ordinal 132: ShouldSystemUseDarkMode() → BOOL (non-zero = dark)
    // SAFETY: `lib` is loaded until the FreeLibrary below; the ordinal is
    // passed in GetProcAddress's MAKEINTRESOURCE form. ASSUMED (undocumented):
    // ordinal 132 takes no arguments and returns a BOOL (i32).
    let result = unsafe {
        let f_ptr = GetProcAddress(lib, 132usize as *const i8);
        if f_ptr.is_null() {
            true
        } else {
            let f: unsafe extern "system" fn() -> i32 = std::mem::transmute(f_ptr);
            f() != 0
        }
    };
    // SAFETY: `lib` came from LoadLibraryA above and is freed once; the
    // function pointer is not used after this.
    unsafe { FreeLibrary(lib) };
    result
}

/// Read `AppsUseLightTheme` DWORD from HKCU Personalize key.
/// Returns `Some(0)` for dark, `Some(1)` for light, `None` if absent or unreadable.
fn read_apps_use_light_theme_hkcu() -> Option<u32> {
    use winapi::shared::winerror::ERROR_SUCCESS;
    use winapi::um::winnt::{KEY_READ, REG_DWORD};
    use winapi::um::winreg::{RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER};

    let subkey: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let value_name: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();

    let mut hkey = std::ptr::null_mut();
    // SAFETY: `subkey` is NUL-terminated and outlives the call; `hkey` is a
    // valid out-param, set only on success.
    let open_res =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
    if open_res as u32 != ERROR_SUCCESS {
        return None;
    }

    let mut data: u32 = 0;
    let mut data_size = std::mem::size_of::<u32>() as u32;
    let mut reg_type: u32 = 0;
    // SAFETY: `hkey` is open (the open succeeded); `value_name` is
    // NUL-terminated; `data` is a u32 and `data_size` says 4 bytes, so a
    // larger value fails with ERROR_MORE_DATA instead of overrunning it.
    let query_res = unsafe {
        RegQueryValueExW(
            hkey,
            value_name.as_ptr(),
            std::ptr::null_mut(),
            &mut reg_type,
            &mut data as *mut u32 as *mut u8,
            &mut data_size,
        )
    };
    // SAFETY: `hkey` was opened above and is closed once.
    unsafe { RegCloseKey(hkey) };

    if query_res as u32 == ERROR_SUCCESS && reg_type == REG_DWORD {
        Some(data)
    } else {
        None
    }
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
    // SAFETY: `value` is a 4-byte BOOL-sized value that lives across the
    // call and its exact size is passed; DWM validates `hwnd` (a stale one
    // just fails).
    unsafe {
        DwmSetWindowAttribute(
            hwnd as HWND,
            20, // DWMWA_USE_IMMERSIVE_DARK_MODE
            &value as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}
#[cfg(all(test, windows))]
mod tests {
    //! #702: checked through other bindings — `winreg` for the registry,
    //! the windows crate for DWM.
    use super::*;
    use crate::test_win::TestWindow;
    use ::windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};

    fn registry_value() -> Option<u32> {
        winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
            .ok()?
            .get_value::<u32, _>("AppsUseLightTheme")
            .ok()
    }

    #[test]
    fn the_theme_is_read_from_the_users_registry_value() {
        let oracle = registry_value();
        assert_eq!(read_apps_use_light_theme_hkcu(), oracle);
        // Without the value the answer comes from uxtheme; nothing to compare.
        if let Some(light) = oracle {
            assert_eq!(is_system_dark_mode(), light == 0);
        }
    }

    #[test]
    fn the_title_bar_takes_the_theme_asked_for() {
        let w = TestWindow::visible("titlebar");
        let read = || {
            let mut value = 0i32;
            // SAFETY: `value` is a BOOL-sized out-param and its size is passed.
            unsafe {
                DwmGetWindowAttribute(
                    w.hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE,
                    (&mut value as *mut i32).cast(),
                    4,
                )
            }
            .ok()
            .map(|()| value)
        };
        apply_titlebar_theme(w.raw(), true);
        let dark = read();
        apply_titlebar_theme(w.raw(), false);
        let light = read();
        // Windows before 11 may not answer; when it does, it is what was set.
        if dark.is_some() || light.is_some() {
            assert_eq!((dark, light), (Some(1), Some(0)));
        }
        apply_titlebar_theme(0, true);
    }

    #[test]
    fn enabling_dark_menus_does_not_fail() {
        // Undocumented uxtheme ordinals: nothing to read back. It must find
        // them (or skip them) without crashing; the tray menu's look is on
        // the manual checklist.
        enable();
    }
}
