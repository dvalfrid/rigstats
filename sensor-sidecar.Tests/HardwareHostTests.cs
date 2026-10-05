using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// <see cref="HardwareHost.WithHardwareLockAsync{T}"/> is the async-safe
/// replacement for <c>SensorWorker</c>'s old <c>lock (_sampleLock)</c> — the
/// one property that actually matters is that it still serializes concurrent
/// callers (a fan write must never interleave with a Super I/O read). This
/// doesn't touch real hardware: the action delegate never calls
/// <c>Accept</c>/reads real sensors, so it's safe to run without
/// <c>HardwareHost.StartAsync</c> ever having opened the underlying
/// <c>Computer</c>.
/// </summary>
public class HardwareHostTests
{
    [Fact]
    public async Task WithHardwareLockAsync_serializes_concurrent_callers()
    {
        var host = new HardwareHost();
        var concurrent = 0;
        var maxConcurrent = 0;
        var gate = new object();

        async Task<int> Slow(LibreHardwareMonitor.Hardware.IComputer _)
        {
            lock (gate)
            {
                concurrent++;
                maxConcurrent = Math.Max(maxConcurrent, concurrent);
            }
            await Task.Delay(20);
            lock (gate) { concurrent--; }
            return 0;
        }

        var tasks = Enumerable.Range(0, 8)
            .Select(_ => host.WithHardwareLockAsync(c => Slow(c).GetAwaiter().GetResult(), CancellationToken.None))
            .ToArray();

        await Task.WhenAll(tasks);

        Assert.Equal(1, maxConcurrent);
    }

    [Fact]
    public async Task StopAsync_waits_for_an_in_flight_call_before_closing()
    {
        var host = new HardwareHost();
        using var entered = new ManualResetEventSlim();
        using var release = new ManualResetEventSlim();

        var inFlight = Task.Run(() => host.WithHardwareLockAsync(_ =>
        {
            entered.Set();
            release.Wait();
            return 0;
        }, CancellationToken.None));
        entered.Wait();

        var stop = host.StopAsync(CancellationToken.None);
        await Task.Delay(200);
        Assert.False(stop.IsCompleted);

        release.Set();
        await inFlight;
        await stop;
    }

    [Fact]
    public async Task Calls_after_StopAsync_throw_instead_of_touching_the_closed_computer()
    {
        var host = new HardwareHost();
        await host.StopAsync(CancellationToken.None);

        var touched = false;
        await Assert.ThrowsAsync<InvalidOperationException>(() =>
            host.WithHardwareLockAsync(_ => touched = true, CancellationToken.None));
        await Assert.ThrowsAsync<InvalidOperationException>(() =>
            host.GetTelemetryLineAsync(CancellationToken.None));
        Assert.False(touched);
    }

    [Fact]
    public async Task Many_concurrent_callers_share_one_sample_per_interval()
    {
        // #208: however many telemetry clients and service-side consumers ask,
        // LHM is read at most once per SampleMaxAgeMs (900 ms).
        var samples = 0;
        var host = new HardwareHost(() =>
        {
            Interlocked.Increment(ref samples);
            return new SensorPayload(null, null, [], [], null, [], [], [], null);
        });
        const int callers = 8;
        var until = DateTime.UtcNow + TimeSpan.FromSeconds(3);
        var calls = 0;

        await Task.WhenAll(Enumerable.Range(0, callers).Select(i => Task.Run(async () =>
        {
            while (DateTime.UtcNow < until)
            {
                if (i % 2 == 0)
                    await host.GetTelemetryLineAsync(CancellationToken.None);
                else
                    await host.GetSampleAsync(CancellationToken.None);
                Interlocked.Increment(ref calls);
                await Task.Delay(50);
            }
        })));

        // ~3 s / 900 ms → 4 samples (the first is immediate); far fewer than
        // the hundreds of calls made.
        Assert.InRange(samples, 3, 5);
        Assert.True(calls > callers * 20, $"only {calls} calls");
    }
}
