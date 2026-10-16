using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// The LHM side of the hardware worker (#374): hardware it leaves out is
/// neither read nor reported, and every read is announced first, so a crash
/// can be put on the hardware being read.
public class LhmHardwareTests
{
    [Fact]
    public void Skipped_hardware_is_neither_read_nor_reported()
    {
        var computer = FanSamples.Board(2); // /motherboard and /gpu-amd/0
        var read = new List<string>();
        var hardware = new LhmHardware(computer, new HashSet<string> { "/gpu-amd/0" }, h => read.Add(h.Identifier.ToString()));

        var sample = hardware.Sample();

        Assert.Equal(["/motherboard"], read);
        Assert.Empty(sample.GpuDevices);
        Assert.Contains("# skipped: /gpu-amd/0", hardware.SensorTree());
        Assert.Equal(2, hardware.FanHeaders().Count); // the fans are still there
    }

    [Fact]
    public void Without_a_skip_list_everything_is_read()
    {
        var computer = FanSamples.Board(2);
        var read = new List<string>();

        var sample = new LhmHardware(computer, updating: h => read.Add(h.Identifier.ToString())).Sample();

        Assert.Equal(["/motherboard", "/gpu-amd/0"], read);
        Assert.Single(sample.GpuDevices);
    }
}
