using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Two ROG Omni receivers at once (#296): each receiver's channels kept
/// together, so each one's paired list is read on its own. The paths are the
/// owner's two receivers as Windows listed them (2026-10-19).
/// </summary>
public class OmniReceiversTests
{
    private static HidDeviceInfo Channel(string instance, string col, ushort page, byte reportId) =>
        new($@"\\?\hid#vid_0b05&pid_1ace&mi_02&{col}#{instance}#{{4d1e55b2-f16f-11cf-88cb-001111000030}}",
            0x0B05, 0x1ACE, page, 1, 64, 64, OutputReportId: reportId);

    private static readonly HidDeviceInfo[] TwoReceivers =
    [
        Channel("a&17fdc3f8&0&0000", "col01", 0xFF02, 3),
        Channel("a&17fdc3f8&0&0001", "col02", 0xFF00, 1),
        Channel("a&17fdc3f8&0&0002", "col03", 0xFF01, 2),
        Channel("a&154033f3&0&0000", "col01", 0xFF02, 3),
        Channel("a&154033f3&0&0001", "col02", 0xFF00, 1),
        Channel("a&154033f3&0&0002", "col03", 0xFF01, 2),
        // Not a channel: the receiver's LampArray on another interface.
        new(@"\\?\hid#vid_0b05&pid_1ace&mi_05#a&1335bea6&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}", 0x0B05, 0x1ACE, 0x59, 1, 0, 0),
    ];

    [Fact]
    public void Each_receiver_keeps_its_own_three_channels()
    {
        var receivers = OmniMouse.Receivers(TwoReceivers, UsbParent.Instance);

        Assert.Equal(2, receivers.Count);
        Assert.All(receivers, r => Assert.Equal(3, r.Channels.Count));
        Assert.All(receivers, r => Assert.All(r.Channels, c => Assert.Equal(r.Key, UsbParent.Instance(c.Path))));
        Assert.All(receivers, r => Assert.Equal((byte?)1, r.Own!.OutputReportId));
        Assert.All(receivers, r => Assert.Equal((byte?)3, r.Channel(3)!.OutputReportId));
        Assert.Null(receivers[0].Channel(4));
    }

    [Fact]
    public void One_receiver_is_one_group_as_before()
    {
        var one = OmniMouse.Receivers(TwoReceivers.Take(3).ToList(), UsbParent.Instance);
        Assert.Single(one);
    }

    [Fact]
    public void A_hid_path_becomes_its_device_id_and_interface_instance()
    {
        const string path = @"\\?\hid#vid_0b05&pid_1ace&mi_02&col01#a&17fdc3f8&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}";
        Assert.Equal(@"HID\VID_0B05&PID_1ACE&MI_02&COL01\A&17FDC3F8&0&0000", UsbParent.DeviceId(path));
        Assert.Equal("a&17fdc3f8&0", UsbParent.Instance(path));
        Assert.Null(UsbParent.DeviceId("odd"));
    }
}
