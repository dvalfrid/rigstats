//! Watches running programs for the app rules (#307) on a background
//! thread: every [`TICK`] it takes a snapshot (process names and start
//! times via sysinfo, the foreground window's program via Win32), runs
//! `profile_rules::RuleEngine` and, when a rule says so, switches profile
//! through the control pipe. Nothing here touches the UI thread; the
//! Control Center reads [`RulesShared`] for the status line and the "Add
//! app" picker.
#![allow(unsafe_code)] // EnumWindows / GetForegroundWindow FFI below.

use crate::lock_ext::LockSafe;
use rigstats_backend::control::ControlCmd;
use rigstats_backend::profile_rules::{RuleEngine, RuleStatus, RunningProgram, Snapshot};
use rigstats_backend::settings::Settings;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TICK: Duration = Duration::from_secs(2);
/// The running-programs list is gathered while the Control Center asked
/// for it within this long.
const LIST_WANTED_FOR: Duration = Duration::from_secs(5);

/// What the watcher and the rest of the app share.
#[derive(Debug, Default)]
pub struct RulesShared {
    /// Profile ids that exist (from the control pipe), so a rule for a
    /// deleted profile is ignored.
    pub profile_ids: Vec<String>,
    /// What the rules are doing now.
    pub status: RuleStatus,
    /// Programs with a window, for the Control Center's picker (sorted by
    /// file name); only kept up to date while asked for.
    pub running_apps: Vec<RunningApp>,
    /// When the Control Center last asked for `running_apps`.
    pub list_wanted_at: Option<Instant>,
}

pub type RulesHandle = Arc<Mutex<RulesShared>>;

/// A program with a window: its file name (lower case) and where it lives.
#[derive(Debug, Clone, PartialEq)]
pub struct RunningApp {
    pub exe: String,
    pub path: Option<String>,
}

/// Starts the watcher thread for the process lifetime.
pub fn spawn(
    settings: Arc<Mutex<Settings>>,
    active_profile: Arc<Mutex<Option<String>>>,
    shared: RulesHandle,
    cmd_tx: tokio::sync::mpsc::Sender<ControlCmd>,
    dir: PathBuf,
) {
    let _ = std::thread::Builder::new()
        .name("app-rules".into())
        .spawn(move || {
            let mut sys = sysinfo::System::new();
            let mut engine = RuleEngine::default();
            loop {
                std::thread::sleep(TICK);
                let rules = settings.lock_safe().app_rules.clone();
                let list_wanted = shared
                    .lock_safe()
                    .list_wanted_at
                    .is_some_and(|t| t.elapsed() < LIST_WANTED_FOR);
                if rules.is_empty() && !list_wanted {
                    engine.reset();
                    shared.lock_safe().status = RuleStatus::Idle;
                    continue;
                }
                // Paths only for the picker's icons, read once per process.
                let kind = if list_wanted {
                    sysinfo::ProcessRefreshKind::nothing()
                        .with_exe(sysinfo::UpdateKind::OnlyIfNotSet)
                } else {
                    sysinfo::ProcessRefreshKind::nothing()
                };
                // All processes; the ones that ended are removed, as 0.30 did.
                sys.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, kind);
                let now_unix = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let name_of = |pid: u32| {
                    sys.process(sysinfo::Pid::from_u32(pid))
                        .map(|p| p.name().to_string_lossy().to_ascii_lowercase())
                };
                let snap = Snapshot {
                    now_unix,
                    running: sys
                        .processes()
                        .values()
                        .map(|p| RunningProgram {
                            exe: p.name().to_string_lossy().to_ascii_lowercase(),
                            started_unix: p.start_time(),
                        })
                        .collect(),
                    foreground: foreground_pid().and_then(name_of),
                };
                if list_wanted {
                    let mut seen = HashSet::new();
                    let mut apps: Vec<RunningApp> = windowed_pids()
                        .into_iter()
                        .filter_map(|pid| sys.process(sysinfo::Pid::from_u32(pid)))
                        .map(|p| RunningApp {
                            exe: p.name().to_string_lossy().to_ascii_lowercase(),
                            path: p.exe().map(|x| x.display().to_string()),
                        })
                        .filter(|a| a.exe != "rigstats.exe" && a.exe != "explorer.exe")
                        .filter(|a| seen.insert(a.exe.clone()))
                        .collect();
                    apps.sort_by(|x, y| x.exe.cmp(&y.exe));
                    shared.lock_safe().running_apps = apps;
                }
                if rules.is_empty() {
                    continue;
                }
                let profile_ids = shared.lock_safe().profile_ids.clone();
                let ids: Vec<&str> = profile_ids.iter().map(String::as_str).collect();
                let active = active_profile.lock_safe().clone();
                if let Some(id) = engine.tick(&rules, &ids, active.as_deref(), &snap) {
                    let why = match engine.status() {
                        RuleStatus::Active { exe, .. } => format!("{exe} is running"),
                        _ => "the rule's program closed".to_owned(),
                    };
                    rigstats_backend::debug::log_debug(
                        &dir,
                        &format!("app rules: switching to profile '{id}' — {why}"),
                    );
                    let _ = cmd_tx.try_send(ControlCmd::ApplyProfile(id));
                }
                shared.lock_safe().status = engine.status();
            }
        });
}

/// The process that owns the foreground window.
#[cfg(windows)]
fn foreground_pid() -> Option<u32> {
    let hwnd = unsafe { winapi::um::winuser::GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { winapi::um::winuser::GetWindowThreadProcessId(hwnd, &mut pid) };
    (pid != 0).then_some(pid)
}

#[cfg(not(windows))]
fn foreground_pid() -> Option<u32> {
    None
}

/// Processes with a visible, titled top-level window — the programs a user
/// would recognise, for the picker.
#[cfg(windows)]
fn windowed_pids() -> HashSet<u32> {
    use winapi::shared::minwindef::{BOOL, LPARAM, TRUE};
    use winapi::shared::windef::HWND;

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let pids = &mut *(lparam as *mut HashSet<u32>);
        if winapi::um::winuser::IsWindowVisible(hwnd) != 0
            && winapi::um::winuser::GetWindowTextLengthW(hwnd) > 0
        {
            let mut pid = 0u32;
            winapi::um::winuser::GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != 0 {
                pids.insert(pid);
            }
        }
        TRUE
    }

    let mut pids = HashSet::new();
    unsafe {
        winapi::um::winuser::EnumWindows(Some(collect), &mut pids as *mut _ as LPARAM);
    }
    pids
}

#[cfg(not(windows))]
fn windowed_pids() -> HashSet<u32> {
    HashSet::new()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP, WS_VISIBLE,
    };

    /// #702: a process with a visible, titled window is in the list — the
    /// test makes one, off every screen.
    #[test]
    fn a_visible_titled_window_puts_its_process_in_the_list() {
        // SAFETY: "STATIC" is a system class (nothing to register); the
        // strings are static; no parent, menu, instance or parameter.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("rigstats-test-702"),
                WS_POPUP | WS_VISIBLE,
                -32000,
                -32000,
                10,
                10,
                None,
                None,
                None,
                None,
            )
        }
        .expect("a test window");
        let found = windowed_pids().contains(&std::process::id());
        // SAFETY: the window made above, on this thread.
        let _ = unsafe { DestroyWindow(hwnd) };
        assert!(found);
    }
}
