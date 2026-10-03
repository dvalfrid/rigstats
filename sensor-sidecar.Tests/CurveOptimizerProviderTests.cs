using System.Text.Json;
using SensorSidecar.Control;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Curve Optimizer (#191): undervolt only, bounded, per-core only where the
/// core numbering is unambiguous, every lowering write arms the boot-crash
/// guard, and the BIOS values survive a restart in the same boot.
/// </summary>
public sealed class CurveOptimizerProviderTests : IDisposable
{
    private static readonly DateTimeOffset Boot = new(2026, 10, 3, 8, 0, 0, TimeSpan.Zero);

    private readonly string _dir = Path.Combine(Path.GetTempPath(), "rigstats-co-tests-" + Guid.NewGuid());

    public CurveOptimizerProviderTests() => Directory.CreateDirectory(_dir);

    public void Dispose()
    {
        try { Directory.Delete(_dir, recursive: true); } catch { }
    }

    /// A 9800X3D-like SMU: CCD 0, cores 0–7, each holding its offset.
    private sealed class FakeCo(int cores = 8, int bios = 0) : ICurveOptimizerSmu
    {
        public int[] Offsets { get; } = Enumerable.Repeat(bios, cores).ToArray();
        public int SetAllCalls { get; private set; }
        public int SetPerCoreCalls { get; private set; }
        public bool IgnoreWrites { get; set; }

        public IReadOnlyList<PhysicalCore> Cores() =>
            Enumerable.Range(0, Offsets.Length).Select(i => new PhysicalCore(0, i)).ToList();

        public int GetOffset(PhysicalCore core) => Offsets[core.Core];

        public void SetOffset(PhysicalCore core, int offset)
        {
            SetPerCoreCalls++;
            if (!IgnoreWrites)
                Offsets[core.Core] = offset;
        }

        public void SetAllOffsets(int offset)
        {
            SetAllCalls++;
            if (!IgnoreWrites)
                Array.Fill(Offsets, offset);
        }
    }

    private string MarkerPath => Path.Combine(_dir, "pending-apply");

    private CurveOptimizerProvider Provider(ICurveOptimizerSmu? smu, int windowsCores = 8, bool dryRun = false, DateTimeOffset? boot = null) =>
        new(smu, windowsCores, "not supported", new BootCrashGuard(MarkerPath, TimeSpan.FromMinutes(3)),
            dryRun, Path.Combine(_dir, "curve-opt-baseline.json"), boot ?? Boot);

    private static ProfilePart Co(int? allCore = null, Dictionary<string, int>? perCore = null) =>
        new() { CurveOpt = new CurveOptPart { AllCore = allCore, PerCore = perCore } };

    [Fact]
    public void Resolve_prefers_per_core_then_all_core_then_bios_and_clamps()
    {
        var bios = new[] { 0, -5, 0, 0 };
        var part = new CurveOptPart { AllCore = -15, PerCore = new() { ["1"] = -45, ["2"] = 7 } };

        Assert.Equal(new[] { -15, -30, 0, -15 }, CurveOptimizerProvider.Resolve(part, bios, perCore: true));
        // Per-core off: the per-core map is ignored entirely.
        Assert.Equal(new[] { -15, -15, -15, -15 }, CurveOptimizerProvider.Resolve(part, bios, perCore: false));
        // Nothing requested: the BIOS values stay, even outside the bounds.
        var deepBios = new[] { -40, 0 };
        Assert.Equal(deepBios, CurveOptimizerProvider.Resolve(new CurveOptPart(), deepBios, perCore: true));
    }

    [Theory]
    [InlineData(8, 8, true)]
    [InlineData(6, 8, false)] // fused-off cores: numbering would be ambiguous
    [InlineData(8, 0, false)] // Windows count unknown
    public void Per_core_needs_matching_core_counts(int smuCores, int windowsCores, bool expected)
    {
        Assert.Equal(expected, CurveOptimizerProvider.PerCoreSupported(smuCores, windowsCores));
    }

    [Fact]
    public void All_core_is_one_smu_command_and_verifies()
    {
        var smu = new FakeCo();
        var provider = Provider(smu);
        var part = Co(allCore: -20);

        provider.Apply(part);

        Assert.Equal(1, smu.SetAllCalls);
        Assert.Equal(0, smu.SetPerCoreCalls);
        Assert.All(smu.Offsets, v => Assert.Equal(-20, v));
        Assert.True(provider.Verify(part));
    }

    [Fact]
    public void Per_core_writes_only_the_cores_that_differ()
    {
        var smu = new FakeCo();
        var provider = Provider(smu);

        provider.Apply(Co(perCore: new() { ["3"] = -10, ["5"] = -25 }));

        Assert.Equal(2, smu.SetPerCoreCalls);
        Assert.Equal(new[] { 0, 0, 0, -10, 0, -25, 0, 0 }, smu.Offsets);
    }

    [Fact]
    public void Verify_fails_when_the_smu_ignored_the_write()
    {
        var smu = new FakeCo { IgnoreWrites = true };
        var provider = Provider(smu);
        var part = Co(allCore: -20);

        provider.Apply(part);

        Assert.False(provider.Verify(part));
    }

    [Fact]
    public void An_undervolt_arms_the_crash_guard_but_returning_to_bios_does_not()
    {
        var smu = new FakeCo();
        var provider = Provider(smu);

        provider.Apply(Co()); // BIOS values, nothing to write
        Assert.False(File.Exists(MarkerPath));

        provider.Apply(Co(allCore: -20));
        Assert.True(File.Exists(MarkerPath));
    }

    [Fact]
    public void Restore_and_release_go_back_to_the_bios_values()
    {
        var smu = new FakeCo(bios: -5);
        var provider = Provider(smu);
        var snapshot = provider.Capture();
        provider.Apply(Co(allCore: -20));

        provider.Restore(snapshot);
        Assert.All(smu.Offsets, v => Assert.Equal(-5, v));

        provider.Apply(Co(allCore: -20));
        provider.ReleaseToFirmware();
        Assert.All(smu.Offsets, v => Assert.Equal(-5, v));
    }

    [Fact]
    public void Release_never_writes_when_nothing_was_changed()
    {
        var smu = new FakeCo();
        Provider(smu).ReleaseToFirmware();

        Assert.Equal(0, smu.SetAllCalls + smu.SetPerCoreCalls);
    }

    [Fact]
    public void A_restart_in_the_same_boot_keeps_the_bios_baseline()
    {
        var smu = new FakeCo();
        Provider(smu).Apply(Co(allCore: -20));

        // New service instance, same boot: the SMU still holds -20.
        Provider(smu, boot: Boot.AddSeconds(30)).Apply(Co());

        Assert.All(smu.Offsets, v => Assert.Equal(0, v));
    }

    [Fact]
    public void Dry_run_never_writes()
    {
        var smu = new FakeCo();
        var provider = Provider(smu, dryRun: true);

        provider.Apply(Co(allCore: -20));

        Assert.Equal(0, smu.SetAllCalls + smu.SetPerCoreCalls);
        Assert.True(provider.Verify(Co(allCore: -20)));
    }

    [Fact]
    public void Unsupported_cpus_never_see_the_feature_and_never_fail_a_profile()
    {
        var provider = Provider(null);

        var caps = provider.Probe();
        Assert.False(caps.Supported);
        Assert.Equal("not supported", caps.Reason);
        Assert.True(provider.Validate(Co(allCore: -20)).Ok);
        provider.Apply(Co(allCore: -20)); // no throw
    }

    [Fact]
    public void Probe_reports_bounds_cores_and_per_core_support()
    {
        var details = Provider(new FakeCo(), windowsCores: 6).Probe().Details!;

        Assert.Equal(-30, details["min"]!.GetValue<int>());
        Assert.Equal(0, details["max"]!.GetValue<int>());
        Assert.Equal(8, details["cores"]!.GetValue<int>());
        Assert.False(details["per_core"]!.GetValue<bool>());
        Assert.Equal(8, details["bios"]!.AsArray().Count);
    }

    [Fact]
    public void Validate_rejects_a_core_key_that_is_not_a_number()
    {
        Assert.False(Provider(new FakeCo()).Validate(Co(perCore: new() { ["core3"] = -10 })).Ok);
    }

    [Fact]
    public void Curve_opt_part_round_trips_through_json()
    {
        var json = JsonSerializer.Serialize(Co(allCore: -15, perCore: new() { ["2"] = -20 }), ControlJson.Options);
        var back = JsonSerializer.Deserialize<ProfilePart>(json, ControlJson.Options)!;

        Assert.Contains("\"curve_opt\":{\"all_core\":-15,\"per_core\":{\"2\":-20}}", json);
        Assert.Equal(-20, back.CurveOpt!.PerCore!["2"]);
    }

    [Fact]
    public void Windows_reports_a_physical_core_count()
    {
        Assert.InRange(CurveOptimizerProvider.WindowsPhysicalCores(), 1, Environment.ProcessorCount);
    }
}

/// <summary>Curve Optimizer SMU encoding (Zen 4/5 RSMU).</summary>
public class CurveOptimizerEncodingTests
{
    [Fact]
    public void Core_masks_put_the_ccd_and_core_in_the_top_bits()
    {
        // The masks the 9800X3D answered to.
        Assert.Equal(0x00000000u, AmdSmuMap.CoreMask(0, 0));
        Assert.Equal(0x00700000u, AmdSmuMap.CoreMask(0, 7));
        Assert.Equal(0x10300000u, AmdSmuMap.CoreMask(1, 3));
    }

    [Fact]
    public void Offsets_are_16_bit_twos_complement_under_the_mask()
    {
        Assert.Equal(0x0030FFECu, AmdSmuMap.CurveOptimizerArg(AmdSmuMap.CoreMask(0, 3), -20));
        Assert.Equal(0x0000FFE2u, AmdSmuMap.CurveOptimizerArg(0, -30));
        Assert.Equal(0x00000005u, AmdSmuMap.CurveOptimizerArg(0, 5));
    }

    [Theory]
    [InlineData(0x00000000u, 0)]
    [InlineData(0x0000FFECu, -20)]
    [InlineData(0xABCDFFE2u, -30)] // upper bits are not part of the value
    [InlineData(0x00000005u, 5)]
    public void Readback_decodes_the_low_16_bits_signed(uint raw, int expected)
    {
        Assert.Equal(expected, AmdSmuMap.CurveOptimizerValue(raw));
    }

    [Fact]
    public void Only_verified_generations_have_curve_optimizer_ids()
    {
        Assert.Equal(new CurveOptimizerCommands(0x6, 0x7, 0xD5), AmdSmuMap.CurveOptimizer("Granite Ridge"));
        Assert.Null(AmdSmuMap.CurveOptimizer("Vermeer"));
        Assert.Null(AmdSmuMap.CurveOptimizer("Raphael"));
    }
}
