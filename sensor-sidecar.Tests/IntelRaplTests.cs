using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Intel RAPL decode/encode (#209) against the worked examples in
/// docs/intel-power-limits.md (U1, T1–T5, P1–P5), all with units 0x000A0E03.
/// </summary>
public sealed class IntelRaplTests
{
    private static readonly RaplUnits Units = IntelRapl.Units(0x000A0E03);

    // P1: PL1 125 W on + clamp, Tau 56 s; PL2 253 W on + clamp; unlocked.
    private const ulong P1 = 0x000187E8_00DF83E8;

    [Fact]
    public void Units_decode_the_typical_core_value()
    {
        Assert.Equal(new RaplUnits(3, 14, 10), Units);
        Assert.Equal(0.125, Units.PowerW);
        Assert.Equal(1.0 / 1024, Units.TimeS);
    }

    [Theory]
    [InlineData(0x0A, 1.0)]
    [InlineData(0x0B, 2.0)]
    [InlineData(0x4B, 3.0)]
    [InlineData(0x6E, 28.0)]
    [InlineData(0x6F, 56.0)]
    public void Time_windows_decode_as_in_the_SDM_and_coreboot(int raw, double seconds) =>
        Assert.Equal(seconds, IntelRapl.TimeWindowSeconds(raw, Units));

    [Fact]
    public void Power_limit_decodes_every_field()
    {
        var l = IntelRapl.Decode(P1);
        Assert.Equal(new PkgPowerLimit(1000, true, true, 0x6F, 2024, true, true, 0, false), l);
        Assert.Equal(125.0, IntelRapl.Watts(l.Pl1Raw, Units));
        Assert.Equal(253.0, IntelRapl.Watts(l.Pl2Raw, Units));
    }

    [Fact]
    public void An_unlimited_board_reads_the_field_maximum()
    {
        var l = IntelRapl.Decode(0x0001FFFF_00DFFFFF);
        Assert.Equal(4095.875, IntelRapl.Watts(l.Pl1Raw, Units));
        Assert.Equal(4095.875, IntelRapl.Watts(l.Pl2Raw, Units));
    }

    [Fact]
    public void The_lock_bit_is_read()
    {
        Assert.True(IntelRapl.Decode(0x800187E8_00DF83E8).Locked);
        Assert.False(IntelRapl.Decode(P1).Locked);
    }

    [Fact]
    public void New_limits_change_only_the_two_fields()
    {
        var raw = IntelRapl.WithLimits(P1, IntelRapl.PowerToRaw(95, Units), IntelRapl.PowerToRaw(200, Units));
        Assert.Equal(0x00018640_00DF82F8UL, raw);
    }

    [Fact]
    public void New_limits_never_lock_and_always_enable()
    {
        // P1 locked, both limits disabled.
        var lockedOff = 0x800187E8_00DF83E8UL & ~((1UL << 15) | (1UL << 47));
        var l = IntelRapl.Decode(IntelRapl.WithLimits(lockedOff, 760, 1600));
        Assert.False(l.Locked);
        Assert.True(l.Pl1Enabled);
        Assert.True(l.Pl2Enabled);
        Assert.Equal(0x6F, l.Tau1Raw);
    }

    [Theory]
    [InlineData(87.3, 698u)] // P5: rounds down, 87.25 W
    [InlineData(125.0, 1000u)]
    [InlineData(-5.0, 0u)]
    [InlineData(1e9, 0x7FFFu)]
    public void Watts_round_down_into_the_field(double watts, uint raw) =>
        Assert.Equal(raw, IntelRapl.PowerToRaw(watts, Units));

    [Fact]
    public void Power_info_decodes_its_ranges()
    {
        // TDP 125 W, min 35 W, max 253 W, max window raw 0x2F.
        var raw = (0x2FUL << 48) | (2024UL << 32) | (280UL << 16) | 1000UL;
        Assert.Equal(new PkgPowerInfo(1000, 280, 2024, 0x2F), IntelRapl.DecodeInfo(raw));
    }

    [Fact]
    public void The_pinned_IntelMSR_hash_is_the_one_LHM_ships()
    {
        // Fails on an LHM upgrade that changes the module: check the new
        // build's ioctls and write list before moving the pin.
        using var blob = typeof(LibreHardwareMonitor.Hardware.Computer).Assembly
            .GetManifestResourceStream("LibreHardwareMonitor.Resources.PawnIo.IntelMSR.bin")!;
        Assert.Equal(IntelRapl.IntelMsrSha256, Convert.ToHexStringLower(System.Security.Cryptography.SHA256.HashData(blob)));
    }

    [Fact]
    public void A_module_with_another_hash_is_refused_before_the_driver_is_opened()
    {
        var e = Assert.Throws<InvalidOperationException>(() => PawnIoModule.LoadFromLhm("IntelMSR.bin", "00"));
        Assert.Contains("not the expected build", e.Message);
    }

    [Fact]
    public void Only_genuine_intel_is_recognised()
    {
        Assert.True(IntelRapl.IsGenuineIntel((0x23, 0x756E6547, 0x6C65746E, 0x49656E69)));
        Assert.False(IntelRapl.IsGenuineIntel((0x10, 0x68747541, 0x444D4163, 0x69746E65)));
    }
}
