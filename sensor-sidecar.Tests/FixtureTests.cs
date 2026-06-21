using LibreHardwareMonitor.Hardware;
using SensorSidecar;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Auto-discovers every fixture under <c>fixtures/</c> that contains a
/// <c>sensor-tree.txt</c>, rebuilds the LHM tree, and runs the real
/// <see cref="SensorReader.Extract"/> against it. Adding a fixture folder adds
/// coverage with no new code. See fixtures/README.md.
/// </summary>
public class FixtureTests
{
    private static string FixturesRoot =>
        Path.Combine(AppContext.BaseDirectory, "fixtures");

    public static IEnumerable<object[]> Fixtures()
    {
        if (!Directory.Exists(FixturesRoot))
            yield break;
        foreach (var dir in Directory.EnumerateDirectories(FixturesRoot))
        {
            if (File.Exists(Path.Combine(dir, "sensor-tree.txt")))
                yield return [Path.GetFileName(dir)];
        }
    }

    [Theory]
    [MemberData(nameof(Fixtures))]
    public void Extract_satisfies_invariants_for_real_sensor_tree(string slug)
    {
        var treePath = Path.Combine(FixturesRoot, slug, "sensor-tree.txt");
        var computer = SensorTreeLoader.LoadFile(treePath);

        // 1. Must not throw on real hardware layouts.
        var payload = SensorReader.Extract(computer);

        // 2. One GpuDevice per GPU hardware node.
        var gpuHwCount = computer.Hardware.Count(h =>
            h.HardwareType is HardwareType.GpuNvidia or HardwareType.GpuAmd or HardwareType.GpuIntel);
        Assert.Equal(gpuHwCount, payload.GpuDevices.Count);

        // 3. Filtering contracts hold for everything that survived extraction.
        Assert.All(payload.MbFans, f => Assert.True(f.Rpm > 0, $"{slug}: fan '{f.Label}' rpm must be > 0"));
        Assert.All(payload.MbTemps, t => Assert.True(t.Celsius >= 5, $"{slug}: temp '{t.Label}' must be ≥ 5 °C"));
        Assert.All(payload.MbVoltages, v => Assert.True(v.Volts > 0.1f, $"{slug}: voltage '{v.Label}' must be > 0.1 V"));
        Assert.All(payload.DiskTemps, kv => Assert.True(kv.Value > 0, $"{slug}: disk '{kv.Key}' temp must be > 0"));

        // 4. CPU temperature, when present, must be physically plausible.
        if (payload.CpuTemp is { } cpu)
            Assert.InRange(cpu, 0f, 150f);
    }

    [Fact]
    public void Sample_synthetic_fixture_extracts_expected_shape()
    {
        // Pins the documented sample so the format itself is regression-guarded.
        var path = Path.Combine(FixturesRoot, "_sample-synthetic", "sensor-tree.txt");
        Assert.True(File.Exists(path), "sample fixture must ship with the test assembly");

        var payload = SensorReader.Extract(SensorTreeLoader.LoadFile(path));

        Assert.Equal(52.4f, payload.CpuTemp);
        Assert.Equal(78.1f, payload.CpuPower);
        var gpu = Assert.Single(payload.GpuDevices);
        Assert.Equal(41.0f, gpu.Temp);
        Assert.Equal(53.0f, gpu.HotspotTemp);
        Assert.Equal(24564.0f, gpu.VramTotalMb);
        Assert.Equal(38.0f, payload.DiskTemps["Samsung SSD 990 PRO 2TB"]); // Warning sensor excluded
        Assert.Equal("Nuvoton NCT6799D", payload.MbChip);
        Assert.Single(payload.MbFans); // 0-RPM chassis fan dropped
        Assert.Equal("CPU Fan", payload.MbFans[0].Label);
        Assert.Single(payload.MbVoltages); // "Voltage #5" generic slot dropped
        Assert.Equal("Vcore", payload.MbVoltages[0].Label);
    }
}
