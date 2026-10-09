//! The background threads and tasks `main()` starts besides the poll loop:
//! GPU name detection, the Control Center connection, the tray event
//! thread, the floating-mode heartbeat and the update check.

use eframe::egui;
use rigstats_backend::{control, debug, hardware};
use rigstats_egui::lock_ext::LockSafe;
use rigstats_egui::tray::{gpu_choice_from_menu_id, profile_choice_from_menu_id, Tray, TrayCmd};
#[cfg(windows)]
use rigstats_egui::win_opacity;
use rigstats_egui::{update_flow, windows};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tray_icon::menu::{MenuEvent, MenuId};

/// GPU adapter list for the tray "GPU" submenu and Settings. Detected in
/// the background — it can take a second or more (WMI, or its PowerShell
/// fallback) and must never delay startup; `GpuMenu::poll` fills the
/// submenu in when it arrives. Also off the UI thread because WMI
/// initialises COM as MTA, which fails on the (winit/OLE STA) UI thread.
pub(crate) fn spawn_gpu_name_detection(
    dir: &Path,
    ctx: &egui::Context,
) -> mpsc::Receiver<Vec<String>> {
    let (gpu_names_tx, gpu_names_rx) = mpsc::channel::<Vec<String>>();
    let dir = dir.to_path_buf();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let names = hardware::detect_gpu_names();
        debug::log_debug(&dir, &format!("hardware: gpu_names={names:?}"));
        let _ = gpu_names_tx.send(names);
        ctx.request_repaint();
    });
    gpu_names_rx
}

/// Control Center (#187): `control_task` owns the duplex
/// `\\.\pipe\rigstats-control` connection on its own tokio task, the same
/// way `poll_loop` owns the telemetry pipe. Commands go in over an
/// async-native channel (the task awaits them); events come back over a
/// std channel the UI drains with `try_recv()`, mirroring `PollStats`.
/// Restarted after a panic (#219), on the same command channel.
pub(crate) fn spawn_control_task(
    runtime: &tokio::runtime::Runtime,
    dir: &Path,
) -> (
    tokio::sync::mpsc::Sender<control::ControlCmd>,
    mpsc::Receiver<control::ControlEvent>,
) {
    let (control_cmd_tx, control_cmd_rx) = tokio::sync::mpsc::channel::<control::ControlCmd>(8);
    let (control_event_tx, control_rx) = mpsc::channel::<control::ControlEvent>();
    let control_cmd_rx = Arc::new(tokio::sync::Mutex::new(control_cmd_rx));
    let control_dir = dir.to_path_buf();
    runtime.spawn(debug::supervise(
        dir.to_path_buf(),
        "Control Center connection",
        move || {
            control::control_task(
                control_cmd_rx.clone(),
                control_event_tx.clone(),
                control_dir.clone(),
                env!("CARGO_PKG_VERSION").to_string(),
            )
        },
    ));
    (control_cmd_tx, control_rx)
}

/// The tray menu's item ids, for turning a click into a [`TrayCmd`].
struct TrayMenuIds {
    quit: MenuId,
    settings: MenuId,
    about: MenuId,
    status: MenuId,
    history: MenuId,
    control: MenuId,
    updater: MenuId,
    docs: MenuId,
    lamp: MenuId,
    floating: MenuId,
    recording: MenuId,
    overlay: MenuId,
    overlay_lock: MenuId,
}

impl TrayMenuIds {
    fn of(tray: &Tray) -> Self {
        Self {
            quit: tray.quit_id.clone(),
            settings: tray.settings_id.clone(),
            about: tray.about_id.clone(),
            status: tray.status_id.clone(),
            history: tray.history_id.clone(),
            control: tray.control_id.clone(),
            updater: tray.updater_id.clone(),
            docs: tray.docs_id.clone(),
            lamp: tray.lamp_id.clone(),
            floating: tray.floating_id.clone(),
            recording: tray.recording_id.clone(),
            overlay: tray.overlay_id.clone(),
            overlay_lock: tray.overlay_lock_id.clone(),
        }
    }

    /// The command for a clicked menu item — `None` for Quit (handled on
    /// the tray thread itself) and for ids that aren't commands.
    fn command(&self, id: &MenuId) -> Option<TrayCmd> {
        if *id == self.floating {
            Some(TrayCmd::ToggleFloating)
        } else if *id == self.recording {
            Some(TrayCmd::ToggleRecording)
        } else if *id == self.overlay {
            Some(TrayCmd::ToggleOverlay)
        } else if *id == self.overlay_lock {
            Some(TrayCmd::ToggleOverlayLock)
        } else if *id == self.settings {
            Some(TrayCmd::OpenSettings)
        } else if *id == self.about {
            Some(TrayCmd::OpenAbout)
        } else if *id == self.status {
            Some(TrayCmd::OpenStatus)
        } else if *id == self.history {
            Some(TrayCmd::OpenHistory)
        } else if *id == self.control {
            Some(TrayCmd::OpenControlCenter)
        } else if *id == self.updater {
            Some(TrayCmd::OpenUpdater)
        } else if *id == self.docs {
            Some(TrayCmd::OpenDocs)
        } else if *id == self.lamp {
            Some(TrayCmd::ToggleLamp)
        } else if let Some(pref) = gpu_choice_from_menu_id(id) {
            Some(TrayCmd::SelectGpu(pref))
        } else {
            profile_choice_from_menu_id(id).map(TrayCmd::SelectProfile)
        }
    }
}

/// Polls tray menu events at 50 ms intervals on its own thread and hands
/// the commands to the UI thread, waking it with `request_repaint()`. Quit
/// is handled here directly with `process::exit`, so it is never delayed
/// by a missed repaint. `recording_active` mirrors
/// `RigStatsApp.recording_active` — see the field's doc comment and the
/// `force_repaint` call below for why (#177).
pub(crate) fn spawn_tray_event_thread(
    tray: &Tray,
    dir: &Path,
    ctx: &egui::Context,
    recording_active: Arc<AtomicBool>,
    tray_anchor: crate::app::tray_card::TrayAnchor,
) -> mpsc::Receiver<TrayCmd> {
    let (tray_tx, tray_rx) = mpsc::channel::<TrayCmd>();

    // Windows' `TrackPopupMenu` (shown on tray right-click) runs its own
    // nested modal message loop on *this* thread — winit's own event loop,
    // and therefore `RigStatsApp::ui()`, doesn't run at all while a context
    // menu is open. Dismissing the menu without picking an item (Escape /
    // click-away) never produces a `MenuEvent`, so nothing else reacts
    // afterward and whatever was mid-animation (the recording blink) could
    // stay frozen (#177).
    //
    // Tried and empirically confirmed *not* to fix it (logged evidence:
    // dozens of `ctx.request_repaint()` calls, made both from a background
    // polling thread and synchronously via `TrayIconEvent::set_event_handler`
    // right before the nested loop starts, produced zero subsequent frames):
    // `egui::Context::request_repaint()`/`request_repaint_of()`. This is a
    // confirmed upstream winit bug, not something specific to this app:
    // https://github.com/rust-windowing/winit/issues/4608 — "redraw_request
    // is ignored while the system popup menu is shown", open as of
    // 2026-09-24, no fix yet. Weekly upstream check: Claude Code routine
    // `trig_01G4L76vcP4am32VnF36tBUZ` (Mondays 08:00 UTC), comments on
    // #177 if winit/eframe ships a fix — check that thread before
    // re-investigating whether this workaround can be removed.
    //
    // This isn't actually new to this codebase either way —
    // `win_opacity::force_repaint`'s doc comment already notes
    // `request_repaint_of` "doesn't work reliably ... on Windows" for a
    // different deferred-viewport scenario, with the same fix used here:
    // skip egui's repaint-scheduling layer entirely and post a real
    // `WM_PAINT` straight to the window via `InvalidateRect`, which winit
    // turns into a `RedrawRequested` no matter what state its own repaint
    // bookkeeping thinks it's in.
    //
    // `tray_icon::TrayIcon` is `Rc<RefCell<..>>`-backed internally — not
    // `Send` — so this can't live on a background thread; `TrayIconEvent`'s
    // `set_event_handler` (vs. the polled `receiver()`) runs synchronously
    // on this thread for the right-click that *opens* the menu, before
    // `TrackPopupMenu` blocks it — the earliest possible point to act.
    //
    // The same handler feeds the tray hover card (#290, `app::tray_card`):
    // hovering stores the icon's rect, a click hides the card until the
    // pointer has left (the menu is opening), Leave resets it. The card also
    // checks the pointer itself each frame, since Leave doesn't arrive while
    // the menu is open.
    let card_watch = tray_anchor.clone();
    tray_icon::TrayIconEvent::set_event_handler(Some(move |event: tray_icon::TrayIconEvent| {
        use tray_icon::TrayIconEvent as E;
        match &event {
            E::Enter { rect, .. } | E::Move { rect, .. } => {
                tray_anchor.lock_safe().icon = Some([
                    rect.position.x as i32,
                    rect.position.y as i32,
                    rect.size.width as i32,
                    rect.size.height as i32,
                ]);
            }
            E::Click { .. } | E::DoubleClick { .. } => {
                tray_anchor.lock_safe().suppressed = true;
            }
            E::Leave { .. } => {
                *tray_anchor.lock_safe() = crate::app::tray_card::HoverState::default();
            }
            _ => {}
        }
        #[cfg(windows)]
        win_opacity::force_repaint(win_opacity::find_hwnd("RigStats"));
    }));

    let ids = TrayMenuIds::of(tray);
    let dir_tray = dir.to_path_buf();
    let ctx = ctx.clone();
    std::thread::spawn(move || loop {
        // Guard each iteration: a panic in a tray/Win32 call must not
        // silently kill this thread, or tray clicks would stop working
        // for the rest of the session. Recover and log instead.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut repaint = false;
            if let Ok(ev) = MenuEvent::receiver().try_recv() {
                // Use the foreground rights that come with the tray-menu interaction.
                // We immediately bring the (off-screen) parent window to the foreground
                // so that our process owns foreground when the dialog is created a few
                // milliseconds later.  Without this the dialog window would be created
                // as a background window and SetForegroundWindow would be refused.
                #[cfg(windows)]
                #[allow(unsafe_code)]
                unsafe {
                    winapi::um::winuser::AllowSetForegroundWindow(0xFFFF_FFFFu32); // ASFW_ANY
                    let parent_hwnd = win_opacity::find_hwnd("RigStats");
                    if parent_hwnd != 0 {
                        winapi::um::winuser::SetForegroundWindow(parent_hwnd as _);
                    }
                }

                if ev.id == ids.quit {
                    debug::append_debug_log(&dir_tray, "shutdown: clean (tray quit)");
                    std::process::exit(0);
                }
                if let Some(c) = ids.command(&ev.id) {
                    let _ = tray_tx.send(c);
                    repaint = true;
                }
            }
            // tray-icon click/hover events are handled synchronously via
            // `TrayIconEvent::set_event_handler` above (registering a
            // handler stops them from reaching this channel at all — see
            // that comment for why), so there is nothing left to drain
            // here.
            if repaint {
                ctx.request_repaint();
            }
            // While recording, keep posting a real repaint on every tick
            // (see `recording_active_shared`'s doc comment, #177) rather
            // than only reacting to tray events — a context menu can stay
            // open for an arbitrary, unbounded time, and neither egui's
            // repaint request nor a single pre-emptive force_repaint()
            // reliably survives that. This thread runs independently of
            // whatever nested Win32 loop may be blocking the main thread,
            // so the very next tick after the menu closes — whenever that
            // is — lands a real WM_PAINT no matter how it closed.
            if recording_active.load(Ordering::Relaxed) {
                #[cfg(windows)]
                win_opacity::force_repaint(win_opacity::find_hwnd("RigStats"));
            }
            // The tray hover card's guard, independent of the UI thread's
            // frames (see `tray_card::watchdog`).
            #[cfg(windows)]
            crate::app::tray_card::watchdog(&card_watch);
        }));
        if outcome.is_err() {
            debug::log_warn(
                &dir_tray,
                "tray: event handler panicked — thread recovered, continuing",
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    });
    tray_rx
}

/// Heartbeat: wakes the parent eframe loop at ~1 fps when floating mode is
/// active. With `show_viewport_immediate`, all panels are rendered
/// synchronously as part of the parent frame, so one `request_repaint()`
/// per second is enough to drive all panel updates.
pub(crate) fn spawn_floating_heartbeat(ctx: &egui::Context, floating: Arc<AtomicBool>) {
    let ctx = ctx.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(950));
        if floating.load(Ordering::Relaxed) {
            ctx.request_repaint();
        }
    });
}

/// The version from the `--just-updated=VERSION` argument the NSIS
/// `/autoupdate` installer passes, if any.
fn just_updated_version(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter()
        .find(|a| a.starts_with("--just-updated="))
        .and_then(|a| a.split_once('=').map(|x| x.1.to_owned()))
        .filter(|v| !v.is_empty())
}

/// The updater window's state, open flag and focus flag.
pub(crate) type UpdaterHandles = (
    Arc<Mutex<windows::updater::UpdaterState>>,
    Arc<AtomicBool>,
    Arc<AtomicBool>,
);

/// Opens the updater window as "just updated" when the installer started
/// this process, and runs the background update check: 10 s after
/// start-up, then every 6 h. When a newer version is found and downloaded,
/// it opens the updater window.
pub(crate) fn start_updater(
    runtime: &tokio::runtime::Runtime,
    dir: &Path,
    ctx: &egui::Context,
) -> UpdaterHandles {
    let updater_win: Arc<Mutex<windows::updater::UpdaterState>> =
        Arc::new(Mutex::new(windows::updater::UpdaterState::default()));
    let updater_open = Arc::new(AtomicBool::new(false));
    let updater_focus = Arc::new(AtomicBool::new(false));

    if let Some(version) = just_updated_version(std::env::args()) {
        updater_win.lock_safe().status = windows::updater::UpdateStatus::JustUpdated { version };
        updater_open.store(true, Ordering::Relaxed);
        updater_focus.store(true, Ordering::Relaxed);
    }

    let win = updater_win.clone();
    let open = updater_open.clone();
    let focus = updater_focus.clone();
    let ctx = ctx.clone();
    let dir_upd = dir.to_path_buf();
    runtime.spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        loop {
            let (win2, ctx2) = (win.clone(), ctx.clone());
            let outcome = tokio::task::spawn_blocking(move || {
                update_flow::run_check_and_download(&win2, &ctx2, update_flow::Trigger::Background)
            })
            .await
            .unwrap_or_else(|e| update_flow::Outcome::Failed(e.to_string()));
            match outcome {
                update_flow::Outcome::Ready => {
                    open.store(true, Ordering::Relaxed);
                    focus.store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                }
                update_flow::Outcome::Failed(e) => {
                    debug::log_warn(
                        &dir_upd,
                        &format!("update-check: background check failed — {e}"),
                    );
                }
                update_flow::Outcome::UpToDate | update_flow::Outcome::Busy => {}
            }
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
        }
    });

    (updater_win, updater_open, updater_focus)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> TrayMenuIds {
        let id = MenuId::new;
        TrayMenuIds {
            quit: id("quit"),
            settings: id("settings"),
            about: id("about"),
            status: id("status"),
            history: id("history"),
            control: id("control"),
            updater: id("updater"),
            docs: id("docs"),
            lamp: id("lamp"),
            floating: id("floating"),
            recording: id("recording"),
            overlay: id("overlay"),
            overlay_lock: id("overlay_lock"),
        }
    }

    #[test]
    fn every_menu_item_maps_to_its_command() {
        let ids = ids();
        let cases = [
            ("settings", TrayCmd::OpenSettings),
            ("about", TrayCmd::OpenAbout),
            ("status", TrayCmd::OpenStatus),
            ("history", TrayCmd::OpenHistory),
            ("control", TrayCmd::OpenControlCenter),
            ("updater", TrayCmd::OpenUpdater),
            ("docs", TrayCmd::OpenDocs),
            ("lamp", TrayCmd::ToggleLamp),
            ("floating", TrayCmd::ToggleFloating),
            ("recording", TrayCmd::ToggleRecording),
            ("overlay", TrayCmd::ToggleOverlay),
            ("overlay_lock", TrayCmd::ToggleOverlayLock),
        ];
        for (id, cmd) in cases {
            assert_eq!(ids.command(&MenuId::new(id)), Some(cmd), "{id}");
        }
    }

    #[test]
    fn quit_and_unknown_ids_are_not_commands() {
        let ids = ids();
        assert_eq!(ids.command(&MenuId::new("quit")), None);
        assert_eq!(ids.command(&MenuId::new("separator-7")), None);
    }

    #[test]
    fn profile_rows_carry_their_profile_id() {
        assert_eq!(
            ids().command(&MenuId::new("profile:id:gaming")),
            Some(TrayCmd::SelectProfile("gaming".into()))
        );
    }

    #[test]
    fn just_updated_version_is_read_from_the_arguments() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            just_updated_version(args(&["rigstats.exe", "--just-updated=1.45.0"])),
            Some("1.45.0".into())
        );
        assert_eq!(just_updated_version(args(&["rigstats.exe"])), None);
        assert_eq!(
            just_updated_version(args(&["rigstats.exe", "--just-updated="])),
            None
        );
    }
}
