using System.Text.Json;
using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// <see cref="HardwareHost"/> over a fake <see cref="ISensorHardware"/> (the
/// hardware worker in the service, #374): every call one at a time — a fan
/// write must never interleave with a Super I/O read — one sample per
/// interval for every consumer, a stop that waits for an in-flight call, and
/// a telemetry line that carries its age when no fresh sample can be had.
/// </summary>
public class HardwareHostTests
{
    private sealed class FakeHardware : ISensorHardware
    {
        private int _concurrent;
        private int _samples;
        public int MaxConcurrent;
        public int Samples => Volatile.Read(ref _samples);
        public bool Disposed { get; private set; }
        public Func<Exception?> SampleFails { get; set; } = () => null;
        public ManualResetEventSlim? Gate { get; set; }
        public ManualResetEventSlim Entered { get; } = new();

        public SensorPayload Sample()
        {
            using var _ = Enter();
            Interlocked.Increment(ref _samples);
            if (SampleFails() is { } e)
                throw e;
            return FanSamples.Temps(cpu: 50);
        }

        public string SensorTree() => "# tree";
        public IReadOnlyList<FanHeaderInfo> FanHeaders() => [];

        public void SetFan(string id, double duty)
        {
            using var _ = Enter();
            Thread.Sleep(20);
        }

        public void SetFanDefault(string id) { }
        public FanReadback ReadFan(string id) => new(true, true, 50);
        public void Dispose() => Disposed = true;

        private Scope Enter()
        {
            var now = Interlocked.Increment(ref _concurrent);
            InterlockedMax(ref MaxConcurrent, now);
            Entered.Set();
            Gate?.Wait();
            return new Scope(() => Interlocked.Decrement(ref _concurrent));
        }

        private static void InterlockedMax(ref int target, int value)
        {
            int seen;
            while ((seen = Volatile.Read(ref target)) < value && Interlocked.CompareExchange(ref target, value, seen) != seen) { }
        }

        private sealed class Scope(Action end) : IDisposable
        {
            public void Dispose() => end();
        }
    }

    [Fact]
    public async Task Samples_and_fan_writes_never_run_at_the_same_time()
    {
        var hardware = new FakeHardware();
        var host = new HardwareHost(hardware);

        await Task.WhenAll(Enumerable.Range(0, 8).Select(i => Task.Run(async () =>
        {
            if (i % 2 == 0)
                host.SetFan("/lpc/x/0/control/0", 40);
            else
                await host.GetSampleAsync(CancellationToken.None);
        })));

        Assert.Equal(1, hardware.MaxConcurrent);
    }

    [Fact]
    public async Task StopAsync_waits_for_an_in_flight_call_before_closing()
    {
        using var gate = new ManualResetEventSlim();
        var hardware = new FakeHardware { Gate = gate };
        var host = new HardwareHost(hardware);

        var inFlight = Task.Run(() => host.SetFan("/lpc/x/0/control/0", 40));
        hardware.Entered.Wait();

        var stop = host.StopAsync(CancellationToken.None);
        await Task.Delay(200);
        Assert.False(stop.IsCompleted);
        Assert.False(hardware.Disposed);

        gate.Set();
        await inFlight;
        await stop;
        Assert.True(hardware.Disposed);
    }

    [Fact]
    public async Task Calls_after_StopAsync_throw_instead_of_touching_the_closed_hardware()
    {
        var hardware = new FakeHardware();
        var host = new HardwareHost(hardware);
        await host.StopAsync(CancellationToken.None);

        Assert.Throws<InvalidOperationException>(() => host.SetFan("/lpc/x/0/control/0", 40));
        await Assert.ThrowsAsync<InvalidOperationException>(() => host.GetTelemetryLineAsync(CancellationToken.None));
        Assert.Equal(0, hardware.Samples);
    }

    [Fact]
    public async Task Many_concurrent_callers_share_one_sample_per_interval()
    {
        // #208: however many telemetry clients and service-side consumers ask,
        // the hardware is read at most once per SampleMaxAgeMs (900 ms).
        var hardware = new FakeHardware();
        var host = new HardwareHost(hardware);
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
        Assert.InRange(hardware.Samples, 3, 5);
        Assert.True(calls > callers * 20, $"only {calls} calls");
    }

    [Fact]
    public async Task Without_a_fresh_sample_telemetry_carries_the_last_one_with_its_age_and_fans_get_none()
    {
        // #374: the hardware worker restarting. The app sees how old the
        // sample is (and takes > 5 s as no data); the fan loop never decides
        // on an old temperature.
        long now = 10_000;
        var hardware = new FakeHardware();
        var host = new HardwareHost(hardware, clock: () => now);
        var live = JsonDocument.Parse(await host.GetTelemetryLineAsync(CancellationToken.None)).RootElement;
        Assert.False(live.TryGetProperty("sample_age_ms", out _));

        hardware.SampleFails = () => new InvalidOperationException("The hardware worker is restarting.");
        now += 6_000;

        var stale = JsonDocument.Parse(await host.GetTelemetryLineAsync(CancellationToken.None)).RootElement;
        Assert.Equal(6_000, stale.GetProperty("sample_age_ms").GetInt64());
        Assert.Equal(50, stale.GetProperty("cpu_temp").GetDouble());
        await Assert.ThrowsAsync<InvalidOperationException>(() => host.GetSampleAsync(CancellationToken.None));

        hardware.SampleFails = () => null;
        var back = JsonDocument.Parse(await host.GetTelemetryLineAsync(CancellationToken.None)).RootElement;
        Assert.False(back.TryGetProperty("sample_age_ms", out _));
    }

    [Fact]
    public async Task Without_any_sample_yet_telemetry_fails()
    {
        var hardware = new FakeHardware { SampleFails = () => new InvalidOperationException("not started") };
        var host = new HardwareHost(hardware);

        await Assert.ThrowsAsync<InvalidOperationException>(() => host.GetTelemetryLineAsync(CancellationToken.None));
    }
}
