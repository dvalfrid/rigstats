using System.Text.Json;
using System.Text.Json.Nodes;
using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Headset settings (#428): the GearLink framing in <see cref="HeadsetCommands"/>
/// and <see cref="LightingProvider"/>'s headset methods with a fake headset.
/// </summary>
public class HeadsetSettingsTests
{
    private static readonly HeadsetLimits DeltaII = new(SidetoneMax: 20, NoiseLevels: 3);
    private static readonly HeadsetLimits Pelta = new(SidetoneMax: 100, NoiseLevels: 0);

    private static byte[] Reply(byte command, byte key, params byte[] data)
    {
        var reply = new byte[64];
        reply[0] = 0xCC;
        reply[1] = command;
        reply[2] = key;
        data.CopyTo(reply, 5);
        return reply;
    }

    [Fact]
    public void Every_settings_model_is_a_headset_RIGStats_finds_and_verified_ones_are_listed()
    {
        Assert.All(AsusHeadsetDevice.SettingsModels.Keys, id => Assert.NotNull(AsusHeadsetDevice.Model(id)));
        Assert.All(AsusHeadsetDevice.VerifiedSettingsModels, id => Assert.Contains(id, AsusHeadsetDevice.SettingsModels.Keys));
        Assert.All(AsusHeadsetDevice.SettingsModels.Values, l => Assert.InRange(l.NoiseLevels, 0, 3));
    }

    [Fact]
    public void Only_documented_commands_are_read()
    {
        Assert.Equal([(0x12, 36), (0x12, 25), (0x41, 32)],
            HeadsetCommands.Reads.Select(r => ((int)r.Command, (int)r.Key)));
    }

    [Fact]
    public void Replies_parse_into_settings_clamped_to_the_model()
    {
        var settings = HeadsetCommands.Parse(
            Reply(0x12, 36, 1), Reply(0x12, 25, 30), Reply(0x41, 32, 1, 5), DeltaII);

        Assert.Equal(new HeadsetSettings(true, 20, true, 2), settings);
    }

    [Fact]
    public void A_model_without_noise_levels_reads_level_zero()
    {
        var settings = HeadsetCommands.Parse(
            Reply(0x12, 36, 0), Reply(0x12, 25, 64), Reply(0x41, 32, 1, 2), Pelta);

        Assert.Equal(new HeadsetSettings(false, 64, true, 0), settings);
    }

    [Fact]
    public void A_short_reply_reads_nothing()
    {
        Assert.Null(HeadsetCommands.Parse([0xCC, 0x12, 36], Reply(0x12, 25), Reply(0x41, 32), DeltaII));
    }

    [Fact]
    public void Only_what_changed_is_written_switch_before_level()
    {
        var now = new HeadsetSettings(false, 5, false, 0);
        var wanted = new HeadsetSettings(true, 12, true, 2);

        var writes = HeadsetCommands.Writes(now, wanted, DeltaII);

        Assert.Equal(
            ["41 11 01", "61 11 0C", "41 02 01", "41 10 02"],
            writes.Select(w => $"{w.Command:X2} {w.Key:X2} {Convert.ToHexString(w.Data)}"));
        Assert.Empty(HeadsetCommands.Writes(now, now, DeltaII));
        Assert.Equal(["61 11 07"],
            HeadsetCommands.Writes(now, now with { SidetoneLevel = 7 }, DeltaII)
                .Select(w => $"{w.Command:X2} {w.Key:X2} {Convert.ToHexString(w.Data)}"));
    }

    [Fact]
    public void A_model_without_noise_levels_never_gets_a_level_write()
    {
        var writes = HeadsetCommands.Writes(new(false, 0, false, 0), new(false, 0, true, 2), Pelta);

        Assert.Equal(["41 02 01"], writes.Select(w => $"{w.Command:X2} {w.Key:X2} {Convert.ToHexString(w.Data)}"));
    }

    [Fact]
    public void Values_are_clamped_to_the_model()
    {
        Assert.Equal(new HeadsetSettings(true, 20, true, 2), HeadsetCommands.Clamp(new(true, 99, true, 9), DeltaII));
        Assert.Equal(new HeadsetSettings(true, 0, true, 0), HeadsetCommands.Clamp(new(true, -3, true, 2), Pelta));
    }

    [Fact]
    public void The_pipe_shape_deserializes_into_settings()
    {
        var settings = JsonSerializer.Deserialize<HeadsetSettings>(
            """{"sidetone_on":true,"sidetone_level":8,"noise_reduction_on":false,"noise_reduction_level":1}""",
            ControlJson.Options);

        Assert.Equal(new HeadsetSettings(true, 8, false, 1), settings);
    }

    [Fact]
    public void Headsets_lists_only_devices_with_known_settings()
    {
        var headset = new FakeHeadset();
        var provider = Provider(headset, new FakeHeadset("other") { KnownLimits = null });

        var list = provider.Headsets();

        var only = Assert.Single(list)!;
        Assert.Equal("asus-headset-1afa-1", only["id"]!.GetValue<string>());
        Assert.Equal(20, only["sidetone_max"]!.GetValue<int>());
        Assert.Equal(3, only["noise_levels"]!.GetValue<int>());
        Assert.False(only["settings"]!["sidetone_on"]!.GetValue<bool>());
    }

    [Fact]
    public void A_silent_headset_is_listed_without_settings()
    {
        var provider = Provider(new FakeHeadset { Silent = true });

        Assert.Null(Assert.Single(provider.Headsets())!["settings"]);
    }

    [Fact]
    public void Set_writes_clamped_values_and_answers_with_the_read_back()
    {
        var headset = new FakeHeadset();
        var provider = Provider(headset);

        var answer = provider.SetHeadset(headset.Id, new HeadsetSettings(true, 50, true, 9));

        Assert.Equal(new HeadsetSettings(true, 20, true, 2), headset.Written.Single());
        Assert.Equal(20, answer["settings"]!["sidetone_level"]!.GetValue<int>());
    }

    [Fact]
    public void Dry_run_writes_nothing()
    {
        var headset = new FakeHeadset();
        var provider = Provider(headset, dryRun: true);

        provider.SetHeadset(headset.Id, new HeadsetSettings(true, 5, false, 0));

        Assert.Empty(headset.Written);
    }

    [Fact]
    public void An_unknown_headset_is_refused()
    {
        Assert.Throws<InvalidOperationException>(() =>
            Provider(new FakeHeadset()).SetHeadset("nope", new HeadsetSettings(true, 1, true, 1)));
    }

    [Fact]
    public void A_headset_that_did_not_take_the_settings_fails_the_request()
    {
        var headset = new FakeHeadset { Refuses = true };

        Assert.Throws<IOException>(() =>
            Provider(headset).SetHeadset(headset.Id, new HeadsetSettings(true, 3, false, 0)));
    }

    private static LightingProvider Provider(params ILightingDevice[] devices) => Provider(devices, dryRun: false);

    private static LightingProvider Provider(FakeHeadset headset, bool dryRun) => Provider([headset], dryRun);

    private static LightingProvider Provider(ILightingDevice[] devices, bool dryRun) =>
        new(devices, "", () => null, dryRun);

    private sealed class FakeHeadset(string suffix = "1afa-1") : ILightingDevice, IHeadsetSettingsDevice
    {
        public HeadsetLimits? KnownLimits { get; init; } = new(20, 3);
        public bool Silent { get; init; }
        public bool Refuses { get; init; }
        public HeadsetSettings Current { get; private set; } = new(false, 4, false, 1);
        public List<HeadsetSettings> Written { get; } = [];

        public string Id => $"asus-headset-{suffix}";
        public string Name => "ROG Delta II";
        public string Kind => "headset";
        public string Firmware => "";
        public string? Blocked => null;
        public IReadOnlyList<AuraZone> Zones { get; } = [new("lights", "Lights", false, 0, 1)];
        public void Apply(AuraEffect effect, byte red, byte green, byte blue) { }
        public bool Release() => false;
        public JsonObject Diagnostics() => new();

        public HeadsetLimits? Limits => KnownLimits;
        public bool SettingsVerified => false;
        public HeadsetSettings? ReadSettings() => Silent ? null : Current;

        public HeadsetSettings WriteSettings(HeadsetSettings wanted)
        {
            if (Refuses)
                throw new IOException("ROG Delta II did not take the settings.");
            Written.Add(wanted);
            Current = wanted;
            return Current;
        }
    }
}
