using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// #231, #378: something that fails over and over (a refused app retrying, a
/// sensor read failing while the app reconnects) is logged once per reason
/// (and again after the interval), not per occurrence.
/// </summary>
public class RepeatLogTests
{
    private static readonly DateTimeOffset T0 = new(2026, 10, 5, 12, 0, 0, TimeSpan.Zero);

    private static RepeatLog Refusals() => new("Client rejected", "refused");

    [Fact]
    public void RepeatedRefusalsWithTheSameReasonLogOnce()
    {
        var log = Refusals();
        var lines = Enumerable.Range(0, 100)
            .Select(i => log.Line("not the installed rigstats.exe", T0.AddSeconds(2 * i)))
            .Where(l => l is not null)
            .ToList();

        Assert.Equal(["Client rejected: not the installed rigstats.exe"], lines);
    }

    [Fact]
    public void ADifferentReasonLogsAgain()
    {
        var log = Refusals();
        Assert.NotNull(log.Line("reason A", T0));
        Assert.Null(log.Line("reason A", T0.AddSeconds(2)));

        Assert.Equal("Client rejected: reason B", log.Line("reason B", T0.AddSeconds(4)));
    }

    [Fact]
    public void TheSameReasonLogsAgainAfterTheIntervalWithTheCount()
    {
        var log = Refusals();
        log.Line("reason A", T0);
        for (var i = 1; i <= 3; i++)
            Assert.Null(log.Line("reason A", T0.AddSeconds(2 * i)));

        Assert.Equal(
            "Client rejected: reason A (refused 3 more times since)",
            log.Line("reason A", T0 + RepeatLog.Interval));
        Assert.Null(log.Line("reason A", T0 + RepeatLog.Interval + TimeSpan.FromSeconds(2)));
    }

    [Fact]
    public void AnAcceptedClientMakesTheNextRefusalNews()
    {
        var log = Refusals();
        log.Line("reason A", T0);
        Assert.True(log.Active);
        Assert.True(log.Reset());

        Assert.False(log.Active);
        Assert.False(log.Reset()); // nothing held: no "works again" to say
        Assert.Equal("Client rejected: reason A", log.Line("reason A", T0.AddSeconds(2)));
    }

    [Fact]
    public void Occurrences_from_many_threads_log_one_line()
    {
        var log = new RepeatLog("Telemetry sample failed", "failed");
        var lines = new System.Collections.Concurrent.ConcurrentBag<string>();

        Parallel.For(0, 1000, _ =>
        {
            if (log.Line("LHM failed", T0) is { } line)
                lines.Add(line);
        });

        Assert.Single(lines);
    }
}
