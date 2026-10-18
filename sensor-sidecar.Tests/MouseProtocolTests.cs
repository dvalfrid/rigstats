using System.Text.Json;
using SensorSidecar.Control;
using SensorSidecar.Control.Mice;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// ROG mouse settings (#431): <see cref="MouseProtocol"/>'s framing against
/// the layouts documented in G-Helper and GearLink, the catalog's
/// consistency, and <see cref="MouseProvider"/> with a fake mouse.
/// </summary>
public class MouseProtocolTests
{
    private static MouseSpec HarpeAce => MouseCatalog.Find(0x1A94, MouseLink.Omni)!.Spec;

    private static byte[] Reply(byte command, byte key, byte index, params (int At, byte Value)[] bytes)
    {
        var reply = new byte[64];
        reply[0] = 0x03;
        reply[1] = command;
        reply[2] = key;
        reply[3] = index;
        foreach (var (at, value) in bytes)
            reply[at] = value;
        return reply;
    }

    /// The replies a Harpe Ace on profile 2, stage 3 of 400/800/1600/3200
    /// (red, magenta, blue, green), 1000 Hz, 12 ms, snapping off, −5°,
    /// acceleration 2 / deceleration 1, lift-off high, 3 min, 20 % would give.
    private static Dictionary<string, byte[]> HarpeReplies()
    {
        static (int, byte)[] Dpi(params int[] dpis) => dpis.SelectMany((d, i) =>
        {
            var code = (d - 50) / 50;
            return new (int, byte)[] { (5 + 4 * i, (byte)(code & 0xFF)), (6 + 4 * i, (byte)(code >> 8)) };
        }).ToArray();
        return new()
        {
            ["120000"] = Reply(0x12, 0x00, 0x00, (11, 1), (12, 3)),
            ["120400"] = Reply(0x12, 0x04, 0x00, (13, 3), (15, 2), (17, 0), (19, 0xFB), (20, 0xFF)),
            ["120402"] = Reply(0x12, 0x04, 0x02, Dpi(400, 800, 1600, 3200)),
            ["120403"] = Reply(0x12, 0x04, 0x03, (5, 0xFF), (6, 0), (7, 0), (8, 0xFF), (9, 0), (10, 0xFF),
                (11, 0), (12, 0), (13, 0xFF), (14, 0), (15, 0xFF), (16, 0)),
            ["120401"] = Reply(0x12, 0x04, 0x01, (5, 2), (7, 1)),
            ["120600"] = Reply(0x12, 0x06, 0x00, (5, 1), (6, 0x0A), (7, 0x0B), (8, 1), (9, 7), (10, 8), (11, 9)),
            ["120700"] = Reply(0x12, 0x07, 0x00, (5, 72), (6, 2), (7, 20), (11, 0x05), (12, 0x06)),
            // Default bindings, except side back on "Double click" (0x01E3).
            ["120500"] = Reply(0x12, 0x05, 0x00, MouseProtocol.Buttons.SelectMany((b, i) =>
            {
                var code = i == 3 ? 0x01E3 : b.Source;
                return new (int, byte)[] { (5 + 2 * i, (byte)(code & 0xFF)), (6 + 2 * i, (byte)(code >> 8)) };
            }).ToArray()),
        };
    }

    private static MouseSettings HarpeSettings() =>
        MouseProtocol.Parse(HarpeAce, HarpeReplies(), booster: false)!;

    [Fact]
    public void The_harpe_ace_row_matches_g_helper()
    {
        var spec = HarpeAce;
        Assert.Equal((50, 36000, 50), spec.Dpi);
        Assert.Equal(4, spec.DpiStages);
        Assert.Equal(5, spec.Profiles);
        Assert.Equal([125, 250, 500, 1000], spec.PollingHz);
        Assert.True(spec.Booster);
        Assert.Equal((-30, 30, 1), spec.AngleTuning);
        Assert.Equal(9, spec.Acceleration);
        Assert.Equal((10, 50), spec.LowBatteryWarning);
    }

    [Fact]
    public void Catalog_rows_are_unique_and_sane()
    {
        var keys = MouseCatalog.Entries.Select(e => (e.ProductId, e.Link)).ToList();
        Assert.Equal(keys.Count, keys.Distinct().Count());
        Assert.All(MouseCatalog.Entries, e =>
        {
            Assert.InRange(e.Spec.DpiStages, 1, 8);
            Assert.InRange(e.Spec.Profiles, 1, 11);
            Assert.True(e.Spec.Dpi.Min > 0 && e.Spec.Dpi.Min <= e.Spec.Dpi.Max && e.Spec.Dpi.Step > 0);
            Assert.All(e.Spec.PollingHz, hz => Assert.Contains(hz, MouseProtocol.PollingCodes));
            Assert.NotNull(MouseProvider.Interface(e.Endpoint));
            Assert.Equal(e.Link == MouseLink.Omni ? 0x03 : e.ReportId, e.ReportId);
        });
        Assert.All(MouseCatalog.Verified, v => Assert.Contains(v, keys));
    }

    [Fact]
    public void Only_documented_reads_are_asked_and_only_for_what_the_model_has()
    {
        Assert.Equal(["120000", "120400", "120402", "120403", "120401", "120600", "120700", "120500"],
            MouseProtocol.Reads(HarpeAce).Select(MouseProtocol.Key));
        var plain = HarpeAce with { XyDpi = false, DpiColors = false, Acceleration = 0, Deceleration = 0, LiftOff = false, AutoPowerOff = false, LowBatteryWarning = null, Buttons = false };
        Assert.Equal(["120000", "120400"], MouseProtocol.Reads(plain).Select(MouseProtocol.Key));
    }

    [Fact]
    public void Replies_parse_into_settings()
    {
        var s = HarpeSettings();

        Assert.Equal(2, s.Profile);
        Assert.Equal(3, s.ActiveStage);
        Assert.Equal([400, 800, 1600, 3200], s.Stages.Select(x => x.Dpi));
        Assert.Equal(["#ff0000", "#ff00ff", "#0000ff", "#00ff00"], s.Stages.Select(x => x.Color));
        Assert.Equal(1000, s.PollingHz);
        Assert.Equal(2, s.Debounce);
        Assert.False(s.AngleSnapping);
        Assert.Equal(-5, s.AngleTuning);
        Assert.Equal(2, s.Acceleration);
        Assert.Equal(1, s.Deceleration);
        Assert.Equal(1, s.LiftOff);
        Assert.Equal(2, s.PowerOff);
        Assert.Equal(20, s.LowBatteryWarning);
    }

    [Fact]
    public void Booster_polling_reads_the_high_nibble_only_when_the_receiver_reports_booster()
    {
        var replies = HarpeReplies();
        replies["120400"][13] = 0x63; // high nibble 6 (8000 Hz), low bits 3 (1000 Hz)

        Assert.Equal(8000, MouseProtocol.Parse(HarpeAce, replies, booster: true)!.PollingHz);
        Assert.Equal(1000, MouseProtocol.Parse(HarpeAce, replies, booster: false)!.PollingHz);
        Assert.Equal([125, 250, 500, 1000, 2000, 4000, 8000], MouseProtocol.Rates(HarpeAce, booster: true));
    }

    [Fact]
    public void A_missing_reply_reads_nothing()
    {
        var replies = HarpeReplies();
        replies.Remove("120700");

        Assert.Null(MouseProtocol.Parse(HarpeAce, replies, booster: false));
    }

    [Fact]
    public void Only_what_changed_is_written()
    {
        var now = HarpeSettings();
        var wanted = now with
        {
            Stages = [now.Stages[0], new DpiStage(900, "#123456"), now.Stages[2], now.Stages[3]],
            ActiveStage = 1,
            PollingHz = 500,
            AngleTuning = 7,
        };

        var writes = MouseProtocol.Writes(HarpeAce, now, wanted, HarpeReplies()["120600"], HarpeReplies()["120700"]);

        Assert.Equal(
            ["513101001100123456", "5131090001", "5131040002", "51310B000700"],
            writes.Select(w => Convert.ToHexString(w)));
        Assert.Empty(MouseProtocol.Writes(HarpeAce, now, now, null, null));
    }

    [Fact]
    public void Lift_off_and_energy_write_back_the_fields_they_do_not_change()
    {
        var now = HarpeSettings();
        var wanted = now with { LiftOff = 0, PowerOff = 4, LowBatteryWarning = 30 };

        var writes = MouseProtocol.Writes(HarpeAce, now, wanted, HarpeReplies()["120600"], HarpeReplies()["120700"]);

        // 51 35 <type> 00 <material> <height> 00 <k0 k1 k2> — GearLink's setCalibration.
        Assert.Equal("51350A000B0000070809", Convert.ToHexString(writes[0]));
        // 51 37 00 00 <sleep> 00 <low> <battery type> <full-charge> — GearLink's setPowerConfig.
        Assert.Equal("5137000004001E0506", Convert.ToHexString(writes[1]));
    }

    [Fact]
    public void Values_are_clamped_to_the_model()
    {
        var now = HarpeSettings();
        var wanted = now with
        {
            Stages = [new DpiStage(10, "nope"), new DpiStage(99999, "#ABCDEF"), new DpiStage(1234, ""), now.Stages[3]],
            ActiveStage = 9,
            PollingHz = 8000,
            Debounce = 99,
            AngleTuning = -77,
            Acceleration = 50,
            LiftOff = 5,
            PowerOff = 7,
            LowBatteryWarning = 99,
        };

        var c = MouseProtocol.Clamp(HarpeAce, now, wanted, booster: false);

        Assert.Equal([50, 36000, 1250, 3200], c.Stages.Select(s => s.Dpi));
        Assert.Equal(["#ff0000", "#abcdef", "#0000ff", "#00ff00"], c.Stages.Select(s => s.Color));
        Assert.Equal(4, c.ActiveStage);
        Assert.Equal(1000, c.PollingHz); // 8000 Hz only with booster
        Assert.Equal(7, c.Debounce);
        Assert.Equal(-30, c.AngleTuning);
        Assert.Equal(9, c.Acceleration);
        Assert.Equal(1, c.LiftOff);
        Assert.Equal(2, c.PowerOff);
        Assert.Equal(50, c.LowBatteryWarning);
        Assert.Equal(8000, MouseProtocol.Clamp(HarpeAce, now, wanted, booster: true).PollingHz);
    }

    [Fact]
    public void Bindings_parse_from_12_05_and_only_changed_buttons_are_written()
    {
        var now = HarpeSettings();
        Assert.Equal([0x01F0, 0x01F1, 0x01F2, 0x01E3, 0x01E5, 0x01E6, 0x01E8, 0x01E9], now.Buttons);

        var wanted = now with { Buttons = [0x01F0, 0x01F1, 0x01F2, 0x01E4, 0x01E5, 0x0000, 0x01E8, 0x01E9] };
        var writes = MouseProtocol.Writes(HarpeAce, now, wanted, null, null);

        // 51 21 00 00 <button lo hi> <action lo hi> — G-Helper and GearLink's mapButton.
        Assert.Equal(["51210000E401E401", "51210000E6010000"], writes.Select(w => Convert.ToHexString(w)));
    }

    [Fact]
    public void A_button_takes_only_a_known_action_or_its_own_default()
    {
        var now = HarpeSettings();
        var wanted = now with { Buttons = [0x0004, 0x01E3, 0x01F2, 0x01E4, 0x01E6, 0x01E6, 0x00F6, 0x0000] };

        var c = MouseProtocol.Clamp(HarpeAce, now, wanted, booster: false);

        // A key (0x0004) and media (0x00F6) aren't offered yet; the DPI button
        // may go back to its own 0x01E6, side forward may not take it.
        Assert.Equal([0x01F0, 0x01E3, 0x01F2, 0x01E4, 0x01E5, 0x01E6, 0x01E8, 0x0000], c.Buttons);
    }

    [Fact]
    public void Models_with_their_own_button_layout_get_no_bindings()
    {
        Assert.False(MouseCatalog.Find(0x1A1A, MouseLink.Dongle)!.Spec.Buttons); // Chakram X
        Assert.False(MouseCatalog.Find(0x1979, MouseLink.Dongle)!.Spec.Buttons); // Spatha X
        Assert.True(HarpeAce.Buttons);
        Assert.DoesNotContain(MouseProtocol.Reads(MouseCatalog.Find(0x1A1A, MouseLink.Dongle)!.Spec), r => r[1] == 0x05);
    }

    [Fact]
    public void Profile_switch_is_zero_based_on_the_wire()
    {
        Assert.Equal("500202", Convert.ToHexString(MouseProtocol.SwitchProfile(3)));
        Assert.Equal("5003", Convert.ToHexString(MouseProtocol.Save));
    }

    [Fact]
    public void Replies_match_their_question()
    {
        Assert.True(MouseProtocol.Answers(Reply(0x12, 0x04, 0x02), 0x03, [0x12, 0x04, 0x02]));
        Assert.False(MouseProtocol.Answers(Reply(0x12, 0x04, 0x03), 0x03, [0x12, 0x04, 0x02]));
        Assert.True(MouseProtocol.Answers(Reply(0x12, 0x07, 0x00), 0x03, [0x12, 0x07, 0x00]));
        Assert.False(MouseProtocol.Answers(Reply(0x12, 0x07, 0x00), 0x01, [0x12, 0x07, 0x00]));
    }

    [Fact]
    public void A_report_of_the_mouse_own_is_no_answer()
    {
        // Logged live (#442): 120000:031200090001 while 12 00 00 was asked,
        // between real replies 03120000000800070005070003FFFF100008.
        var stray = Convert.FromHexString("031200090001");
        var real = Convert.FromHexString("03120000000800070005070003FFFF100008");
        Assert.False(MouseProtocol.Answers(stray, 0x03, [0x12, 0x00, 0x00]));
        Assert.True(MouseProtocol.Answers(real, 0x03, [0x12, 0x00, 0x00]));
    }

    [Fact]
    public void The_pipe_shape_deserializes_into_settings()
    {
        var s = JsonSerializer.Deserialize<MouseSettings>(
            """{"profile":1,"active_stage":2,"stages":[{"dpi":800,"color":"#ff0000"}],"polling_hz":1000,"debounce":2,"angle_snapping":true,"angle_tuning":-3,"acceleration":null,"deceleration":null,"motion_sync":null,"lift_off":0,"power_off":255,"low_battery_warning":10}""",
            ControlJson.Options)!;

        Assert.Equal(2, s.ActiveStage);
        Assert.Equal(800, s.Stages[0].Dpi);
        Assert.Equal(-3, s.AngleTuning);
        Assert.Equal(255, s.PowerOff);
    }

    // ── MouseProvider ─────────────────────────────────────────────────────

    [Fact]
    public void Probe_lists_known_mice_and_is_unsupported_without_one()
    {
        Assert.False(new MouseProvider([], dryRun: false).Probe().Supported);
        var caps = new MouseProvider([new FakeMouse()], dryRun: false).Probe();
        Assert.True(caps.Supported);
        Assert.Equal(5, caps.Details!["mice"]![0]!["profiles"]!.GetValue<int>());
    }

    [Fact]
    public void A_profile_switches_the_onboard_profile_and_rolls_back()
    {
        var mouse = new FakeMouse();
        var provider = new MouseProvider([mouse], dryRun: false);
        var part = new ProfilePart { Mouse = new MousePart { Profile = 4 } };

        var snapshot = provider.Capture();
        provider.Apply(part);
        Assert.Equal(4, mouse.Current.Profile);
        Assert.True(provider.Verify(part));
        provider.Restore(snapshot);
        Assert.Equal(2, mouse.Current.Profile);
    }

    [Fact]
    public void A_profile_above_the_mouse_count_is_clamped_and_dry_run_switches_nothing()
    {
        var mouse = new FakeMouse();
        new MouseProvider([mouse], dryRun: false).Apply(new ProfilePart { Mouse = new MousePart { Profile = 9 } });
        Assert.Equal(5, mouse.Current.Profile);

        var other = new FakeMouse();
        new MouseProvider([other], dryRun: true).Apply(new ProfilePart { Mouse = new MousePart { Profile = 1 } });
        Assert.Equal(2, other.Current.Profile);
    }

    [Fact]
    public void No_mouse_skips_the_domain_instead_of_failing_the_profile()
    {
        Assert.Throws<DomainUnavailableException>(() =>
            new MouseProvider([], dryRun: false).Apply(new ProfilePart { Mouse = new MousePart { Profile = 1 } }));
        Assert.False(new MouseProvider([], dryRun: false).Affects(new ProfilePart()));
    }

    [Fact]
    public void Set_answers_with_the_read_back_and_unknown_ids_are_refused()
    {
        var mouse = new FakeMouse();
        var provider = new MouseProvider([mouse], dryRun: false);

        var answer = provider.Set(mouse.Id, mouse.Current with { PollingHz = 500 });

        Assert.Equal(500, answer["settings"]!["polling_hz"]!.GetValue<int>());
        Assert.Equal("omni", answer["link"]!.GetValue<string>());
        Assert.Throws<InvalidOperationException>(() => provider.Set("nope", mouse.Current));
    }

    private sealed class FakeMouse : IMouseDevice
    {
        public string Id => "asus-mouse-1a94-omni";
        public MouseEntry Entry { get; } = MouseCatalog.Find(0x1A94, MouseLink.Omni)!;
        public bool Booster => false;
        public MouseSettings Current { get; private set; } = HarpeSettings();
        public MouseSettings? Read() => Current;

        public MouseSettings Write(MouseSettings wanted) => Current = wanted;

        public void SwitchProfile(int profile) => Current = Current with { Profile = profile };
    }
}
