//! The update check + download, shared by the updater window's "Check for
//! Updates" button and the background check loop in `main.rs` (#225).
//!
//! One code path for both: they differ only in what reaches the window,
//! which [`Trigger`] decides. Blocking — call it from a worker thread or
//! `spawn_blocking`, never from the UI thread.

use crate::lock_ext::LockSafe;
use crate::update_check::{self, CheckResult, UpdateInfo, VerifiedInstaller};
use crate::windows::updater::{UpdateStatus, UpdaterState};
use std::sync::{Arc, Mutex};

/// Who started the check.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    /// The window's button: every outcome is shown.
    Manual,
    /// The startup / 6-hourly check: only a download (progress, ready,
    /// failed) reaches the window. "Up to date" and a failed check leave
    /// whatever it shows — e.g. "just updated" — alone.
    Background,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Another check was still running; nothing was done.
    Busy,
    UpToDate,
    /// The installer is downloaded and verified; the window shows it.
    Ready,
    Failed(String),
}

/// Checks for a newer version and, if there is one, downloads and verifies
/// its installer, keeping `win`'s status up to date as it goes.
pub fn run_check_and_download(
    win: &Mutex<UpdaterState>,
    ctx: &egui::Context,
    trigger: Trigger,
) -> Outcome {
    run_with(
        win,
        &|| ctx.request_repaint(),
        trigger,
        update_check::check,
        |info, progress| update_check::download(info, progress),
    )
}

fn run_with(
    win: &Mutex<UpdaterState>,
    repaint: &dyn Fn(),
    trigger: Trigger,
    check: impl FnOnce() -> Result<CheckResult, String>,
    download: impl FnOnce(&UpdateInfo, &dyn Fn(u64, u64)) -> Result<VerifiedInstaller, String>,
) -> Outcome {
    {
        let mut s = win.lock_safe();
        if s.busy {
            return Outcome::Busy;
        }
        s.busy = true;
    }

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        check_and_download(win, repaint, check, download)
    }))
    .unwrap_or_else(|_| Outcome::Failed("Update check failed unexpectedly".to_string()));

    {
        let mut s = win.lock_safe();
        s.busy = false;
        match &outcome {
            Outcome::UpToDate if trigger == Trigger::Manual => s.status = UpdateStatus::UpToDate,
            // A failed download is shown whoever started it — the window
            // would otherwise be left at "Downloading".
            Outcome::Failed(e)
                if trigger == Trigger::Manual
                    || matches!(s.status, UpdateStatus::Downloading { .. }) =>
            {
                s.status = UpdateStatus::Error(e.clone());
            }
            _ => {}
        }
    }
    repaint();
    outcome
}

fn check_and_download(
    win: &Mutex<UpdaterState>,
    repaint: &dyn Fn(),
    check: impl FnOnce() -> Result<CheckResult, String>,
    download: impl FnOnce(&UpdateInfo, &dyn Fn(u64, u64)) -> Result<VerifiedInstaller, String>,
) -> Outcome {
    let info = match check() {
        Ok(CheckResult::UpdateAvailable(info)) => info,
        Ok(CheckResult::UpToDate) => return Outcome::UpToDate,
        Err(e) => return Outcome::Failed(e),
    };
    let set_progress = |downloaded, total| {
        win.lock_safe().status = UpdateStatus::Downloading { downloaded, total };
        repaint();
    };
    set_progress(0, 0);
    match download(&info, &set_progress) {
        Ok(installer) => {
            win.lock_safe().status = UpdateStatus::Ready {
                info,
                installer: Arc::new(installer),
            };
            Outcome::Ready
        }
        Err(e) => Outcome::Failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> UpdateInfo {
        UpdateInfo {
            version: "9.9.9".into(),
            notes: String::new(),
            url: "https://example.invalid/setup.exe".into(),
            sha256: "00".into(),
        }
    }

    fn state(status: UpdateStatus) -> Mutex<UpdaterState> {
        let mut s = UpdaterState::default();
        s.status = status;
        Mutex::new(s)
    }

    fn up_to_date() -> Result<CheckResult, String> {
        Ok(CheckResult::UpToDate)
    }

    fn update_available() -> Result<CheckResult, String> {
        Ok(CheckResult::UpdateAvailable(info()))
    }

    fn check_fails() -> Result<CheckResult, String> {
        Err("offline".into())
    }

    fn no_download(_: &UpdateInfo, _: &dyn Fn(u64, u64)) -> Result<VerifiedInstaller, String> {
        panic!("download must not run")
    }

    fn download_fails(
        _: &UpdateInfo,
        progress: &dyn Fn(u64, u64),
    ) -> Result<VerifiedInstaller, String> {
        progress(10, 100);
        Err("checksum mismatch".into())
    }

    fn run(
        win: &Mutex<UpdaterState>,
        trigger: Trigger,
        check: impl FnOnce() -> Result<CheckResult, String>,
        download: impl FnOnce(&UpdateInfo, &dyn Fn(u64, u64)) -> Result<VerifiedInstaller, String>,
    ) -> Outcome {
        run_with(win, &|| {}, trigger, check, download)
    }

    #[test]
    fn manual_up_to_date_is_shown() {
        let win = state(UpdateStatus::Checking);
        assert_eq!(
            run(&win, Trigger::Manual, up_to_date, no_download),
            Outcome::UpToDate
        );
        assert!(matches!(win.lock_safe().status, UpdateStatus::UpToDate));
    }

    #[test]
    fn background_up_to_date_keeps_just_updated() {
        let win = state(UpdateStatus::JustUpdated {
            version: "1.44.0".into(),
        });
        assert_eq!(
            run(&win, Trigger::Background, up_to_date, no_download),
            Outcome::UpToDate
        );
        assert!(matches!(
            win.lock_safe().status,
            UpdateStatus::JustUpdated { .. }
        ));
    }

    #[test]
    fn manual_failed_check_is_shown() {
        let win = state(UpdateStatus::Checking);
        assert_eq!(
            run(&win, Trigger::Manual, check_fails, no_download),
            Outcome::Failed("offline".into())
        );
        assert!(matches!(&win.lock_safe().status, UpdateStatus::Error(e) if e == "offline"));
    }

    #[test]
    fn background_failed_check_leaves_the_window_alone() {
        let win = state(UpdateStatus::Idle);
        assert_eq!(
            run(&win, Trigger::Background, check_fails, no_download),
            Outcome::Failed("offline".into())
        );
        assert!(matches!(win.lock_safe().status, UpdateStatus::Idle));
    }

    #[test]
    fn failed_download_is_shown_for_both_triggers() {
        for trigger in [Trigger::Manual, Trigger::Background] {
            let win = state(UpdateStatus::Idle);
            assert_eq!(
                run(&win, trigger, update_available, download_fails),
                Outcome::Failed("checksum mismatch".into())
            );
            assert!(
                matches!(&win.lock_safe().status, UpdateStatus::Error(e) if e == "checksum mismatch"),
                "{trigger:?}"
            );
        }
    }

    #[test]
    fn download_progress_reaches_the_window() {
        let win = state(UpdateStatus::Idle);
        let seen = Mutex::new(Vec::new());
        run(
            &win,
            Trigger::Background,
            update_available,
            |_, progress| {
                progress(50, 100);
                if let UpdateStatus::Downloading { downloaded, total } = win.lock_safe().status {
                    seen.lock_safe().push((downloaded, total));
                }
                Err("stop".into())
            },
        );
        assert_eq!(*seen.lock_safe(), vec![(50, 100)]);
    }

    #[test]
    fn a_running_check_blocks_a_second_one() {
        let win = state(UpdateStatus::Checking);
        win.lock_safe().busy = true;
        assert_eq!(
            run(&win, Trigger::Manual, up_to_date, no_download),
            Outcome::Busy
        );
        assert!(matches!(win.lock_safe().status, UpdateStatus::Checking));
    }

    #[test]
    fn a_panic_becomes_an_error_and_frees_the_updater() {
        let win = state(UpdateStatus::Checking);
        let outcome = run(&win, Trigger::Manual, || panic!("boom"), no_download);
        assert_eq!(
            outcome,
            Outcome::Failed("Update check failed unexpectedly".into())
        );
        let s = win.lock_safe();
        assert!(matches!(s.status, UpdateStatus::Error(_)));
        assert!(!s.busy);
    }
}
