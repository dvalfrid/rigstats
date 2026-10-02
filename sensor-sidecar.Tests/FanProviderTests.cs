using System.Text.Json.Nodes;
using LibreHardwareMonitor.Hardware;
using SensorSidecar.Control;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// <see cref="FanProvider"/> driven against mocked LHM trees — synthetic
/// boards plus the real sensor-tree fixtures for <c>Probe()</c> regression.
/// </summary>
public class FanProviderTests
{
    private static string FixturesRoot => Path.Combine(AppContext.BaseDirectory, "fixtures");

    private static FanHeaderConfig Curve(string source = "cpu_package", params double[][] points) => new()
    {
        Source = source,
        Curve = (points.Length == 0 ? [[40, 30], [80, 100]] : points).Select(p => p.ToList()).ToList(),
    };

    private static ProfilePart Part(params (int Header, FanHeaderConfig Config)[] headers) => new()
    {
        Fan = new FanPart { Headers = headers.ToDictionary(h => FanSamples.HeaderId(h.Header), h => h.Config) },
    };

    private static (FanProvider Provider, FakeHardwareHost Host, IComputer Computer) Setup(int headers = 2, bool dryRun = false)
    {
        var computer = FanSamples.Board(headers);
        var host = new FakeHardwareHost(computer);
        return (new FanProvider(host, dryRun), host, computer);
    }

    [Fact]
    public void Probe_reports_lpc_headers_and_available_sources_but_not_gpu_fans()
    {
        var (provider, host, _) = Setup(headers: 3);
        host.Sample = FanSamples.Temps(cpu: 50, gpu: 45, mb: ("System", 30));

        var caps = provider.Probe();

        Assert.True(caps.Supported);
        var ids = caps.Details!["headers"]!.AsArray().Select(h => h!["id"]!.GetValue<string>()).ToList();
        Assert.Equal([FanSamples.HeaderId(0), FanSamples.HeaderId(1), FanSamples.HeaderId(2)], ids);
        var sources = caps.Details["sources"]!.AsArray().Select(s => s!.GetValue<string>()).ToList();
        Assert.Equal(["cpu_package", "gpu", "mb:System"], sources);
    }

    [Fact]
    public void Probe_is_unsupported_on_a_board_without_writable_headers()
    {
        var (provider, _, _) = Setup(headers: 0);
        var caps = provider.Probe();
        Assert.False(caps.Supported);
        Assert.NotNull(caps.Reason);
    }

    [Theory]
    [InlineData("asus-b650m-ryzen7-9800x3d-rx9070xt", 7)]
    [InlineData("asus-z790-i9-13900k-rtx4090", 7)]
    [InlineData("gigabyte-b550-aorus-ryzen7-5700x3d-rtx3060ti", 5)]
    [InlineData("asus-ga403wr-ryzen-ai9-hx370-rtx5070ti", 0)]
    [InlineData("razer-blade-i7-8750h-gtx1060maxq", 0)]
    public void Probe_finds_the_writable_headers_on_real_boards(string slug, int expected)
    {
        var computer = SensorTreeLoader.LoadFile(Path.Combine(FixturesRoot, slug, "sensor-tree.txt"));
        var caps = new FanProvider(new FakeHardwareHost(computer), dryRun: false).Probe();

        Assert.Equal(expected, caps.Details!["headers"]!.AsArray().Count);
        Assert.Equal(expected > 0, caps.Supported);
    }

    [Fact]
    public void Validate_rejects_unknown_headers_sources_and_malformed_curves()
    {
        var (provider, _, _) = Setup();

        Assert.True(provider.Validate(Part((0, Curve()))).Ok);
        Assert.True(provider.Validate(new ProfilePart()).Ok);
        Assert.False(provider.Validate(Part((9, Curve()))).Ok);
        Assert.False(provider.Validate(Part((0, Curve(source: "weather")))).Ok);
        Assert.False(provider.Validate(Part((0, Curve(source: "mb:")))).Ok);
        Assert.False(provider.Validate(Part((0, Curve("cpu_package", [70, 50], [40, 30])))).Ok);
        Assert.False(provider.Validate(Part((0, Curve("cpu_package", [40, double.NaN])))).Ok);
        Assert.False(provider.Validate(Part((0, Curve("cpu_package", [40])))).Ok);
    }

    [Fact]
    public void Apply_claims_the_header_at_the_curve_duty_for_the_current_temperature()
    {
        var (provider, host, computer) = Setup();
        host.Sample = FanSamples.Temps(cpu: 60);

        provider.Apply(Part((0, Curve())));

        var header = FanSamples.Header(computer, 0);
        Assert.Equal(ControlMode.Software, header.Mode);
        Assert.Equal(65, header.Register);
        Assert.True(provider.Verify(Part((0, Curve()))));
        Assert.Equal(ControlMode.Default, FanSamples.Header(computer, 1).Mode);
    }

    [Fact]
    public void Duty_is_clamped_to_the_probed_hardware_range()
    {
        var (provider, host, computer) = Setup();
        var header = FanSamples.Header(computer, 0);
        header.Min = 20;
        host.Sample = FanSamples.Temps(cpu: 30);

        provider.Apply(Part((0, Curve("cpu_package", [40, 0], [80, 150]))));
        Assert.Equal(20, header.Register);

        provider.ApplyDuty(FanSamples.HeaderId(0), 150);
        Assert.Equal(100, header.Register);
    }

    [Fact]
    public void Verify_fails_when_firmware_overrides_the_write()
    {
        var (provider, _, computer) = Setup();
        FanSamples.Header(computer, 0).FirmwareOverrides = true;

        provider.Apply(Part((0, Curve())));

        Assert.False(provider.Verify(Part((0, Curve()))));
    }

    [Fact]
    public void A_header_left_out_of_the_new_profile_is_released_to_firmware()
    {
        var (provider, _, computer) = Setup();
        provider.Apply(Part((0, Curve()), (1, Curve())));

        provider.Apply(Part((1, Curve())));

        Assert.Equal(ControlMode.Default, FanSamples.Header(computer, 0).Mode);
        Assert.Equal(ControlMode.Software, FanSamples.Header(computer, 1).Mode);
        Assert.Equal([FanSamples.HeaderId(1)], provider.GetActiveConfig().Keys);
    }

    [Fact]
    public void ReleaseToFirmware_releases_every_header_and_blocks_late_writes()
    {
        var (provider, _, computer) = Setup();
        provider.Apply(Part((0, Curve()), (1, Curve())));

        provider.ReleaseToFirmware();
        var header = FanSamples.Header(computer, 0);
        var writesBefore = header.Writes.Count;
        provider.ApplyDuty(FanSamples.HeaderId(0), 80); // a loop tick racing the release

        Assert.Equal(ControlMode.Default, header.Mode);
        Assert.Equal(ControlMode.Default, FanSamples.Header(computer, 1).Mode);
        Assert.Equal(writesBefore, header.Writes.Count);
        Assert.Empty(provider.GetActiveConfig());
    }

    [Fact]
    public void Dry_run_never_writes_and_verifies_the_request()
    {
        var (provider, _, computer) = Setup(dryRun: true);

        provider.Apply(Part((0, Curve())));
        provider.ReleaseToFirmware();

        var header = FanSamples.Header(computer, 0);
        Assert.Empty(header.Writes);
        Assert.Equal(0, header.Releases);
        Assert.True(provider.Verify(Part((0, Curve()))));
    }

    [Fact]
    public void Capture_then_restore_round_trips_the_active_curves()
    {
        var (provider, _, computer) = Setup();
        var original = Curve("gpu", [30, 25], [70, 90]);
        provider.Apply(Part((0, original)));
        var snapshot = provider.Capture();

        provider.Apply(Part((1, Curve())));
        provider.Restore(snapshot);

        var restored = Assert.Single(provider.GetActiveConfig());
        Assert.Equal(FanSamples.HeaderId(0), restored.Key);
        Assert.Equal("gpu", restored.Value.Source);
        Assert.Equal([[30.0, 25.0], [70.0, 90.0]], restored.Value.Curve);
        Assert.Equal(ControlMode.Software, FanSamples.Header(computer, 0).Mode);
        Assert.Equal(ControlMode.Default, FanSamples.Header(computer, 1).Mode);
    }

    [Fact]
    public async Task Broker_rolls_back_the_fan_part_when_firmware_overrides_it()
    {
        var (provider, _, computer) = Setup();
        FanSamples.Header(computer, 0).FirmwareOverrides = true;
        var broker = new ControlBroker([provider]);

        var result = await broker.ApplyProfileAsync(
            new Profile { Id = "silent", Name = "Silent", Part = Part((0, Curve())) }, CancellationToken.None);

        Assert.False(result.Ok);
        Assert.Empty(provider.GetActiveConfig());
        Assert.Equal(ControlMode.Default, FanSamples.Header(computer, 0).Mode);
    }

    [Fact]
    public void Snapshot_state_serializes_with_snake_case_keys()
    {
        var (provider, _, _) = Setup();
        provider.Apply(Part((0, Curve())));

        var header = provider.Capture().State!["headers"]![FanSamples.HeaderId(0)]!.AsObject();

        Assert.True(header.ContainsKey("hysteresis_c"));
        Assert.IsType<JsonArray>(header["curve"]);
    }

    [Fact]
    public async Task Identify_spins_an_uncontrolled_header_to_full_then_hands_it_back_to_firmware()
    {
        var (provider, _, computer) = Setup();
        var header = FanSamples.Header(computer, 0);

        var identify = provider.IdentifyAsync(FanSamples.HeaderId(0), TimeSpan.FromMilliseconds(20), CancellationToken.None);
        Assert.Equal(100, header.Register);
        Assert.Equal(ControlMode.Software, header.Mode);
        await identify;

        Assert.Equal(ControlMode.Default, header.Mode);
        Assert.Empty(provider.GetActiveConfig());
    }

    [Fact]
    public async Task Identify_holds_full_speed_over_curve_ticks_then_restores_the_curve_duty()
    {
        var (provider, host, computer) = Setup();
        host.Sample = FanSamples.Temps(cpu: 60);
        provider.Apply(Part((0, Curve())));
        var header = FanSamples.Header(computer, 0);

        var identify = provider.IdentifyAsync(FanSamples.HeaderId(0), Timeout.InfiniteTimeSpan, CancellationToken.None);
        provider.ApplyDuty(FanSamples.HeaderId(0), 40); // a loop tick during identify
        Assert.Equal(100, header.Register);

        // A second identify supersedes the first; ending it restores the
        // duty the loop asked for meanwhile.
        await provider.IdentifyAsync(FanSamples.HeaderId(0), TimeSpan.FromMilliseconds(10), CancellationToken.None);
        Assert.Equal(40, header.Register);
        Assert.Equal(ControlMode.Software, header.Mode);
        Assert.False(identify.IsCompleted);
    }

    [Fact]
    public async Task ReleaseToFirmware_ends_an_identify_immediately()
    {
        var (provider, _, computer) = Setup();
        using var cts = new CancellationTokenSource();
        var identify = provider.IdentifyAsync(FanSamples.HeaderId(0), Timeout.InfiniteTimeSpan, cts.Token);

        provider.ReleaseToFirmware();
        var header = FanSamples.Header(computer, 0);
        Assert.Equal(ControlMode.Default, header.Mode);

        var releases = header.Releases;
        cts.Cancel();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => identify);
        Assert.Equal(releases, header.Releases); // the stale identify's end is a no-op
    }

    [Fact]
    public void HasHeader_only_knows_lpc_control_headers()
    {
        var (provider, _, _) = Setup();
        Assert.True(provider.HasHeader(FanSamples.HeaderId(1)));
        Assert.False(provider.HasHeader("/gpu-amd/0/control/0"));
    }
}
