using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

public class LogitechBatteryTests
{
    private const string MxPath =
        @"\\?\hid#{00001812-0000-1000-8000-00805f9b34fb}_dev_vid&02046d_pid&b025_rev&0015_de6768c18cf5&col02#a&3ae29562&0&0001#{4d1e55b2-f16f-11cf-88cb-001111000030}";

    /// Answers each request from a table keyed by its first four bytes and
    /// records everything written.
    private sealed class Device(Func<string, byte[]?> answer) : IHidDevice
    {
        private readonly Queue<byte[]> _replies = [];
        public List<string> Written { get; } = [];
        public void Write(byte[] report)
        {
            var hex = Convert.ToHexString(report);
            Written.Add(hex);
            if (answer(hex[..8]) is { } reply)
                _replies.Enqueue(reply);
        }
        public byte[]? Read(TimeSpan timeout) => _replies.TryDequeue(out var r) ? r : null;
        public void Dispose() { }
    }

    private static byte[] Reply(params byte[] bytes) => [.. bytes, .. new byte[20 - bytes.Length]];

    [Fact]
    public void Address_comes_from_the_hid_over_gatt_path()
    {
        Assert.Equal("de6768c18cf5", LogitechBattery.Address(MxPath));
        Assert.Null(LogitechBattery.Address(@"\\?\hid#vid_046d&pid_c52b&mi_02&col01#7&1&0&0000#{4d1e55b2}"));
    }

    [Fact]
    public void Only_bluetooth_hidpp_collections_are_candidates()
    {
        HidDeviceInfo Info(string path, ushort vendor, ushort page) => new(path, vendor, 0xB025, page, 0x0202, 20, 20);
        var mx = Info(MxPath, 0x046D, 0xFF43);
        var hid = new[]
        {
            mx,
            Info(MxPath.Replace("col02", "col01"), 0x046D, 0x0001), // the mouse itself
            Info(@"\\?\hid#vid_046d&pid_c52b&mi_02&col01", 0x046D, 0xFF43), // not Bluetooth
            Info(MxPath, 0x0B05, 0xFF43),
        };

        Assert.Equal([mx], LogitechBattery.Candidates(hid));
    }

    [Fact]
    public void Sends_only_root_get_feature_and_the_unified_battery_status()
    {
        var device = new Device(request => request switch
        {
            "11FF000B" => Reply(0x11, 0xFF, 0x00, 0x0B, 0x08, 0x00, 0x00), // 0x1004 at index 8
            "11FF081B" => Reply(0x11, 0xFF, 0x08, 0x1B, 0x50, 0x04, 0x01, 0x01), // 80 %, good, charging
            _ => null,
        });

        var (reading, found) = LogitechBattery.Read(device, null);

        Assert.Equal(new LogitechReading(80, true), reading);
        Assert.Equal(((ushort)0x1004, (byte)8), found);
        Assert.Equal(
            ["11FF000B1004" + new string('0', 28), "11FF081B" + new string('0', 32)],
            device.Written);
    }

    [Fact]
    public void Falls_back_to_battery_status_when_unified_battery_is_missing()
    {
        var silent = new Device(_ => null);
        var asked = new List<string>();
        var older = new Device(request =>
        {
            asked.Add(request);
            return asked.Count switch
            {
                1 => Reply(0x11, 0xFF, 0x00, 0x0B, 0x00), // no 0x1004
                2 => Reply(0x11, 0xFF, 0x00, 0x0B, 0x06), // 0x1000 at index 6
                _ => Reply(0x11, 0xFF, 0x06, 0x0B, 0x32, 0x14, 0x00), // 50 %, discharging
            };
        });

        var (reading, found) = LogitechBattery.Read(older, null);

        Assert.Equal(new LogitechReading(50, false), reading);
        Assert.Equal(((ushort)0x1000, (byte)6), found);
        Assert.Equal(["11FF000B1004", "11FF000B1000", "11FF060B0000"], older.Written.Select(w => w[..12]));
        Assert.Equal((null, null), LogitechBattery.Read(silent, null)); // no answer at all
    }

    [Fact]
    public void A_known_feature_is_one_request()
    {
        var device = new Device(_ => Reply(0x11, 0xFF, 0x08, 0x1B, 0x4B, 0x04, 0x00));

        var (reading, _) = LogitechBattery.Read(device, (0x1004, 8));

        Assert.Equal(new LogitechReading(75, false), reading);
        Assert.Single(device.Written);
    }

    [Fact]
    public void Notifications_and_foreign_replies_are_skipped()
    {
        var notification = Reply(0x11, 0xFF, 0x08, 0x00, 0x20, 0x02, 0x00); // software id 0
        var reply = Reply(0x11, 0x00, 0x08, 0x1B, 0x50, 0x04, 0x03); // BT devices may answer with index 0

        Assert.Null(LogitechBattery.Payload(notification, 0x08, 0x01));
        Assert.Equal(0x50, LogitechBattery.Payload(reply, 0x08, 0x01)![0]);
    }

    [Fact]
    public void An_error_reply_throws()
    {
        var error = Reply(0x11, 0xFF, 0xFF, 0x08, 0x1B, 0x05);

        Assert.Throws<InvalidOperationException>(() => LogitechBattery.Payload(error, 0x08, 0x01));
    }

    [Theory]
    [InlineData(0, false)]
    [InlineData(1, true)]
    [InlineData(2, true)]
    [InlineData(3, true)] // full, still on the cable
    [InlineData(4, false)] // charging error
    public void Unified_charging_status(byte status, bool charging) =>
        Assert.Equal(charging, LogitechBattery.Unified([0x50, 0x04, status, 0x00]).Charging);

    [Fact]
    public void A_level_only_device_keeps_the_windows_percentage()
    {
        var bluetooth = new PeripheralStatus("bt:de6768c18cf5", "MX Anywhere 3 Mac", "mouse", 75, false, "bluetooth");
        var other = new PeripheralStatus("bt:e71c98949069", "Satechi M1 Mouse", "mouse", 100, false, "bluetooth");

        Assert.Null(LogitechBattery.Unified([0x00, 0x04, 0x01, 0x00]).Percent);
        Assert.Equal(
            [bluetooth with { Charging = true }, other],
            LogitechBattery.Apply([bluetooth, other], new Dictionary<string, LogitechReading> { ["de6768c18cf5"] = new(null, true) }));
        Assert.Equal(
            [bluetooth with { Battery = 81, Charging = true }, other],
            LogitechBattery.Apply([bluetooth, other], new Dictionary<string, LogitechReading> { ["de6768c18cf5"] = new(81, true) }));
    }
}
