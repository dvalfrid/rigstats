using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// #383: start-up work nothing waits for runs in the background, never fails
/// the start, and a stop waits for it.
public class BackgroundStartupTaskTests
{
    [Fact]
    public async Task Start_returns_at_once_and_the_work_runs_in_the_background()
    {
        using var release = new ManualResetEventSlim();
        var done = false;
        var task = new BackgroundStartupTask("test", () =>
        {
            release.Wait(TimeSpan.FromSeconds(10));
            done = true;
        });

        var start = task.StartAsync(CancellationToken.None);
        Assert.True(start.IsCompletedSuccessfully);
        Assert.False(done);

        release.Set();
        await task.StopAsync(CancellationToken.None);
        Assert.True(done);
    }

    [Fact]
    public async Task Work_that_throws_never_fails_the_start_or_the_stop()
    {
        var task = new BackgroundStartupTask("test", () => throw new InvalidOperationException("event log gone"));

        await task.StartAsync(CancellationToken.None);
        await task.StopAsync(CancellationToken.None);
    }

    [Fact]
    public async Task A_stop_that_times_out_does_not_throw()
    {
        using var never = new ManualResetEventSlim();
        var task = new BackgroundStartupTask("test", () => never.Wait(TimeSpan.FromSeconds(10)));
        await task.StartAsync(CancellationToken.None);

        using var timeout = new CancellationTokenSource(TimeSpan.FromMilliseconds(100));
        await task.StopAsync(timeout.Token);
        never.Set();
    }
}
