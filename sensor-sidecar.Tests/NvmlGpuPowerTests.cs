using SensorSidecar.Control;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// NVIDIA power limits (#210): NVML's milliwatts as the GPU tab's
/// percentage offset from the driver default, laptop GPUs left out, and
/// AMD + NVIDIA behind one `IGpuPowerApi`.
/// </summary>
public sealed class NvmlGpuPowerTests
{
    [Fact]
    public void A_desktop_range_becomes_whole_percents_inside_the_driver_limits()
    {
        // RTX 4090: 150–600 W, default 450 W.
        var adapter = NvmlGpuPower.Describe("GPU-1", "NVIDIA GeForce RTX 4090", 150_000, 600_000, 450_000);

        Assert.NotNull(adapter);
        Assert.Equal(-66, adapter.Min); // -66.7 % rounded inwards
        Assert.Equal(33, adapter.Max); // +33.3 %
        Assert.Equal(1, adapter.Step);
        Assert.Equal(0, adapter.Default);
    }

    [Theory]
    [InlineData(150_000u, 0u, 600_000u)] // no default
    [InlineData(450_000u, 450_000u, 450_000u)] // a fixed limit: nothing to tune
    [InlineData(500_000u, 450_000u, 600_000u)] // default below the range
    public void A_range_without_room_to_tune_is_not_offered(uint min, uint defaultMw, uint max)
    {
        Assert.Null(NvmlGpuPower.Describe("GPU-1", "GPU", min, max, defaultMw));
    }

    [Theory]
    [InlineData(0, 450_000u)]
    [InlineData(-20, 360_000u)]
    [InlineData(33, 598_500u)]
    [InlineData(50, 600_000u)] // clamped to the driver max
    [InlineData(-90, 150_000u)] // and min
    public void Percent_to_milliwatts_is_relative_to_the_default_and_clamped(int percent, uint expected)
    {
        Assert.Equal(expected, NvmlGpuPower.ToMilliwatts(percent, 450_000, 150_000, 600_000));
    }

    [Fact]
    public void Every_percent_in_range_reads_back_as_itself()
    {
        var adapter = NvmlGpuPower.Describe("GPU-1", "GPU", 150_000, 600_000, 450_000)!;
        for (var pct = adapter.Min; pct <= adapter.Max; pct++)
            Assert.Equal(pct, NvmlGpuPower.ToPercent(NvmlGpuPower.ToMilliwatts(pct, 450_000, 150_000, 600_000), 450_000));
    }

    [Theory]
    [InlineData("NVIDIA GeForce RTX 5070 Ti Laptop GPU", true)]
    [InlineData("NVIDIA GeForce RTX 2070 with Max-Q Design", true)]
    [InlineData("NVIDIA GeForce RTX 4090", false)]
    [InlineData("NVIDIA RTX A4000", false)]
    public void Laptop_gpus_are_recognised_by_name(string name, bool laptop)
    {
        Assert.Equal(laptop, NvmlGpuPower.IsLaptopGpu(name));
    }

    private sealed class OneGpu(GpuPowerAdapter adapter) : IGpuPowerApi
    {
        public int Limit { get; private set; }
        public bool Ended { get; private set; }
        public IReadOnlyList<GpuPowerAdapter> Adapters() => [adapter];
        public int GetPowerLimit(string adapterId) => adapterId == adapter.Id ? Limit : throw new InvalidOperationException("not mine");
        public void SetPowerLimit(string adapterId, int percent) => Limit = adapterId == adapter.Id ? percent : throw new InvalidOperationException("not mine");
        public bool IsAtFactory(string adapterId) => Limit == 0;
        public void ResetToFactory(string adapterId) => Limit = 0;
        public void EndSession() => Ended = true;
    }

    private sealed class BrokenDriver : IGpuPowerApi
    {
        public IReadOnlyList<GpuPowerAdapter> Adapters() => throw new InvalidOperationException("ADLX ADLXInitialize failed (ADLX_FAIL).");
        public int GetPowerLimit(string adapterId) => throw new InvalidOperationException();
        public void SetPowerLimit(string adapterId, int percent) => throw new InvalidOperationException();
        public bool IsAtFactory(string adapterId) => throw new InvalidOperationException();
        public void ResetToFactory(string adapterId) => throw new InvalidOperationException();
        public void EndSession() => throw new InvalidOperationException("already gone");
    }

    private static readonly GpuPowerAdapter Radeon = new("PCI\\VEN_1002", "AMD Radeon RX 9070 XT", -30, 10, 1, 0);
    private static readonly GpuPowerAdapter GeForce = new("GPU-8f3c", "NVIDIA GeForce RTX 4090", -66, 33, 1, 0);

    [Fact]
    public void Of_returns_null_without_drivers_and_a_single_driver_as_is()
    {
        var amd = new OneGpu(Radeon);
        Assert.Null(GpuPowerApis.Of(null, null));
        Assert.Same(amd, GpuPowerApis.Of(amd, null));
    }

    [Fact]
    public void Each_adapter_is_handled_by_the_driver_that_listed_it()
    {
        var amd = new OneGpu(Radeon);
        var nvidia = new OneGpu(GeForce);
        var api = GpuPowerApis.Of(amd, nvidia)!;

        Assert.Equal([Radeon.Id, GeForce.Id], api.Adapters().Select(a => a.Id));
        api.SetPowerLimit(GeForce.Id, -20);
        api.SetPowerLimit(Radeon.Id, 5);

        Assert.Equal(-20, nvidia.Limit);
        Assert.Equal(5, amd.Limit);
        Assert.Equal(-20, api.GetPowerLimit(GeForce.Id));
    }

    [Fact]
    public void A_failing_driver_hides_only_its_own_adapters()
    {
        var nvidia = new OneGpu(GeForce);
        var api = GpuPowerApis.Of(new BrokenDriver(), nvidia)!;

        Assert.Equal([GeForce.Id], api.Adapters().Select(a => a.Id));
        api.EndSession(); // the broken driver throwing doesn't stop the others
        Assert.True(nvidia.Ended);
    }

    [Fact]
    public void Only_when_every_driver_fails_does_the_list_throw()
    {
        var api = GpuPowerApis.Of(new BrokenDriver(), new BrokenDriver())!;

        Assert.Throws<InvalidOperationException>(() => api.Adapters());
    }

    [Fact]
    public void A_profile_reaches_an_nvidia_card_next_to_a_broken_amd_driver()
    {
        // The #241 laptop, with a desktop-class NVIDIA card instead.
        var nvidia = new OneGpu(GeForce);
        var provider = new GpuPowerProvider(GpuPowerApis.Of(new BrokenDriver(), nvidia), dryRun: false, originalsPath: null);
        var part = new ProfilePart
        {
            Gpu = new GpuPart { Adapters = new() { [GeForce.Id] = new GpuAdapterConfig { PowerLimitPct = -15 } } },
        };

        Assert.True(provider.Probe().Supported);
        provider.Apply(part);

        Assert.Equal(-15, nvidia.Limit);
        Assert.True(provider.Verify(part));
    }
}
