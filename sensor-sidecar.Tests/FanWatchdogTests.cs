using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// #222, #374: a fan loop that stops ticking while curves are active first gets
/// the hardware worker restarted; only when that keeps not helping are the
/// fans handed back to firmware and the service failed, so the service
/// manager restarts it.
/// </summary>
public class FanWatchdogTests
{
    private sealed class Rig
    {
        public long Now { get; set; } = 1_000_000;
        public long LastTick { get; set; } = 1_000_000;
        public bool Active { get; set; } = true;
        public Action Release { get; set; } = () => { };
        public List<string> Calls { get; } = [];
        public FanWatchdog Watchdog { get; }

        public Rig(TimeSpan? releaseLimit = null) =>
            Watchdog = new FanWatchdog(
                () => Now,
                () => LastTick,
                () => Active,
                () =>
                {
                    lock (Calls) Calls.Add("release");
                    Release();
                },
                reason =>
                {
                    lock (Calls) Calls.Add("restart");
                },
                message =>
                {
                    lock (Calls) Calls.Add("fail-fast");
                },
                releaseLimit ?? TimeSpan.FromSeconds(3));

        /// Moves the clock in watchdog-sized steps, ticking the loop or not.
        public bool Advance(TimeSpan by, bool loopTicks)
        {
            var fired = false;
            for (var t = TimeSpan.Zero; t < by; t += TimeSpan.FromSeconds(2))
            {
                Now += 2000;
                if (loopTicks)
                    LastTick = Now;
                fired |= Watchdog.Check();
            }
            return fired;
        }
    }

    [Fact]
    public void No_action_while_the_loop_ticks()
    {
        var rig = new Rig();

        Assert.False(rig.Advance(TimeSpan.FromMinutes(1), loopTicks: true));
        Assert.Empty(rig.Calls);
    }

    [Fact]
    public void No_action_when_no_curve_is_active()
    {
        var rig = new Rig { Active = false };

        Assert.False(rig.Advance(TimeSpan.FromMinutes(1), loopTicks: false));
        Assert.Empty(rig.Calls);
    }

    [Fact]
    public void A_stall_restarts_the_hardware_worker_and_watches_on()
    {
        var rig = new Rig();

        Assert.False(rig.Advance(FanWatchdog.StallLimit - TimeSpan.FromSeconds(2), loopTicks: false));
        Assert.False(rig.Advance(TimeSpan.FromSeconds(2), loopTicks: false));

        Assert.Equal(["restart"], rig.Calls);
        // The new worker gets a full window; the loop ticks again.
        Assert.False(rig.Advance(TimeSpan.FromSeconds(30), loopTicks: true));
        Assert.Equal(["restart"], rig.Calls);
    }

    [Fact]
    public void Restarts_that_dont_help_end_in_release_and_fail_fast()
    {
        var rig = new Rig();

        // Never ticks again: each window ends in a restart, until the budget is spent.
        Assert.True(rig.Advance(FanWatchdog.StallLimit * (FanWatchdog.MaxRestarts + 1), loopTicks: false));

        Assert.Equal(["restart", "restart", "release", "fail-fast"], rig.Calls);
    }

    [Fact]
    public void Restarts_long_ago_dont_count()
    {
        var rig = new Rig();
        for (var i = 0; i < FanWatchdog.MaxRestarts + 2; i++)
        {
            Assert.False(rig.Advance(FanWatchdog.StallLimit, loopTicks: false));
            // Healthy for longer than the window before the next stall.
            Assert.False(rig.Advance(FanWatchdog.RestartWindow + TimeSpan.FromSeconds(10), loopTicks: true));
        }

        Assert.All(rig.Calls, c => Assert.Equal("restart", c));
    }

    [Fact]
    public void A_release_that_never_returns_still_ends_in_fail_fast()
    {
        using var hang = new ManualResetEventSlim();
        var rig = new Rig(releaseLimit: TimeSpan.FromMilliseconds(200)) { Release = () => hang.Wait() };

        Assert.True(rig.Advance(FanWatchdog.StallLimit * (FanWatchdog.MaxRestarts + 1), loopTicks: false));

        Assert.Equal(["restart", "restart", "release", "fail-fast"], rig.Calls);
        hang.Set();
    }

    [Fact]
    public void Waking_from_sleep_gives_the_loop_a_fresh_window()
    {
        var rig = new Rig();
        rig.Advance(TimeSpan.FromSeconds(4), loopTicks: true);

        // Asleep for an hour: neither the loop nor the watchdog ran.
        rig.Now += (long)TimeSpan.FromHours(1).TotalMilliseconds;
        Assert.False(rig.Watchdog.Check());

        // The loop is back: still nothing.
        Assert.False(rig.Advance(TimeSpan.FromSeconds(30), loopTicks: true));
        Assert.Empty(rig.Calls);
    }

    [Fact]
    public void A_loop_that_stays_stalled_after_waking_is_still_caught()
    {
        var rig = new Rig();
        rig.Now += (long)TimeSpan.FromHours(1).TotalMilliseconds;
        Assert.False(rig.Watchdog.Check());

        Assert.False(rig.Advance(FanWatchdog.StallLimit, loopTicks: false));
        Assert.Equal(["restart"], rig.Calls);
    }
}
