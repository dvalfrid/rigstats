using System.Text.Json;
using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Every captured Aura controller in <c>fixtures/aura/</c>: its config
/// table must yield the zones the board really has. Adding a board from a
/// diagnostics export is a new JSON file, no code (see the README there).
/// </summary>
public class AuraFixtureTests
{
    private static readonly string Root = Path.Combine(AppContext.BaseDirectory, "fixtures", "aura");

    public static IEnumerable<object[]> Fixtures() =>
        Directory.Exists(Root)
            ? Directory.EnumerateFiles(Root, "*.json").Select(f => new object[] { Path.GetFileName(f) })
            : [];

    [Theory]
    [MemberData(nameof(Fixtures))]
    public void Config_table_yields_the_boards_zones(string file)
    {
        var fixture = JsonDocument.Parse(File.ReadAllText(Path.Combine(Root, file))).RootElement;
        var productId = Convert.ToUInt16(fixture.GetProperty("product_id").GetString(), 16);
        var table = Convert.FromHexString(fixture.GetProperty("config_table").GetString()!);
        Assert.Equal(60, table.Length);

        var family = AuraUsb.Family(productId);
        Assert.NotNull(family); // a fixture is always a known controller

        var zones = AuraUsb.Zones(family.Value, table);
        var expected = fixture.GetProperty("expected_zones").EnumerateArray().ToList();
        Assert.Equal(expected.Count, zones.Count);
        for (var i = 0; i < zones.Count; i++)
        {
            Assert.Equal(expected[i].GetProperty("id").GetString(), zones[i].Id);
            Assert.Equal(expected[i].GetProperty("addressable").GetBoolean(), zones[i].Addressable);
            Assert.Equal(expected[i].GetProperty("effect_channel").GetInt32(), zones[i].EffectChannel);
            Assert.Equal(expected[i].GetProperty("leds").GetInt32(), zones[i].Leds);
        }
    }

    [Fact]
    public void There_is_at_least_one_fixture()
    {
        Assert.NotEmpty(Fixtures());
    }
}

/// <summary>ASUS Aura USB packet encoding.</summary>
public class AuraUsbTests
{
    private static byte[] Reply(params byte[] body)
    {
        var report = new byte[65];
        body.CopyTo(report, 0);
        return report;
    }

    [Theory]
    [InlineData((ushort)0x19AF, AuraFamily.Motherboard)]
    [InlineData((ushort)0x1BED, AuraFamily.Motherboard)]
    [InlineData((ushort)0x18F3, AuraFamily.Motherboard)]
    [InlineData((ushort)0x1867, AuraFamily.Addressable)]
    public void Known_controllers_have_a_family(ushort pid, AuraFamily family)
    {
        Assert.Equal(family, AuraUsb.Family(pid));
    }

    [Fact]
    public void Other_asus_devices_are_not_controllers()
    {
        Assert.Null(AuraUsb.Family(0x1ACE)); // a keyboard
        Assert.Null(AuraUsb.Family(0x1BA3)); // a monitor's Aura device
    }

    [Fact]
    public void Requests_are_65_byte_reports_with_id_0xEC()
    {
        Assert.Equal(new byte[] { 0xEC, 0x82 }, AuraUsb.FirmwareRequest()[..2]);
        Assert.Equal(new byte[] { 0xEC, 0xB0 }, AuraUsb.ConfigTableRequest()[..2]);
        Assert.Equal(65, AuraUsb.ConfigTableRequest().Length);
    }

    [Fact]
    public void Firmware_and_config_replies_parse()
    {
        var fw = Reply(0xEC, 0x02);
        System.Text.Encoding.ASCII.GetBytes("AULA3-AR32-0304").CopyTo(fw, 2);
        Assert.Equal("AULA3-AR32-0304", AuraUsb.ParseFirmware(fw));

        var cfg = Reply(0xEC, 0x30, 0x00, 0x00, 0xAA);
        Assert.Equal(0xAA, AuraUsb.ParseConfigTable(cfg)![0]);
        Assert.Null(AuraUsb.ParseConfigTable(Reply(0xEC, 0x02)));
    }

    [Fact]
    public void Motherboard_zones_put_onboard_leds_first()
    {
        var table = new byte[60];
        table[0x02] = 2; // ARGB headers
        table[0x1B] = 5; // onboard LEDs

        var zones = AuraUsb.Zones(AuraFamily.Motherboard, table);

        Assert.Equal(["mainboard", "argb1", "argb2"], zones.Select(z => z.Id));
        Assert.Equal([0, 1, 2], zones.Select(z => z.EffectChannel));
        Assert.Equal(5, zones[0].Leds);
    }

    [Fact]
    public void Motherboard_effect_is_one_effect_and_one_colour_report_per_zone()
    {
        var zones = new[]
        {
            new AuraZone("mainboard", "Motherboard", false, 0, 2),
            new AuraZone("argb1", "ARGB header 1", true, 1, 1),
        };

        var reports = AuraUsb.SetEffect(AuraFamily.Motherboard, zones, AuraEffect.Static, 0xFF, 0x00, 0x33);

        Assert.Equal(4, reports.Count);
        Assert.Equal(new byte[] { 0xEC, 0x35, 0x00, 0x00, 0x00, 0x01 }, reports[0][..6]); // zone 0, static
        // Colour: mask 0b11 over the first two LEDs, RGB from offset 5.
        Assert.Equal(new byte[] { 0xEC, 0x36, 0x00, 0x03, 0x00, 0xFF, 0x00, 0x33, 0xFF, 0x00, 0x33 }, reports[1][..11]);
        Assert.Equal(new byte[] { 0xEC, 0x35, 0x01, 0x00, 0x00, 0x01 }, reports[2][..6]);
        // Third LED overall: mask bit 2, colour at 5 + 3*2.
        Assert.Equal(new byte[] { 0xEC, 0x36, 0x00, 0x04 }, reports[3][..4]);
        Assert.Equal(new byte[] { 0xFF, 0x00, 0x33 }, reports[3][11..14]);
    }

    [Fact]
    public void Off_sends_mode_zero_and_black()
    {
        var zones = new[] { new AuraZone("argb1", "ARGB header 1", true, 0, 1) };

        var reports = AuraUsb.SetEffect(AuraFamily.Motherboard, zones, AuraEffect.Off, 0xFF, 0xFF, 0xFF);

        Assert.Equal(0x00, reports[0][5]);
        Assert.Equal(new byte[] { 0, 0, 0 }, reports[1][5..8]);
    }

    [Fact]
    public void Addressable_effect_carries_the_colour_in_one_report()
    {
        var zones = new[] { new AuraZone("argb1", "ARGB header 1", true, 0, 1), new AuraZone("argb2", "ARGB header 2", true, 1, 1) };

        var reports = AuraUsb.SetEffect(AuraFamily.Addressable, zones, AuraEffect.Breathing, 1, 2, 3);

        Assert.Equal(2, reports.Count);
        Assert.Equal(new byte[] { 0xEC, 0x3B, 0x01, 0x00, 0x02, 1, 2, 3 }, reports[1][..8]);
    }

    [Fact]
    public void Colours_parse_and_scale()
    {
        Assert.Equal(((byte)0xFF, (byte)0x00, (byte)0x33), AuraUsb.ParseColor("#ff0033"));
        Assert.Null(AuraUsb.ParseColor("red"));
        Assert.Null(AuraUsb.ParseColor("#gg0000"));
        Assert.Equal(((byte)128, (byte)0, (byte)26), AuraUsb.Scale(0xFF, 0x00, 0x33, 0.5));
    }
}

/// <summary>
/// Lighting (#192): Aura Sync — a profile's effect goes to every device;
/// Armoury Crate disables the domain instead of fighting it; one failing
/// device doesn't keep the others dark; a profile without a lighting part
/// leaves the lights alone.
/// </summary>
public class LightingProviderTests
{
    private sealed class FakeDevice(string id = "aura-usb-19af", bool fails = false) : ILightingDevice
    {
        public string Id => id;
        public string Name => $"Device {id}";
        public string Kind => "motherboard";
        public string Firmware => "AULA3-AR32-0304";
        public IReadOnlyList<AuraZone> Zones { get; } =
            [new("argb1", "ARGB header 1", true, 0, 1), new("argb2", "ARGB header 2", true, 1, 1)];
        public List<(AuraEffect Effect, byte R, byte G, byte B)> Applied { get; } = [];

        public void Apply(AuraEffect effect, byte red, byte green, byte blue)
        {
            if (fails)
                throw new IOException("unplugged");
            Applied.Add((effect, red, green, blue));
        }
    }

    private static LightingProvider Provider(params ILightingDevice[] devices) =>
        new(devices, "No lighting controller found.", () => null, dryRun: false);

    private static ProfilePart Lights(string? effect, string? color = null, double? brightness = null) =>
        new() { Aura = new AuraPart { Effect = effect, Color = color, Brightness = brightness } };

    [Fact]
    public void Probe_lists_every_device_and_its_zones()
    {
        var details = Provider(new FakeDevice(), new FakeDevice("monitor-1")).Probe().Details!;

        var devices = details["devices"]!.AsArray();
        Assert.Equal(2, devices.Count);
        Assert.Equal("AULA3-AR32-0304", devices[0]!["firmware"]!.GetValue<string>());
        Assert.Equal(2, devices[0]!["zones"]!.AsArray().Count);
        Assert.Contains("spectrum_cycle", details["effects"]!.AsArray().Select(e => e!.GetValue<string>()));
    }

    [Fact]
    public void Aura_sync_sends_the_effect_to_every_device_scaled_by_brightness()
    {
        var board = new FakeDevice();
        var monitor = new FakeDevice("monitor-1");

        Provider(board, monitor).Apply(Lights("breathing", "#ff0033", 0.5));

        Assert.Equal((AuraEffect.Breathing, (byte)128, (byte)0, (byte)26), Assert.Single(board.Applied));
        Assert.Equal(board.Applied, monitor.Applied);
    }

    [Fact]
    public void One_failing_device_does_not_keep_the_others_dark()
    {
        var gone = new FakeDevice("gone", fails: true);
        var board = new FakeDevice();

        Provider(gone, board).Apply(Lights("static", "#00ff00")); // no throw

        Assert.Single(board.Applied);
    }

    [Fact]
    public void Every_device_failing_fails_the_apply()
    {
        Assert.Throws<InvalidOperationException>(() =>
            Provider(new FakeDevice("a", fails: true)).Apply(Lights("static")));
    }

    [Fact]
    public void Armoury_crate_disables_the_domain_with_a_reason_and_no_writes()
    {
        var board = new FakeDevice();
        var provider = new LightingProvider([board], "", () => "Armoury Crate (LightingService)", dryRun: false);

        var caps = provider.Probe();
        provider.Apply(Lights("static", "#ff0000"));

        Assert.False(caps.Supported);
        Assert.Contains("Armoury Crate", caps.Reason);
        Assert.True(caps.Details!["conflict"]!.GetValue<bool>()); // shown, not hidden
        Assert.Empty(board.Applied);
    }

    [Fact]
    public void No_device_is_unsupported_with_the_reason_and_hidden()
    {
        var caps = Provider().Probe();

        Assert.False(caps.Supported);
        Assert.Equal("No lighting controller found.", caps.Reason);
        Assert.Null(caps.Details); // no conflict flag → the tab stays hidden
    }

    [Fact]
    public void A_profile_without_lighting_leaves_the_lights_alone()
    {
        var board = new FakeDevice();

        Provider(board).Apply(new ProfilePart { PowerPlan = "balanced" });

        Assert.Empty(board.Applied);
    }

    [Fact]
    public void Restore_puts_back_what_this_service_last_set()
    {
        var board = new FakeDevice();
        var provider = Provider(board);
        provider.Apply(Lights("static", "#00ff00"));
        var snapshot = provider.Capture();
        provider.Apply(Lights("off"));

        provider.Restore(snapshot);

        Assert.Equal((AuraEffect.Static, (byte)0, (byte)255, (byte)0), board.Applied[^1]);
    }

    [Fact]
    public void Validate_rejects_unknown_effects_and_bad_colours()
    {
        var provider = Provider(new FakeDevice());

        Assert.False(provider.Validate(Lights("disco")).Ok);
        Assert.False(provider.Validate(Lights("static", "red")).Ok);
        Assert.True(provider.Validate(Lights("spectrum_cycle")).Ok);
    }

    [Fact]
    public void Dry_run_never_writes_and_preview_ignores_invalid_parts()
    {
        var board = new FakeDevice();
        new LightingProvider([board], "", () => null, dryRun: true).Apply(Lights("static", "#ffffff"));
        Assert.Empty(board.Applied);

        var live = Provider(board);
        live.Preview(new AuraPart { Effect = "disco" });
        Assert.Empty(board.Applied);
        live.Preview(new AuraPart { Effect = "off" });
        Assert.Single(board.Applied);
    }

    [Fact]
    public void Resolve_defaults_to_full_white_static()
    {
        Assert.Equal((AuraEffect.Static, (byte)255, (byte)255, (byte)255), LightingProvider.Resolve(new AuraPart()));
    }

    [Fact]
    public void Aura_part_round_trips_through_json()
    {
        var json = JsonSerializer.Serialize(Lights("static", "#ff0033", 0.8), ControlJson.Options);

        Assert.Contains("\"aura\":{\"effect\":\"static\",\"color\":\"#ff0033\",\"brightness\":0.8}", json);
    }
}
