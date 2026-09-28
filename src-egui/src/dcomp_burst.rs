//! Shared DComp per-pixel-transparency reveal-burst policy.
//!
//! Both the overlay (`render_overlay_viewport`) and each floating panel
//! (`render_floating_panels`) need the same fix for the same underlying
//! problem: `WS_EX_NOREDIRECTIONBITMAP`, applied once right after a
//! secondary `show_viewport_immediate` viewport is created, doesn't
//! reliably make DWM pick up per-pixel alpha — the swap chain only seems to
//! reconfigure after a genuine resize, and even then needs a few frames to
//! settle. Left visible during that transition, the window shows a white or
//! washed-out box until it self-corrects.
//!
//! The fix — confirmed by direct visual testing for both callers — is to
//! create/resize the window hidden, force a real resize, reapply the style
//! bit for a couple of frames while forcing fast repaints, then reveal only
//! once settled (or a safety timeout elapses, in case the HWND is never
//! found). [`DcompRevealBurst`] is the state machine for *when* to do each
//! of those steps; it never touches Win32 or egui itself; the caller reads
//! [`BurstTick`]'s flags and performs whatever Win32/`ViewportCommand` calls
//! its own context needs (they differ enough — main-vs-panel HWND tracking,
//! click-through handling, nudge amount — that a shared executor wasn't
//! worth the indirection, just the decision logic).

/// What a [`DcompRevealBurst::tick`] call wants the caller to do this frame.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct BurstTick {
    /// The HWND was just found for the first time this burst — the caller
    /// should send its own `ViewportCommand::InnerSize` nudge (a few px off
    /// the real target size) to force the genuine reconfigure the fix needs.
    pub just_started: bool,
    /// Call `win_opacity::set_no_redirection_bitmap(hwnd)` this frame.
    pub reapply_style: bool,
    /// Force a fast repaint this frame — `force_repaint(hwnd)`,
    /// `force_repaint(main_hwnd)`, and `ctx.request_repaint()`. True
    /// whenever still bursting or still waiting on the HWND to appear.
    pub force_repaint: bool,
    /// Send `ViewportCommand::Visible(true)` this frame — the burst settled,
    /// or the safety timeout was hit waiting for the HWND.
    pub reveal: bool,
}

/// Per-viewport reveal-burst state. One instance per window that needs this
/// treatment — the overlay owns a single one; floating mode keys one per
/// panel, since each visible panel is its own secondary viewport.
#[derive(Default)]
pub struct DcompRevealBurst {
    hwnd: isize,
    reapply_frames: u8,
    pending_reveal: bool,
    reveal_safety_frames: u8,
}

impl DcompRevealBurst {
    /// Frames to keep re-checking after the HWND is found. Only the first
    /// two do real work (the caller's resize nudge, then its snap-back
    /// landing the second reconfigure); the rest are margin for DWM to
    /// settle. Kept small deliberately: while hidden, each frame's
    /// paint/present cycle is throttled to a measured, fixed ~100 ms
    /// regardless of what work happens inside it (confirmed empirically —
    /// cutting the expensive `set_no_redirection_bitmap` calls from every
    /// frame down to just the first two made no difference to total burst
    /// time), almost certainly Windows/DXGI throttling paint delivery for
    /// invisible windows — so every extra margin frame here is ~100 ms of
    /// directly visible reveal delay, not a free safety net.
    const BURST_FRAMES: u8 = 6;
    /// Hard cap on how long the window may stay hidden waiting for the
    /// burst to complete, in case the HWND is never found.
    const SAFETY_FRAMES: u8 = 90;

    /// Call when a fresh window is about to be created (on activation, or a
    /// panel becoming visible) — the old HWND (if any) no longer exists, so
    /// this resets state to make the next `tick` treat it as unseen and
    /// start a fresh burst once the new one is found.
    pub fn start(&mut self, dcomp_available: bool) {
        self.hwnd = 0;
        self.reapply_frames = 0;
        self.pending_reveal = dcomp_available;
        self.reveal_safety_frames = Self::SAFETY_FRAMES;
    }

    /// Whether the window should stay hidden right now — feed the negation
    /// into `ViewportBuilder::with_visible`.
    pub fn pending_reveal(&self) -> bool {
        self.pending_reveal
    }

    /// The HWND found by the most recent `tick`, or 0 if not found yet (or
    /// DComp isn't available, in which case this is never populated — see
    /// `tick`'s fallback branch). The caller needs this to actually make the
    /// Win32 calls `BurstTick`'s flags ask for.
    pub fn hwnd(&self) -> isize {
        self.hwnd
    }

    /// Advance one frame. `find_hwnd` is only called while DComp is
    /// available and the HWND is still unknown, so a caller's lookup only
    /// runs on the frame(s) it's actually needed.
    pub fn tick(&mut self, find_hwnd: impl FnOnce() -> isize, dcomp_available: bool) -> BurstTick {
        if !dcomp_available {
            // Fallback path — the caller uses whole-window opacity instead
            // (its own `set_opacity` call, outside this state machine
            // entirely), so there's nothing for the burst to do.
            // `pending_reveal` is already false from `start` in this case,
            // so the window was never hidden to begin with.
            self.pending_reveal = false;
            return BurstTick::default();
        }
        let mut out = BurstTick::default();
        if self.hwnd == 0 {
            self.hwnd = find_hwnd();
            if self.hwnd != 0 {
                self.reapply_frames = Self::BURST_FRAMES;
                out.just_started = true;
            }
        }
        if self.hwnd != 0 {
            if self.reapply_frames > 0 {
                let frames_before_this_tick = self.reapply_frames;
                self.reapply_frames -= 1;
                out.reapply_style = frames_before_this_tick >= Self::BURST_FRAMES - 1;
                out.force_repaint = true;
            }
            if self.reapply_frames == 0 && self.pending_reveal {
                self.pending_reveal = false;
                out.reveal = true;
            }
        } else if self.pending_reveal {
            // HWND not found yet this frame (window still being created) —
            // the burst above hasn't started, so tick the independent
            // safety timeout instead of hiding forever if `find_hwnd` never
            // matches.
            self.reveal_safety_frames = self.reveal_safety_frames.saturating_sub(1);
            if self.reveal_safety_frames == 0 {
                self.pending_reveal = false;
                out.reveal = true;
            }
            out.force_repaint = true;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_dcomp_never_hides_or_bursts() {
        let mut b = DcompRevealBurst::default();
        b.start(false);
        assert!(!b.pending_reveal());
        let tick = b.tick(|| 123, false);
        assert_eq!(tick, BurstTick::default());
        assert!(!b.pending_reveal());
    }

    #[test]
    fn dcomp_available_hides_until_hwnd_found_then_starts_burst() {
        let mut b = DcompRevealBurst::default();
        b.start(true);
        assert!(b.pending_reveal(), "must hide immediately on start");

        // HWND not found yet — polls, stays hidden, doesn't reveal.
        let tick = b.tick(|| 0, true);
        assert!(!tick.just_started);
        assert!(!tick.reapply_style);
        assert!(tick.force_repaint, "must keep polling while waiting");
        assert!(!tick.reveal);
        assert!(b.pending_reveal());

        // HWND found — burst starts.
        let tick = b.tick(|| 42, true);
        assert!(tick.just_started);
        assert!(tick.reapply_style, "first burst frame always reapplies");
        assert!(tick.force_repaint);
        assert!(!tick.reveal, "burst just started, more frames to go");
        assert!(b.pending_reveal());
    }

    #[test]
    fn burst_reveals_after_exactly_burst_frames_and_only_reapplies_style_for_first_two() {
        let mut b = DcompRevealBurst::default();
        b.start(true);

        let mut reveal_tick = None;
        for i in 0..(DcompRevealBurst::BURST_FRAMES as usize + 2) {
            let tick = b.tick(|| 42, true);
            if i == 0 {
                assert!(tick.just_started, "hwnd found on the first tick");
            } else {
                assert!(!tick.just_started);
            }
            if i < 2 {
                assert!(tick.reapply_style, "first two ticks reapply the style bit");
            } else if reveal_tick.is_none() {
                assert!(
                    !tick.reapply_style,
                    "tick {i} past the first two must not reapply style"
                );
            }
            if tick.reveal {
                reveal_tick = Some(i);
                break;
            }
        }
        assert_eq!(
            reveal_tick,
            Some(DcompRevealBurst::BURST_FRAMES as usize - 1),
            "must reveal on the BURST_FRAMES-th tick (0-indexed) after the hwnd was found"
        );
        assert!(!b.pending_reveal());
    }

    #[test]
    fn hwnd_never_found_reveals_after_safety_timeout() {
        let mut b = DcompRevealBurst::default();
        b.start(true);
        for _ in 0..DcompRevealBurst::SAFETY_FRAMES - 1 {
            let tick = b.tick(|| 0, true);
            assert!(!tick.reveal, "must not reveal before the safety cap");
        }
        let last = b.tick(|| 0, true);
        assert!(last.reveal, "must reveal once the safety cap is hit");
        assert!(!b.pending_reveal());
    }

    #[test]
    fn start_resets_an_already_settled_burst_for_reuse() {
        let mut b = DcompRevealBurst::default();
        b.start(true);
        for _ in 0..DcompRevealBurst::BURST_FRAMES + 1 {
            b.tick(|| 7, true);
        }
        assert!(!b.pending_reveal(), "should have settled and revealed");

        // A fresh window (old HWND destroyed) — start() must make tick()
        // rediscover it and burst again, not silently skip because a
        // previous instance already recorded a size/hwnd.
        b.start(true);
        assert!(b.pending_reveal());
        let tick = b.tick(|| 99, true);
        assert!(tick.just_started, "must treat the new hwnd as unseen");
    }
}
