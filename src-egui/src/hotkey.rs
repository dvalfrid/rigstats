//! Global hotkey listener — a few fixed chords, no capture UI, no
//! remapping, no third-party crate: `RegisterHotKey` is a single Win32 call
//! and this app already depends on `winapi`'s `winuser` feature for other
//! direct FFI (see `win32_behind.rs`).
//!
//! - `Ctrl+Alt+O`: show/hide the overlay without leaving the game (#183) —
//!   the conventional in-game-overlay toggle (Xbox Game Bar, Steam, MSI
//!   Afterburner, …).
//! - `Ctrl+Alt+P`: cycle the active Control Center profile (#187).
//! - `Ctrl+Alt+M`: mute/unmute the microphone (#425).

#![allow(unsafe_code)]

use eframe::egui;
use rigstats_backend::debug;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use winapi::um::winuser::{
    GetMessageW, RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MSG, WM_HOTKEY,
};

/// What fired — sent over `tx` each time a chord is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    ToggleOverlay,
    CycleProfile,
    ToggleMicMute,
}

/// One fixed `Ctrl+Alt+<key>` chord: its `RegisterHotKey` id, virtual key,
/// name for the log and the event it raises.
struct Chord {
    id: i32,
    vk: u32,
    name: &'static str,
    event: HotkeyEvent,
}

const CHORDS: [Chord; 3] = [
    Chord {
        id: 1,
        vk: 0x4F, // O
        name: "Ctrl+Alt+O",
        event: HotkeyEvent::ToggleOverlay,
    },
    Chord {
        id: 2,
        vk: 0x50, // P
        name: "Ctrl+Alt+P",
        event: HotkeyEvent::CycleProfile,
    },
    Chord {
        id: 3,
        vk: 0x4D, // M
        name: "Ctrl+Alt+M",
        event: HotkeyEvent::ToggleMicMute,
    },
];

/// Spawns a dedicated thread that registers the fixed chords and sends the
/// corresponding [`HotkeyEvent`] over `tx` each time one fires.
/// `RegisterHotKey`/`WM_HOTKEY` delivery is thread-affine — the message
/// queue belongs to whichever thread *registered* the hotkey — so
/// registration must happen on this same spawned thread, right before its
/// own blocking `GetMessageW` loop. (Registering from the caller's thread
/// and then polling from here would silently never see the messages: they'd
/// queue up on the caller's thread, which runs eframe's own event loop, not
/// this one.)
///
/// If a chord is already owned by another app, *that one* registration
/// fails and is logged — the thread still runs the loop for whichever chords
/// did register; there is no conflict UI in v1.
///
/// `ctx` is used to wake the main eframe loop the instant a chord fires —
/// without it, the event sits in `tx`'s channel until the app's own ~1 fps
/// idle repaint happens to tick, adding up to a second of dead time before
/// the keypress is even noticed (same reason the tray thread in `main.rs`
/// calls `request_repaint()` right after sending its own command).
pub fn spawn(dir: PathBuf, tx: Sender<HotkeyEvent>, ctx: egui::Context) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let registered: Vec<&Chord> = CHORDS
            .iter()
            .filter(|chord| {
                let ok = unsafe {
                    RegisterHotKey(
                        std::ptr::null_mut(),
                        chord.id,
                        (MOD_CONTROL | MOD_ALT) as u32,
                        chord.vk,
                    )
                } != 0;
                if !ok {
                    debug::log_warn(
                        &dir,
                        &format!(
                            "hotkey: RegisterHotKey({}) failed — likely already bound by another app",
                            chord.name
                        ),
                    );
                }
                ok
            })
            .collect();
        if registered.is_empty() {
            return; // Nothing registered — no point running the message loop.
        }

        loop {
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            // Blocks until a message arrives; returns 0 on WM_QUIT (never
            // posted here — this thread runs for the process lifetime), <0
            // on error.
            let ret = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
            if ret <= 0 {
                break;
            }
            if msg.message == WM_HOTKEY {
                let fired = registered
                    .iter()
                    .find(|chord| chord.id == msg.wParam as i32)
                    .map(|chord| chord.event);
                if let Some(event) = fired {
                    let _ = tx.send(event);
                    ctx.request_repaint();
                }
            }
        }
        for chord in registered {
            unsafe {
                UnregisterHotKey(std::ptr::null_mut(), chord.id);
            }
        }
    })
}
