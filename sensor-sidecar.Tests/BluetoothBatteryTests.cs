using SensorSidecar.Control;
using Xunit;
using Node = SensorSidecar.Control.BluetoothBatteries.Node;

namespace SensorSidecar.Tests;

public class BluetoothBatteryTests
{
    private static readonly DateTime T0 = new(2026, 10, 13, 18, 0, 0, DateTimeKind.Utc);

    private static Node Mouse(int battery = 75, bool connected = true) =>
        new("de6768c18cf5", "MX Anywhere 3 Mac", "Input.Mouse", battery, connected);

    [Theory]
    [InlineData("Input.Mouse", "mouse")]
    [InlineData("Input.Keyboard", "keyboard")]
    [InlineData("Input.Keyboard,Input.Mouse", "keyboard")]
    [InlineData("Audio.Headphone", "headset")]
    [InlineData("Audio.Headset", "headset")]
    [InlineData("Input.Gaming", "device")]
    [InlineData("", "device")]
    public void Kind_comes_from_the_container_category(string category, string kind) =>
        Assert.Equal(kind, BluetoothBatteries.KindOf(category));

    [Fact]
    public void A_connected_device_is_shown_as_bluetooth_without_charging()
    {
        var shown = Assert.Single(new BluetoothBatteryTracker().Update([Mouse()], T0));

        Assert.Equal(new PeripheralStatus("bt:de6768c18cf5", "MX Anywhere 3 Mac", "mouse", 75, false, "bluetooth"), shown);
    }

    [Fact]
    public void A_disconnected_device_keeps_its_level_for_ten_minutes_then_drops_off()
    {
        var tracker = new BluetoothBatteryTracker();
        tracker.Update([Mouse()], T0);

        Assert.False(Assert.Single(tracker.Update([Mouse(connected: false)], T0.AddMinutes(9))).Connected);
        Assert.Empty(tracker.Update([Mouse(connected: false)], T0.AddMinutes(10)));
        Assert.Empty(tracker.Update([Mouse(connected: false)], T0.AddMinutes(11)));
        Assert.True(Assert.Single(tracker.Update([Mouse()], T0.AddMinutes(12))).Connected); // back on
    }

    [Fact]
    public void A_device_already_disconnected_at_start_is_not_shown() =>
        Assert.Empty(new BluetoothBatteryTracker().Update([Mouse(connected: false)], T0));

    [Fact]
    public void Rog_readings_win_over_the_same_device_on_bluetooth()
    {
        var rog = new PeripheralStatus("asus-1", "ROG Harpe Ace", "mouse", 60, true, "bluetooth");
        var bluetooth = new PeripheralStatus("bt:1", "rog harpe ace ", "mouse", 55, false, "bluetooth");
        var other = new PeripheralStatus("bt:2", "MX Anywhere 3 Mac", "mouse", 75, false, "bluetooth");

        Assert.Equal([rog, other], BluetoothBatteries.Merge([rog], [bluetooth, other]));
    }
}
