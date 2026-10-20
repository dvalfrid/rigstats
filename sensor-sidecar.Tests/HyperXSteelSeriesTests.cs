using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// HyperX Alloy Origins Core (#511) and SteelSeries Siberia (#512) against
/// fake devices: the exact reports OpenRGB documents, a frame sent again
/// while the keyboard needs it, a failing device never failing the apply,
/// yielding to the vendor's software, and discovery on a beta tester's real
/// collections — nothing sent while looking.
/// </summary>
public class HyperXSteelSeriesTests
{
    private sealed class FakeHid : IHidDevice
    {
        private readonly object _gate = new();
        private readonly List<byte[]> _written = [];
        public Func<byte[], Exception?> Fails { get; set; } = _ => null;
        public bool Disposed { get; private set; }

        public List<byte[]> Written
        {
            get
            {
                lock (_gate)
                    return [.. _written];
            }
        }

        public void Write(byte[] report)
        {
            if (Fails(report) is { } e)
                throw e;
            lock (_gate)
                _written.Add((byte[])report.Clone());
        }

        public byte[]? Read(TimeSpan timeout) => throw new InvalidOperationException("nothing is ever read");
        public void Dispose() => Disposed = true;
    }

    // The tester's collections (beta diagnostics).
    private static HidDeviceInfo AlloyVendor => new(@"\\?\hid#vid_0951&pid_16e6&mi_02#a", 0x0951, 0x16E6, 0xFF13, 0x0001,
        OutputReportLength: 65, InputReportLength: 65, FeatureReportLength: 65, Product: "HyperX Alloy Origins Core");
    private static HidDeviceInfo AlloyKeys => new(@"\\?\hid#vid_0951&pid_16e6&mi_00#b", 0x0951, 0x16E6, 0x0001, 0x0006,
        OutputReportLength: 2, InputReportLength: 9, Product: "HyperX Alloy Origins Core");
    private static HidDeviceInfo Siberia => new(@"\\?\hid#vid_1038&pid_1229&mi_03#c", 0x1038, 0x1229, 0x000C, 0x0001,
        OutputReportLength: 16, InputReportLength: 16, Product: "SteelSeries Siberia 350");

    private static HyperXKeyboardDevice Keyboard(FakeHid hid, Func<string?>? own = null, TimeSpan? keepAlive = null) =>
        new(AlloyVendor, HyperXKeyboardDevice.Models[(0x0951, 0x16E6)], hid, own ?? (() => null), keepAlive ?? TimeSpan.FromHours(1));

    private static SteelSeriesHeadsetDevice Headset(FakeHid hid, Func<string?>? own = null) =>
        new(Siberia, SteelSeriesHeadsetDevice.Models[0x1229], hid, own ?? (() => null));

    // ── HyperX: frames ────────────────────────────────────────────────

    [Fact]
    public void Every_key_slot_gets_the_colour_in_green_red_blue_rows()
    {
        var buffer = HyperXKeyboardDevice.ColourBuffer(0x11, 0x22, 0x33);
        Assert.Equal(380, buffer.Length);
        // Key 0: green at 0, red at 16, blue at 32; key 6 (second sector) at 8.
        Assert.Equal((0x22, 0x11, 0x33), (buffer[0], buffer[16], buffer[32]));
        Assert.Equal((0x22, 0x11, 0x33), (buffer[8], buffer[24], buffer[40]));
        // Key 12 starts the next 48-byte segment; key 93 is the last.
        Assert.Equal(0x22, buffer[48]);
        Assert.Equal(0x22, buffer[93 / 12 * 48 + (93 / 6 & 1) * 8 + 93 % 6]);
        // The two padding bytes after each 6 colours stay zero.
        Assert.Equal((0, 0), (buffer[6], buffer[7]));
        // 94 slots × 3 colours set, nothing else.
        Assert.Equal(94 * 3, buffer.Count(b => b != 0));
    }

    [Fact]
    public void A_frame_is_seven_reports_of_60_bytes_with_a_sequence_number()
    {
        var reports = HyperXKeyboardDevice.ColourReports(1, 2, 3);
        Assert.Equal(7, reports.Count);
        Assert.All(reports, r => Assert.Equal(65, r.Length));
        Assert.Equal(Enumerable.Range(0, 7).Select(i => (byte)i), reports.Select(r => r[2]));
        Assert.All(reports, r => Assert.Equal((0x00, 0xA2, 0x00), (r[0], r[1], r[3])));
        Assert.Equal([60, 60, 60, 60, 60, 60, 20], reports.Select(r => (int)r[4]));
        var joined = reports.SelectMany(r => r[5..(5 + r[4])]).ToArray();
        Assert.Equal(HyperXKeyboardDevice.ColourBuffer(1, 2, 3), joined);
    }

    [Fact]
    public void Brightness_is_the_a7_report() =>
        Assert.Equal("00A7000001FF", Convert.ToHexString(HyperXKeyboardDevice.BrightnessReport(0xFF), 0, 6));

    [Fact]
    public void Static_sends_brightness_then_one_frame()
    {
        var hid = new FakeHid();
        using var keyboard = Keyboard(hid);
        keyboard.Apply(AuraEffect.Static, 0x10, 0x20, 0x30);
        var written = hid.Written;
        Assert.Equal(8, written.Count);
        Assert.Equal(0xA7, written[0][1]);
        Assert.All(written.Skip(1), r => Assert.Equal(0xA2, r[1]));
        Assert.Null(keyboard.Problem);
    }

    [Fact]
    public void Off_is_a_black_frame_kept_coming_so_the_keyboard_doesnt_go_back_to_its_effect()
    {
        var hid = new FakeHid();
        using var keyboard = Keyboard(hid, keepAlive: TimeSpan.FromMilliseconds(10));
        keyboard.Apply(AuraEffect.Off, 0xFF, 0xFF, 0xFF);
        var deadline = DateTime.UtcNow.AddSeconds(5);
        while (hid.Written.Count < 1 + 7 * 3 && DateTime.UtcNow < deadline)
            Thread.Sleep(5);
        var frames = hid.Written.Skip(1).ToList();
        Assert.True(frames.Count >= 21, $"{frames.Count} reports");
        Assert.All(frames, r => Assert.All(r[5..], b => Assert.Equal(0, b)));
    }

    [Fact]
    public void Release_stops_sending_and_hands_the_keyboard_back()
    {
        var hid = new FakeHid();
        using var keyboard = Keyboard(hid, keepAlive: TimeSpan.FromMilliseconds(10));
        keyboard.Apply(AuraEffect.Static, 1, 2, 3);
        Thread.Sleep(50);
        Assert.True(keyboard.Release());
        var after = hid.Written.Count;
        Thread.Sleep(100);
        Assert.Equal(after, hid.Written.Count);
    }

    [Fact]
    public void A_failing_keyboard_never_throws_and_shows_the_problem()
    {
        var hid = new FakeHid { Fails = _ => new IOException("The device is not connected.") };
        using var keyboard = Keyboard(hid);
        keyboard.Apply(AuraEffect.Static, 1, 2, 3);
        Assert.Contains("not connected", keyboard.Problem);

        hid.Fails = _ => null;
        keyboard.Apply(AuraEffect.Static, 1, 2, 3);
        Assert.Null(keyboard.Problem);
    }

    [Fact]
    public void A_keyboard_unplugged_while_sending_stops_and_says_so()
    {
        var hid = new FakeHid();
        using var keyboard = Keyboard(hid, keepAlive: TimeSpan.FromMilliseconds(10));
        keyboard.Apply(AuraEffect.Static, 1, 2, 3);
        hid.Fails = _ => new IOException("gone");
        var deadline = DateTime.UtcNow.AddSeconds(5);
        while (keyboard.Problem is null && DateTime.UtcNow < deadline)
            Thread.Sleep(5);
        Assert.Equal("gone", keyboard.Problem);
    }

    [Fact]
    public void Ngenuity_started_while_sending_gets_the_keyboard()
    {
        string? running = null;
        var hid = new FakeHid();
        using var keyboard = Keyboard(hid, () => running, TimeSpan.FromMilliseconds(5));
        keyboard.Apply(AuraEffect.Static, 1, 2, 3);
        running = "HyperX NGENUITY";
        var deadline = DateTime.UtcNow.AddSeconds(5);
        while (keyboard.Problem is null && DateTime.UtcNow < deadline)
            Thread.Sleep(5);
        Assert.Contains("HyperX NGENUITY is running", keyboard.Problem);
        var after = hid.Written.Count;
        Thread.Sleep(60);
        Assert.Equal(after, hid.Written.Count);
        Assert.Equal("HyperX NGENUITY is running", keyboard.Blocked);
    }

    // ── SteelSeries ───────────────────────────────────────────────────

    [Fact]
    public void A_colour_is_the_four_documented_messages()
    {
        var hid = new FakeHid();
        using var headset = Headset(hid);
        headset.Apply(AuraEffect.Static, 0xAA, 0xBB, 0xCC);
        Assert.Equal(
        [
            "0100950280BF", "010080025220", "01008303AABBCC", "010093020380",
        ], hid.Written.Select(r => Convert.ToHexString(r, 0, 4 + r[3])).ToArray());
        Assert.All(hid.Written, r => Assert.Equal(16, r.Length));
        Assert.Null(headset.Problem);
    }

    [Theory]
    [InlineData(AuraEffect.Off, "01008303000000")]
    [InlineData(AuraEffect.Breathing, "01008303102030")]
    [InlineData(AuraEffect.SpectrumCycle, "01008303102030")]
    public void Off_is_black_and_animations_show_the_colour(AuraEffect effect, string colourMessage)
    {
        var hid = new FakeHid();
        using var headset = Headset(hid);
        headset.Apply(effect, 0x10, 0x20, 0x30);
        Assert.Equal(colourMessage, Convert.ToHexString(hid.Written[2], 0, 7));
    }

    [Fact]
    public void A_failing_headset_never_throws_and_shows_the_problem()
    {
        var hid = new FakeHid { Fails = _ => new IOException("gone") };
        using var headset = Headset(hid);
        headset.Apply(AuraEffect.Static, 1, 2, 3);
        Assert.Equal("gone", headset.Problem);
    }

    [Fact]
    public void SteelSeries_gg_blocks_the_headset()
    {
        using var headset = Headset(new FakeHid(), () => "SteelSeries GG");
        Assert.Equal("SteelSeries GG is running", headset.Blocked);
    }

    // ── Discovery ─────────────────────────────────────────────────────

    [Fact]
    public void Discovery_finds_the_testers_keyboard_and_headset_and_sends_nothing()
    {
        var opened = new List<FakeHid>();
        IHidDevice Open(HidDeviceInfo _)
        {
            var hid = new FakeHid();
            opened.Add(hid);
            return hid;
        }
        HidDeviceInfo[] hid = [AlloyKeys, AlloyVendor, Siberia];

        var keyboards = HyperXKeyboardDevice.Discover(hid, Open);
        var headsets = SteelSeriesHeadsetDevice.Discover(hid, Open);

        Assert.Equal("hyperx-0951-16e6", Assert.Single(keyboards).Id);
        Assert.Equal("steelseries-1038-1229", Assert.Single(headsets).Id);
        Assert.Equal(("keyboard", "headset"), (keyboards[0].Kind, headsets[0].Kind));
        Assert.Equal(2, opened.Count);
        Assert.All(opened, h => Assert.Empty(h.Written));
    }

    [Fact]
    public void Only_the_documented_collection_is_a_channel()
    {
        Assert.True(HyperXKeyboardDevice.IsChannel(AlloyVendor));
        Assert.False(HyperXKeyboardDevice.IsChannel(AlloyKeys));
        Assert.False(HyperXKeyboardDevice.IsChannel(AlloyVendor with { Path = @"\\?\hid#vid_0951&pid_16e6&mi_01#x" }));
        Assert.True(HyperXKeyboardDevice.IsChannel(AlloyVendor with { VendorId = 0x03F0, ProductId = 0x098F }));
        Assert.True(SteelSeriesHeadsetDevice.IsChannel(Siberia));
        Assert.False(SteelSeriesHeadsetDevice.IsChannel(Siberia with { Path = @"\\?\hid#vid_1038&pid_1229&mi_00#y" }));
        Assert.False(SteelSeriesHeadsetDevice.IsChannel(Siberia with { OutputReportLength = 0 }));
        Assert.False(SteelSeriesHeadsetDevice.IsChannel(Siberia with { ProductId = 0x12AA }));
    }

    [Fact]
    public void A_device_that_cant_be_opened_is_skipped()
    {
        Assert.Empty(HyperXKeyboardDevice.Discover([AlloyVendor], _ => throw new IOException("in use")));
        Assert.Empty(SteelSeriesHeadsetDevice.Discover([Siberia], _ => throw new UnauthorizedAccessException("denied")));
    }

    [Fact]
    public void Diagnostics_and_known_as_name_the_protocols()
    {
        Assert.Equal("hyperx_keyboard", LightingDiagnostics.KnownAs(AlloyVendor));
        Assert.Equal("steelseries_headset", LightingDiagnostics.KnownAs(Siberia));
        Assert.Null(LightingDiagnostics.KnownAs(AlloyKeys));
        using var keyboard = Keyboard(new FakeHid());
        Assert.Equal("hyperx_alloy_origins_core", (string?)keyboard.Diagnostics()["protocol"]);
        using var headset = Headset(new FakeHid());
        Assert.Equal("steelseries_siberia", (string?)headset.Diagnostics()["protocol"]);
    }

    [Fact]
    public void Every_model_is_unverified_until_seen_on_a_device()
    {
        Assert.All(HyperXKeyboardDevice.Models.Values, m => Assert.False(m.Verified));
        Assert.All(SteelSeriesHeadsetDevice.Models.Values, m => Assert.False(m.Verified));
    }

    [Fact]
    public void Disposing_closes_the_device()
    {
        var hid = new FakeHid();
        Keyboard(hid).Dispose();
        Assert.True(hid.Disposed);
    }
}
