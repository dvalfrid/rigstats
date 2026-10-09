//! Wallpaper-mode supervisor (#225): decides, frame by frame, when to enter
//! and leave wallpaper mode and when to (re)spawn, reap or kill the
//! `rigstats-wallpaper` host process.
//!
//! The decisions live in [`WallpaperSupervisor`], which talks to the host
//! only through the [`Host`] trait and takes the time as an argument, so
//! they are unit-tested without processes or waiting. `main.rs` carries out
//! the window side of the returned [`Effect`]s.

use crate::poll::PollMode;
use std::time::{Duration, Instant};

/// A host that dies within this long of being spawned counts as a fast
/// failure (back off); one that lived longer died for an external reason,
/// e.g. an Explorer restart destroyed its WorkerW child (respawn promptly).
const FAST_FAIL: Duration = Duration::from_secs(4);
/// Grace period for the host to exit by itself after leaving the mode
/// before it is killed.
const TEARDOWN_GRACE: Duration = Duration::from_secs(3);
/// Consecutive fast failures after which respawning stops; toggling the
/// mode off and on resets it.
const MAX_FAILS: u32 = 5;

/// What [`Host::status`] reports about the host process.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HostStatus {
    /// No host process is held.
    None,
    Running,
    /// The held host has exited, with its exit code when there is one.
    Exited(Option<i32>),
}

/// The host process, as the supervisor sees it.
pub trait Host {
    fn status(&mut self) -> HostStatus;
    fn spawn(&mut self) -> Result<(), String>;
    /// Kills and reaps the held host, if any.
    fn kill(&mut self);
    /// Drops the handle of a host that has exited.
    fn forget(&mut self);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogLevel {
    Debug,
    Warn,
    Error,
}

/// What the caller has to do after a [`WallpaperSupervisor::step`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Effect {
    /// The host binary is missing: switch the layer back to normal, persist
    /// it and restore the window on-screen. The mode was not entered.
    AbortHostMissing,
    /// Entered the mode: hand the window's position to the host and park
    /// the window off-screen.
    Enter,
    /// Left the mode: restore the window on-screen.
    Leave,
    Log(LogLevel, String),
}

#[derive(Default)]
pub struct WallpaperSupervisor {
    /// True while wallpaper mode is active.
    active: bool,
    /// When the current host was spawned.
    last_spawn: Option<Instant>,
    /// Consecutive fast spawn failures — drives the exponential backoff.
    spawn_fails: u32,
    /// Set when leaving the mode while the host is alive: it exits by itself
    /// (the layer change is already on disk), and `kill()` only follows
    /// after [`TEARDOWN_GRACE`].
    teardown_at: Option<Instant>,
}

impl WallpaperSupervisor {
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// In wallpaper mode the host owns the dashboard; this app polls lightly
    /// for what it still shows or checks itself — the overlay, the tray hover
    /// card and the alerts (#299).
    pub fn poll_mode(&self) -> PollMode {
        if self.active {
            PollMode::Light
        } else {
            PollMode::Full
        }
    }

    /// Enters or leaves the mode. `want` is whether wallpaper mode should be
    /// on; `host_missing` is only asked when entering. The caller applies
    /// the returned effects — on [`Effect::Enter`] that saves the position
    /// the host reads at start-up — before calling [`Self::supervise`],
    /// which spawns the host.
    pub fn transition(
        &mut self,
        want: bool,
        host_missing: impl FnOnce() -> bool,
        host: &mut dyn Host,
        now: Instant,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        if want && !self.active {
            // Parking the window while no host can appear would hide the
            // dashboard entirely (a packaging regression, e.g. the installer
            // not bundling the host) — stay in normal mode instead.
            if host_missing() {
                effects.push(Effect::Log(
                    LogLevel::Error,
                    "wallpaper: rigstats-wallpaper.exe not found next to the app — \
                     staying in normal mode (reinstall/update to restore wallpaper mode)"
                        .into(),
                ));
                effects.push(Effect::AbortHostMissing);
                return effects;
            }
            self.active = true;
            // A fresh session: reset the throttle, and reap a host still
            // draining from a just-left session so two never run at once.
            self.spawn_fails = 0;
            self.last_spawn = None;
            self.teardown_at = None;
            host.kill();
            effects.push(Effect::Enter);
            effects.push(Effect::Log(
                LogLevel::Debug,
                "wallpaper: entering — parking main window".into(),
            ));
        } else if !want && self.active {
            self.active = false;
            // Graceful teardown: never TerminateProcess the host right away —
            // repeatedly killing the GPU host leaked graphics/desktop-heap
            // resources until a later spawn failed at DLL-init (0xC0000142).
            if host.status() != HostStatus::None {
                self.teardown_at = Some(now);
            }
            effects.push(Effect::Leave);
            effects.push(Effect::Log(
                LogLevel::Debug,
                "wallpaper: leaving — restoring main window".into(),
            ));
        }

        effects
    }

    /// Looks after the host: (re)spawns it while active, reaps or kills it
    /// after leaving. Returns only [`Effect::Log`]s.
    pub fn supervise(&mut self, host: &mut dyn Host, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.active {
            self.supervise_host(host, now, &mut effects);
        } else {
            self.drain_host(host, now, &mut effects);
        }
        effects
    }

    /// After leaving: reap the host once it has exited by itself, kill it
    /// if it overruns the grace period.
    fn drain_host(&mut self, host: &mut dyn Host, now: Instant, effects: &mut Vec<Effect>) {
        let Some(since) = self.teardown_at else {
            return;
        };
        if host.status() != HostStatus::Running {
            host.forget();
            self.teardown_at = None;
            self.last_spawn = None;
            effects.push(Effect::Log(
                LogLevel::Debug,
                "wallpaper: host exited cleanly".into(),
            ));
        } else if now.duration_since(since) >= TEARDOWN_GRACE {
            host.kill();
            self.teardown_at = None;
            self.last_spawn = None;
            effects.push(Effect::Log(
                LogLevel::Warn,
                "wallpaper: host did not exit in time, killed".into(),
            ));
        }
    }

    /// While active: (re)spawn the host when it isn't running, backing off
    /// exponentially after fast failures.
    fn supervise_host(&mut self, host: &mut dyn Host, now: Instant, effects: &mut Vec<Effect>) {
        let exit_code = match host.status() {
            HostStatus::Running => return,
            HostStatus::None => None,
            HostStatus::Exited(code) => {
                host.forget();
                // A loader failure such as 0xC0000142 kills the host before
                // its main() runs, so it can never log the cause itself —
                // this exit code is the only trace that reaches a log.
                Some(match code {
                    Some(c) => format!("exit code {c} (0x{:08X})", c as u32),
                    None => "no exit code".to_string(),
                })
            }
        };
        if let Some(code_desc) = exit_code {
            let fast_fail = self
                .last_spawn
                .map_or(true, |t| now.duration_since(t) < FAST_FAIL);
            if fast_fail {
                self.spawn_fails = self.spawn_fails.saturating_add(1);
                effects.push(Effect::Log(
                    LogLevel::Warn,
                    format!(
                        "wallpaper: host exited within 4s of spawn ({code_desc}); \
                         consecutive fast failures: {}",
                        self.spawn_fails
                    ),
                ));
            } else {
                self.spawn_fails = 0;
                effects.push(Effect::Log(
                    LogLevel::Debug,
                    format!("wallpaper: host exited after running ({code_desc}); respawning"),
                ));
            }
        }

        if self.spawn_fails >= MAX_FAILS {
            if self.spawn_fails == MAX_FAILS {
                self.spawn_fails += 1; // log the giving-up once
                effects.push(Effect::Log(
                    LogLevel::Error,
                    "wallpaper: host failed to start repeatedly — giving up \
                     (toggle wallpaper mode off and on to retry)"
                        .into(),
                ));
            }
            return;
        }
        // 0 s, 2 s, 4 s, 8 s … capped at 30 s.
        let backoff = if self.spawn_fails == 0 {
            Duration::ZERO
        } else {
            Duration::from_secs(2u64.saturating_pow(self.spawn_fails).min(30))
        };
        if self
            .last_spawn
            .is_some_and(|t| now.duration_since(t) < backoff)
        {
            return;
        }
        self.last_spawn = Some(now);
        match host.spawn() {
            Ok(()) => effects.push(Effect::Log(
                LogLevel::Debug,
                "wallpaper: spawned host process".into(),
            )),
            Err(e) => {
                self.spawn_fails = self.spawn_fails.saturating_add(1);
                effects.push(Effect::Log(
                    LogLevel::Error,
                    format!("wallpaper: spawn host failed — {e}"),
                ));
            }
        }
    }
}

/// Absolute path to the sibling `rigstats-wallpaper.exe`, or `None` if the
/// running exe's own path can't be resolved.
pub fn host_path() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("rigstats-wallpaper.exe"))
}

/// The real [`Host`]: the `rigstats-wallpaper` process, spawned with this
/// process's PID so it exits if the main app goes away.
#[derive(Default)]
pub struct ChildHost {
    child: Option<std::process::Child>,
}

impl Host for ChildHost {
    fn status(&mut self) -> HostStatus {
        match self.child.as_mut().map(std::process::Child::try_wait) {
            None => HostStatus::None,
            Some(Ok(None)) => HostStatus::Running,
            Some(Ok(Some(status))) => HostStatus::Exited(status.code()),
            Some(Err(_)) => HostStatus::Exited(None),
        }
    }

    fn spawn(&mut self) -> Result<(), String> {
        let host = host_path().ok_or("could not resolve current exe path")?;
        let child = std::process::Command::new(host)
            .env("RIGSTATS_PARENT_PID", std::process::id().to_string())
            .spawn()
            .map_err(|e| e.to_string())?;
        self.child = Some(child);
        Ok(())
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn forget(&mut self) {
        self.child = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scripted host: `status` reports `state`, `spawn` succeeds unless
    /// `spawn_error` is set and starts a running host.
    #[derive(Default)]
    struct FakeHost {
        state: Option<HostStatus>,
        spawn_error: Option<String>,
        spawns: u32,
        kills: u32,
    }

    impl FakeHost {
        fn running() -> Self {
            Self {
                state: Some(HostStatus::Running),
                ..Self::default()
            }
        }
        fn exit(&mut self, code: Option<i32>) {
            self.state = Some(HostStatus::Exited(code));
        }
    }

    impl Host for FakeHost {
        fn status(&mut self) -> HostStatus {
            self.state.unwrap_or(HostStatus::None)
        }
        fn spawn(&mut self) -> Result<(), String> {
            self.spawns += 1;
            match &self.spawn_error {
                Some(e) => Err(e.clone()),
                None => {
                    self.state = Some(HostStatus::Running);
                    Ok(())
                }
            }
        }
        fn kill(&mut self) {
            if self.state.take().is_some() {
                self.kills += 1;
            }
        }
        fn forget(&mut self) {
            self.state = None;
        }
    }

    impl WallpaperSupervisor {
        /// One frame, as `main.rs` runs it: transition, then supervise.
        fn step(
            &mut self,
            want: bool,
            host_missing: impl FnOnce() -> bool,
            host: &mut dyn Host,
            now: Instant,
        ) -> Vec<Effect> {
            let mut effects = self.transition(want, host_missing, host, now);
            effects.extend(self.supervise(host, now));
            effects
        }
    }

    fn secs(t0: Instant, s: u64) -> Instant {
        t0 + Duration::from_secs(s)
    }

    fn has(effects: &[Effect], e: &Effect) -> bool {
        effects.contains(e)
    }

    fn logs(effects: &[Effect], level: LogLevel) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Log(l, m) if *l == level => Some(m.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn entering_parks_the_window_and_spawns_the_host() {
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::default(),
            Instant::now(),
        );
        let fx = sup.step(true, || false, &mut host, t0);
        assert!(has(&fx, &Effect::Enter));
        assert!(sup.is_active());
        assert_eq!(host.spawns, 1);
        assert_eq!(host.status(), HostStatus::Running);
    }

    #[test]
    fn entering_spawns_only_in_supervise() {
        // The caller saves the position the host reads between the two.
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::default(),
            Instant::now(),
        );
        let fx = sup.transition(true, || false, &mut host, t0);
        assert!(has(&fx, &Effect::Enter));
        assert_eq!(host.spawns, 0);
        sup.supervise(&mut host, t0);
        assert_eq!(host.spawns, 1);
    }

    #[test]
    fn a_missing_host_binary_aborts_without_entering() {
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::default(),
            Instant::now(),
        );
        let fx = sup.step(true, || true, &mut host, t0);
        assert!(has(&fx, &Effect::AbortHostMissing));
        assert!(!has(&fx, &Effect::Enter));
        assert!(!sup.is_active());
        assert_eq!(host.spawns, 0);
        assert_eq!(logs(&fx, LogLevel::Error).len(), 1);
    }

    #[test]
    fn host_missing_is_only_checked_when_entering() {
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::default(),
            Instant::now(),
        );
        sup.step(true, || false, &mut host, t0);
        sup.step(true, || panic!("asked again"), &mut host, secs(t0, 1));
        sup.step(false, || panic!("asked on leave"), &mut host, secs(t0, 2));
    }

    #[test]
    fn a_running_host_is_left_alone() {
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::default(),
            Instant::now(),
        );
        sup.step(true, || false, &mut host, t0);
        let fx = sup.step(true, || false, &mut host, secs(t0, 60));
        assert!(fx.is_empty());
        assert_eq!(host.spawns, 1);
    }

    #[test]
    fn a_host_that_exits_after_running_is_respawned_at_once() {
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::default(),
            Instant::now(),
        );
        sup.step(true, || false, &mut host, t0);
        host.exit(Some(1)); // e.g. Explorer restarted
        let fx = sup.step(true, || false, &mut host, secs(t0, 600));
        assert_eq!(host.spawns, 2);
        assert!(logs(&fx, LogLevel::Debug)
            .iter()
            .any(|m| m.contains("exited after running (exit code 1 (0x00000001))")));
        assert!(logs(&fx, LogLevel::Warn).is_empty());
    }

    #[test]
    fn fast_failures_back_off_exponentially_then_give_up() {
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::default(),
            Instant::now(),
        );
        sup.step(true, || false, &mut host, t0);
        let mut now = t0;
        // Each host dies right away; the next spawn waits 2, 4, 8, 16 s.
        for (fails, wait) in [(1u32, 2u64), (2, 4), (3, 8), (4, 16)] {
            host.exit(Some(0xC000_0142_u32 as i32));
            let fx = sup.step(true, || false, &mut host, now);
            assert!(logs(&fx, LogLevel::Warn)[0]
                .ends_with(&format!("consecutive fast failures: {fails}")));
            let spawns = host.spawns;
            sup.step(
                true,
                || false,
                &mut host,
                now + Duration::from_secs(wait - 1),
            );
            assert_eq!(host.spawns, spawns, "spawned before the {wait}s backoff");
            now += Duration::from_secs(wait);
            sup.step(true, || false, &mut host, now);
            assert_eq!(
                host.spawns,
                spawns + 1,
                "not spawned after the {wait}s backoff"
            );
        }
        // Fifth fast failure: give up, logged once.
        host.exit(None);
        let fx = sup.step(true, || false, &mut host, now);
        assert_eq!(logs(&fx, LogLevel::Error).len(), 1);
        let spawns = host.spawns;
        let fx = sup.step(true, || false, &mut host, secs(now, 3600));
        assert!(fx.is_empty());
        assert_eq!(host.spawns, spawns);
    }

    #[test]
    fn re_entering_resets_the_backoff() {
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::default(),
            Instant::now(),
        );
        sup.step(true, || false, &mut host, t0);
        host.exit(None);
        sup.step(true, || false, &mut host, t0); // fast failure #1, backing off
        sup.step(false, || false, &mut host, secs(t0, 1));
        let spawns = host.spawns;
        sup.step(true, || false, &mut host, secs(t0, 1));
        assert_eq!(
            host.spawns,
            spawns + 1,
            "a fresh session spawns without backoff"
        );
    }

    #[test]
    fn a_failed_spawn_counts_as_a_fast_failure() {
        let mut host = FakeHost {
            spawn_error: Some("not found".into()),
            ..FakeHost::default()
        };
        let (mut sup, t0) = (WallpaperSupervisor::default(), Instant::now());
        let fx = sup.step(true, || false, &mut host, t0);
        assert_eq!(
            logs(&fx, LogLevel::Error),
            ["wallpaper: spawn host failed — not found"]
        );
        sup.step(true, || false, &mut host, secs(t0, 1));
        assert_eq!(host.spawns, 1, "backs off 2 s after a failed spawn");
        sup.step(true, || false, &mut host, secs(t0, 2));
        assert_eq!(host.spawns, 2);
    }

    #[test]
    fn leaving_lets_the_host_exit_by_itself() {
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::default(),
            Instant::now(),
        );
        sup.step(true, || false, &mut host, t0);
        let fx = sup.step(false, || false, &mut host, secs(t0, 10));
        assert!(has(&fx, &Effect::Leave));
        assert!(!sup.is_active());
        assert_eq!(
            host.kills, 0,
            "not killed while it can still exit by itself"
        );
        host.exit(Some(0));
        let fx = sup.step(false, || false, &mut host, secs(t0, 11));
        assert_eq!(
            logs(&fx, LogLevel::Debug),
            ["wallpaper: host exited cleanly"]
        );
        assert_eq!(host.kills, 0);
        assert_eq!(host.status(), HostStatus::None);
    }

    #[test]
    fn a_host_that_overruns_the_grace_period_is_killed() {
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::default(),
            Instant::now(),
        );
        sup.step(true, || false, &mut host, t0);
        sup.step(false, || false, &mut host, secs(t0, 10));
        sup.step(false, || false, &mut host, secs(t0, 12));
        assert_eq!(host.kills, 0);
        let fx = sup.step(false, || false, &mut host, secs(t0, 13));
        assert_eq!(host.kills, 1);
        assert_eq!(logs(&fx, LogLevel::Warn).len(), 1);
    }

    #[test]
    fn re_entering_while_the_old_host_drains_kills_it_first() {
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::default(),
            Instant::now(),
        );
        sup.step(true, || false, &mut host, t0);
        sup.step(false, || false, &mut host, secs(t0, 10));
        sup.step(true, || false, &mut host, secs(t0, 11)); // toggled back quickly
        assert_eq!(host.kills, 1);
        assert_eq!(host.spawns, 2);
        assert_eq!(host.status(), HostStatus::Running);
    }

    #[test]
    fn leaving_without_a_host_has_nothing_to_drain() {
        let mut host = FakeHost {
            spawn_error: Some("x".into()),
            ..FakeHost::default()
        };
        let (mut sup, t0) = (WallpaperSupervisor::default(), Instant::now());
        sup.step(true, || false, &mut host, t0);
        let fx = sup.step(false, || false, &mut host, secs(t0, 1));
        assert_eq!(fx.len(), 2, "{fx:?}"); // Leave + its log, no drain log
    }

    #[test]
    fn poll_mode_is_light_in_wallpaper_mode_and_full_outside() {
        let (mut sup, mut host, t0) = (
            WallpaperSupervisor::default(),
            FakeHost::running(),
            Instant::now(),
        );
        assert_eq!(sup.poll_mode(), PollMode::Full);
        sup.step(true, || false, &mut host, t0);
        // Never paused: the main app still feeds the overlay, the tray hover
        // card and the alerts while the host draws the dashboard (#299).
        assert_eq!(sup.poll_mode(), PollMode::Light);
        sup.step(false, || false, &mut host, secs(t0, 1));
        assert_eq!(sup.poll_mode(), PollMode::Full);
    }
}
