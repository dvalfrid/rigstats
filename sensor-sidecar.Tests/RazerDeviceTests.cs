using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Razer's HID protocol (#510) against a fake device: the exact report bytes
/// for each effect (command numbers and transaction ids as openrazer
/// documents them), the checksum, reply checking, a failing device never
/// failing the apply, yielding to Razer Synapse, and discovery — nothing sent
/// while looking, models that show a LampArray left to LampArrayDevice.
/// </summary>
public class RazerDeviceTests
{
    private sealed class FakeTransport : IRazerTransport
    {
        public List<byte[]> Sent { get; } = [];
        public Func<byte[], byte[]?> Reply { get; set; } = Ok;
        public bool Disposed { get; private set; }

        public byte[]? Exchange(byte[] request)
        {
            Sent.Add(request);
            return Reply(request);
        }

        public void Dispose() => Disposed = true;

        /// The device's normal answer: the same frame, status 0x02.
        public static byte[] Ok(byte[] request) => WithStatus(request, RazerProtocol.StatusOk);

        public static byte[] WithStatus(byte[] request, byte status)
        {
            var reply = (byte[])request.Clone();
            reply[RazerProtocol.Status] = status;
            return reply;
        }
    }

    // The tester's collections (beta diagnostics, #510).
    private static HidDeviceInfo DeathAdder2013 => new(@"\\?\hid#vid_1532&pid_0037&mi_00#a", 0x1532, 0x0037, 0x0001, 0x0002,
        OutputReportLength: 0, InputReportLength: 9, FeatureReportLength: 91, Product: "Razer DeathAdder 2013");
    private static HidDeviceInfo DeathAdder2013Keys => new(@"\\?\hid#vid_1532&pid_0037&mi_01&col01#b", 0x1532, 0x0037, 0x0001, 0x0006,
        OutputReportLength: 0, InputReportLength: 16, FeatureReportLength: 0, Product: "Razer DeathAdder 2013");
    private static HidDeviceInfo Blade2018 => new(@"\\?\hid#vid_1532&pid_0233&mi_02#c", 0x1532, 0x0233, 0x0001, 0x0002,
        OutputReportLength: 0, InputReportLength: 9, FeatureReportLength: 91, Product: "Razer Blade");
    private static HidDeviceInfo BlackWidowV3MiniRazer => new(@"\\?\hid#vid_1532&pid_0258&mi_03#d", 0x1532, 0x0258, 0x000C, 0x0001,
        OutputReportLength: 0, InputReportLength: 0, FeatureReportLength: 91, Product: "Razer BlackWidow V3 Mini");
    private static HidDeviceInfo BlackWidowV3MiniLampArray => new(@"\\?\hid#vid_1532&pid_0258&mi_04&col02#e", 0x1532, 0x0258, LampArrayDevice.LightingPage, 0x0001,
        OutputReportLength: 0, InputReportLength: 0, FeatureReportLength: 64, Product: "Razer BlackWidow V3 Mini");

    private static (RazerDevice Device, FakeTransport Transport) Open(HidDeviceInfo info, Func<string?>? razerSoftware = null)
    {
        var transport = new FakeTransport();
        return (new RazerDevice(info, RazerDevice.Model(info.ProductId)!, transport, razerSoftware ?? (() => null)), transport);
    }

    private static string Hex(byte[] report, int count) => Convert.ToHexString(report, 0, count);

    // ── Frames ────────────────────────────────────────────────────────

    [Fact]
    public void A_frame_is_91_bytes_with_the_header_where_razer_puts_it()
    {
        var f = RazerProtocol.Frame(0xFF, 0x03, 0x0A, 0x06, 0x11, 0x22, 0x33);
        Assert.Equal(91, f.Length);
        Assert.Equal(0x00, f[0]); // report id
        Assert.Equal(0x00, f[RazerProtocol.Status]);
        Assert.Equal(0xFF, f[RazerProtocol.TransactionId]);
        Assert.Equal(new byte[] { 0, 0, 0 }, f[3..6]); // remaining packets, protocol type
        Assert.Equal(4, f[RazerProtocol.DataSize]);
        Assert.Equal((0x03, 0x0A), (f[RazerProtocol.CommandClass], f[RazerProtocol.CommandId]));
        Assert.Equal(new byte[] { 0x06, 0x11, 0x22, 0x33 }, f[9..13]);
        Assert.All(f[13..RazerProtocol.Crc], b => Assert.Equal(0, b));
        Assert.Equal(0x00, f[90]); // reserved
    }

    [Fact]
    public void The_checksum_is_the_xor_of_the_header_and_arguments()
    {
        var f = RazerProtocol.Frame(0xFF, 0x03, 0x0A, 0x06, 0x11, 0x22, 0x33);
        // Data size, class, id and the arguments; the transaction id is not in it.
        Assert.Equal((byte)(0x04 ^ 0x03 ^ 0x0A ^ 0x06 ^ 0x11 ^ 0x22 ^ 0x33), f[RazerProtocol.Crc]);
        Assert.Equal(f[RazerProtocol.Crc], RazerProtocol.Checksum(f));
    }

    [Fact]
    public void More_than_80_argument_bytes_is_refused() =>
        Assert.Throws<ArgumentException>(() => RazerProtocol.Frame(0x3F, 0x03, 0x00, new byte[81]));

    // ── DeathAdder 2013: two classic LEDs, transaction id 0x3F ─────────

    [Fact]
    public void DeathAdder_static_sets_state_colour_and_effect_on_both_leds()
    {
        var (device, transport) = Open(DeathAdder2013);
        device.Apply(AuraEffect.Static, 0x10, 0x20, 0x30);

        Assert.Equal(
        [
            "00003F000000030300010101", // scroll wheel on
            "00003F0000000503010101102030", // its colour
            "00003F000000030302010100", // static
            "00003F000000030300010401", // logo on
            "00003F0000000503010104102030",
            "00003F000000030302010400",
        ], transport.Sent.Select(r => Hex(r, 9 + r[RazerProtocol.DataSize])).ToArray());
        Assert.All(transport.Sent, r => Assert.Equal(RazerProtocol.Checksum(r), r[RazerProtocol.Crc]));
        Assert.Null(device.Problem);
    }

    [Fact]
    public void DeathAdder_off_only_switches_both_leds_off()
    {
        var (device, transport) = Open(DeathAdder2013);
        device.Apply(AuraEffect.Off, 0xFF, 0xFF, 0xFF);

        Assert.Equal(["00003F000000030300010100", "00003F000000030300010400"],
            transport.Sent.Select(r => Hex(r, 12)).ToArray());
    }

    [Theory]
    [InlineData(AuraEffect.Breathing)]
    [InlineData(AuraEffect.SpectrumCycle)]
    public void Breathing_and_spectrum_show_as_the_static_colour(AuraEffect effect)
    {
        var (asStatic, staticSent) = Open(DeathAdder2013);
        asStatic.Apply(AuraEffect.Static, 1, 2, 3);
        var (other, otherSent) = Open(DeathAdder2013);
        other.Apply(effect, 1, 2, 3);
        Assert.Equal(staticSent.Sent.Select(Convert.ToHexString), otherSent.Sent.Select(Convert.ToHexString));
    }

    // ── Blade 15 (2018): the standard matrix, transaction id 0xFF ──────

    [Fact]
    public void Blade_static_is_one_standard_matrix_report()
    {
        var (device, transport) = Open(Blade2018);
        device.Apply(AuraEffect.Static, 0xAA, 0xBB, 0xCC);

        var sent = Assert.Single(transport.Sent);
        Assert.Equal("0000FF0000000403" + "0A" + "06AABBCC", Hex(sent, 13));
    }

    [Fact]
    public void Blade_off_is_the_matrix_effect_none()
    {
        var (device, transport) = Open(Blade2018);
        device.Apply(AuraEffect.Off, 0, 0, 0);

        Assert.Equal("0000FF00000001030A00", Hex(Assert.Single(transport.Sent), 10));
    }

    // ── Replies ───────────────────────────────────────────────────────

    [Theory]
    [InlineData(RazerProtocol.StatusOk, null)]
    [InlineData(RazerProtocol.StatusBusy, "busy")]
    [InlineData(RazerProtocol.StatusFailure, "failed")]
    [InlineData(RazerProtocol.StatusTimeout, "timed out")]
    [InlineData(RazerProtocol.StatusNotSupported, "not supported")]
    [InlineData(0x00, "not taken up yet")]
    public void A_reply_status_says_whether_the_command_took(byte status, string? problem)
    {
        var request = RazerProtocol.MatrixNone(0xFF);
        Assert.Equal(problem, RazerProtocol.Problem(request, FakeTransport.WithStatus(request, status)));
    }

    [Fact]
    public void A_reply_to_another_command_or_none_is_a_problem()
    {
        var request = RazerProtocol.MatrixNone(0xFF);
        Assert.Equal("no reply", RazerProtocol.Problem(request, null));
        Assert.Equal("no reply", RazerProtocol.Problem(request, new byte[10]));
        var other = FakeTransport.Ok(RazerProtocol.LedState(0xFF, 1, true));
        Assert.StartsWith("reply to another command", RazerProtocol.Problem(request, other));
    }

    [Fact]
    public void A_failing_device_never_throws_it_shows_the_problem_until_a_write_takes()
    {
        var (device, transport) = Open(Blade2018);
        transport.Reply = r => FakeTransport.WithStatus(r, RazerProtocol.StatusNotSupported);
        device.Apply(AuraEffect.Static, 1, 2, 3); // must not throw
        Assert.Contains("not supported", device.Problem);

        transport.Reply = _ => throw new System.ComponentModel.Win32Exception(5, "Access denied");
        device.Apply(AuraEffect.Static, 1, 2, 3);
        Assert.Contains("Access denied", device.Problem);

        transport.Reply = FakeTransport.Ok;
        device.Apply(AuraEffect.Static, 1, 2, 3);
        Assert.Null(device.Problem);
    }

    [Fact]
    public void A_classic_led_stops_at_the_first_command_that_fails()
    {
        var (device, transport) = Open(DeathAdder2013);
        transport.Reply = r => FakeTransport.WithStatus(r, RazerProtocol.StatusFailure);
        device.Apply(AuraEffect.Static, 1, 2, 3);
        Assert.Single(transport.Sent);
        Assert.NotNull(device.Problem);
    }

    // ── Who owns the lights ───────────────────────────────────────────

    [Fact]
    public void Blocked_while_razer_software_runs()
    {
        string? running = "Razer Synapse";
        var (device, _) = Open(Blade2018, () => running);
        Assert.Equal("Razer Synapse is running", device.Blocked);
        running = null;
        Assert.Null(device.Blocked);
    }

    // ── Discovery ─────────────────────────────────────────────────────

    [Fact]
    public void Discovery_finds_the_testers_two_devices_and_sends_nothing()
    {
        var transports = new List<FakeTransport>();
        var hid = new[] { DeathAdder2013, DeathAdder2013Keys, Blade2018, BlackWidowV3MiniRazer, BlackWidowV3MiniLampArray };
        var found = RazerDevice.Discover(hid, _ =>
        {
            var t = new FakeTransport();
            transports.Add(t);
            return t;
        });

        Assert.Equal(["razer-1532-0037", "razer-1532-0233"], found.Select(d => d.Id).ToArray());
        Assert.Equal(["Razer DeathAdder 2013", "Razer Blade 15 (2018)"], found.Select(d => d.Name).ToArray());
        Assert.Equal(["mouse", "keyboard"], found.Select(d => d.Kind).ToArray());
        Assert.All(transports, t => Assert.Empty(t.Sent));
    }

    [Fact]
    public void A_model_that_also_shows_a_lamparray_is_left_to_lamparray()
    {
        // Same id as the Blade, plus a LampArray collection: not driven twice.
        var bladeLampArray = Blade2018 with { Path = @"\\?\hid#vid_1532&pid_0233&mi_03#f", UsagePage = LampArrayDevice.LightingPage, FeatureReportLength = 64 };
        Assert.Empty(RazerDevice.Discover([Blade2018, bladeLampArray], _ => new FakeTransport()));
    }

    [Fact]
    public void Only_known_models_on_their_91_byte_collection_are_channels()
    {
        Assert.True(RazerDevice.IsChannel(DeathAdder2013));
        Assert.True(RazerDevice.IsChannel(Blade2018));
        Assert.False(RazerDevice.IsChannel(DeathAdder2013Keys)); // no feature report
        Assert.False(RazerDevice.IsChannel(BlackWidowV3MiniRazer)); // not in the model table
        Assert.False(RazerDevice.IsChannel(DeathAdder2013 with { VendorId = 0x0B05 }));
    }

    [Fact]
    public void A_collection_that_cant_be_opened_is_skipped() =>
        Assert.Empty(RazerDevice.Discover([DeathAdder2013], _ => null));

    [Fact]
    public void Disposing_the_device_closes_its_handle()
    {
        var (device, transport) = Open(Blade2018);
        device.Dispose();
        Assert.True(transport.Disposed);
    }

    [Fact]
    public void Diagnostics_name_the_protocol_and_the_collection()
    {
        var (device, _) = Open(Blade2018);
        var d = device.Diagnostics();
        Assert.Equal("razer", (string?)d["protocol"]);
        Assert.Equal("0233", (string?)d["product_id"]);
        Assert.Equal(2, (int?)d["interface"]);
        Assert.Equal("FF", (string?)d["transaction_id"]);
        Assert.Equal("razer", LightingDiagnostics.KnownAs(Blade2018));
        Assert.Null(LightingDiagnostics.KnownAs(DeathAdder2013Keys));
        Assert.Equal("lamp_array", LightingDiagnostics.KnownAs(BlackWidowV3MiniLampArray));
    }

    [Fact]
    public void Every_model_is_unverified_until_seen_on_a_device() =>
        Assert.All(RazerDevice.Models.Values, m => Assert.False(m.Verified));

    [Theory]
    [InlineData(0x00, true)]
    [InlineData(0x01, true)]
    [InlineData(0x02, false)]
    [InlineData(0x03, false)]
    [InlineData(0x05, false)]
    public void New_and_busy_replies_are_read_again(byte status, bool pending)
    {
        // #538: a DeathAdder 2013 answered 00 (not taken up yet) when read
        // 2 ms after the request — that is a wait, not a refusal.
        Assert.Equal(pending, RazerProtocol.Pending(status));
    }
}
