using System.Runtime.CompilerServices;
using LibreHardwareMonitor.Hardware;
using NSubstitute;
using SensorSidecar.Control;

namespace SensorSidecar.Tests;

/// <summary>
/// A stateful stand-in for one LHM <see cref="IControl"/>: <c>SetSoftware</c>
/// claims the header and writes the PWM "register" the owning sensor's
/// <c>Value</c> reads back; <c>SetDefault</c> hands it back to firmware.
/// <see cref="SensorTreeLoader"/> attaches one to every <c>Control</c> sensor
/// in a dump; tests reach it with <see cref="Of"/>.
/// </summary>
public sealed class FakeFanHeader
{
    private static readonly ConditionalWeakTable<IControl, FakeFanHeader> ByControl = new();

    public ControlMode Mode { get; private set; } = ControlMode.Default;
    public float? Register { get; set; }
    public float Min { get; set; }
    public float Max { get; set; } = 100;
    /// Firmware that silently ignores software writes (register never moves).
    public bool FirmwareOverrides { get; set; }
    public List<float> Writes { get; } = [];
    public int Releases { get; private set; }
    public IControl Control { get; }

    public FakeFanHeader(float? register)
    {
        Register = register;
        Control = Substitute.For<IControl>();
        Control.ControlMode.Returns(_ => Mode);
        Control.MinSoftwareValue.Returns(_ => Min);
        Control.MaxSoftwareValue.Returns(_ => Max);
        Control.When(c => c.SetSoftware(Arg.Any<float>())).Do(call =>
        {
            var value = call.Arg<float>();
            Mode = ControlMode.Software;
            Writes.Add(value);
            if (!FirmwareOverrides)
                Register = value;
        });
        Control.When(c => c.SetDefault()).Do(_ =>
        {
            Mode = ControlMode.Default;
            Releases++;
        });
        ByControl.Add(Control, this);
    }

    public static FakeFanHeader Of(ISensor sensor) =>
        ByControl.TryGetValue(sensor.Control, out var header)
            ? header
            : throw new InvalidOperationException($"{sensor.Identifier} has no fake control.");
}

/// <summary>
/// <see cref="IHardwareHost"/> over a mocked tree — the lock is a no-op (tests
/// are single-threaded) and the sample is whatever the test sets.
/// </summary>
public sealed class FakeHardwareHost(IComputer computer) : IHardwareHost
{
    public SensorPayload Sample { get; set; } = FanSamples.Temps();

    /// When set, each sample is computed on demand (e.g. RPM that follows a
    /// fake header's PWM register) instead of returning `Sample`.
    public Func<SensorPayload>? SampleSource { get; set; }

    public Task<string> GetTelemetryLineAsync(CancellationToken ct) => throw new NotSupportedException();

    public Task<SensorPayload> GetSampleAsync(CancellationToken ct) => Task.FromResult(SampleSource?.Invoke() ?? Sample);

    public Task<T> WithHardwareLockAsync<T>(Func<IComputer, T> action, CancellationToken ct) =>
        Task.FromResult(action(computer));

    public void WriteSensorTree(string path) { }
}

public static class FanSamples
{
    /// A sample with just the temperatures fan curves read.
    public static SensorPayload Temps(float? cpu = 50, float? gpu = 45, params (string Label, float Celsius)[] mb) =>
        new(
            CpuTemp: cpu,
            CpuPower: null,
            GpuDevices: gpu is null
                ? []
                : [new GpuDevice("GPU", "test", null, gpu, null, null, null, null, null, null, null, null, null, null)],
            DiskTemps: [],
            RamTemp: null,
            MbFans: [],
            MbTemps: mb.Select(t => new MbTemp(t.Label, t.Celsius)).ToList(),
            MbVoltages: [],
            MbChip: null);

    /// A board with one Super I/O chip and `count` writable headers
    /// (`/lpc/nct6799d/0/control/<n>`), plus a GPU fan control that must
    /// never be picked up as a motherboard header.
    public static IComputer Board(int count)
    {
        var lines = new List<string>
        {
            "HW  Motherboard          id=/motherboard name=Test Board",
            "  SUB SuperIO            id=/lpc/nct6799d/0 name=Nuvoton NCT6799D",
        };
        for (var i = 0; i < count; i++)
            lines.Add($"    S  Control         id=/lpc/nct6799d/0/control/{i} name=Fan #{i + 1} val=40");
        lines.Add("HW  GpuAmd               id=/gpu-amd/0 name=Test GPU");
        lines.Add("  S  Control         id=/gpu-amd/0/control/0 name=GPU Fan val=0");
        return SensorTreeLoader.Load(lines);
    }

    public static string HeaderId(int n) => $"/lpc/nct6799d/0/control/{n}";

    public static FakeFanHeader Header(IComputer computer, int n) =>
        FakeFanHeader.Of(computer.Hardware
            .SelectMany(h => h.SubHardware)
            .SelectMany(s => s.Sensors)
            .Single(s => s.Identifier.ToString() == HeaderId(n)));
}
