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
        public IReadOnlyList<Problem> Problems { get; set; } = [];
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
        // the hardware is read at most once per SampleMaxAgeMs (900 ms). The
        // clock is the test's: a wall-clock version wanted 160 calls in 3 s
        // and got 152 on a busy CI runner, which says nothing about sharing.
        long now = 10_000;
        var hardware = new FakeHardware();
        var host = new HardwareHost(hardware, clock: () => now);
        const int callers = 8, rounds = 4, callsEach = 10;

        for (var round = 1; round <= rounds; round++)
        {
            // One interval: 80 concurrent calls, one read of the hardware.
            await Task.WhenAll(Enumerable.Range(0, callers).Select(i => Task.Run(async () =>
            {
                for (var n = 0; n < callsEach; n++)
                {
                    if (i % 2 == 0)
                        await host.GetTelemetryLineAsync(CancellationToken.None);
                    else
                        await host.GetSampleAsync(CancellationToken.None);
                }
            })));
            Assert.Equal(round, hardware.Samples);
            now += 1_000; // past SampleMaxAgeMs: the next round reads again
        }
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
    public async Task A_hung_call_holding_the_hardware_does_not_hold_up_telemetry()
    {
        // Seen live (#374): a hung worker held the lock for its 8 s limit and
        // the app reconnected every 1.2 s meanwhile. Now telemetry gets the
        // last sample, with its age, right away.
        long now = 10_000;
        using var gate = new ManualResetEventSlim();
        var hardware = new FakeHardware();
        var host = new HardwareHost(hardware, clock: () => now);
        await host.GetTelemetryLineAsync(CancellationToken.None);

        hardware.Gate = gate;
        hardware.Entered.Reset();
        var hung = Task.Run(() => host.SetFan("/lpc/x/0/control/0", 40));
        hardware.Entered.Wait();
        now += 2_000;

        var started = DateTime.UtcNow;
        var line = JsonDocument.Parse(await host.GetTelemetryLineAsync(CancellationToken.None)).RootElement;

        Assert.True(DateTime.UtcNow - started < TimeSpan.FromSeconds(2), "telemetry waited for the hung call");
        Assert.Equal(2_000, line.GetProperty("sample_age_ms").GetInt64());
        gate.Set();
        await hung;
    }

    [Fact]
    public async Task The_client_whose_request_starts_a_hanging_sample_still_gets_a_line()
    {
        // Seen live (#374): with one app connected, its own request is the
        // one that samples — and it hung with the worker. The sample now runs
        // in the background; the request gets the last line with its age.
        long now = 10_000;
        using var gate = new ManualResetEventSlim();
        var hardware = new FakeHardware();
        var host = new HardwareHost(hardware, clock: () => now);
        await host.GetTelemetryLineAsync(CancellationToken.None);

        hardware.Gate = gate; // the next sample hangs
        now += 1_500;
        // The gate stays shut, so a request that waited for the sample would
        // never return; the generous bound only keeps a busy CI runner (a
        // starved thread pool) from failing it on timing.
        var line = JsonDocument.Parse(
            await host.GetTelemetryLineAsync(CancellationToken.None).WaitAsync(TimeSpan.FromSeconds(10))).RootElement;

        Assert.Equal(1_500, line.GetProperty("sample_age_ms").GetInt64());

        // The worker answers again: the next request gets a live line.
        gate.Set();
        hardware.Gate = null;
        now += 1_000;
        await Task.Delay(100);
        var live = JsonDocument.Parse(await host.GetTelemetryLineAsync(CancellationToken.None)).RootElement;
        Assert.False(live.TryGetProperty("sample_age_ms", out _));
    }

    [Fact]
    public async Task Hardware_left_out_is_on_the_telemetry_line_live_and_aged()
    {
        // #374: the app's Status shows it, from any telemetry line.
        long now = 10_000;
        var hardware = new FakeHardware();
        var host = new HardwareHost(hardware, clock: () => now);
        var clean = JsonDocument.Parse(await host.GetTelemetryLineAsync(CancellationToken.None)).RootElement;
        Assert.False(clean.TryGetProperty("hardware_problems", out _));

        hardware.Problems = [Control.Workers.HardwareProblems.Off("/gpu-nvidia/0", "RTX", "crashed 3 times")];
        hardware.SampleFails = () => new InvalidOperationException("restarting");
        now += 2_000;
        var aged = JsonDocument.Parse(await host.GetTelemetryLineAsync(CancellationToken.None)).RootElement;
        Assert.Equal("hardware_off", aged.GetProperty("hardware_problems")[0].GetProperty("id").GetString());

        hardware.SampleFails = () => null;
        now += 1_000;
        await Task.Delay(100);
        var live = JsonDocument.Parse(await host.GetTelemetryLineAsync(CancellationToken.None)).RootElement;
        Assert.Equal("RTX isn't read any more", live.GetProperty("hardware_problems")[0].GetProperty("title").GetString());
    }

    [Fact]
    public async Task Without_any_sample_yet_telemetry_fails()
    {
        var hardware = new FakeHardware { SampleFails = () => new InvalidOperationException("not started") };
        var host = new HardwareHost(hardware);

        await Assert.ThrowsAsync<InvalidOperationException>(() => host.GetTelemetryLineAsync(CancellationToken.None));
    }
}
