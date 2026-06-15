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
    let open_res =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) };
    if open_res as u32 != ERROR_SUCCESS {
        return None;
    }

    let mut data: u32 = 0;
    let mut data_size = std::mem::size_of::<u32>() as u32;
    let mut reg_type: u32 = 0;
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
    unsafe {
        DwmSetWindowAttribute(
            hwnd as HWND,
            20, // DWMWA_USE_IMMERSIVE_DARK_MODE
            &value as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );
    }
}
