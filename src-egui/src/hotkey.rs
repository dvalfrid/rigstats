//! Global hotkey listener — two fixed chords in v1, no capture UI, no
//! remapping, no third-party crate: `RegisterHotKey` is a single Win32 call
//! and this app already depends on `winapi`'s `winuser` feature for other
//! direct FFI (see `win32_behind.rs`).
//!
//! - `Ctrl+Alt+O`: show/hide the overlay without leaving the game (#183) —
//!   the conventional in-game-overlay toggle (Xbox Game Bar, Steam, MSI
//!   Afterburner, …).
//! - `Ctrl+Alt+P`: cycle the active Control Center profile (#187).

#![allow(unsafe_code)]

use eframe::egui;
use rigstats_backend::debug;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use winapi::um::winuser::{
    GetMessageW, RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MSG, WM_HOTKEY,
};

const TOGGLE_OVERLAY_ID: i32 = 1;
const CYCLE_PROFILE_ID: i32 = 2;
const VK_O: u32 = 0x4F;
const VK_P: u32 = 0x50;

/// What fired — sent over `tx` each time either chord is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    ToggleOverlay,
    CycleProfile,
}

/// Spawns a dedicated thread that registers both fixed chords and sends the
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
/// fails and is logged — the thread still runs the loop for whichever chord
/// (if any) did register; there is no conflict UI in v1.
///
/// `ctx` is used to wake the main eframe loop the instant a chord fires —
/// without it, the event sits in `tx`'s channel until the app's own ~1 fps
/// idle repaint happens to tick, adding up to a second of dead time before
/// the keypress is even noticed (same reason the tray thread in `main.rs`
/// calls `request_repaint()` right after sending its own command).
pub fn spawn(dir: PathBuf, tx: Sender<HotkeyEvent>, ctx: egui::Context) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let overlay_registered = unsafe {
            RegisterHotKey(
                std::ptr::null_mut(),
                TOGGLE_OVERLAY_ID,
                (MOD_CONTROL | MOD_ALT) as u32,
                VK_O,
            )
        };
        if overlay_registered == 0 {
            debug::log_warn(
                &dir,
                "hotkey: RegisterHotKey(Ctrl+Alt+O) failed — likely already bound by another app",
            );
        }

        let profile_registered = unsafe {
            RegisterHotKey(
                std::ptr::null_mut(),
                CYCLE_PROFILE_ID,
                (MOD_CONTROL | MOD_ALT) as u32,
                VK_P,
            )
        };
        if profile_registered == 0 {
            debug::log_warn(
                &dir,
                "hotkey: RegisterHotKey(Ctrl+Alt+P) failed — likely already bound by another app",
            );
        }

        if overlay_registered == 0 && profile_registered == 0 {
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
                let event = match msg.wParam as i32 {
                    TOGGLE_OVERLAY_ID if overlay_registered != 0 => {
                        Some(HotkeyEvent::ToggleOverlay)
                    }
                    CYCLE_PROFILE_ID if profile_registered != 0 => Some(HotkeyEvent::CycleProfile),
                    _ => None,
                };
                if let Some(event) = event {
                    let _ = tx.send(event);
                    ctx.request_repaint();
                }
            }
        }
        unsafe {
            if overlay_registered != 0 {
                UnregisterHotKey(std::ptr::null_mut(), TOGGLE_OVERLAY_ID);
            }
            if profile_registered != 0 {
                UnregisterHotKey(std::ptr::null_mut(), CYCLE_PROFILE_ID);
            }
        }
    })
}
