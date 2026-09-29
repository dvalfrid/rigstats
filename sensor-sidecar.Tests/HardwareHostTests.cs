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
}
