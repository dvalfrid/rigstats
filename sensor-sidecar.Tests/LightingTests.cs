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
    private sealed class FakeDevice(string id = "aura-usb-19af", bool fails = false, string? blocked = null) : ILightingDevice, IDisposable
    {
        public bool Disposed { get; private set; }
        public void Dispose() => Disposed = true;
        public string? Blocked => blocked;
        public int Releases { get; private set; }
        public void Release() => Releases++;
        public System.Text.Json.Nodes.JsonObject Diagnostics() => new() { ["product_id"] = "19AF", ["config_table"] = "1E9F" };
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

    private sealed class FakeWdlMouse(bool verified = true) : ILightingDevice, IWdlDevice
    {
        public bool On { get; private set; }
        public string Id => "lamparray-0b05-1ace-2";
        public string Name => "ROG Harpe Ace Aim Lab Edition";
        public string Kind => "mouse";
        public string Firmware => "";
        public string? Blocked => null;
        public IReadOnlyList<AuraZone> Zones { get; } = [new("lamps", "1 lamp", true, 0, 1)];
        public List<(AuraEffect Effect, byte R, byte G, byte B)> Applied { get; } = [];
        public void Apply(AuraEffect effect, byte red, byte green, byte blue) => Applied.Add((effect, red, green, blue));
        public void Release() { }
        public System.Text.Json.Nodes.JsonObject Diagnostics() => new();
        public bool HasWdl => verified;
        public bool KnownModel => true;
        public bool? WdlOn() => On;
        public void EnableWdl() => On = true;
    }

    [Fact]
    public void Probe_reports_the_wdl_state_only_for_devices_that_offer_the_switch()
    {
        var devices = Provider(new FakeWdlMouse(), new FakeDevice()).Probe().Details!["devices"]!.AsArray();

        Assert.False(devices[0]!["wdl_on"]!.GetValue<bool>());
        Assert.True(devices[0]!["wdl_toggle"]!.GetValue<bool>());
        Assert.Null(devices[1]!["wdl_on"]);
        Assert.Null(devices[1]!["wdl_toggle"]);
    }

    [Fact]
    public void Enabling_wdl_switches_the_device_and_gives_it_the_current_lighting()
    {
        var mouse = new FakeWdlMouse();
        var provider = Provider(mouse);
        provider.Apply(Lights("static", "#00ff00"));
        mouse.Applied.Clear();

        provider.EnableWdl(mouse.Id);

        Assert.True(mouse.On);
        Assert.Equal((AuraEffect.Static, (byte)0, (byte)255, (byte)0), Assert.Single(mouse.Applied));
    }

    [Fact]
    public void Enabling_wdl_on_an_unverified_or_unknown_device_is_refused()
    {
        var unverified = new FakeWdlMouse(verified: false);
        var provider = Provider(unverified);

        Assert.Throws<InvalidOperationException>(() => provider.EnableWdl(unverified.Id));
        Assert.Throws<InvalidOperationException>(() => provider.EnableWdl("nope"));
        Assert.False(unverified.On);
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
    public void A_device_owned_by_windows_dynamic_lighting_is_skipped_and_explained()
    {
        var mouse = new FakeDevice("lamparray-0b05-1ace-1", blocked: "Controlled by Windows Dynamic Lighting.");
        var board = new FakeDevice();
        var provider = Provider(mouse, board);

        provider.Apply(Lights("static", "#ff0000"));

        Assert.Empty(mouse.Applied);
        Assert.Single(board.Applied);
        var devices = provider.Probe().Details!["devices"]!.AsArray();
        Assert.StartsWith("Controlled by Windows", devices[0]!["blocked"]!.GetValue<string>());
        Assert.Null(devices[1]!["blocked"]);
    }

    [Fact]
    public void Only_blocked_devices_is_not_a_failure()
    {
        Provider(new FakeDevice(blocked: "Windows")).Apply(Lights("static")); // no throw
    }

    [Fact]
    public void Release_hands_every_device_back()
    {
        var board = new FakeDevice();
        var mouse = new FakeDevice("mouse");

        Provider(board, mouse).ReleaseToFirmware();

        Assert.Equal(1, board.Releases);
        Assert.Equal(1, mouse.Releases);
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

    /// A machine whose HID collections and found devices the test changes,
    /// with a clock it moves.
    private sealed class Machine
    {
        public List<string> Paths { get; } = ["board"];
        public Func<List<ILightingDevice>> Found { get; set; } = () => [];
        public long Now { get; set; }
        public int Discoveries { get; private set; }
        public int Scanned { get; private set; }
        public Func<LightingScan>? Retry { get; set; }

        public LightingProvider Provider() => new(
            () => Paths.Select(p => new HidDeviceInfo(p, 0x0B05, 0x19AF, 0xFF72, 1, 65, 65)).ToList(),
            _ =>
            {
                Discoveries++;
                return new LightingScan(Found(), "No lighting controller found.", Retry);
            },
            () => null,
            dryRun: false,
            (_, _, _) => Scanned++,
            () => Now);
    }

    [Fact]
    public void Rescan_keeps_present_devices_and_closes_the_fresh_duplicates()
    {
        var board = new FakeDevice();
        var machine = new Machine { Found = () => [board] };
        var provider = machine.Provider();
        var duplicate = new FakeDevice();
        var keyboard = new FakeDevice("asus-keyboard-1c24-1");
        machine.Found = () => [duplicate, keyboard];
        machine.Paths.Add("keyboard");
        machine.Now = 5000;

        provider.Rescan();

        Assert.Equal(new ILightingDevice[] { board, keyboard }, provider.Devices);
        Assert.True(duplicate.Disposed);
        Assert.False(board.Disposed);
        Assert.Equal(2, machine.Scanned); // diagnostics rewritten
    }

    [Fact]
    public void A_new_device_gets_the_current_lighting_and_a_gone_one_is_closed()
    {
        var board = new FakeDevice();
        var receiver = new FakeDevice("asus-keyboard-1ace-1");
        var machine = new Machine { Found = () => [board, receiver] };
        var provider = machine.Provider();
        provider.Apply(Lights("static", "#ff0000"));

        // The keyboard switched from its receiver to the cable.
        var cable = new FakeDevice("asus-keyboard-1c24-1");
        machine.Found = () => [new FakeDevice(), cable];
        machine.Paths.Add("cable");
        machine.Now = 5000;
        provider.Probe();

        Assert.True(receiver.Disposed);
        Assert.Equal((AuraEffect.Static, (byte)255, (byte)0, (byte)0), Assert.Single(cable.Applied));
        Assert.Single(board.Applied); // not re-applied
    }

    [Fact]
    public void Rescan_is_throttled_and_skips_discovery_when_nothing_changed()
    {
        var machine = new Machine { Found = () => [new FakeDevice()] };
        var provider = machine.Provider();

        machine.Paths.Add("keyboard");
        machine.Now = 1000;
        provider.Rescan(); // within two seconds of the first scan
        Assert.Equal(1, machine.Discoveries);

        machine.Now = 3000;
        provider.Rescan();
        Assert.Equal(2, machine.Discoveries);

        machine.Now = 6000;
        provider.Rescan(); // same collections: no probing
        Assert.Equal(2, machine.Discoveries);
    }

    [Fact]
    public void A_headset_switched_on_behind_its_dongle_is_found_by_the_retry_alone()
    {
        var board = new FakeDevice();
        var headset = new FakeDevice("asus-headset-1afa-1");
        var headsetOn = false;
        var asked = 0;
        Func<LightingScan>? retry = null;
        retry = () =>
        {
            asked++;
            return headsetOn ? new LightingScan([headset], "") : new LightingScan([], "", retry);
        };
        var machine = new Machine { Found = () => [board] };
        machine.Retry = retry;
        var provider = machine.Provider();
        provider.Apply(Lights("static", "#0000ff"));

        machine.Now = 3000;
        provider.Probe(); // still off
        headsetOn = true;
        machine.Now = 6000;
        provider.Probe();
        machine.Now = 9000;
        provider.Probe(); // found: nothing left to ask

        Assert.Equal(1, machine.Discoveries); // the others were not probed again
        Assert.Equal(2, asked);
        Assert.Equal(new ILightingDevice[] { board, headset }, provider.Devices);
        Assert.Equal((AuraEffect.Static, (byte)0, (byte)0, (byte)255), Assert.Single(headset.Applied));
    }

    private sealed class FakeLightBar : ILightingDevice, ILampDevice
    {
        public string Id => "asus-monitor-1ac8-1";
        public string Name => "ROG Aura Monitor Light Bar";
        public string Kind => "light_bar";
        public string Firmware => "";
        public string? Blocked => null;
        public IReadOnlyList<AuraZone> Zones { get; } = [new("leds", "Light bar", true, 0, 3)];
        public bool HasLamp => true;
        public List<AuraEffect> Applied { get; } = [];
        public List<LampPart> Lamps { get; } = [];
        public void Apply(AuraEffect effect, byte red, byte green, byte blue) => Applied.Add(effect);
        public void SetLamp(LampPart lamp) => Lamps.Add(lamp);
        public bool Lit { get; set; }
        public bool LampOn() => Lit;
        public void SwitchLamp(bool on) => Lit = on;
        public void Release() { }
        public System.Text.Json.Nodes.JsonObject Diagnostics() => new();
    }

    [Fact]
    public void A_lamp_only_part_sets_the_lamp_and_leaves_the_rgb_alone()
    {
        var board = new FakeDevice();
        var bar = new FakeLightBar();
        var provider = Provider(board, bar);

        provider.Apply(new ProfilePart { Aura = new AuraPart { Lamp = new LampPart { On = false } } });

        Assert.Empty(board.Applied);
        Assert.Empty(bar.Applied);
        Assert.False(Assert.Single(bar.Lamps).On);
        var devices = provider.Probe().Details!["devices"]!.AsArray();
        Assert.False(devices[0]!["lamp"]!.GetValue<bool>());
        Assert.True(devices[1]!["lamp"]!.GetValue<bool>());
        Assert.Null(devices[0]!["lamp_on"]);
        Assert.False(devices[1]!["lamp_on"]!.GetValue<bool>()); // read from the device
        bar.Lit = true; // switched from the tray
        Assert.True(provider.Probe().Details!["devices"]![1]!["lamp_on"]!.GetValue<bool>());
    }

    [Fact]
    public void The_lamp_toggle_turns_every_lamp_off_when_any_is_lit()
    {
        var first = new FakeLightBar { Lit = true };
        var second = new FakeLightBar();
        var provider = Provider(new FakeDevice(), first, second);

        Assert.False(provider.ToggleLamp());
        Assert.False(first.Lit || second.Lit);
        Assert.True(provider.ToggleLamp());
        Assert.True(first.Lit && second.Lit);
        Assert.Null(Provider(new FakeDevice()).ToggleLamp()); // no lamp
    }

    [Fact]
    public void An_effect_and_a_lamp_set_both()
    {
        var bar = new FakeLightBar();

        Provider(bar).Apply(new ProfilePart
        {
            Aura = new AuraPart { Effect = "static", Lamp = new LampPart { On = true, Brightness = 0.5 } },
        });

        Assert.Equal(AuraEffect.Static, Assert.Single(bar.Applied));
        Assert.Single(bar.Lamps);
    }

    [Fact]
    public void Preview_never_rescans()
    {
        var machine = new Machine { Found = () => [new FakeDevice()] };
        var provider = machine.Provider();
        machine.Paths.Add("keyboard");
        machine.Now = 5000;

        provider.Preview(new AuraPart { Effect = "static" });

        Assert.Equal(1, machine.Discoveries);
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

/// <summary>
/// ASUS Aura monitors and the light bar (#212): direct-mode frames, and the
/// service-drawn breathing / spectrum effects.
/// </summary>
public class AsusMonitorTests
{
    [Theory]
    [InlineData((ushort)0x1BA3, "ROG Strix XG27AQDMG", "monitor")]
    [InlineData((ushort)0x1AC8, "ROG Aura Monitor Light Bar", "light_bar")]
    public void Current_family_models_are_known(ushort pid, string name, string kind)
    {
        var model = AsusMonitorDevice.Model(pid)!;
        Assert.Equal((name, kind), (model.Name, model.Kind));
    }

    [Fact]
    public void Old_feature_report_monitors_are_out_of_scope()
    {
        Assert.Null(AsusMonitorDevice.Model(0x198C)); // XG27AQ
        Assert.Null(AsusMonitorDevice.Model(0x19AF)); // the motherboard controller
    }

    [Theory]
    [InlineData(AuraEffect.Static, "EC3500000001000001FF0033")]
    [InlineData(AuraEffect.Breathing, "EC3500000002000001FF0033")] // verified: breathes on its own
    [InlineData(AuraEffect.SpectrumCycle, "EC3500000004000001000000")] // verified: cycles on its own
    [InlineData(AuraEffect.Off, "EC3500000001000001000000")]
    public void Built_in_effects_are_one_effect_report(AuraEffect effect, string start)
    {
        var report = AsusMonitorDevice.EffectReport(effect, 0xFF, 0x00, 0x33);

        Assert.Equal(65, report.Length);
        Assert.Equal(Hex(start), report[..12]);
        Assert.All(report[12..], b => Assert.Equal(0, b));
    }

    [Fact]
    public void Only_verified_models_use_their_own_effects()
    {
        Assert.True(AsusMonitorDevice.HasBuiltInEffects(0x1BA3)); // XG27AQDMG
        Assert.True(AsusMonitorDevice.HasBuiltInEffects(0x1AC8)); // light bar
        Assert.False(AsusMonitorDevice.HasBuiltInEffects(0x1B2B)); // PG32UCDM: drawn by the service
    }

    [Fact]
    public void The_lamp_report_is_the_static_effect_on_channel_1()
    {
        var report = AsusMonitorDevice.LampReport(12, 20);

        Assert.Equal(65, report.Length);
        Assert.Equal(new byte[] { 0xEC, 0x35, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 12, 20 }, report[..11]);
        Assert.All(report[11..], b => Assert.Equal(0, b));
    }

    [Fact]
    public void The_lamp_reads_back_from_the_get_effect_reply()
    {
        // Captured from the light bar with the lamp at 12/20.
        var reply = Hex("EC310110000127FFFF000C1400000000");

        Assert.Equal(((byte)12, (byte)20), AsusMonitorDevice.ParseLampReply(reply));
        Assert.Null(AsusMonitorDevice.ParseLampReply(Hex("EC300000001B00000000000000000000"))); // a config reply
    }

    [Theory]
    [InlineData(false, 1.0, 4000, 0, 0)]
    [InlineData(true, 1.0, 2700, 0, 178)] // full and warm: the 70 % budget, all warm
    [InlineData(true, 1.0, 6500, 178, 0)]
    [InlineData(true, 0.5, 4600, 44, 45)] // 89 split in the middle
    [InlineData(true, 0.0, 2700, 0, 1)] // on still lights
    [InlineData(true, 2.0, 9000, 178, 0)] // clamped
    public void Lamp_channels_split_brightness_by_temperature(bool on, double brightness, int kelvin, int cool, int warm)
    {
        Assert.Equal(((byte)cool, (byte)warm), Lamp.Channels(new LampPart { On = on, Brightness = brightness, Temperature = kelvin }));
    }

    private static byte[] Hex(string hex) => Convert.FromHexString(hex);

    [Fact]
    public void A_direct_frame_sets_every_led()
    {
        var frame = AsusMonitorDevice.DirectFrame(3, 0xFF, 0x00, 0x33);

        Assert.Equal(65, frame.Length);
        Assert.Equal(new byte[] { 0xEC, 0x40, 0x84, 0x00, 0x03 }, frame[..5]);
        Assert.Equal(new byte[] { 0xFF, 0x00, 0x33, 0xFF, 0x00, 0x33, 0xFF, 0x00, 0x33 }, frame[5..14]);
        Assert.All(frame[14..], b => Assert.Equal(0, b));
    }

    [Fact]
    public void Off_and_static_are_single_frames()
    {
        Assert.False(SoftwareEffect.IsAnimated(AuraEffect.Static));
        Assert.False(SoftwareEffect.IsAnimated(AuraEffect.Off));
        Assert.True(SoftwareEffect.IsAnimated(AuraEffect.Breathing));
        Assert.Equal(((byte)0, (byte)0, (byte)0), SoftwareEffect.Frame(AuraEffect.Off, 255, 255, 255, TimeSpan.Zero));
        Assert.Equal(((byte)10, (byte)20, (byte)30), SoftwareEffect.Frame(AuraEffect.Static, 10, 20, 30, TimeSpan.FromSeconds(3)));
    }

    [Fact]
    public void Breathing_goes_dark_full_dark_over_one_period()
    {
        var half = SoftwareEffect.BreathingPeriod / 2;

        Assert.Equal(((byte)0, (byte)0, (byte)0), SoftwareEffect.Frame(AuraEffect.Breathing, 200, 100, 50, TimeSpan.Zero));
        Assert.Equal(((byte)200, (byte)100, (byte)50), SoftwareEffect.Frame(AuraEffect.Breathing, 200, 100, 50, half));
        Assert.Equal(((byte)0, (byte)0, (byte)0), SoftwareEffect.Frame(AuraEffect.Breathing, 200, 100, 50, SoftwareEffect.BreathingPeriod));
    }

    [Fact]
    public void Spectrum_walks_the_hue_circle()
    {
        Assert.Equal(((byte)255, (byte)0, (byte)0), SoftwareEffect.Hue(0));
        Assert.Equal(((byte)0, (byte)255, (byte)0), SoftwareEffect.Hue(120));
        Assert.Equal(((byte)0, (byte)0, (byte)255), SoftwareEffect.Hue(240));
        Assert.Equal(SoftwareEffect.Hue(0), SoftwareEffect.Frame(AuraEffect.SpectrumCycle, 1, 2, 3, SoftwareEffect.SpectrumPeriod));
    }

    [Fact]
    public void The_effect_loop_draws_the_first_frame_at_once_and_stops()
    {
        var frames = new List<(byte, byte, byte)>();
        using var loop = new SoftwareEffectLoop("test", (r, g, b) => { lock (frames) frames.Add((r, g, b)); });

        loop.Start(AuraEffect.Static, 1, 2, 3);
        Assert.Equal((1, 2, 3), Assert.Single(frames));

        loop.Start(AuraEffect.SpectrumCycle, 0, 0, 0);
        // Wait for the animation to run (a slow CI machine may need a while).
        var deadline = DateTime.UtcNow.AddSeconds(5);
        while (DateTime.UtcNow < deadline)
        {
            lock (frames)
                if (frames.Count >= 3)
                    break;
            Thread.Sleep(20);
        }
        loop.Stop(); // waits for the thread: no frame can land after this
        int count;
        lock (frames)
            count = frames.Count;
        Thread.Sleep(150);
        lock (frames)
            Assert.Equal(count, frames.Count); // nothing after Stop
        Assert.True(count >= 3);
    }

    [Fact]
    public void A_new_effect_is_never_overwritten_by_a_late_frame_of_the_old_one()
    {
        // Breathing -> static, many times: the last frame must always be the
        // static colour (Stop used to return before the old thread's last draw).
        var last = (0, 0, 0);
        var gate = new object();
        using var loop = new SoftwareEffectLoop("test", (r, g, b) =>
        {
            Thread.Sleep(2); // a slow device write widens the race
            lock (gate)
                last = (r, g, b);
        });
        for (var i = 0; i < 20; i++)
        {
            loop.Start(AuraEffect.Breathing, 255, 0, 0);
            Thread.Sleep(45);
            loop.Start(AuraEffect.Static, 0, 0, 255);
            Thread.Sleep(60);
            lock (gate)
                Assert.Equal((0, 0, 255), last);
        }
    }
}

/// <summary>Models taken from ASUS Gear Link's device data (#240).</summary>
public class GearLinkModelTests
{
    [Fact]
    public void Gear_link_keyboards_use_the_azoth_x_protocol_and_scale()
    {
        var gearLink = AsusKeyboardDevice.Models.Values.Where(m => m.GearLink).ToList();
        Assert.True(gearLink.Count >= 40);
        Assert.All(gearLink, m =>
        {
            Assert.True(m.PerKey);
            Assert.Equal(100, m.BrightnessMax);
            Assert.False(m.Verified);
        });
        // Gear Link's slider is 0–100 for these, not OpenRGB's 0–4.
        Assert.Equal(100, AsusKeyboardDevice.Model(0x1AAE)!.BrightnessMax);
        Assert.Equal(100, AsusKeyboardDevice.Model(0x1AB5)!.BrightnessMax);
    }

    [Fact]
    public void Only_the_delta_ii_headset_is_verified()
    {
        Assert.Equal("ROG Pelta", AsusHeadsetDevice.Model(0x1B82));
        Assert.Equal(0x1AFA, Assert.Single(AsusHeadsetDevice.VerifiedModels));
    }
}

/// <summary>HID LampArray (Windows Dynamic Lighting standard) helpers.</summary>
public class LampArrayTests
{
    [Theory]
    [InlineData(1u, "keyboard")]
    [InlineData(2u, "mouse")]
    [InlineData(7u, "chassis")]
    [InlineData(99u, "peripheral")]
    public void Lamp_array_kinds_have_names(uint kind, string name)
    {
        Assert.Equal(name, LampArrayDevice.KindName(kind));
    }

    [Theory]
    // The receiver names itself, not the mouse behind it.
    [InlineData(0x0B05, 0x1ACE, "ROG OMNI RECEIVER", "mouse", "Mouse via ROG Omni receiver")]
    [InlineData(0x0B05, 0x1ACE, "ROG OMNI RECEIVER", "game_controller", "Game controller via ROG Omni receiver")]
    // Laptop keyboards report their controller chip.
    [InlineData(0x0B05, 0x19B6, "ITE Device(8910)", "keyboard", "Laptop keyboard")]
    [InlineData(0x0B05, 0x19B6, "ITE Device(8910)", "peripheral", "ITE Device(8910)")]
    [InlineData(0x046D, 0xC33F, "G815 Keyboard", "keyboard", "G815 Keyboard")]
    [InlineData(0x046D, 0xC33F, null, "keyboard", "LampArray 046D:C33F")]
    public void Lamp_array_display_names_say_what_the_device_is(int vendor, int product, string? productString,
        string kind, string expected)
    {
        Assert.Equal(expected, LampArrayDevice.DisplayName((ushort)vendor, (ushort)product, productString, kind));
    }

    [Fact]
    public void The_receivers_paired_mouse_names_its_lamp_array()
    {
        Assert.Equal("ROG Harpe Ace Aim Lab Edition",
            LampArrayDevice.DisplayName(0x0B05, 0x1ACE, "ROG OMNI RECEIVER", "mouse", "ROG Harpe Ace Aim Lab Edition"));
    }
}

/// <summary>
/// The ROG Omni receiver's paired-device list and the mouse's WDL state
/// (#236) — replies recorded from a ROG Harpe Ace Aim Lab Edition.
/// </summary>
public class OmniMouseTests
{
    private static byte[] Hex(string hex) => Convert.FromHexString(hex);

    private static byte[] Reply(params byte[] bytes)
    {
        var report = new byte[64];
        bytes.CopyTo(report, 0);
        return report;
    }

    [Fact]
    public void Paired_list_gives_the_mouse_product_id_and_its_channel()
    {
        var paired = OmniMouse.ParsePaired(Reply(0x01, 0xA0, 0x00, 0x01, 0x00, 0x94, 0x1A, 0x03, 0x05));

        Assert.Equal(new OmniMouse.Paired(0x1A94, 3), Assert.Single(paired));
        Assert.Equal("ROG Harpe Ace Aim Lab Edition", OmniMouse.Models[0x1A94].Name);
    }

    [Fact]
    public void Only_the_ace_generation_has_the_wdl_switch_and_only_the_tested_mouse_is_verified()
    {
        // Gear Link's WDL switch and Companion's WDL=1 agree on these four.
        Assert.Equal(new ushort[] { 0x1A94, 0x1B18, 0x1B65, 0x1B69 },
            OmniMouse.Models.Where(m => m.Value.Wdl).Select(m => m.Key).Order());
        Assert.Equal(0x1A94, Assert.Single(OmniMouse.Models, m => m.Value.Verified).Key);
    }

    [Fact]
    public void Paired_list_reads_a_keyboard_and_a_mouse_four_bytes_each()
    {
        // A real ROG Omni receiver with the ROG Azoth X and the Harpe Ace Aim Lab
        // Edition paired (diagnostics export, 2026-10-09).
        var paired = OmniMouse.ParsePaired(Hex("01A0000200251C0204941A03050000000000000000000000"));

        Assert.Equal([new OmniMouse.Paired(0x1C25, 2), new OmniMouse.Paired(0x1A94, 3)], paired);
    }

    [Fact]
    public void A_known_keyboard_in_the_paired_list_marks_its_channel_as_a_keyboard()
    {
        IReadOnlyList<OmniMouse.Paired> paired = [new(0x1C25, 2), new(0x1A94, 3)];

        Assert.True(AsusKeyboardDevice.IsPairedKeyboard(paired, 2));
        Assert.False(AsusKeyboardDevice.IsPairedKeyboard(paired, 3)); // the mouse
        Assert.False(AsusKeyboardDevice.IsPairedKeyboard(paired, 1)); // the receiver
        Assert.False(AsusKeyboardDevice.IsPairedKeyboard([new(0x1ACE, 2)], 2)); // the receiver's own id
    }

    [Fact]
    public void The_paired_list_names_the_keyboard_behind_the_receiver()
    {
        IReadOnlyList<OmniMouse.Paired> paired = [new(0x1C25, 2), new(0x1A94, 3)];

        Assert.Equal("ROG Azoth X", AsusKeyboardDevice.PairedKeyboardName(paired, 2));
        Assert.Null(AsusKeyboardDevice.PairedKeyboardName(paired, 3)); // the mouse
        Assert.Null(AsusKeyboardDevice.PairedKeyboardName([new(0x9999, 2)], 2)); // unknown id
    }

    [Fact]
    public void Keyboard_ask_skips_other_input_reports_until_the_echo()
    {
        var stray = Hex("0212000200010000");          // not the layout echo
        var layout = Hex("0212120000020B000000000000000000");
        var hid = new QueuedHid(stray, layout);

        var reply = AsusKeyboardDevice.Ask(hid, 64, 0x02, 0x12, TimeSpan.FromSeconds(1));

        Assert.Equal(layout, reply);
        Assert.Equal(new byte[] { 0x02, 0x12, 0x12 }, hid.Written.Single()[..3]);
        Assert.Null(AsusKeyboardDevice.Ask(new QueuedHid(stray), 64, 0x02, 0x12, TimeSpan.FromSeconds(1)));
    }

    [Fact]
    public void Paired_list_ignores_other_replies_and_empty_slots()
    {
        Assert.Empty(OmniMouse.ParsePaired(Reply(0x01, 0xA1, 0x01, 0x00, 0x00, 0x04)));
        Assert.Empty(OmniMouse.ParsePaired(Reply(0x01, 0xA0, 0x00, 0x00)));
        Assert.Empty(OmniMouse.ParsePaired(Reply(0x01, 0xA0, 0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x05)));
    }

    /// Replies queued in order; records what was written.
    private sealed class QueuedHid(params byte[][] replies) : IHidDevice
    {
        private readonly Queue<byte[]> _replies = new(replies);
        public List<byte[]> Written { get; } = [];
        public void Write(byte[] report) => Written.Add(report);
        public byte[]? Read(TimeSpan timeout) => _replies.Count > 0 ? _replies.Dequeue() : null;
        public void Dispose() { }
    }

    [Fact]
    public void Ask_skips_replies_to_other_questions()
    {
        // Gear Link's getMouseExistInfo reply (12 00 02) shares three bytes
        // with the WDL reply (12 00 09); only the full echo counts.
        var hid = new QueuedHid(
            Reply(0x03, 0x12, 0x07, 0x00, 0x63),
            Reply(0x03, 0x12, 0x00, 0x02, 0x00, 0x01),
            Reply(0x03, 0x12, 0x00, 0x09, 0x00, 0x00));

        var reply = OmniMouse.Ask(hid, [0x03, 0x12, 0x00, 0x09, 0x00]);

        Assert.False(OmniMouse.ParseWdl(reply!, 3));
    }

    [Fact]
    public void Ask_gives_null_when_the_mouse_doesnt_answer()
    {
        Assert.Null(OmniMouse.Ask(new QueuedHid(), [0x03, 0x51, 0x42, 0x00, 0x00, 0x01]));
    }

    /// Answers each written request once, from recorded replies.
    private sealed class RecordedHid(Func<byte[], byte[]?> answer) : IHidDevice
    {
        private byte[]? _pending;
        // Padded to the report length, as WindowsHidDevice does.
        public void Write(byte[] report) => _pending = answer([.. report, .. new byte[64 - report.Length]]);
        public byte[]? Read(TimeSpan timeout)
        {
            var reply = _pending;
            _pending = null;
            return reply;
        }
        public void Dispose() { }
    }

    [Fact]
    public void Probe_reports_the_receiver_and_its_paired_mouse_for_the_diagnostics()
    {
        HidDeviceInfo Channel(ushort page, byte reportId) =>
            new($@"\\?\hid#vid_0b05&pid_1ace&mi_02&col0{reportId}", 0x0B05, 0x1ACE, page, 0x0001, 64, 64, OutputReportId: reportId);
        var hid = new[] { Channel(0xFF00, 1), Channel(0xFF01, 2), Channel(0xFF02, 3) };
        // Replies recorded from the Harpe Ace Aim Lab Edition on the Omni receiver.
        var device = new RecordedHid(request => Convert.ToHexString(request.AsSpan(0, 4)) switch
        {
            "01A10100" => Reply(0x01, 0xA1, 0x01, 0x00, 0x00, 0x04, 0x00, 0x07),
            "01A00000" => Reply(0x01, 0xA0, 0x00, 0x01, 0x00, 0x94, 0x1A, 0x03, 0x05),
            "03120000" => Reply(0x03, 0x12, 0x00, 0x00, 0x00, 0x04, 0x00, 0x07, 0x00, 0x05, 0x07),
            "03120002" => Reply(0x03, 0x12, 0x00, 0x02, 0x00, 0x01),
            "03120009" => Reply(0x03, 0x12, 0x00, 0x09, 0x00, 0x00),
            _ => null,
        });

        var receiver = Assert.Single(OmniMouse.Probe(hid, _ => device))!.AsObject();

        Assert.Equal("1ACE", receiver["receiver_pid"]!.GetValue<string>());
        Assert.StartsWith("01A1010000040007", receiver["firmware"]!.GetValue<string>());
        var mouse = Assert.Single(receiver["paired"]!.AsArray())!.AsObject();
        Assert.Equal("1A94", mouse["product_id"]!.GetValue<string>());
        Assert.Equal("ROG Harpe Ace Aim Lab Edition", mouse["model"]!.GetValue<string>());
        Assert.False(mouse["wdl_on"]!.GetValue<bool>());
        Assert.StartsWith("0312000200", mouse["present"]!.GetValue<string>());
    }

    [Fact]
    public void Probe_skips_machines_without_an_asus_receiver()
    {
        var keyboard = new HidDeviceInfo(@"\\?\hid#vid_046d&pid_c33f", 0x046D, 0xC33F, 0xFF00, 0x0001, 64, 64, OutputReportId: 1);
        Assert.Empty(OmniMouse.Probe([keyboard], _ => throw new InvalidOperationException("must not open")));
    }

    [Fact]
    public void Wdl_reply_reads_on_and_off()
    {
        Assert.True(OmniMouse.ParseWdl(Reply(0x03, 0x12, 0x00, 0x09, 0x00, 0x01), 3));
        Assert.False(OmniMouse.ParseWdl(Reply(0x03, 0x12, 0x00, 0x09, 0x00, 0x00), 3));
        Assert.Null(OmniMouse.ParseWdl(Reply(0x03, 0x12, 0x00, 0x02, 0x00, 0x01), 3));
        Assert.Null(OmniMouse.ParseWdl(Reply(0x02, 0x12, 0x00, 0x09, 0x00, 0x01), 3));
    }
}

/// <summary>
/// lighting-devices.json in the diagnostics export: recognised devices with
/// their raw data, and every HID collection — without machine identifiers.
/// </summary>
public class LightingDiagnosticsTests
{
    private sealed class Device : ILightingDevice
    {
        public string Id => "aura-usb-19af";
        public string Name => "ASUS Aura motherboard controller";
        public string Kind => "motherboard";
        public string Firmware => "AULA3-AR32-0304";
        public string? Blocked => null;
        public IReadOnlyList<AuraZone> Zones { get; } = [new("argb1", "ARGB header 1", true, 0, 1)];
        public void Apply(AuraEffect effect, byte red, byte green, byte blue) { }
        public void Release() { }
        public System.Text.Json.Nodes.JsonObject Diagnostics() => new() { ["config_table"] = "1E9F03" };
    }

    private static HidDeviceInfo Hid(ushort vid, ushort pid, ushort page, string mi = "00") => new(
        $@"\?\hid#vid_{vid:x4}&pid_{pid:x4}&mi_{mi}#9&2bc98134&0&0000#{{4d1e55b2-f16f-11cf-88cb-001111000030}}",
        vid, pid, page, 0x01, 65, 65, 0, "Some device");

    [Fact]
    public void Devices_carry_their_raw_data_and_the_scan_lists_every_collection()
    {
        var hid = new[] { Hid(0x0B05, 0x19AF, 0xFF72, "02"), Hid(0x046D, 0xC547, 0xFF00) };

        var doc = LightingDiagnostics.Build(hid, [new Device()], "", null, dynamicLightingOn: true);

        var device = doc["devices"]![0]!;
        Assert.Equal("1E9F03", device["config_table"]!.GetValue<string>());
        Assert.Equal("motherboard", device["kind"]!.GetValue<string>());
        Assert.True(doc["windows_dynamic_lighting_on"]!.GetValue<bool>());

        var scan = doc["hid_scan"]!.AsArray();
        Assert.Equal(2, scan.Count);
        Assert.Equal("046D", scan[0]!["vendor_id"]!.GetValue<string>()); // sorted by vendor
        Assert.Null(scan[0]!["known_as"]); // unknown → a candidate for new support
        Assert.Equal("aura_motherboard", scan[1]!["known_as"]!.GetValue<string>());
        Assert.Equal(2, scan[1]!["interface"]!.GetValue<int>());
    }

    [Fact]
    public void Nothing_identifies_the_machine()
    {
        var doc = LightingDiagnostics.Build([Hid(0x0B05, 0x19AF, 0xFF72)], [], "No lighting controller found.", null, false);

        var json = doc.ToJsonString();
        Assert.DoesNotContain("2bc98134", json); // instance id from the HID path
        Assert.DoesNotContain(@"\?\", json);
        Assert.Equal("No lighting controller found.", doc["unavailable_reason"]!.GetValue<string>());
    }

    [Theory]
    [InlineData((ushort)0x0B05, (ushort)0x19AF, (ushort)0xFF72, "aura_motherboard")]
    [InlineData((ushort)0x0B05, (ushort)0x1BA3, (ushort)0xFF72, "aura_monitor")]
    [InlineData((ushort)0x0B05, (ushort)0x1AC8, (ushort)0xFF72, "aura_light_bar")]
    [InlineData((ushort)0x1532, (ushort)0x0099, (ushort)0x0059, "lamp_array")]
    [InlineData((ushort)0x0B05, (ushort)0x1ACE, (ushort)0xFF00, "asus_keyboard_channel")]
    [InlineData((ushort)0x0B05, (ushort)0x1ACE, (ushort)0xFFC0, null)]
    public void Collections_are_classified(ushort vid, ushort pid, ushort page, string? expected)
    {
        Assert.Equal(expected, LightingDiagnostics.KnownAs(Hid(vid, pid, page)));
    }

    [Fact]
    public void The_interface_number_comes_from_the_hid_path()
    {
        Assert.Equal(5, Hid(0x0B05, 0x1ACE, 0x59, "05").Interface);
        Assert.Null(new HidDeviceInfo(@"\?\hid#vid_0b05&pid_19af#x", 0x0B05, 0x19AF, 0, 0, 0, 0).Interface);
    }
}

/// <summary>
/// ASUS TUF-protocol keyboards (#213): the ROG Azoth X via the ROG Omni
/// receiver — replies and commands as captured on the hardware.
/// </summary>
public class AsusKeyboardTests
{
    private static byte[] Hex(string hex) => Convert.FromHexString(hex);

    [Fact]
    public void The_keyboard_channel_answers_get_layout_with_a_layout()
    {
        // Captured on the Omni receiver: keyboard (report 0x02), mouse (0x03).
        Assert.True(AsusKeyboardDevice.IsKeyboardLayoutReply(Hex("0212120000020B000000000000000000"), 0x02));
        Assert.False(AsusKeyboardDevice.IsKeyboardLayoutReply(Hex("03121200000000000000000000000000"), 0x03));
        Assert.False(AsusKeyboardDevice.IsKeyboardLayoutReply(Hex("0212000000080007"), 0x02)); // a version reply
        Assert.False(AsusKeyboardDevice.IsKeyboardLayoutReply(Hex("0212120000020B00"), 0x03)); // other report id
    }

    [Theory]
    [InlineData(AuraEffect.Static, "02512C00001E64000002FF0000")]
    [InlineData(AuraEffect.Breathing, "02512C01001E64000002FF0000")]
    [InlineData(AuraEffect.SpectrumCycle, "02512C02001E64000002FF0000")]
    public void Effects_are_the_commands_verified_on_the_azoth_x(AuraEffect effect, string expected)
    {
        var report = AsusKeyboardDevice.EffectReport(AsusKeyboardDevice.Model(0x1ACE)!, 0x02, 64, effect, 0xFF, 0x00, 0x00);

        Assert.Equal(64, report.Length);
        Assert.Equal(expected, Convert.ToHexString(report[..13]));
    }

    [Fact]
    public void Off_is_static_black()
    {
        var report = AsusKeyboardDevice.EffectReport(AsusKeyboardDevice.Model(0x1ACE)!, 0x02, 64, AuraEffect.Off, 0xFF, 0xFF, 0xFF);

        Assert.Equal(0x00, report[3]);
        Assert.Equal(new byte[] { 0, 0, 0 }, report[10..13]);
    }

    [Theory]
    [InlineData((ushort)0x1ACE, "Keyboard via ROG Omni receiver", 100, (byte)30, true)]
    [InlineData((ushort)0x1C24, "ROG Azoth X", 100, (byte)30, true)]
    [InlineData((ushort)0x1A83, "ROG Azoth", 100, (byte)30, true)] // measured: OpenRGB's 0–4 was very dim
    [InlineData((ushort)0x1AB3, "ROG Strix Scope II", 4, (byte)30, true)]
    [InlineData((ushort)0x194B, "TUF Gaming K3", 4, (byte)8, true)]
    [InlineData((ushort)0x1899, "TUF Gaming K5", 4, (byte)30, false)]
    [InlineData((ushort)0x1945, "TUF Gaming K1", 4, (byte)1, false)]
    public void OpenRGB_documented_models_are_known(ushort pid, string name, int brightnessMax, byte speed, bool perKey)
    {
        var model = AsusKeyboardDevice.Model(pid)!;

        Assert.Equal((name, brightnessMax, speed, perKey), (model.Name, model.BrightnessMax, model.Speed, model.PerKey));
    }

    [Fact]
    public void The_falchion_ace_hfx_speaks_the_azoth_x_protocol()
    {
        // Captured: get layout on interface 1 (report 0x00) — same layout as the Azoth X.
        Assert.True(AsusKeyboardDevice.IsKeyboardLayoutReply(Hex("0012120000020B000000000000000000"), 0x00));

        // The static green it took, at 0–100 brightness (4 made it very dim).
        var model = AsusKeyboardDevice.Model(0x1B7E)!;
        var report = AsusKeyboardDevice.EffectReport(model, 0x00, 65, AuraEffect.Static, 0x00, 0xFF, 0x00);
        Assert.Equal("00512C00001E6400000200FF00", Convert.ToHexString(report[..13]));
        Assert.Equal(("ROG Falchion Ace HFX", 100), (model.Name, model.BrightnessMax));
    }

    [Fact]
    public void A_keyboard_driven_directly_is_left_out_of_lamparray_discovery()
    {
        static HidDeviceInfo Collection(ushort pid, ushort page, int iface) =>
            new($"hid#vid_0b05&pid_{pid:x4}&mi_{iface:x2}", 0x0B05, pid, page, 1, 65, 65);
        var hid = new[]
        {
            Collection(0x1B7E, 0xFF00, 1), // Falchion Ace HFX: driven by its own protocol
            Collection(0x1B7E, 0x0059, 4), // ...and its LampArray: must not be driven too
            Collection(0x1ACE, 0x0059, 3), // Omni receiver's LampArray: a paired mouse, kept
        };

        var left = AsusKeyboardDevice.WithoutAsusProducts(hid, [0x1B7E]);

        Assert.Equal(new ushort[] { 0x1ACE }, left.Select(h => h.ProductId));
    }

    [Fact]
    public void Out_of_scope_keyboards_are_not_offered()
    {
        Assert.Null(AsusKeyboardDevice.Model(0x184D)); // ROG Claymore: other command layout
        Assert.Null(AsusKeyboardDevice.Model(0x190C)); // Strix Scope TKL: direct per-key only
        Assert.Null(AsusKeyboardDevice.Model(0x19AF)); // the motherboard controller
    }

    [Fact]
    public void Keyboards_without_the_per_key_marker_put_the_colour_one_byte_earlier()
    {
        var k5 = AsusKeyboardDevice.EffectReport(AsusKeyboardDevice.Model(0x1899)!, 0x00, 65, AuraEffect.Static, 1, 2, 3);
        var scope = AsusKeyboardDevice.EffectReport(AsusKeyboardDevice.Model(0x1AB3)!, 0x00, 65, AuraEffect.Static, 1, 2, 3);

        Assert.Equal(new byte[] { 1, 2, 3 }, k5[9..12]);
        Assert.Equal(new byte[] { 0x02, 1, 2, 3 }, scope[9..13]);
        Assert.Equal(0x04, scope[6]); // brightness max on the 0–4 scale
    }
}

/// <summary>
/// ASUS GearLink-protocol headsets: the ROG Delta II — frames and replies
/// as captured on the hardware.
/// </summary>
public class AsusHeadsetTests
{
    private static byte[] Hex(string hex) => Convert.FromHexString(hex);

    [Fact]
    public void The_lighting_write_is_the_frame_verified_on_the_delta_ii()
    {
        var data = AsusHeadsetDevice.LightingData(AuraEffect.Static, 0x00, 0x00, 0xFF);
        var frame = AsusHeadsetDevice.Frame(0xCC, 64, 0x51, 40, data);

        Assert.Equal(64, frame.Length);
        Assert.Equal("CC51280000016400" + "00FF", Convert.ToHexString(frame[..10]));
    }

    [Theory]
    [InlineData(AuraEffect.Static, (byte)0x01)]
    [InlineData(AuraEffect.Breathing, (byte)0x02)]
    [InlineData(AuraEffect.SpectrumCycle, (byte)0x04)]
    public void Effects_use_gearlinks_numbering(AuraEffect effect, byte id)
    {
        Assert.Equal(id, AsusHeadsetDevice.LightingData(effect, 1, 2, 3)[0]);
    }

    [Fact]
    public void Off_is_static_at_brightness_zero()
    {
        Assert.Equal(new byte[] { 0x01, 0, 0, 0, 0 }, AsusHeadsetDevice.LightingData(AuraEffect.Off, 9, 9, 9));
    }

    [Fact]
    public void A_write_is_verified_by_the_read_back()
    {
        var blue = AsusHeadsetDevice.LightingData(AuraEffect.Static, 0x00, 0x00, 0xFF);

        // Captured: get lighting after the blue write, and before it (red, 50).
        Assert.True(AsusHeadsetDevice.ReadBackMatches(Hex("CC1203000001640000FF000000000000"), 0xCC, blue));
        Assert.False(AsusHeadsetDevice.ReadBackMatches(Hex("CC120300000132FF0000000000000000"), 0xCC, blue));
        Assert.False(AsusHeadsetDevice.ReadBackMatches(Hex("CC512800000000000000000000000000"), 0xCC, blue)); // the set ack
        Assert.False(AsusHeadsetDevice.ReadBackMatches(null, 0xCC, blue));
    }

    [Fact]
    public void Firmware_comes_from_device_info()
    {
        // Captured reply to get deviceInfo (key 0).
        Assert.Equal("headset 0.9.4.0, dongle 0.9.4.0", AsusHeadsetDevice.FirmwareText(Hex("CC12000000000904000009040000000000")));
    }

    [Fact]
    public void A_dongle_answering_without_its_headset_is_not_a_headset()
    {
        Assert.True(AsusHeadsetDevice.HeadsetConnected(Hex("CC12000000000904000009040000000000")));
        // The headset off: the dongle still answers, the headset version zero.
        Assert.False(AsusHeadsetDevice.HeadsetConnected(Hex("CC12000000000000000009040000000000")));
    }

    [Fact]
    public void Only_known_headsets()
    {
        Assert.Equal("ROG Delta II", AsusHeadsetDevice.Model(0x1AFA));
        Assert.Null(AsusHeadsetDevice.Model(0x1ACE));
    }
}

/// <summary>A failing discovery can't take the service down (#219).</summary>
public class LightingDiscoveryFailureTests
{
    [Fact]
    public void A_discovery_that_throws_leaves_no_devices_and_the_next_rescan_tries_again()
    {
        var fail = true;
        var calls = 0;
        var provider = new LightingProvider(
            () => [],
            _ =>
            {
                calls++;
                return fail ? throw new NullReferenceException("driver bug") : new LightingScan([], "none");
            },
            () => null,
            dryRun: true);
        Assert.Empty(provider.Devices);
        Assert.False(provider.Probe().Supported);

        fail = false;
        provider.Rescan(force: true); // same (empty) HID set, but the failed scan is redone
        Assert.Equal(2, calls);
    }
}
