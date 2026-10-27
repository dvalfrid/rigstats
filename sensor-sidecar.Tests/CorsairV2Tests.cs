using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Corsair M65 RGB Ultra (wired) and MM700 on Corsair's V2 protocol (#620)
/// against a fake device: exactly the frames `docs/corsair-v2-protocol.md`
/// allows, byte for byte and nothing else; the endpoint fallback only on a
/// refused START; release only after RIGStats took control; yielding to
/// iCUE; and discovery on a beta tester's real collections — nothing sent
/// while looking.
/// </summary>
public class CorsairV2Tests
{
    /// Records every write; answers each with the reply `Reply` gives (by
    /// default an echo `00 08 <cmd> 00` — status OK), read once.
    private sealed class FakeHid : IHidDevice
    {
        private readonly Queue<byte[]> _replies = new();
        public List<byte[]> Written { get; } = [];
        public Func<byte[], byte[]?> Reply { get; set; } = Echo(0x00);
        public Func<byte[], Exception?> Fails { get; set; } = _ => null;
        public int Reads { get; private set; }
        public bool Disposed { get; private set; }

        public static Func<byte[], byte[]?> Echo(byte status) => report =>
        {
            var reply = new byte[65];
            reply[1] = report[1];
            reply[2] = report[2];
            reply[3] = status;
            return reply;
        };

        public void Write(byte[] report)
        {
            if (Fails(report) is { } e)
                throw e;
            Written.Add((byte[])report.Clone());
            if (Reply(report) is { } reply)
                _replies.Enqueue(reply);
        }

        public byte[]? Read(TimeSpan timeout)
        {
            Reads++;
            return _replies.TryDequeue(out var reply) ? reply : null;
        }

        public void Dispose() => Disposed = true;
    }

    // The tester's collections (#598, fixtures/hid-scan/gigabyte-z890-ultra9-285k-rtx5070ti.json).
    private static HidDeviceInfo M65 => new(@"\\?\hid#vid_1b1c&pid_1b9e&mi_01#a", 0x1B1C, 0x1B9E, 0xFF42, 0x0001,
        OutputReportLength: 65, InputReportLength: 65, Product: "CORSAIR M65 RGB ULTRA Gaming Mouse", OutputReportId: 0);
    private static HidDeviceInfo M65Input => new(@"\\?\hid#vid_1b1c&pid_1b9e&mi_02#b", 0x1B1C, 0x1B9E, 0xFF42, 0x0002,
        OutputReportLength: 0, InputReportLength: 65, Product: "CORSAIR M65 RGB ULTRA Gaming Mouse");
    private static HidDeviceInfo M65Mouse => new(@"\\?\hid#vid_1b1c&pid_1b9e&mi_00#c", 0x1B1C, 0x1B9E, 0x0001, 0x0002,
        OutputReportLength: 0, InputReportLength: 12, Product: "CORSAIR M65 RGB ULTRA Gaming Mouse");
    private static HidDeviceInfo MM700 => new(@"\\?\hid#vid_1b1c&pid_1b9b&mi_01#d", 0x1B1C, 0x1B9B, 0xFF42, 0x0001,
        OutputReportLength: 65, InputReportLength: 65, Product: "CORSAIR MM700 RGB Gaming Mousepad", OutputReportId: 0);
    private static HidDeviceInfo MM700Input => new(@"\\?\hid#vid_1b1c&pid_1b9b&mi_00#e", 0x1B1C, 0x1B9B, 0xFF42, 0x0002,
        OutputReportLength: 0, InputReportLength: 65, Product: "CORSAIR MM700 RGB Gaming Mousepad");

    private static CorsairV2Device Mouse(FakeHid hid, Func<string?>? own = null) =>
        new(M65, CorsairV2Device.Models[0x1B9E], hid, own ?? (() => null));

    /// A full 65-byte report from its leading bytes; the rest zero.
    private static string Frame(string leading) => leading.Replace(" ", "").PadRight(130, '0');

    private static string[] Hex(IEnumerable<byte[]> reports) => reports.Select(r => Convert.ToHexString(r)).ToArray();

    // ── Frames ────────────────────────────────────────────────────────

    [Fact]
    public void The_frames_are_the_documented_bytes()
    {
        Assert.Equal(Frame("00 08 01 03 00 02"), Convert.ToHexString(CorsairV2Device.SetModeReport(CorsairV2Device.ModeSoftware)));
        Assert.Equal(Frame("00 08 01 03 00 01"), Convert.ToHexString(CorsairV2Device.SetModeReport(CorsairV2Device.ModeHardware)));
        Assert.Equal(Frame("00 08 02 5F"), Convert.ToHexString(CorsairV2Device.GetReport(0x5F)));
        Assert.Equal(Frame("00 08 0D 00 22"), Convert.ToHexString(CorsairV2Device.StartTransactionReport(0x22)));
        Assert.Equal(Frame("00 08 0D 00 01"), Convert.ToHexString(CorsairV2Device.StartTransactionReport(0x01)));
        Assert.Equal(Frame("00 08 05 01 00"), Convert.ToHexString(CorsairV2Device.StopTransactionReport()));
    }

    [Fact]
    public void A_colour_is_nine_planar_bytes_in_one_block_packet()
    {
        Assert.Equal("111111222222333333", Convert.ToHexString(CorsairV2Device.ColourData(0x11, 0x22, 0x33)));
        Assert.Equal(Frame("00 08 06 00 09 00 00 00 111111 222222 333333"),
            Convert.ToHexString(CorsairV2Device.ColourBlockReport(0x11, 0x22, 0x33)));
        Assert.Equal(65, CorsairV2Device.ColourBlockReport(1, 2, 3).Length);
    }

    [Fact]
    public void Status_is_report_byte_3()
    {
        Assert.Equal((byte)0x05, CorsairV2Device.Status([0x00, 0x08, 0x0D, 0x05]));
        Assert.Equal((byte)0x00, CorsairV2Device.Status(FakeHid.Echo(0)(new byte[65])));
        Assert.Null(CorsairV2Device.Status(null));
        Assert.Null(CorsairV2Device.Status([0x00, 0x08]));
    }

    // ── What is sent ──────────────────────────────────────────────────

    [Fact]
    public void Exactly_the_documented_frames_for_first_colour_second_colour_off_and_release()
    {
        var hid = new FakeHid();
        using var mouse = Mouse(hid);

        mouse.Apply(AuraEffect.Static, 0xAA, 0xBB, 0xCC);
        mouse.Apply(AuraEffect.Static, 0x10, 0x20, 0x30);
        mouse.Apply(AuraEffect.Off, 0xFF, 0xFF, 0xFF);
        Assert.True(mouse.Release());

        Assert.Equal(
        [
            // First use: endpoint check (22 answered OK), software mode, GET 5F.
            Frame("00 08 0D 00 22"),
            Frame("00 08 05 01 00"),
            Frame("00 08 01 03 00 02"),
            Frame("00 08 02 5F"),
            // First colour.
            Frame("00 08 0D 00 22"),
            Frame("00 08 06 00 09 00 00 00 AAAAAA BBBBBB CCCCCC"),
            Frame("00 08 05 01 00"),
            // Second colour: no mode switch, no GET again.
            Frame("00 08 0D 00 22"),
            Frame("00 08 06 00 09 00 00 00 101010 202020 303030"),
            Frame("00 08 05 01 00"),
            // Off: the same write, all zeros.
            Frame("00 08 0D 00 22"),
            Frame("00 08 06 00 09 00 00 00"),
            Frame("00 08 05 01 00"),
            // Release: hardware mode.
            Frame("00 08 01 03 00 01"),
        ], Hex(hid.Written));
        Assert.Null(mouse.Problem);
        Assert.Equal("22", (string?)mouse.Diagnostics()["endpoint"]);
    }

    [Fact]
    public void A_refused_start_on_22_moves_to_endpoint_01_and_stays_there()
    {
        var hid = new FakeHid();
        hid.Reply = r => r[2] == CorsairV2Device.CmdStartTransaction && r[4] == 0x22 ? FakeHid.Echo(0x05)(r) : FakeHid.Echo(0x00)(r);
        using var mouse = Mouse(hid);

        mouse.Apply(AuraEffect.Static, 1, 2, 3);
        mouse.Apply(AuraEffect.Static, 4, 5, 6);

        Assert.Equal(
        [
            Frame("00 08 0D 00 22"),
            Frame("00 08 0D 00 01"),
            Frame("00 08 05 01 00"),
            Frame("00 08 01 03 00 02"),
            Frame("00 08 02 5F"),
            Frame("00 08 0D 00 01"),
            Frame("00 08 06 00 09 00 00 00 010101 020202 030303"),
            Frame("00 08 05 01 00"),
            Frame("00 08 0D 00 01"),
            Frame("00 08 06 00 09 00 00 00 040404 050505 060606"),
            Frame("00 08 05 01 00"),
        ], Hex(hid.Written));
        Assert.Equal("01", (string?)mouse.Diagnostics()["endpoint"]);
    }

    [Fact]
    public void A_device_that_never_answers_stays_on_endpoint_22_as_in_openrgb()
    {
        var hid = new FakeHid { Reply = _ => null };
        using var mouse = Mouse(hid);
        mouse.Apply(AuraEffect.Static, 1, 2, 3);
        Assert.DoesNotContain(hid.Written, r => r[2] == CorsairV2Device.CmdStartTransaction && r[4] == 0x01);
        Assert.Equal(7, hid.Written.Count);
    }

    [Fact]
    public void Pending_input_is_read_away_before_a_transaction_never_answered_with_a_write()
    {
        var hid = new FakeHid();
        using var mouse = Mouse(hid);
        mouse.Apply(AuraEffect.Static, 1, 2, 3);
        // Each write gets one reply read; the drain before each START reads
        // until nothing comes.
        Assert.True(hid.Reads >= hid.Written.Count);
        Assert.Equal(7, hid.Written.Count);
    }

    [Fact]
    public void Animations_show_as_the_colour()
    {
        var hid = new FakeHid();
        using var mouse = Mouse(hid);
        mouse.Apply(AuraEffect.Breathing, 0x10, 0x20, 0x30);
        mouse.Apply(AuraEffect.SpectrumCycle, 0x10, 0x20, 0x30);
        var blocks = hid.Written.Where(r => r[2] == CorsairV2Device.CmdBlockWriteFirst).ToList();
        Assert.Equal(2, blocks.Count);
        Assert.All(blocks, b => Assert.Equal(Frame("00 08 06 00 09 00 00 00 101010 202020 303030"), Convert.ToHexString(b)));
    }

    [Fact]
    public void Release_sends_nothing_when_rigstats_never_took_control()
    {
        var hid = new FakeHid();
        using var mouse = Mouse(hid);
        Assert.False(mouse.Release());
        Assert.Empty(hid.Written);
    }

    [Fact]
    public void After_a_release_the_next_colour_takes_control_again_without_a_new_endpoint_check()
    {
        var hid = new FakeHid();
        using var mouse = Mouse(hid);
        mouse.Apply(AuraEffect.Static, 1, 2, 3);
        Assert.True(mouse.Release());
        Assert.False(mouse.Release());
        hid.Written.Clear();

        mouse.Apply(AuraEffect.Static, 1, 2, 3);

        Assert.Equal(
        [
            Frame("00 08 01 03 00 02"),
            Frame("00 08 02 5F"),
            Frame("00 08 0D 00 22"),
            Frame("00 08 06 00 09 00 00 00 010101 020202 030303"),
            Frame("00 08 05 01 00"),
        ], Hex(hid.Written));
    }

    [Fact]
    public void A_failing_device_never_throws_and_shows_the_problem()
    {
        var hid = new FakeHid { Fails = _ => new IOException("The device is not connected.") };
        using var mouse = Mouse(hid);
        mouse.Apply(AuraEffect.Static, 1, 2, 3);
        Assert.Contains("not connected", mouse.Problem);
        Assert.False(mouse.Release());

        hid.Fails = _ => null;
        mouse.Apply(AuraEffect.Static, 1, 2, 3);
        Assert.Null(mouse.Problem);
    }

    [Fact]
    public void A_failure_after_software_mode_still_hands_the_device_back()
    {
        var hid = new FakeHid { Fails = r => r[2] == CorsairV2Device.CmdBlockWriteFirst ? new IOException("gone") : null };
        using var mouse = Mouse(hid);
        mouse.Apply(AuraEffect.Static, 1, 2, 3);
        Assert.Equal("gone", mouse.Problem);
        Assert.True(mouse.Release());
        Assert.Equal(Frame("00 08 01 03 00 01"), Convert.ToHexString(hid.Written[^1]));
    }

    [Fact]
    public void Icue_blocks_the_device()
    {
        using var mouse = Mouse(new FakeHid(), () => "Corsair iCUE");
        Assert.Equal("Corsair iCUE is running", mouse.Blocked);
        using var free = Mouse(new FakeHid());
        Assert.Null(free.Blocked);
        Assert.Equal(["iCUE"], CorsairV2Device.CorsairSoftware);
    }

    // ── Discovery ─────────────────────────────────────────────────────

    [Fact]
    public void Discovery_finds_the_testers_mouse_and_mousepad_and_sends_nothing()
    {
        var opened = new List<FakeHid>();
        IHidDevice Open(HidDeviceInfo _)
        {
            var hid = new FakeHid();
            opened.Add(hid);
            return hid;
        }

        var found = CorsairV2Device.Discover([M65Mouse, M65, M65Input, MM700Input, MM700], Open);

        Assert.Equal(["corsair-1b1c-1b9b", "corsair-1b1c-1b9e"], found.Select(d => d.Id).ToArray());
        Assert.Equal(["mousepad", "mouse"], found.Select(d => d.Kind).ToArray());
        Assert.All(found, d => Assert.Equal(3, Assert.Single(d.Zones).Leds));
        Assert.Equal(2, opened.Count);
        Assert.All(opened, h => Assert.Empty(h.Written));
        Assert.All(opened, h => Assert.Equal(0, h.Reads));
    }

    [Fact]
    public void Only_the_documented_collection_is_a_channel()
    {
        Assert.True(CorsairV2Device.IsChannel(M65));
        Assert.True(CorsairV2Device.IsChannel(MM700));
        Assert.False(CorsairV2Device.IsChannel(M65Input));
        Assert.False(CorsairV2Device.IsChannel(MM700Input));
        Assert.False(CorsairV2Device.IsChannel(M65Mouse));
        Assert.False(CorsairV2Device.IsChannel(M65 with { Path = @"\\?\hid#vid_1b1c&pid_1b9e&mi_02#x" }));
        Assert.False(CorsairV2Device.IsChannel(M65 with { OutputReportLength = 0 }));
        // The wireless M65 RGB Ultra and the SLIPSTREAM receiver are out of scope.
        Assert.False(CorsairV2Device.IsChannel(M65 with { ProductId = 0x1BB5 }));
        Assert.False(CorsairV2Device.IsChannel(M65 with { ProductId = 0x2B00 }));
        Assert.False(CorsairV2Device.IsChannel(M65 with { VendorId = 0x1B1D }));
    }

    [Fact]
    public void A_device_that_cant_be_opened_is_skipped() =>
        Assert.Empty(CorsairV2Device.Discover([M65], _ => throw new IOException("in use")));

    [Fact]
    public void Diagnostics_and_known_as_name_the_protocol()
    {
        Assert.Equal("corsair_v2", LightingDiagnostics.KnownAs(M65));
        Assert.Equal("corsair_v2", LightingDiagnostics.KnownAs(MM700));
        Assert.Null(LightingDiagnostics.KnownAs(M65Input));
        using var mouse = Mouse(new FakeHid());
        Assert.Equal("corsair_v2", (string?)mouse.Diagnostics()["protocol"]);
        Assert.Null(mouse.Diagnostics()["endpoint"]);
    }

    [Fact]
    public void Every_model_is_unverified_until_seen_on_a_device() =>
        Assert.All(CorsairV2Device.Models.Values, m => Assert.False(m.Verified));

    [Fact]
    public void Disposing_closes_the_device_without_sending()
    {
        var hid = new FakeHid();
        Mouse(hid).Dispose();
        Assert.True(hid.Disposed);
        Assert.Empty(hid.Written);
    }
}
