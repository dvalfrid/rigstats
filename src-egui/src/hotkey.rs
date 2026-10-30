//! Global hotkey listener — no third-party crate: `RegisterHotKey` is a
//! single Win32 call through the `windows` crate, which this app already
//! uses for its other direct FFI (see `win32_behind.rs`).
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
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayoutList, MapVirtualKeyExW, RegisterHotKey, ToUnicodeEx, UnregisterHotKey, HKL,
    HOT_KEY_MODIFIERS, MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL, MOD_SHIFT, VK_CONTROL, VK_LCONTROL,
    VK_LSHIFT, VK_MENU, VK_RMENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_APP, WM_HOTKEY,
};

/// What fired — sent over `tx` each time a chord is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    ToggleOverlay,
    CycleProfile,
    ToggleMicMute,
    /// The profile at this position in the profile bar (0-based).
    SwitchProfile(usize),
    /// The tray menu's actions (#594), run as the tray runs them.
    ToggleLamp,
    OpenControlCenter,
    OpenSettings,
    OpenStatus,
    ToggleRecording,
    ToggleOverlayLock,
}

impl HotkeyEvent {
    /// The tray command a tray-action shortcut runs (#594): the shortcut does
    /// exactly what the tray menu's row does. `None` for the shortcuts with a
    /// handler of their own.
    pub fn tray_cmd(self) -> Option<crate::tray::TrayCmd> {
        use crate::tray::TrayCmd;
        match self {
            HotkeyEvent::ToggleLamp => Some(TrayCmd::ToggleLamp),
            HotkeyEvent::OpenControlCenter => Some(TrayCmd::OpenControlCenter),
            HotkeyEvent::OpenSettings => Some(TrayCmd::OpenSettings),
            HotkeyEvent::OpenStatus => Some(TrayCmd::OpenStatus),
            HotkeyEvent::ToggleRecording => Some(TrayCmd::ToggleRecording),
            HotkeyEvent::ToggleOverlayLock => Some(TrayCmd::ToggleOverlayLock),
            HotkeyEvent::ToggleOverlay
            | HotkeyEvent::CycleProfile
            | HotkeyEvent::ToggleMicMute
            | HotkeyEvent::SwitchProfile(_) => None,
        }
    }
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
        (4, shortcuts.desk_lamp, HotkeyEvent::ToggleLamp),
        (5, shortcuts.control_center, HotkeyEvent::OpenControlCenter),
        (6, shortcuts.settings, HotkeyEvent::OpenSettings),
        (7, shortcuts.status, HotkeyEvent::OpenStatus),
        (8, shortcuts.recording, HotkeyEvent::ToggleRecording),
        (9, shortcuts.overlay_lock, HotkeyEvent::ToggleOverlayLock),
        // 10…18 are the profiles' F-keys below.
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
        HotkeyEvent::ToggleLamp => "turn the desk lamp on or off".to_owned(),
        HotkeyEvent::OpenControlCenter => "open the Control Center".to_owned(),
        HotkeyEvent::OpenSettings => "open Settings".to_owned(),
        HotkeyEvent::OpenStatus => "open Status".to_owned(),
        HotkeyEvent::ToggleRecording => "start or stop recording".to_owned(),
        HotkeyEvent::ToggleOverlayLock => "lock or unlock the overlay".to_owned(),
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
        // SAFETY: plain-value arguments; `thread` is the hotkey thread's id,
        // stored only after its queue exists. Should that thread be gone the
        // post just fails.
        unsafe {
            let _ = PostThreadMessageW(thread, WM_REREGISTER, WPARAM(0), LPARAM(0));
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
    let mut layouts = [HKL::default(); 32];
    // SAFETY: `layouts` is passed as a slice, so Windows writes at most its
    // length (32) handles; the result is clamped to the array by `take`
    // below as well.
    let count = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) };
    let mut state = [0u8; 256];
    for vk in [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_RMENU] {
        state[vk.0 as usize] = 0x80;
    }
    if chord.mods.shift {
        state[VK_SHIFT.0 as usize] = 0x80;
        state[VK_LSHIFT.0 as usize] = 0x80;
    }
    layouts
        .iter()
        .take(count.max(0) as usize)
        .find_map(|&layout| {
            // SAFETY: plain-value arguments; `layout` is a handle Windows
            // just listed (an unknown one only makes the call return 0).
            let scan = unsafe { MapVirtualKeyExW(chord.vk, MAPVK_VK_TO_VSC, Some(layout)) };
            let mut buf = [0u16; 8];
            // Flag 0x4: leave the keyboard state (dead keys) as it was.
            // SAFETY: `state` is the 256-byte key-state array ToUnicodeEx
            // reads; `buf` is passed as a slice, so at most its 8 UTF-16
            // units are written; both are locals alive for the call.
            let n = unsafe { ToUnicodeEx(chord.vk, scan, &state, &mut buf, 0x4, Some(layout)) };
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
    let mut flags = HOT_KEY_MODIFIERS(0);
    if chord.mods.ctrl {
        flags |= MOD_CONTROL;
    }
    if chord.mods.alt {
        flags |= MOD_ALT;
    }
    if chord.mods.shift {
        flags |= MOD_SHIFT;
    }
    flags.0
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
            // SAFETY: plain-value arguments; a null HWND ties the hotkey to
            // this (the hotkey) thread's queue, where it is unregistered.
            let ok = unsafe {
                RegisterHotKey(
                    None,
                    binding.id,
                    HOT_KEY_MODIFIERS(mod_flags(binding.chord)),
                    binding.chord.vk,
                )
            }
            .is_ok();
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
        let mut msg = MSG::default();
        // SAFETY: `msg` is a valid, writable MSG; no window means any
        // message of this thread.
        unsafe {
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        }
        // SAFETY: no arguments; cannot fail.
        THREAD_ID.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
        let mut registered = register_wanted(&dir);

        loop {
            let mut msg = MSG::default();
            // Blocks until a message arrives; returns 0 on WM_QUIT (never
            // posted here — this thread runs for the process lifetime), <0
            // on error.
            // SAFETY: `msg` is a valid, writable MSG; no window means any
            // message of this thread.
            let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if ret.0 <= 0 {
                break;
            }
            match msg.message {
                WM_HOTKEY => {
                    let fired = registered
                        .iter()
                        .find(|binding| binding.id == msg.wParam.0 as i32)
                        .map(|binding| binding.event);
                    if let Some(event) = fired {
                        let _ = tx.send(event);
                        ctx.request_repaint();
                    }
                }
                WM_REREGISTER => {
                    for binding in registered.drain(..) {
                        // SAFETY: plain-value arguments; `binding.id` was
                        // registered on this thread with a null HWND.
                        unsafe {
                            let _ = UnregisterHotKey(None, binding.id);
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
            // SAFETY: plain-value arguments; `binding.id` was registered on
            // this thread with a null HWND.
            unsafe {
                let _ = UnregisterHotKey(None, binding.id);
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

    /// Shortcuts with only `action` on (a chord of its own), the rest off.
    fn only(action: rigstats_backend::shortcut::Action) -> Shortcuts {
        use rigstats_backend::shortcut::SINGLE_ACTIONS;
        let mut s = Shortcuts::default();
        for a in SINGLE_ACTIONS {
            s.set(a, None);
        }
        s.profiles = None;
        s.set(action, Shortcut::parse("Ctrl+Shift+F10"));
        s
    }

    /// Every action with a chord is registered, each with its own event and
    /// id (#594) — an action added to `Shortcuts` but forgotten in
    /// `bindings` fails here.
    #[test]
    fn every_action_registers_its_own_event() {
        use crate::tray::TrayCmd;
        use rigstats_backend::shortcut::{Action, SINGLE_ACTIONS};
        let expected = [
            (Action::Overlay, HotkeyEvent::ToggleOverlay, None),
            (Action::NextProfile, HotkeyEvent::CycleProfile, None),
            (Action::Microphone, HotkeyEvent::ToggleMicMute, None),
            (
                Action::DeskLamp,
                HotkeyEvent::ToggleLamp,
                Some(TrayCmd::ToggleLamp),
            ),
            (
                Action::ControlCenter,
                HotkeyEvent::OpenControlCenter,
                Some(TrayCmd::OpenControlCenter),
            ),
            (
                Action::Settings,
                HotkeyEvent::OpenSettings,
                Some(TrayCmd::OpenSettings),
            ),
            (
                Action::Status,
                HotkeyEvent::OpenStatus,
                Some(TrayCmd::OpenStatus),
            ),
            (
                Action::Recording,
                HotkeyEvent::ToggleRecording,
                Some(TrayCmd::ToggleRecording),
            ),
            (
                Action::OverlayLock,
                HotkeyEvent::ToggleOverlayLock,
                Some(TrayCmd::ToggleOverlayLock),
            ),
        ];
        assert_eq!(expected.len(), SINGLE_ACTIONS.len());
        let mut ids = Vec::new();
        for (action, event, tray) in expected {
            let bound = bindings(&only(action));
            assert_eq!(bound.len(), 1, "{action:?}");
            assert_eq!(bound[0].event, event, "{action:?}");
            assert_eq!(event.tray_cmd(), tray, "{action:?}");
            ids.push(bound[0].id);
        }
        // With all of them and the profiles on, every id is still unique.
        let mut all = Shortcuts::default();
        for (n, a) in SINGLE_ACTIONS.into_iter().enumerate() {
            all.set(a, Some(Shortcut::new(Mods::CTRL_SHIFT, 0x41 + n as u32)));
        }
        let mut every: Vec<_> = bindings(&all).iter().map(|b| b.id).collect();
        assert_eq!(every.len(), SINGLE_ACTIONS.len() + PROFILE_SHORTCUTS);
        every.sort_unstable();
        every.dedup();
        assert_eq!(every.len(), SINGLE_ACTIONS.len() + PROFILE_SHORTCUTS);
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SINGLE_ACTIONS.len());
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
        // MOD_ALT | MOD_SHIFT
        assert_eq!(mod_flags(first.chord), 0x0001 | 0x0004);
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
#[cfg(all(test, windows))]
mod win32_tests {
    //! #702: the Win32 side, checked through the windows crate.
    use super::*;
    use ::windows::Win32::Foundation::{LPARAM, WPARAM};
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayoutList as layout_list, MapVirtualKeyExW as map_key, RegisterHotKey as hold,
        ToUnicodeEx as to_unicode, UnregisterHotKey as release, HKL as Layout, HOT_KEY_MODIFIERS,
        MAP_VIRTUAL_KEY_TYPE,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_HOTKEY as HOTKEY};
    use rigstats_backend::shortcut::{Action, Mods, SINGLE_ACTIONS};
    use std::time::{Duration, Instant};

    fn layouts() -> Vec<Layout> {
        let mut list = [Layout::default(); 32];
        // SAFETY: `list` is writable and its length is passed.
        let n = unsafe { layout_list(Some(&mut list)) };
        list[..n.max(0) as usize].to_vec()
    }

    /// What AltGr (+ Shift) with `vk` types on `layout`, from Windows.
    fn typed(layout: Layout, vk: u32, shift: bool) -> Option<char> {
        let mut state = [0u8; 256];
        for k in [0x11, 0xA2, 0x12, 0xA5] {
            state[k] = 0x80;
        }
        if shift {
            state[0x10] = 0x80;
            state[0xA0] = 0x80;
        }
        // SAFETY: plain values; `layout` is one Windows listed.
        let scan = unsafe { map_key(vk, MAP_VIRTUAL_KEY_TYPE(0), Some(layout)) };
        let mut buf = [0u16; 8];
        // SAFETY: `state` is the 256-byte key state; `buf` is writable and
        // its length is passed; flag 0x4 leaves the keyboard state alone.
        let n = unsafe { to_unicode(vk, scan, &state, &mut buf, 0x4, Some(layout)) };
        (n != 0)
            .then(|| char::decode_utf16(buf.iter().copied()).next()?.ok())
            .flatten()
            .filter(|c| !c.is_control())
    }

    #[test]
    fn alt_gr_characters_are_what_windows_types() {
        let layouts = layouts();
        let keys = (0x30..=0x39)
            .chain(0x41..=0x5A)
            .chain([0xBA, 0xBB, 0xBC, 0xDB, 0xDD, 0xE2]);
        for vk in keys {
            for shift in [false, true] {
                let mods = if shift {
                    Mods::CTRL_ALT_SHIFT
                } else {
                    Mods::CTRL_ALT
                };
                let chord = Shortcut::new(mods, vk);
                let oracle = layouts.iter().find_map(|&l| typed(l, vk, shift));
                assert_eq!(alt_gr_char(chord), oracle, "{chord}");
            }
        }
        // On a PC with the Swedish layout, AltGr+2 is @.
        if layouts
            .iter()
            .any(|l| (l.0 as usize >> 16) & 0xFFFF == 0x041D)
        {
            assert_eq!(
                alt_gr_char(Shortcut::parse("Ctrl+Alt+2").unwrap()),
                Some('@')
            );
        }
    }

    /// Whether another thread could take `chord` now — it can't while the
    /// hotkey thread holds it.
    fn free(chord: Shortcut) -> bool {
        let flags = HOT_KEY_MODIFIERS(mod_flags(chord));
        // SAFETY: plain values; registered to this thread and released at once.
        let ok = unsafe { hold(None, 0x7000, flags, chord.vk) }.is_ok();
        if ok {
            // SAFETY: the id registered just above on this thread.
            let _ = unsafe { release(None, 0x7000) };
        }
        ok
    }

    fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
        let start = Instant::now();
        while !done() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "timed out: {what}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The whole thread: registering, a chord firing, a chord another
    /// program holds, pausing. Chords no one uses (F21–F23) keep it off the
    /// user's own; the thread lives on for the rest of the test run, holding
    /// nothing.
    #[test]
    fn the_hotkey_thread_holds_fires_reports_and_pauses() {
        let overlay = Shortcut::parse("Ctrl+Alt+Shift+F23").unwrap();
        let next = Shortcut::parse("Ctrl+Alt+Shift+F22").unwrap();
        let mut shortcuts = Shortcuts::default();
        for a in SINGLE_ACTIONS {
            shortcuts.set(a, None);
        }
        shortcuts.profiles = None;
        shortcuts.set(Action::Overlay, Some(overlay));
        assert!(free(overlay), "the test chord is free to begin with");

        let dir = std::env::temp_dir().join(format!("rigstats-hotkey-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let _thread = spawn(dir.clone(), shortcuts.clone(), tx, egui::Context::default());
        wait_until("the chord is held", || !free(overlay));

        // A WM_HOTKEY with the overlay's id raises its event.
        let thread = THREAD_ID.load(Ordering::SeqCst);
        // SAFETY: plain values; the hotkey thread's id.
        unsafe { PostThreadMessageW(thread, HOTKEY, WPARAM(1), LPARAM(0)) }.unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)),
            Ok(HotkeyEvent::ToggleOverlay)
        );

        // A chord this thread holds is "taken" for the hotkey thread.
        // SAFETY: plain values; released below on this thread.
        unsafe { hold(None, 0x7001, HOT_KEY_MODIFIERS(mod_flags(next)), next.vk) }.unwrap();
        shortcuts.set(Action::NextProfile, Some(next));
        apply(&shortcuts);
        let want = vec![(next.to_string(), "switch to the next profile".to_owned())];
        wait_until("the taken chord is listed", || taken() == want);
        assert!(!free(overlay));
        // SAFETY: the id registered above on this thread.
        unsafe { release(None, 0x7001) }.unwrap();

        // Paused: nothing held. Resumed: held again.
        set_paused(true);
        wait_until("paused frees the chord", || free(overlay));
        set_paused(false);
        wait_until("resumed holds it again", || !free(overlay));

        // Hold nothing for the rest of the run.
        shortcuts.set(Action::Overlay, None);
        shortcuts.set(Action::NextProfile, None);
        apply(&shortcuts);
        wait_until("nothing held at the end", || {
            free(overlay) && taken().is_empty()
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
