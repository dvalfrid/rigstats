using SensorSidecar.Control.Lighting;
using SensorSidecar.Control.Mice;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Finding several ROG mice at once (#484): every identical mouse, each with
/// its own id, and a mouse on its cable taken once.
/// </summary>
public class MouseDiscoveryTests
{
    // The Harpe Ace Aim Lab Edition by cable: 1A92, mi_00, report 0.
    private static readonly MouseEntry Wired = MouseCatalog.Find(0x1A92, MouseLink.Wired)!;

    private static HidDeviceInfo Collection(string instance, string col = "") =>
        new($@"\\?\hid#vid_0b05&pid_1a92&mi_00{col}#{instance}#{{4d1e55b2-f16f-11cf-88cb-001111000030}}",
            0x0B05, 0x1A92, 0xFF01, 1, 64, 64, OutputReportId: 0);

    [Fact]
    public void Two_identical_mice_on_cable_are_both_found_with_their_own_ids()
    {
        var plan = MouseProvider.Plan([Collection("8&aaaa&0&0000"), Collection("8&bbbb&0&0000")]);

        Assert.Equal(["asus-mouse-1a92-wired", "asus-mouse-1a92-wired-2"], plan.Select(c => c.Id));
        Assert.Equal(2, plan.Select(c => c.Channel.Path).Distinct().Count());
    }

    [Fact]
    public void One_mouse_is_one_entry_even_with_two_matching_collections()
    {
        var plan = MouseProvider.Plan([Collection("8&aaaa&0&0000", "&col01"), Collection("8&aaaa&0&0001", "&col02")]);

        Assert.Equal("asus-mouse-1a92-wired", Assert.Single(plan).Id);
    }

    [Fact]
    public void A_receiver_mouse_already_on_its_cable_is_left_out()
    {
        var cabled = new MouseProvider.Candidate(Collection("8&aaaa&0&0000"), Wired, "asus-mouse-1a92-wired");
        var harpeOmni = MouseCatalog.Find(0x1A94, MouseLink.Omni)!;
        var otherOmni = MouseCatalog.Entries.First(e => e.Link == MouseLink.Omni && e.Spec.Name != Wired.Spec.Name);

        Assert.True(MouseProvider.CabledToo([cabled], harpeOmni));
        Assert.False(MouseProvider.CabledToo([cabled], otherOmni));
        // A dongle is another mouse: only a cable makes the receiver's entry a duplicate.
        var dongle = cabled with { Entry = MouseCatalog.Find(0x1A94, MouseLink.Dongle)! };
        Assert.False(MouseProvider.CabledToo([dongle], harpeOmni));
    }

    [Fact]
    public void The_device_instance_drops_the_collections_own_part()
    {
        Assert.Equal("8&31cacbe7&0", MouseProvider.DeviceInstance(@"\\?\hid#vid_0b05&pid_1c24&mi_02&col03#8&31cacbe7&0&0002#{4d1e55b2-f16f-11cf-88cb-001111000030}"));
        Assert.Equal("odd", MouseProvider.DeviceInstance("odd"));
    }
}
