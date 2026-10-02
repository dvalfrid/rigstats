using SensorSidecar.Control;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// <see cref="FanCurveLoop"/>'s per-tick safety behaviour — the issue's
/// acceptance criteria: critical-temp and sensor-loss overrides.
/// </summary>
public class FanCurveLoopTests
{
    private sealed class Rig
    {
        public FakeHardwareHost Host { get; }
        public FanProvider Provider { get; }
        public FanCurveLoop Loop { get; }
        public LibreHardwareMonitor.Hardware.IComputer Computer { get; }

        private readonly List<FanEvent> _events = [];

        public Rig(int headers = 2)
        {
            Computer = FanSamples.Board(headers);
            Host = new FakeHardwareHost(Computer);
            Provider = new FanProvider(Host, dryRun: false);
            Loop = new FanCurveLoop(Host, Provider);
            Loop.EventRaised += _events.Add;
        }

        public void Apply(params (int Header, string Source)[] headers) =>
            Provider.Apply(new ProfilePart
            {
                Fan = new FanPart
                {
                    Headers = headers.ToDictionary(
                        h => FanSamples.HeaderId(h.Header),
                        h => new FanHeaderConfig { Source = h.Source, Curve = [[40, 30], [80, 70]] }),
                },
            });

        public Task Tick() => Loop.TickAsync(CancellationToken.None);

        public float? Duty(int header) => FanSamples.Header(Computer, header).Register;

        public List<FanEvent> DrainEvents()
        {
            var events = _events.ToList();
            _events.Clear();
            return events;
        }
    }

    [Fact]
    public async Task Tick_follows_the_curve_of_each_header_source()
    {
        var rig = new Rig();
        rig.Apply((0, "cpu_package"), (1, "gpu"));

        rig.Host.Sample = FanSamples.Temps(cpu: 60, gpu: 70);
        await rig.Tick();

        Assert.Equal(50, rig.Duty(0));
        Assert.Equal(60, rig.Duty(1));
        var update = Assert.IsType<FanEvent.DutyUpdate>(rig.DrainEvents().Last());
        Assert.Equal(50, update.DutyByHeader[FanSamples.HeaderId(0)]);
    }

    [Fact]
    public async Task Critical_cpu_temperature_forces_every_header_to_full_speed_and_trips_once()
    {
        var rig = new Rig();
        rig.Apply((0, "cpu_package"), (1, "mb:System"));
        rig.Host.Sample = FanSamples.Temps(cpu: 97, gpu: 50, mb: ("System", 35));

        await rig.Tick();
        await rig.Tick();

        Assert.Equal(100, rig.Duty(0));
        Assert.Equal(100, rig.Duty(1));
        Assert.Single(rig.DrainEvents().OfType<FanEvent.SafetyTripped>());
    }

    [Fact]
    public async Task Critical_gpu_temperature_overrides_a_cpu_sourced_curve()
    {
        var rig = new Rig();
        rig.Apply((0, "cpu_package"));
        rig.Host.Sample = FanSamples.Temps(cpu: 40, gpu: 92);

        await rig.Tick();

        Assert.Equal(100, rig.Duty(0));
    }

    [Fact]
    public async Task Curves_resume_after_the_critical_condition_clears()
    {
        var rig = new Rig();
        rig.Apply((0, "cpu_package"));
        rig.Host.Sample = FanSamples.Temps(cpu: 97);
        await rig.Tick();

        rig.Host.Sample = FanSamples.Temps(cpu: 60);
        await rig.Tick();

        Assert.Equal(50, rig.Duty(0));
    }

    [Fact]
    public async Task Losing_the_source_sensor_forces_that_header_to_full_speed()
    {
        var rig = new Rig();
        rig.Apply((0, "gpu"), (1, "cpu_package"));
        rig.Host.Sample = FanSamples.Temps(cpu: 60, gpu: 60);
        await rig.Tick();

        rig.Host.Sample = FanSamples.Temps(cpu: 60, gpu: null);
        await rig.Tick();

        Assert.Equal(100, rig.Duty(0));
        Assert.Equal(50, rig.Duty(1));
    }

    [Fact]
    public async Task A_new_curve_is_not_held_back_by_the_previous_curves_hysteresis()
    {
        var rig = new Rig();
        rig.Apply((0, "cpu_package"));
        rig.Host.Sample = FanSamples.Temps(cpu: 60);
        await rig.Tick();
        Assert.Equal(50, rig.Duty(0));

        rig.Provider.Apply(new ProfilePart
        {
            Fan = new FanPart
            {
                Headers = new()
                {
                    [FanSamples.HeaderId(0)] = new FanHeaderConfig { Source = "cpu_package", Curve = [[40, 20], [80, 40]] },
                },
            },
        });
        rig.Host.Sample = FanSamples.Temps(cpu: 59);
        await rig.Tick();

        Assert.Equal(29.5, rig.Duty(0)!.Value, precision: 4);
    }

    [Fact]
    public async Task Released_headers_are_not_written_by_the_next_tick()
    {
        var rig = new Rig();
        rig.Apply((0, "cpu_package"));
        rig.Provider.ReleaseToFirmware();
        var writes = FanSamples.Header(rig.Computer, 0).Writes.Count;

        await rig.Tick();

        Assert.Equal(writes, FanSamples.Header(rig.Computer, 0).Writes.Count);
        Assert.Empty(rig.DrainEvents());
    }
}
