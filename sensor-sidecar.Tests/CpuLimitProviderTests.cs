using System.Text.Json;
using SensorSidecar.Control;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// CPU package limits (#189): only ever lowered from the BIOS values,
/// verified by readback, released back to the BIOS values, and the BIOS
/// values themselves survive a service restart within the same boot.
/// </summary>
public sealed class CpuLimitProviderTests : IDisposable
{
    private static readonly AmdLimits Stock = new(162, 120, 180);
    private static readonly DateTimeOffset Boot = new(2026, 10, 3, 8, 0, 0, TimeSpan.Zero);

    private readonly string _dir = Path.Combine(Path.GetTempPath(), "rigstats-cpulimit-tests-" + Guid.NewGuid());

    public CpuLimitProviderTests() => Directory.CreateDirectory(_dir);

    public void Dispose()
    {
        try { Directory.Delete(_dir, recursive: true); } catch { }
    }

    /// An SMU whose PM table reflects every write, unless told to ignore them.
    private sealed class FakeSmu(AmdLimits current) : IRyzenSmu
    {
        public AmdLimits Current { get; set; } = current;
        public List<AmdLimits> Writes { get; } = [];
        public bool IgnoreWrites { get; set; }
        public string Generation => "Granite Ridge";
        public uint PmTableVersion => 0x620105;
        public AmdLimits ReadLimits() => Current;

        public void SetLimits(AmdLimits limits)
        {
            Writes.Add(limits);
            if (!IgnoreWrites)
                Current = limits;
        }
    }

    private string BaselinePath => Path.Combine(_dir, "cpu-limit-baseline.json");
    private string MarkerPath => Path.Combine(_dir, "pending-apply");

    private CpuLimitProvider Provider(IRyzenSmu? smu, bool dryRun = false, DateTimeOffset? boot = null) =>
        new(smu, "not supported", new BootCrashGuard(MarkerPath, TimeSpan.FromMinutes(3)), dryRun, BaselinePath, boot ?? Boot);

    private static ProfilePart Limit(double? ppt = null, double? tdc = null, double? edc = null) =>
        new() { CpuLimit = new CpuLimitPart { Amd = new AmdCpuLimit { PptW = ppt, TdcA = tdc, EdcA = edc } } };

    [Fact]
    public void Resolve_uses_the_bios_value_for_missing_limits_and_clamps_the_rest()
    {
        var resolved = CpuLimitProvider.Resolve(new AmdCpuLimit { PptW = 300, TdcA = 10, EdcA = null }, Stock);

        Assert.Equal(new AmdLimits(162, 30, 180), resolved); // never above BIOS, never below the floor.
    }

    [Fact]
    public void Resolve_rounds_to_whole_units()
    {
        Assert.Equal(88, CpuLimitProvider.Resolve(new AmdCpuLimit { PptW = 87.6 }, Stock).PptW);
    }

    [Fact]
    public void The_floor_never_exceeds_a_low_bios_value()
    {
        var lowStock = new AmdLimits(40, 25, 40);
        Assert.Equal(lowStock, CpuLimitProvider.MinFor(lowStock));
    }

    [Fact]
    public void Apply_writes_the_clamped_limits_and_verify_reads_them_back()
    {
        var smu = new FakeSmu(Stock);
        var provider = Provider(smu);
        var part = Limit(ppt: 88, tdc: 75, edc: 150);

        Assert.True(provider.Validate(part).Ok);
        provider.Apply(part);

        Assert.Equal(new AmdLimits(88, 75, 150), Assert.Single(smu.Writes));
        Assert.True(provider.Verify(part));
    }

    [Fact]
    public void Verify_fails_when_the_smu_ignored_the_write()
    {
        var smu = new FakeSmu(Stock) { IgnoreWrites = true };
        var provider = Provider(smu);
        var part = Limit(ppt: 88);

        provider.Apply(part);

        Assert.False(provider.Verify(part));
    }

    [Fact]
    public void Validate_rejects_nonsense_values()
    {
        var provider = Provider(new FakeSmu(Stock));

        Assert.False(provider.Validate(Limit(ppt: -5)).Ok);
        Assert.False(provider.Validate(Limit(tdc: double.NaN)).Ok);
    }

    [Fact]
    public void An_empty_part_at_bios_values_writes_nothing()
    {
        var smu = new FakeSmu(Stock);
        var provider = Provider(smu);

        provider.Apply(new ProfilePart { CpuLimit = new CpuLimitPart() });

        Assert.Empty(smu.Writes);
        Assert.False(File.Exists(MarkerPath)); // nothing risky happened.
    }

    [Fact]
    public void An_empty_part_puts_lowered_limits_back_to_bios_values()
    {
        var smu = new FakeSmu(Stock);
        var provider = Provider(smu);
        provider.Apply(Limit(ppt: 88));

        provider.Apply(new ProfilePart { CpuLimit = new CpuLimitPart() });

        Assert.Equal(Stock, smu.Current);
    }

    [Fact]
    public void A_lowering_write_arms_the_boot_crash_guard()
    {
        var provider = Provider(new FakeSmu(Stock));

        provider.Apply(Limit(ppt: 88));

        Assert.True(File.Exists(MarkerPath));
    }

    [Fact]
    public void Restore_puts_the_snapshot_back()
    {
        var smu = new FakeSmu(Stock);
        var provider = Provider(smu);
        var snapshot = provider.Capture();
        provider.Apply(Limit(ppt: 88));

        provider.Restore(snapshot);

        Assert.Equal(Stock, smu.Current);
    }

    [Fact]
    public void Release_returns_to_bios_values_only_after_a_change()
    {
        var smu = new FakeSmu(Stock);
        var provider = Provider(smu);

        provider.ReleaseToFirmware();
        Assert.Empty(smu.Writes); // never touched → no SMU write at all.

        provider.Apply(Limit(ppt: 88));
        provider.ReleaseToFirmware();
        Assert.Equal(Stock, smu.Current);
    }

    [Fact]
    public void A_restart_in_the_same_boot_keeps_the_bios_baseline()
    {
        var smu = new FakeSmu(Stock);
        Provider(smu).Apply(Limit(ppt: 88));

        // New service instance, same boot: the SMU still holds 88 W.
        var restarted = Provider(smu, boot: Boot.AddSeconds(30));
        restarted.Apply(new ProfilePart { CpuLimit = new CpuLimitPart() });

        Assert.Equal(Stock, smu.Current);
    }

    [Fact]
    public void A_new_boot_reads_the_baseline_again()
    {
        Provider(new FakeSmu(Stock)).Apply(Limit(ppt: 88));

        // Rebooted with a new BIOS setting: the old file must not be trusted.
        var newBios = new AmdLimits(142, 95, 140);
        var smu = new FakeSmu(newBios);
        var probe = Provider(smu, boot: Boot.AddHours(5)).Probe();

        Assert.Equal(142, probe.Details!["stock"]!["ppt_w"]!.GetValue<double>());
    }

    [Fact]
    public void Dry_run_never_writes()
    {
        var smu = new FakeSmu(Stock);
        var provider = Provider(smu, dryRun: true);
        var part = Limit(ppt: 88);

        provider.Apply(part);

        Assert.Empty(smu.Writes);
        Assert.True(provider.Verify(part));
    }

    [Fact]
    public void Unsupported_cpu_reports_why_and_skips_without_failing_the_profile()
    {
        var provider = Provider(null);

        var caps = provider.Probe();
        Assert.False(caps.Supported);
        Assert.Equal("not supported", caps.Reason);

        var part = Limit(ppt: 88);
        Assert.True(provider.Validate(part).Ok);
        provider.Apply(part); // no throw
        Assert.True(provider.Verify(part));
    }

    private sealed class BrokenSmu : IRyzenSmu
    {
        public string Generation => "Granite Ridge";
        public uint PmTableVersion => 0x620105;
        public AmdLimits ReadLimits() => throw new System.ComponentModel.Win32Exception(1359);
        public void SetLimits(AmdLimits limits) => throw new System.ComponentModel.Win32Exception(1359);
    }

    [Fact]
    public void An_smu_that_does_not_answer_is_reported_unsupported_not_thrown()
    {
        var caps = Provider(new BrokenSmu()).Probe();

        Assert.False(caps.Supported);
        Assert.Contains("SMU did not answer", caps.Reason);
    }

    [Fact]
    public void Probe_reports_stock_min_and_current()
    {
        var caps = Provider(new FakeSmu(Stock)).Probe();

        Assert.True(caps.Supported);
        var details = caps.Details!;
        Assert.Equal("amd", details["vendor"]!.GetValue<string>());
        Assert.Equal("0x620105", details["pm_table"]!.GetValue<string>());
        Assert.Equal(180, details["stock"]!["edc_a"]!.GetValue<double>());
        Assert.Equal(45, details["min"]!["ppt_w"]!.GetValue<double>());
    }

    [Fact]
    public void Cpu_limit_part_round_trips_through_json()
    {
        var part = Limit(ppt: 88, edc: 150);

        var json = JsonSerializer.Serialize(part, ControlJson.Options);
        var back = JsonSerializer.Deserialize<ProfilePart>(json, ControlJson.Options)!;

        Assert.Contains("\"cpu_limit\":{\"amd\":{\"ppt_w\":88,\"edc_a\":150}}", json);
        Assert.Equal(88, back.CpuLimit!.Amd!.PptW);
        Assert.Null(back.CpuLimit.Amd.TdcA);
    }
}

/// <summary>
/// Which CPUs get CPU limits at all: both the SMU command ids and the PM
/// table layout must be known.
/// </summary>
public class AmdSmuMapTests
{
    [Theory]
    [InlineData(0x1A, 0x44, 0, "Granite Ridge", 0x56u)]
    [InlineData(0x19, 0x61, 0, "Raphael", 0x56u)]
    [InlineData(0x19, 0x21, 0, "Vermeer", 0x53u)]
    [InlineData(0x17, 0x71, 0, "Matisse", 0x53u)]
    public void Desktop_ryzen_generations_have_their_own_command_ids(uint family, uint model, uint pkg, string name, uint setPpt)
    {
        var generation = AmdSmuMap.Generation(family, model, pkg);

        Assert.NotNull(generation);
        Assert.Equal(name, generation.Value.Name);
        Assert.Equal(setPpt, generation.Value.Commands.SetPpt);
    }

    [Theory]
    [InlineData(0x19, 0x61, 1)] // Dragon Range (mobile) shares Raphael's model.
    [InlineData(0x19, 0x50, 0)] // Cezanne APU — different mailbox semantics.
    [InlineData(0x17, 0x31, 7)] // Threadripper 3000.
    [InlineData(0x06, 0x97, 0)] // Intel.
    public void Other_cpus_are_not_offered(uint family, uint model, uint pkg)
    {
        Assert.Null(AmdSmuMap.Generation(family, model, pkg));
    }

    [Fact]
    public void Only_hardware_verified_pm_tables_have_a_layout()
    {
        Assert.Equal(new PmTableLayout(2, 8, 63), AmdSmuMap.Layout(0x620105));
        Assert.Null(AmdSmuMap.Layout(0x540104));
    }

    [Fact]
    public void Smu_arguments_are_milli_units()
    {
        Assert.Equal(88_000u, AmdSmuMap.ToSmuArg(88));
        Assert.Equal(162_500u, AmdSmuMap.ToSmuArg(162.5));
    }

    [Fact]
    public void Pm_floats_unpack_low_half_first()
    {
        long low = BitConverter.SingleToInt32Bits(162f) & 0xFFFFFFFFL;
        long high = (long)BitConverter.SingleToInt32Bits(41.5f) << 32;
        var table = new[] { low | high };

        Assert.Equal(162f, AmdSmuMap.PmFloat(table, 0));
        Assert.Equal(41.5f, AmdSmuMap.PmFloat(table, 1));
    }
}

/// <summary>
/// The boot-crash guard's marker lifecycle.
/// </summary>
public sealed class BootCrashGuardTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "rigstats-guard-tests-" + Guid.NewGuid());
    private string Marker => Path.Combine(_dir, "pending-apply");

    public BootCrashGuardTests() => Directory.CreateDirectory(_dir);

    public void Dispose()
    {
        try { Directory.Delete(_dir, recursive: true); } catch { }
    }

    [Fact]
    public void A_clean_start_is_not_tripped()
    {
        using var guard = new BootCrashGuard(Marker, TimeSpan.FromMinutes(3));

        Assert.False(guard.TrippedAtStart);
        Assert.Null(guard.Notice);
    }

    [Fact]
    public void A_leftover_marker_trips_once_and_is_consumed()
    {
        File.WriteAllText(Marker, "x");

        using var guard = new BootCrashGuard(Marker, TimeSpan.FromMinutes(3));

        Assert.True(guard.TrippedAtStart);
        Assert.NotNull(guard.Notice);
        Assert.False(File.Exists(Marker)); // the next boot is not skipped again.
        guard.AcknowledgeNotice();
        Assert.Null(guard.Notice);
    }

    [Fact]
    public async Task Arm_writes_the_marker_and_stable_uptime_clears_it()
    {
        using var guard = new BootCrashGuard(Marker, TimeSpan.FromMilliseconds(100));

        guard.Arm();
        Assert.True(guard.IsArmed);

        for (var i = 0; i < 50 && guard.IsArmed; i++)
            await Task.Delay(20);
        Assert.False(guard.IsArmed);
    }

    [Fact]
    public void Stopping_the_service_mid_window_keeps_the_marker()
    {
        var guard = new BootCrashGuard(Marker, TimeSpan.FromMinutes(3));
        guard.Arm();

        guard.Dispose();

        Assert.True(File.Exists(Marker));
    }
}

/// <summary>Vendor detection from raw CPUID leaf 0 registers.</summary>
public class CpuVendorTests
{
    [Fact]
    public void A_ryzen_9800x3d_is_authentic_amd()
    {
        // Captured on the dev rig: eax=D ebx=68747541 ecx=444D4163 edx=69746E65.
        Assert.True(AmdSmuMap.IsAuthenticAmd((0xD, 0x68747541, 0x444D4163, 0x69746E65)));
    }

    [Fact]
    public void Genuine_intel_and_eax_only_matches_are_not()
    {
        Assert.False(AmdSmuMap.IsAuthenticAmd((0x20, 0x756E6547, 0x6C65746E, 0x49656E69)));
        Assert.False(AmdSmuMap.IsAuthenticAmd((0x68747541, 0, 0, 0)));
    }
}
