//! Global hotkey listener — no third-party crate: `RegisterHotKey` is a
//! single Win32 call and this app already depends on `winapi`'s `winuser`
//! feature for other direct FFI (see `win32_behind.rs`).
//!
//! The chords come from `Settings.shortcuts` (#569, `rigstats_backend::shortcut`);
//! by default:
//!
//! - `Ctrl+Alt+O`: show/hide the overlay without leaving the game (#183) —
//!   the conventional in-game-overlay toggle (Xbox Game Bar, Steam, MSI
//!   Afterburner, …).
//! - `Ctrl+Alt+P`: cycle the active Control Center profile (#187).
//! - `Ctrl+Alt+M`: mute/unmute the microphone (#425).
//! - `Ctrl+Alt+F1`…`F9`: switch to the 1st…9th Control Center profile, in
//!   the profile bar's order (#456) — so a macro key on any keyboard, bound
//!   to the chord in its own software, switches straight to a profile.
//!   F-keys, not digits: Ctrl+Alt is AltGr, and AltGr+2 types `@` on Swedish
//!   and German layouts. A chord picked in Settings is checked the same way
//!   against the installed layouts ([`alt_gr_char`]).
//!
//! [`apply`] hands the thread new chords; it unregisters the old ones and
//! registers the new ones without a restart. [`set_paused`] holds them all
//! off while Settings waits for a new chord, so pressing one RIGStats holds
//! reaches the window instead of firing.

#![allow(unsafe_code)]

use crate::lock_ext::LockSafe;
use eframe::egui;
use rigstats_backend::debug;
use rigstats_backend::shortcut::{Shortcut, Shortcuts, PROFILE_SHORTCUTS};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::thread::JoinHandle;
use winapi::shared::minwindef::HKL;
use winapi::um::processthreadsapi::GetCurrentThreadId;
use winapi::um::winuser::{
    GetKeyboardLayoutList, GetMessageW, MapVirtualKeyExW, PeekMessageW, PostThreadMessageW,
    RegisterHotKey, ToUnicodeEx, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MSG,
    PM_NOREMOVE, VK_CONTROL, VK_LCONTROL, VK_LSHIFT, VK_MENU, VK_RMENU, VK_SHIFT, WM_APP,
    WM_HOTKEY,
};

/// What fired — sent over `tx` each time a chord is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    ToggleOverlay,
    CycleProfile,
    ToggleMicMute,
    /// The profile at this position in the profile bar (0-based).
    SwitchProfile(usize),
}

/// One chord to register: its `RegisterHotKey` id, the chord and the event
/// it raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Binding {
    id: i32,
    chord: Shortcut,
    event: HotkeyEvent,
}

/// Every chord the shortcuts turn on.
fn bindings(shortcuts: &Shortcuts) -> Vec<Binding> {
    let fixed = [
        (1, shortcuts.overlay, HotkeyEvent::ToggleOverlay),
        (2, shortcuts.next_profile, HotkeyEvent::CycleProfile),
        (3, shortcuts.microphone, HotkeyEvent::ToggleMicMute),
    ];
    let profiles = (0..PROFILE_SHORTCUTS).map(|i| {
        (
            10 + i as i32,
            shortcuts.profile(i),
            HotkeyEvent::SwitchProfile(i),
        )
    });
    fixed
        .into_iter()
        .chain(profiles)
        .filter_map(|(id, chord, event)| chord.map(|chord| Binding { id, chord, event }))
        .collect()
}

/// What a chord does, in the words Status uses.
fn purpose(event: HotkeyEvent) -> String {
    match event {
        HotkeyEvent::ToggleOverlay => "show or hide the overlay".to_owned(),
        HotkeyEvent::CycleProfile => "switch to the next profile".to_owned(),
        HotkeyEvent::ToggleMicMute => "mute the microphone".to_owned(),
        HotkeyEvent::SwitchProfile(i) => format!("switch to profile {}", i + 1),
    }
}

/// What the thread should hold: the chords, unless paused.
struct Wanted {
    shortcuts: Shortcuts,
    paused: bool,
}

static WANTED: Mutex<Option<Wanted>> = Mutex::new(None);

/// The hotkey thread's id, for waking it; 0 until it has a message queue.
static THREAD_ID: AtomicU32 = AtomicU32::new(0);

/// Posted to the hotkey thread: register what [`WANTED`] says now.
const WM_REREGISTER: u32 = WM_APP + 1;

/// The chords another program held when they were last registered, each
/// with what it does (#568).
static TAKEN: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// The chords that couldn't be registered because another program holds
/// them — for Status's Problems group and Settings → Shortcuts (#568).
pub fn taken() -> Vec<(String, String)> {
    TAKEN.lock_safe().clone()
}

/// The shortcuts in use now (the defaults before [`spawn`]).
pub fn current() -> Shortcuts {
    WANTED
        .lock_safe()
        .as_ref()
        .map_or_else(Shortcuts::default, |w| w.shortcuts.clone())
}

/// The chord that switches to the profile at `index` in the profile bar,
/// for showing beside it; `None` past the ninth or when they are off.
pub fn profile_chord(index: usize) -> Option<String> {
    current().profile(index).map(|c| c.to_string())
}

/// Changes what the thread holds and wakes it, when it differs.
fn update(change: impl FnOnce(&mut Wanted)) {
    {
        let mut wanted = WANTED.lock_safe();
        let w = wanted.get_or_insert_with(|| Wanted {
            shortcuts: Shortcuts::default(),
            paused: false,
        });
        let before = (w.shortcuts.clone(), w.paused);
        change(w);
        if before == (w.shortcuts.clone(), w.paused) {
            return;
        }
    }
    // Before the thread has a queue it reads `WANTED` itself when it starts.
    let thread = THREAD_ID.load(Ordering::SeqCst);
    if thread != 0 {
        unsafe {
            PostThreadMessageW(thread, WM_REREGISTER, 0, 0);
        }
    }
}

/// Registers `shortcuts` in place of the chords held now — Settings' live
/// preview, Save and Cancel all come through here.
pub fn apply(shortcuts: &Shortcuts) {
    update(|w| w.shortcuts = shortcuts.clone());
}

/// Holds every chord off (`true`) while Settings waits for a new one, and
/// registers them again after (`false`).
pub fn set_paused(paused: bool) {
    update(|w| w.paused = paused);
}

/// The character AltGr types with `chord`'s key on one of the installed
/// keyboard layouts — a chord with Ctrl+Alt would stop that character from
/// being typed. `None` without both Ctrl and Alt, or when no layout types
/// one. Windows answers (`ToUnicodeEx`, without changing the keyboard's
/// state), so it holds for every layout the user can switch to.
pub fn alt_gr_char(chord: Shortcut) -> Option<char> {
    if !chord.mods.is_alt_gr() {
        return None;
    }
    let mut layouts: [HKL; 32] = [std::ptr::null_mut(); 32];
    let count = unsafe { GetKeyboardLayoutList(layouts.len() as i32, layouts.as_mut_ptr()) };
    let mut state = [0u8; 256];
    for vk in [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_RMENU] {
        state[vk as usize] = 0x80;
    }
    if chord.mods.shift {
        state[VK_SHIFT as usize] = 0x80;
        state[VK_LSHIFT as usize] = 0x80;
    }
    layouts
        .iter()
        .take(count.max(0) as usize)
        .find_map(|&layout| {
            let scan = unsafe { MapVirtualKeyExW(chord.vk, 0, layout) }; // MAPVK_VK_TO_VSC
            let mut buf = [0u16; 8];
            // Flag 0x4: leave the keyboard state (dead keys) as it was.
            let n = unsafe {
                ToUnicodeEx(
                    chord.vk,
                    scan,
                    state.as_ptr(),
                    buf.as_mut_ptr(),
                    buf.len() as i32,
                    0x4,
                    layout,
                )
            };
            // A dead key (n < 0) types a character too, with the next key.
            (n != 0)
                .then(|| char::decode_utf16(buf.iter().copied()).next()?.ok())
                .flatten()
                .filter(|c| !c.is_control())
        })
}

/// Windows' virtual-key code for a key Settings captured — the keys
/// `rigstats_backend::shortcut::key_name` knows; `None` for the rest.
pub fn vk_for(key: egui::Key) -> Option<u32> {
    use egui::Key as K;
    const LETTERS: [K; 26] = [
        K::A,
        K::B,
        K::C,
        K::D,
        K::E,
        K::F,
        K::G,
        K::H,
        K::I,
        K::J,
        K::K,
        K::L,
        K::M,
        K::N,
        K::O,
        K::P,
        K::Q,
        K::R,
        K::S,
        K::T,
        K::U,
        K::V,
        K::W,
        K::X,
        K::Y,
        K::Z,
    ];
    const DIGITS: [K; 10] = [
        K::Num0,
        K::Num1,
        K::Num2,
        K::Num3,
        K::Num4,
        K::Num5,
        K::Num6,
        K::Num7,
        K::Num8,
        K::Num9,
    ];
    const F_KEYS: [K; 24] = [
        K::F1,
        K::F2,
        K::F3,
        K::F4,
        K::F5,
        K::F6,
        K::F7,
        K::F8,
        K::F9,
        K::F10,
        K::F11,
        K::F12,
        K::F13,
        K::F14,
        K::F15,
        K::F16,
        K::F17,
        K::F18,
        K::F19,
        K::F20,
        K::F21,
        K::F22,
        K::F23,
        K::F24,
    ];
    let index_in = |keys: &[K]| keys.iter().position(|&k| k == key).map(|i| i as u32);
    index_in(&LETTERS)
        .map(|i| 0x41 + i)
        .or_else(|| index_in(&DIGITS).map(|i| 0x30 + i))
        .or_else(|| index_in(&F_KEYS).map(|i| rigstats_backend::shortcut::VK_F1 + i))
        .or(match key {
            K::Space => Some(0x20),
            K::PageUp => Some(0x21),
            K::PageDown => Some(0x22),
            K::End => Some(0x23),
            K::Home => Some(0x24),
            K::ArrowLeft => Some(0x25),
            K::ArrowUp => Some(0x26),
            K::ArrowRight => Some(0x27),
            K::ArrowDown => Some(0x28),
            K::Insert => Some(0x2D),
            K::Delete => Some(0x2E),
            _ => None,
        })
}

/// `RegisterHotKey`'s modifier flags for a chord.
fn mod_flags(chord: Shortcut) -> u32 {
    let mut flags = 0;
    if chord.mods.ctrl {
        flags |= MOD_CONTROL;
    }
    if chord.mods.alt {
        flags |= MOD_ALT;
    }
    if chord.mods.shift {
        flags |= MOD_SHIFT;
    }
    flags as u32
}

/// Registers what [`WANTED`] says (nothing while paused) and, when not
/// paused, records the chords another program holds in [`TAKEN`].
fn register_wanted(dir: &Path) -> Vec<Binding> {
    let (shortcuts, paused) = {
        let wanted = WANTED.lock_safe();
        wanted.as_ref().map_or_else(
            || (Shortcuts::default(), false),
            |w| (w.shortcuts.clone(), w.paused),
        )
    };
    if paused {
        return Vec::new();
    }
    let mut taken = Vec::new();
    let registered = bindings(&shortcuts)
        .into_iter()
        .filter(|binding| {
            let ok = unsafe {
                RegisterHotKey(
                    std::ptr::null_mut(),
                    binding.id,
                    mod_flags(binding.chord),
                    binding.chord.vk,
                )
            } != 0;
            if !ok {
                debug::log_warn(
                    dir,
                    &format!(
                        "hotkey: RegisterHotKey({}) failed — likely already bound by another app",
                        binding.chord
                    ),
                );
                taken.push((binding.chord.to_string(), purpose(binding.event)));
            }
            ok
        })
        .collect();
    *TAKEN.lock_safe() = taken;
    registered
}

/// Spawns a dedicated thread that registers `shortcuts` and sends the
/// corresponding [`HotkeyEvent`] over `tx` each time one fires.
/// `RegisterHotKey`/`WM_HOTKEY` delivery is thread-affine — the message
/// queue belongs to whichever thread *registered* the hotkey — so
/// registration must happen on this same spawned thread, right before its
/// own blocking `GetMessageW` loop. (Registering from the caller's thread
/// and then polling from here would silently never see the messages: they'd
/// queue up on the caller's thread, which runs eframe's own event loop, not
/// this one.) [`apply`] and [`set_paused`] wake it with a thread message to
/// register again, for the same reason.
///
/// If a chord is already owned by another app, *that one* registration
/// fails, is logged and is listed by [`taken`] — the thread still runs the
/// loop for whichever chords did register.
///
/// `ctx` is used to wake the main eframe loop the instant a chord fires —
/// without it, the event sits in `tx`'s channel until the app's own ~1 fps
/// idle repaint happens to tick, adding up to a second of dead time before
/// the keypress is even noticed (same reason the tray thread in `main.rs`
/// calls `request_repaint()` right after sending its own command).
pub fn spawn(
    dir: PathBuf,
    shortcuts: Shortcuts,
    tx: Sender<HotkeyEvent>,
    ctx: egui::Context,
) -> JoinHandle<()> {
    update(|w| w.shortcuts = shortcuts);
    std::thread::spawn(move || {
        // A thread gets its message queue on its first message call; only
        // then can `apply` post to it.
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        unsafe {
            PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
        }
        THREAD_ID.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
        let mut registered = register_wanted(&dir);

        loop {
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            // Blocks until a message arrives; returns 0 on WM_QUIT (never
            // posted here — this thread runs for the process lifetime), <0
            // on error.
            let ret = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
            if ret <= 0 {
                break;
            }
            match msg.message {
                WM_HOTKEY => {
                    let fired = registered
                        .iter()
                        .find(|binding| binding.id == msg.wParam as i32)
                        .map(|binding| binding.event);
                    if let Some(event) = fired {
                        let _ = tx.send(event);
                        ctx.request_repaint();
                    }
                }
                WM_REREGISTER => {
                    for binding in registered.drain(..) {
                        unsafe {
                            UnregisterHotKey(std::ptr::null_mut(), binding.id);
                        }
                    }
                    registered = register_wanted(&dir);
                    // Settings shows which chords another program holds.
                    ctx.request_repaint();
                }
                _ => {}
            }
        }
        for binding in registered {
            unsafe {
                UnregisterHotKey(std::ptr::null_mut(), binding.id);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rigstats_backend::shortcut::{Mods, VK_F1};

    #[test]
    fn each_of_the_first_nine_profiles_has_its_own_f_key() {
        let all = bindings(&Shortcuts::default());
        let profile: Vec<_> = all
            .iter()
            .filter_map(|b| match b.event {
                HotkeyEvent::SwitchProfile(i) => Some((i, b.chord.vk, b.chord.to_string())),
                _ => None,
            })
            .collect();
        assert_eq!(profile.len(), PROFILE_SHORTCUTS);
        assert_eq!(profile[1], (1, VK_F1 + 1, "Ctrl+Alt+F2".to_owned()));
        // Every RegisterHotKey id is unique.
        let mut ids: Vec<_> = all.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
    }

    #[test]
    fn a_shortcut_turned_off_is_not_registered() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.microphone = None;
        shortcuts.profiles = None;
        let events: Vec<_> = bindings(&shortcuts).iter().map(|b| b.event).collect();
        assert_eq!(
            events,
            [HotkeyEvent::ToggleOverlay, HotkeyEvent::CycleProfile]
        );
        shortcuts.profiles = Some(Mods::ALT_SHIFT);
        let first = bindings(&shortcuts)
            .into_iter()
            .find(|b| b.event == HotkeyEvent::SwitchProfile(0))
            .unwrap();
        assert_eq!(first.chord.to_string(), "Alt+Shift+F1");
        assert_eq!(mod_flags(first.chord), (MOD_ALT | MOD_SHIFT) as u32);
    }

    #[test]
    fn captured_keys_map_to_windows_key_codes() {
        assert_eq!(vk_for(egui::Key::A), Some(0x41));
        assert_eq!(vk_for(egui::Key::M), Some(0x4D));
        assert_eq!(vk_for(egui::Key::Num7), Some(0x37));
        assert_eq!(vk_for(egui::Key::F12), Some(VK_F1 + 11));
        assert_eq!(vk_for(egui::Key::PageUp), Some(0x21));
        assert_eq!(vk_for(egui::Key::Escape), None);
        // Every key mapped has a name, so it reads back after Save.
        for vk in [0x41, 0x37, VK_F1 + 23, 0x2E] {
            assert!(
                rigstats_backend::shortcut::key_name(vk).is_some(),
                "{vk:#x}"
            );
        }
    }

    #[test]
    fn f_keys_and_chords_without_ctrl_alt_never_type_a_character() {
        // Windows' answer for whatever layouts this PC has: no layout puts a
        // character on AltGr+F5, and without both Ctrl and Alt it's no AltGr.
        let f5 = Shortcut::parse("Ctrl+Alt+F5").unwrap();
        assert_eq!(alt_gr_char(f5), None);
        assert_eq!(alt_gr_char(Shortcut::parse("Ctrl+Shift+2").unwrap()), None);
    }
}
