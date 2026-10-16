using System.Text;
using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

public class LogitechReceiverTests
{
    private const string BoltShort = @"\\?\hid#vid_046d&pid_c548&mi_02&col01#b&6fd059d&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
    private const string BoltLong = @"\\?\hid#vid_046d&pid_c548&mi_02&col02#b&6fd059d&0&0001#{4d1e55b2-f16f-11cf-88cb-001111000030}";

    /// Answers each request from a table keyed by its full hex and records
    /// everything written.
    private sealed class Receiver(Func<string, byte[]?> answer) : IHidDevice
    {
        private readonly Queue<byte[]> _replies = [];
        public List<string> Written { get; } = [];
        public void Write(byte[] report)
        {
            var hex = Convert.ToHexString(report);
            Written.Add(hex);
            if (answer(hex) is { } reply)
                _replies.Enqueue(reply);
        }
        public byte[]? Read(TimeSpan timeout) => _replies.TryDequeue(out var r) ? r : null;
        public void Dispose() { }
    }

    private static byte[] Long(params byte[] bytes) => [.. bytes, .. new byte[20 - bytes.Length]];
    private static byte[] Short(params byte[] bytes) => [.. bytes, .. new byte[7 - bytes.Length]];

    private static byte[] Error(byte sub, byte address, byte code) => Short(0x10, 0xFF, 0x8F, sub, address, code);

    [Fact]
    public void A_receivers_two_hidpp_collections_are_paired_by_their_windows_instance()
    {
        HidDeviceInfo Info(string path, ushort pid, ushort page, ushort usage) => new(path, 0x046D, pid, page, usage, 7, 7);
        var hid = new[]
        {
            Info(BoltShort, 0xC548, 0xFF00, 0x0001),
            Info(BoltLong, 0xC548, 0xFF00, 0x0002),
            Info(BoltShort.Replace("mi_02&col01", "mi_01&col01"), 0xC548, 0x0001, 0x0002), // the mouse itself
            Info(BoltShort.Replace("c548", "c539"), 0xC539, 0xFF00, 0x0001), // Lightspeed: not read
        };

        var receiver = Assert.Single(LogitechReceiver.Receivers(hid));

        Assert.Equal("046d-c548-b&6fd059d&0", receiver.Key);
        Assert.Equal(LogitechReceiver.Family.Bolt, receiver.Family);
        Assert.Equal(BoltShort, receiver.ShortReports.Path);
        Assert.Equal(BoltLong, receiver.LongReports.Path);
    }

    [Theory]
    [InlineData((ushort)0xC548, LogitechReceiver.Family.Bolt)]
    [InlineData((ushort)0xC52B, LogitechReceiver.Family.Unifying)]
    [InlineData((ushort)0xC532, LogitechReceiver.Family.Unifying)]
    public void Known_receivers(ushort productId, LogitechReceiver.Family family) =>
        Assert.Equal(family, LogitechReceiver.FamilyOf(productId));

    [Fact]
    public void A_bolt_receiver_is_asked_only_for_pairing_information_and_names()
    {
        var receiver = new Receiver(request => request switch
        {
            // slot 1: a mouse (kind 2), named "M650"
            "10FF83B5510000" => Long(0x11, 0xFF, 0x83, 0xB5, 0x51, 0x02, 0x2A, 0xB0),
            "10FF83B5610100" => Long([0x11, 0xFF, 0x83, 0xB5, 0x61, 0x01, 0x04, .. Encoding.ASCII.GetBytes("M650")]),
            // slots 2–6: empty
            var r when r.StartsWith("10FF83B55", StringComparison.Ordinal) => Error(0x83, 0xB5, 0x02),
            _ => null,
        });

        var paired = LogitechReceiver.Paired(receiver, LogitechReceiver.Family.Bolt);

        Assert.Equal((1, "M650", "mouse"), Assert.Single(paired));
        Assert.Equal(
            ["10FF83B5510000", "10FF83B5610100", "10FF83B5520000", "10FF83B5530000", "10FF83B5540000", "10FF83B5550000", "10FF83B5560000"],
            receiver.Written);
    }

    [Fact]
    public void A_unifying_receiver_uses_its_own_sub_registers()
    {
        var receiver = new Receiver(request => request switch
        {
            // slot 1: pairing 0x20, kind in byte 7 (1 = keyboard); name 0x40
            "10FF83B5200000" => Long(0x11, 0xFF, 0x83, 0xB5, 0x20, 0x00, 0x00, 0x40, 0x10, 0x00, 0x00, 0x01),
            "10FF83B5400000" => Long([0x11, 0xFF, 0x83, 0xB5, 0x40, 0x04, .. Encoding.ASCII.GetBytes("K270")]),
            var r when r.StartsWith("10FF83B52", StringComparison.Ordinal) => Error(0x83, 0xB5, 0x02),
            _ => null,
        });

        var paired = LogitechReceiver.Paired(receiver, LogitechReceiver.Family.Unifying);

        Assert.Equal((1, "K270", "keyboard"), Assert.Single(paired));
        Assert.All(receiver.Written, w => Assert.Matches("^10FF83B5(2[0-5]|40)0000$", w));
    }

    [Fact]
    public void A_receiver_answer_must_match_the_sub_register()
    {
        var request = LogitechReceiver.InfoRequest(0x52);

        Assert.Null(LogitechReceiver.Answer(Long(0x11, 0xFF, 0x83, 0xB5, 0x51, 0x02), request)); // another slot
        Assert.Null(LogitechReceiver.Answer(Long(0x11, 0x01, 0x83, 0xB5, 0x52, 0x02), request)); // not the receiver
        Assert.Equal([0x52, 0x02], LogitechReceiver.Answer(Long(0x11, 0xFF, 0x83, 0xB5, 0x52, 0x02), request)![..2]);
        Assert.Throws<InvalidOperationException>(() => LogitechReceiver.Answer(Error(0x83, 0xB5, 0x02), request));
    }

    [Fact]
    public void The_connection_count_is_registers_second_parameter()
    {
        var receiver = new Receiver(request => request == "10FF8102000000" ? Short(0x10, 0xFF, 0x81, 0x02, 0x00, 0x01, 0x00) : null);

        var answer = LogitechReceiver.Ask(receiver, LogitechReceiver.RegisterRequest(0x02));

        Assert.Equal(1, LogitechReceiver.ConnectionCount(answer!));
    }

    [Fact]
    public void A_device_behind_the_receiver_is_asked_at_its_slot()
    {
        var device = new Receiver(request => request[..8] switch
        {
            "1101000B" => Long(0x11, 0x01, 0x00, 0x0B, 0x08), // 0x1004 at index 8
            "1101081B" => Long(0x11, 0x01, 0x08, 0x1B, 0x41, 0x04, 0x00), // 65 %, discharging
            _ => null,
        });

        var (reading, _) = LogitechBattery.Read(device, null, slot: 1);

        Assert.Equal(new LogitechReading(65, false), reading);
        Assert.All(device.Written, w => Assert.StartsWith("1101", w));
    }

    [Fact]
    public void A_sleeping_device_behind_the_receiver_is_an_error_not_a_reading()
    {
        // The receiver answers for a device that is off: HID++ 1.0 error 8F.
        var device = new Receiver(request => request[..8] == "1101000B" ? Short(0x10, 0x01, 0x8F, 0x00, 0x0B, 0x09) : null);

        Assert.Throws<InvalidOperationException>(() => LogitechBattery.Read(device, null, slot: 1));
    }

    [Fact]
    public void Kinds_follow_hidpp_device_kinds()
    {
        Assert.Equal("keyboard", LogitechReceiver.KindName(0x01));
        Assert.Equal("mouse", LogitechReceiver.KindName(0x02));
        Assert.Equal("mouse", LogitechReceiver.KindName(0x08)); // trackball
        Assert.Equal("device", LogitechReceiver.KindName(0x04)); // presenter
    }
}
