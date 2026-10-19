using System.Text.Json;
using SensorSidecar.Control;
using SensorSidecar.Control.Headsets;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// The headset equalizer (#483): the <c>12 21</c> reply, the <c>41 03</c> /
/// <c>41 06</c> writes, the presets, and the <c>headset</c> profile domain
/// with a fake headset.
/// </summary>
public class HeadsetEqTests
{
    private static readonly HeadsetEqLimits Delta = HeadsetEqCommands.Delta;
    private static readonly HeadsetEqLimits Pelta = HeadsetEqCommands.Pelta;

    [Fact]
    public void The_status_reply_gives_on_off_and_ten_gains()
    {
        // G-Helper's own test reply shape for 12 21: on, ten gains at 50.
        var reply = Convert.FromHexString("CC12210000" + "01" + "32323232323232323232" + "00000000");
        var eq = HeadsetEqCommands.Parse(reply, 0xCC, Delta)!;
        Assert.True(eq.On);
        Assert.Equal(Enumerable.Repeat(50, 10), eq.Gains);

        Assert.Null(HeadsetEqCommands.Parse(reply, 0x01, Delta)); // another report id
        Assert.Null(HeadsetEqCommands.Parse(Convert.FromHexString("CC12240000" + "01" + "32323232323232323232"), 0xCC, Delta)); // sidetone
        Assert.Null(HeadsetEqCommands.Parse(Convert.FromHexString("CC1221000001"), 0xCC, Delta)); // short
        // A gain above the model's scale reads as its top.
        Assert.Equal(24, HeadsetEqCommands.Parse(Convert.FromHexString("CC12210000" + "00" + "FF0C0C0C0C0C0C0C0C0C"), 0xCC, Pelta)!.Gains[0]);
    }

    [Fact]
    public void Only_what_differs_is_written_switch_first_band_by_band()
    {
        var flat = new HeadsetEq(false, Enumerable.Repeat(50, 10).ToArray());
        var wanted = HeadsetEqCommands.Clamp(new HeadsetEq(true, [50, 50, 90, 50, 50, 50, 50, 50, 50, 8]), Delta);

        var writes = HeadsetEqCommands.Writes(flat, wanted, Delta);

        Assert.Equal(["4103-01", "4106-025A", "4106-0908"],
            writes.Select(w => $"{w.Command:X2}{w.Key:X2}-{Convert.ToHexString(w.Data)}"));
        Assert.Empty(HeadsetEqCommands.Writes(wanted, wanted, Delta));
    }

    [Fact]
    public void Clamp_keeps_the_scale_and_the_unused_bands_flat()
    {
        var eq = HeadsetEqCommands.Clamp(new HeadsetEq(true, [0, 99, 120, -5, 50, 50, 50, 50, 50, 50]), Delta);
        Assert.Equal([50, 50, 100, 0, 50, 50, 50, 50, 50, 50], eq.Gains);
        // Too few gains: the rest flat.
        Assert.Equal(Enumerable.Repeat(12, 10), HeadsetEqCommands.Clamp(new HeadsetEq(true, []), Pelta).Gains);
        // The Delta II family's 32 and 64 Hz don't count when comparing.
        Assert.True(new HeadsetEq(true, [0, 0, 50, 50, 50, 50, 50, 50, 50, 50]).SameAs(new HeadsetEq(true, Enumerable.Repeat(50, 10).ToArray()), Delta));
    }

    [Fact]
    public void Every_preset_has_ten_gains_inside_its_scale()
    {
        foreach (var limits in new[] { Delta, Pelta })
        {
            Assert.All(limits.Presets, p => Assert.Equal(10, p.Gains.Length));
            Assert.All(limits.Presets, p => Assert.All(p.Gains, g => Assert.InRange(g, 0, limits.Max)));
            Assert.All(limits.Presets.Skip(0), p => Assert.True(p.Gains.Take(limits.FirstBand).All(g => g == limits.Flat)));
            Assert.Equal(limits.Presets.Count, limits.Presets.Select(p => p.Name).Distinct().Count());
        }
        Assert.Equal("FPS", HeadsetEqCommands.Preset(Pelta, "fps")!.Name);
        Assert.Null(HeadsetEqCommands.Preset(Delta, "FPS"));
    }

    [Fact]
    public void A_profile_switches_every_headset_that_has_the_preset()
    {
        var delta = new FakeHeadset("delta", Delta);
        var pelta = new FakeHeadset("pelta", Pelta);
        var provider = new HeadsetProvider(() => [delta, pelta], dryRun: false);
        var part = new ProfilePart { Headset = new HeadsetPart { EqPreset = "Rock" } };

        provider.Apply(part);

        Assert.Equal(HeadsetEqCommands.Preset(Delta, "Rock")!.Gains, delta.Eq.Gains);
        Assert.Equal(HeadsetEqCommands.Preset(Pelta, "Rock")!.Gains, pelta.Eq.Gains);
        Assert.True(delta.Eq.On);
        Assert.True(provider.Verify(part));

        // FPS is a Pelta preset only: the Delta II keeps what it has.
        var before = delta.Eq;
        provider.Apply(new ProfilePart { Headset = new HeadsetPart { EqPreset = "FPS" } });
        Assert.Same(before, delta.Eq);
        Assert.Equal(HeadsetEqCommands.Preset(Pelta, "FPS")!.Gains, pelta.Eq.Gains);
    }

    [Fact]
    public void A_silent_headset_is_skipped_and_restore_puts_the_eq_back()
    {
        var off = new FakeHeadset("off", Delta) { Silent = true };
        var on = new FakeHeadset("on", Delta);
        var provider = new HeadsetProvider(() => [off, on], dryRun: false);
        var snapshot = provider.Capture();

        Assert.Throws<DeviceAsleepException>(() => provider.Apply(new ProfilePart { Headset = new HeadsetPart { EqPreset = "Vocal" } }));
        Assert.Equal(HeadsetEqCommands.Preset(Delta, "Vocal")!.Gains, on.Eq.Gains);

        provider.Restore(snapshot);
        Assert.False(on.Eq.On);
        Assert.Equal(Enumerable.Repeat(50, 10), on.Eq.Gains);

        Assert.Throws<DomainUnavailableException>(() =>
            new HeadsetProvider(() => [], dryRun: false).Apply(new ProfilePart { Headset = new HeadsetPart { EqPreset = "Rock" } }));
        Assert.False(provider.Affects(new ProfilePart()));
        Assert.False(provider.Validate(new ProfilePart { Headset = new HeadsetPart { EqPreset = " " } }).Ok);
    }

    [Fact]
    public void The_part_and_the_set_request_have_their_wire_shapes()
    {
        var part = JsonSerializer.Deserialize<ProfilePart>("""{"headset":{"eq_preset":"FPS"}}""", ControlJson.Options)!;
        Assert.Equal("FPS", part.Headset!.EqPreset);
        Assert.Contains("\"eq_preset\":\"FPS\"", JsonSerializer.Serialize(part, ControlJson.Options), StringComparison.Ordinal);

        var eq = JsonSerializer.Deserialize<HeadsetEq>("""{"on":true,"gains":[50,50,60,50,50,50,50,50,50,40]}""", ControlJson.Options)!;
        Assert.True(eq.On);
        Assert.Equal(60, eq.Gains[2]);
    }

    private sealed class FakeHeadset(string id, HeadsetEqLimits limits) : IHeadsetEqDevice
    {
        public string Id => id;
        public string Name => $"ROG headset {id}";
        public HeadsetEqLimits? EqLimits => limits;
        public bool EqVerified => false;
        public bool Silent { get; init; }
        public HeadsetEq Eq { get; private set; } = new(false, Enumerable.Repeat(limits.Flat, 10).ToArray());

        public HeadsetEq? ReadEq() => Silent ? null : Eq;

        public HeadsetEq WriteEq(HeadsetEq wanted)
        {
            if (Silent)
                throw new IOException($"{Name} didn't answer.");
            return Eq = HeadsetEqCommands.Clamp(wanted, limits);
        }
    }
}
