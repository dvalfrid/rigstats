using System.Text.Json;
using SensorSidecar.Control;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// GPU power limits (#190): clamped to the driver's range, verified by
/// readback, and the value in force before RIGStats is what a profile
/// without a value, a release and a restart go back to.
/// </summary>
public sealed class GpuPowerProviderTests : IDisposable
{
    private const string Id = @"PCI\VEN_1002&DEV_7550&SUBSYS_06331043&REV_C0\6&196380D0&0&00000009";
    private static readonly GpuPowerAdapter Rx9070Xt = new(Id, "AMD Radeon RX 9070 XT", -30, 10, 1, 0);

    private readonly string _dir = Path.Combine(Path.GetTempPath(), "rigstats-gpu-tests-" + Guid.NewGuid());

    public GpuPowerProviderTests() => Directory.CreateDirectory(_dir);

    public void Dispose()
    {
        try { Directory.Delete(_dir, recursive: true); } catch { }
    }

    private sealed class FakeGpu(params GpuPowerAdapter[] adapters) : IGpuPowerApi
    {
        public Dictionary<string, int> Limits { get; } = adapters.ToDictionary(a => a.Id, _ => 0);
        public List<(string Id, int Value)> Writes { get; } = [];
        public bool IgnoreWrites { get; set; }
        public IReadOnlyList<GpuPowerAdapter> Adapters() => adapters;
        public int GetPowerLimit(string adapterId) => Limits[adapterId];

        // Any write leaves factory settings, like Adrenalin going "Custom".
        public HashSet<string> AtFactory { get; } = adapters.Select(a => a.Id).ToHashSet();
        public int Resets { get; private set; }

        public void SetPowerLimit(string adapterId, int percent)
        {
            Writes.Add((adapterId, percent));
            if (IgnoreWrites)
                return;
            Limits[adapterId] = percent;
            AtFactory.Remove(adapterId);
        }

        public bool IsAtFactory(string adapterId) => AtFactory.Contains(adapterId);

        public void ResetToFactory(string adapterId)
        {
            Resets++;
            Limits[adapterId] = 0;
            AtFactory.Add(adapterId);
        }
    }

    private string OriginalsPath => Path.Combine(_dir, "gpu-power-original.json");

    private GpuPowerProvider Provider(IGpuPowerApi? api, bool dryRun = false) => new(api, dryRun, OriginalsPath);

    private static ProfilePart Limit(int? pct) => new()
    {
        Gpu = new GpuPart { Adapters = new() { [Id] = new GpuAdapterConfig { PowerLimitPct = pct } } },
    };

    [Theory]
    [InlineData(-10, -10)]
    [InlineData(-50, -30)] // below the driver range
    [InlineData(25, 10)] // above it
    public void Resolve_clamps_to_the_driver_range(int requested, int expected)
    {
        Assert.Equal(expected, GpuPowerProvider.Resolve(requested, 0, Rx9070Xt));
    }

    [Fact]
    public void Resolve_snaps_to_the_driver_step_and_falls_back_to_the_original()
    {
        var stepped = Rx9070Xt with { Step = 5 };
        Assert.Equal(-10, GpuPowerProvider.Resolve(-12, 0, stepped));
        Assert.Equal(5, GpuPowerProvider.Resolve(null, 5, Rx9070Xt));
    }

    [Fact]
    public void Apply_writes_and_verify_reads_back()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        var provider = Provider(gpu);
        var part = Limit(-15);

        provider.Apply(part);

        Assert.Equal((Id, -15), Assert.Single(gpu.Writes));
        Assert.True(provider.Verify(part));
    }

    [Fact]
    public void Verify_fails_when_the_driver_ignored_the_write()
    {
        var gpu = new FakeGpu(Rx9070Xt) { IgnoreWrites = true };
        var provider = Provider(gpu);
        var part = Limit(-15);

        provider.Apply(part);

        Assert.False(provider.Verify(part));
    }

    [Fact]
    public void An_empty_part_returns_to_the_users_own_setting_not_zero()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        gpu.SetPowerLimit(Id, 5); // the user's own Adrenalin setting → Custom
        gpu.Writes.Clear();
        var provider = Provider(gpu);
        provider.Apply(Limit(-20));

        provider.Apply(new ProfilePart { Gpu = new GpuPart() });

        Assert.Equal(5, gpu.Limits[Id]);
    }

    [Fact]
    public void Release_restores_the_original_and_forgets_it()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        var provider = Provider(gpu);
        provider.ReleaseToFirmware();
        Assert.Empty(gpu.Writes); // never changed → nothing to restore.

        provider.Apply(Limit(-20));
        Assert.True(File.Exists(OriginalsPath));
        provider.ReleaseToFirmware();

        Assert.Equal(0, gpu.Limits[Id]);
        Assert.False(File.Exists(OriginalsPath));
    }

    [Fact]
    public void The_original_survives_a_crash()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        gpu.SetPowerLimit(Id, 5); // the user's own Adrenalin setting → Custom
        gpu.Writes.Clear();
        Provider(gpu).Apply(Limit(-20));

        // Service died without releasing; the GPU still holds -20.
        Provider(gpu).ReleaseToFirmware();

        Assert.Equal(5, gpu.Limits[Id]);
    }

    [Fact]
    public void Restore_puts_the_snapshot_back()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        var provider = Provider(gpu);
        var snapshot = provider.Capture();
        provider.Apply(Limit(-20));

        provider.Restore(snapshot);

        Assert.Equal(0, gpu.Limits[Id]);
    }

    [Fact]
    public void A_profile_for_a_gpu_that_is_gone_is_skipped()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        var provider = Provider(gpu);
        var part = new ProfilePart
        {
            Gpu = new GpuPart { Adapters = new() { ["PCI\\OLD"] = new GpuAdapterConfig { PowerLimitPct = -10 } } },
        };

        Assert.True(provider.Validate(part).Ok);
        provider.Apply(part);

        Assert.Empty(gpu.Writes);
    }

    [Fact]
    public void Dry_run_never_writes()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        var provider = Provider(gpu, dryRun: true);

        provider.Apply(Limit(-20));

        Assert.Empty(gpu.Writes);
        Assert.True(provider.Verify(Limit(-20)));
    }

    [Fact]
    public void No_driver_means_unsupported_and_no_errors()
    {
        var provider = Provider(null);

        Assert.False(provider.Probe().Supported);
        provider.Apply(Limit(-20));
        Assert.True(provider.Verify(Limit(-20)));
        provider.ReleaseToFirmware();
    }

    [Fact]
    public void Probe_reports_range_default_original_and_current()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        var provider = Provider(gpu);
        provider.Apply(Limit(-20));

        var adapter = provider.Probe().Details!["adapters"]![0]!;

        Assert.Equal("AMD Radeon RX 9070 XT", adapter["name"]!.GetValue<string>());
        Assert.Equal(-30, adapter["min"]!.GetValue<int>());
        Assert.Equal(10, adapter["max"]!.GetValue<int>());
        Assert.Equal(0, adapter["original"]!.GetValue<int>());
        Assert.Equal(-20, adapter["current"]!.GetValue<int>());
    }

    [Fact]
    public void Gpu_part_round_trips_through_json()
    {
        var json = JsonSerializer.Serialize(Limit(-15), ControlJson.Options);
        var back = JsonSerializer.Deserialize<ProfilePart>(json, ControlJson.Options)!;

        Assert.Contains("\"gpu\":{\"adapters\":{", json);
        Assert.Contains("\"power_limit_pct\":-15", json);
        Assert.Equal(-15, back.Gpu!.Adapters![Id].PowerLimitPct);
    }

    [Fact]
    public void Adlx_results_have_readable_names()
    {
        Assert.Equal("ADLX_NOT_SUPPORTED", AdlxGpuPower.AdlxResultName(12));
        Assert.Equal("99", AdlxGpuPower.AdlxResultName(99));
    }

    [Fact]
    public void Back_to_a_factory_original_is_a_factory_reset_so_adrenalin_leaves_custom()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        var provider = Provider(gpu);
        provider.Apply(Limit(-15));
        Assert.False(gpu.IsAtFactory(Id)); // "Custom"

        provider.Apply(new ProfilePart { Gpu = new GpuPart() });

        Assert.Equal(1, gpu.Resets);
        Assert.True(gpu.IsAtFactory(Id)); // "Default" again
        Assert.Equal(0, gpu.Limits[Id]);
    }

    [Fact]
    public void A_users_own_custom_tuning_is_never_factory_reset()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        gpu.SetPowerLimit(Id, 5); // the user's own Adrenalin setting → Custom
        gpu.Writes.Clear();
        var provider = Provider(gpu);
        provider.Apply(Limit(-15));

        provider.ReleaseToFirmware();

        Assert.Equal(0, gpu.Resets);
        Assert.Equal(5, gpu.Limits[Id]);
    }

    [Fact]
    public void Release_resets_a_factory_gpu_even_if_only_the_mode_was_left_custom()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        var provider = Provider(gpu);
        provider.Apply(Limit(-15));
        gpu.SetPowerLimit(Id, 0); // value back at 0, but still "Custom"

        provider.ReleaseToFirmware();

        Assert.Equal(1, gpu.Resets);
        Assert.True(gpu.IsAtFactory(Id));
    }

    [Fact]
    public void The_original_and_its_factory_state_survive_a_restart()
    {
        var gpu = new FakeGpu(Rx9070Xt);
        Provider(gpu).Apply(Limit(-15));

        var json = File.ReadAllText(OriginalsPath);
        Assert.Contains("\"power_limit_pct\":0", json);
        Assert.Contains("\"at_factory\":true", json);

        Provider(gpu).ReleaseToFirmware(); // a new service instance
        Assert.True(gpu.IsAtFactory(Id));
    }
}
