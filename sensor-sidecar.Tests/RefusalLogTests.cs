using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// #231: a refused app retries every few seconds, so the control pipe logs a
/// refusal once per reason (and again after the interval), not per retry.
/// </summary>
public class RefusalLogTests
{
    private static readonly DateTimeOffset T0 = new(2026, 10, 5, 12, 0, 0, TimeSpan.Zero);

    [Fact]
    public void RepeatedRefusalsWithTheSameReasonLogOnce()
    {
        var log = new RefusalLog();
        var lines = Enumerable.Range(0, 100)
            .Select(i => log.Refused("not the installed rigstats.exe", T0.AddSeconds(2 * i)))
            .Where(l => l is not null)
            .ToList();

        Assert.Equal(["Client rejected: not the installed rigstats.exe"], lines);
    }

    [Fact]
    public void ADifferentReasonLogsAgain()
    {
        var log = new RefusalLog();
        Assert.NotNull(log.Refused("reason A", T0));
        Assert.Null(log.Refused("reason A", T0.AddSeconds(2)));

        Assert.Equal("Client rejected: reason B", log.Refused("reason B", T0.AddSeconds(4)));
    }

    [Fact]
    public void TheSameReasonLogsAgainAfterTheIntervalWithTheCount()
    {
        var log = new RefusalLog();
        log.Refused("reason A", T0);
        for (var i = 1; i <= 3; i++)
            Assert.Null(log.Refused("reason A", T0.AddSeconds(2 * i)));

        Assert.Equal(
            "Client rejected: reason A (refused 3 more times since)",
            log.Refused("reason A", T0 + RefusalLog.Interval));
        Assert.Null(log.Refused("reason A", T0 + RefusalLog.Interval + TimeSpan.FromSeconds(2)));
    }

    [Fact]
    public void AnAcceptedClientMakesTheNextRefusalNews()
    {
        var log = new RefusalLog();
        log.Refused("reason A", T0);
        log.Accepted();

        Assert.Equal("Client rejected: reason A", log.Refused("reason A", T0.AddSeconds(2)));
    }
}
