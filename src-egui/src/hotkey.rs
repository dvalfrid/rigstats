//! Global hotkey listener for showing/hiding the overlay without leaving
//! the game (issue #183) — the conventional in-game-overlay toggle
//! (Xbox Game Bar, Steam, MSI Afterburner, …). Fixed `Ctrl+Alt+O` binding in
//! v1 — no capture UI, no remapping, no third-party crate: `RegisterHotKey`
//! is a single Win32 call and this app already depends on `winapi`'s
//! `winuser` feature for other direct FFI (see `win32_behind.rs`).

#![allow(unsafe_code)]

use eframe::egui;
use rigstats_backend::debug;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use winapi::um::winuser::{
    GetMessageW, RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MSG, WM_HOTKEY,
};

const HOTKEY_ID: i32 = 1;
const VK_O: u32 = 0x4F;

/// Spawns a dedicated thread that registers the fixed Ctrl+Alt+O
/// show/hide-overlay toggle and sends a unit signal over `tx` each time it
/// fires. `RegisterHotKey`/`WM_HOTKEY` delivery is thread-affine — the
/// message queue belongs to whichever thread *registered* the hotkey — so
/// registration must happen on this same spawned thread, right before its
/// own blocking `GetMessageW` loop. (Registering from the caller's thread
/// and then polling from here would silently never see the messages: they'd
/// queue up on the caller's thread, which runs eframe's own event loop, not
/// this one.)
///
/// If the chord is already owned by another app, registration fails and
/// this logs it and the thread exits immediately — there is no conflict UI
/// in v1, the user just won't get the hotkey until whatever else is holding
/// it releases it.
///
/// `ctx` is used to wake the main eframe loop the instant the chord fires —
/// without it, the toggle sits in `tx`'s channel until the app's own ~1 fps
/// idle repaint happens to tick, adding up to a second of dead time before
/// the keypress is even noticed (same reason the tray thread in `main.rs`
/// calls `request_repaint()` right after sending its own command).
pub fn spawn(dir: PathBuf, tx: Sender<()>, ctx: egui::Context) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let registered = unsafe {
            RegisterHotKey(
                std::ptr::null_mut(),
                HOTKEY_ID,
                (MOD_CONTROL | MOD_ALT) as u32,
                VK_O,
            )
        };
        if registered == 0 {
            debug::log_warn(
                &dir,
                "hotkey: RegisterHotKey(Ctrl+Alt+O) failed — likely already bound by another app",
            );
            return;
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
            if msg.message == WM_HOTKEY && msg.wParam as i32 == HOTKEY_ID {
                let _ = tx.send(());
                ctx.request_repaint();
            }
        }
        unsafe {
            UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID);
        }
    })
}
