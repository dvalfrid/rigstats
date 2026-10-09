using System.Text.Json;
using SensorSidecar;
using SensorSidecar.Control;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// Battery replies per device type (#290) — layouts from Gear Link's device
/// modules and G-Helper, see docs/control-architecture.md, "Peripherals:
/// battery and settings".
public class BatteryRepliesTests
{
    private static byte[] Hex(string hex) => Convert.FromHexString(hex);

    [Fact]
    public void Headset_battery_is_byte_6_and_charging_its_own_reply()
    {
        // 12 07: sleep 10 min, 75 %, warn at 20 %, prompt on; 12 08: charging.
        var battery = Hex("CC120700000A4B14010000000000");
        var charging = Hex("CC12080000010000000000000000");

        Assert.Equal(new BatteryStatus(75, true), BatteryReplies.Headset(battery, charging));
        Assert.Equal(new BatteryStatus(75, false), BatteryReplies.Headset(battery, null));
    }

    [Fact]
    public void Mouse_battery_is_byte_5_and_charging_byte_10()
    {
        var reply = Hex("0312070000550A140F0E01000000");

        Assert.Equal(new BatteryStatus(85, true), BatteryReplies.Mouse(reply));
    }

    [Fact]
    public void Keyboard_battery_is_byte_6_and_charging_byte_9()
    {
        var reply = Hex("02120100000052030000140000000000");

        Assert.Equal(new BatteryStatus(82, false), BatteryReplies.Keyboard(reply));
        Assert.Equal(new BatteryStatus(82, true), BatteryReplies.Keyboard(Hex("02120100000052030001140000000000")));
    }

    [Fact]
    public void Standby_zero_and_error_replies_read_as_no_answer()
    {
        Assert.Null(BatteryReplies.Mouse(Hex("0312070000000000000000000000"))); // standby: 0 %, not charging
        Assert.Null(BatteryReplies.Keyboard(Hex("02120100FFAA00000000000000000000"))); // FF AA error
        Assert.Null(BatteryReplies.Headset(Hex("CC12070000FFAA000000"), null)); // FF AA error
        Assert.Null(BatteryReplies.Keyboard(Hex("0212010000"))); // too short
    }
}

/// Each device type's battery query sends only its own documented question
/// (a command sweep once broke a keyboard — see sensor-sidecar/Control/CLAUDE.md).
public class BatteryQueryTests
{
    /// Replies queued in order; records every report written.
    private sealed class RecordingHid(params string[] replies) : IHidDevice
    {
        private readonly Queue<byte[]> _replies = new(replies.Select(Convert.FromHexString));
        public List<byte[]> Written { get; } = [];
        public void Write(byte[] report) => Written.Add(report);
        public byte[]? Read(TimeSpan timeout) => _replies.Count > 0 ? _replies.Dequeue() : null;
        public void Dispose() { }
    }

    private static string Head(byte[] report, int n) => Convert.ToHexString(report[..n]);

    [Fact]
    public void Headset_asks_12_07_then_12_08_and_nothing_else()
    {
        var hid = new RecordingHid("CC120700000A4B140100", "CC120800000100000000");

        Assert.Equal(new BatteryStatus(75, true), AsusHeadsetDevice.QueryBattery(hid, 0xCC, 64));
        Assert.Equal(["CC1207", "CC1208"], hid.Written.Select(r => Head(r, 3)));
    }

    [Fact]
    public void A_silent_headset_is_not_asked_about_charging()
    {
        var hid = new RecordingHid();

        Assert.Null(AsusHeadsetDevice.QueryBattery(hid, 0xCC, 64));
        Assert.Equal(["CC1207"], hid.Written.Select(r => Head(r, 3)));
    }

    [Fact]
    public void Keyboard_asks_only_12_01()
    {
        var hid = new RecordingHid("02120100000052030001140000000000");

        Assert.Equal(new BatteryStatus(82, true), AsusKeyboardDevice.QueryBattery(hid, 64, 0x02));
        Assert.Equal(["021201"], hid.Written.Select(r => Head(r, 3)));
    }

    [Fact]
    public void Mouse_asks_only_12_07_on_its_channel()
    {
        var hid = new RecordingHid("0312070000550A140F0E01000000");

        Assert.Equal(new BatteryStatus(85, true), OmniMouse.QueryBattery(hid, 0x03));
        Assert.Equal(["031207"], hid.Written.Select(r => Head(r, 3)));
    }

    [Fact]
    public void Battery_models_are_known_keyboards_and_leave_wired_ones_out()
    {
        Assert.All(AsusKeyboardDevice.BatteryModels, id => Assert.NotNull(AsusKeyboardDevice.Model(id)));
        // hasPowerInfo false in Gear Link: never asked.
        Assert.DoesNotContain((ushort)0x1B7E, AsusKeyboardDevice.BatteryModels); // Falchion Ace HFX
        Assert.DoesNotContain((ushort)0x1AB5, AsusKeyboardDevice.BatteryModels); // Strix Scope II RX
        Assert.DoesNotContain((ushort)0x1ACE, AsusKeyboardDevice.BatteryModels); // the receiver itself
        Assert.Contains((ushort)0x1C25, AsusKeyboardDevice.BatteryModels);       // Azoth X (wireless)
    }
}

public class ConnectionTests
{
    [Fact]
    public void Bluetooth_shows_in_the_hid_path()
    {
        Assert.Equal(Connection.Bluetooth, Connection.Of(@"\?HID#{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&020b05_PID&1aaf#9&2b6d&0&0000#{4d1e55b2}", "ROG STRIX SCOPE II 96 WIRELESS"));
        Assert.Equal(Connection.Bluetooth, Connection.Of(@"\?HID#BTHENUM#{00001124-0000-1000-8000-00805f9b34fb}_VID&0002046d_PID&b023#8&1", null));
    }

    [Fact]
    public void A_receiver_or_a_dongle_named_for_it_is_2_4_ghz()
    {
        Assert.Equal(Connection.Radio, Connection.Of(@"\?HID#VID_0B05&PID_1ACE&MI_02&Col02#8&1", "ROG OMNI RECEIVER", receiver: true));
        Assert.Equal(Connection.Radio, Connection.Of(@"\?HID#VID_0B05&PID_1AFA&MI_03#8&1", "ROG DELTA II (2.4GHz)"));
        Assert.Equal(Connection.Radio, Connection.Of(@"\?HID#VID_0B05&PID_1A85&MI_02#8&1", "ROG AZOTH ROG Azoth (2.4 GHz)"));
    }

    [Fact]
    public void Anything_else_is_a_cable()
    {
        Assert.Equal(Connection.Usb, Connection.Of(@"\?HID#VID_0B05&PID_1C24&MI_01#8&1", "ROG AZOTH X ROG Azoth X"));
    }
}

public class KeyboardDedupTests
{
    [Fact]
    public void A_receiver_keyboard_also_on_its_cable_is_left_out()
    {
        var keep = AsusKeyboardDevice.KeepIndices([(true, "ROG Azoth X"), (false, "ROG Azoth X"), (false, "ROG Falchion Ace HFX")]);
        Assert.Equal(new HashSet<int> { 1, 2 }, keep);
    }

    [Fact]
    public void Same_names_are_numbered_after_the_receiver_duplicate_is_gone()
    {
        Assert.Equal(["ROG Azoth X", "ROG Azoth X (2)", "ROG Delta II"],
            AsusKeyboardDevice.Numbered(["ROG Azoth X", "ROG Azoth X", "ROG Delta II"]));
    }

    [Fact]
    public void A_receiver_keyboard_alone_stays()
    {
        Assert.Equal(new HashSet<int> { 0 }, AsusKeyboardDevice.KeepIndices([(true, "ROG Azoth X")]));
    }
}

public class PeripheralBatteryMonitorTests
{
    private sealed class FakeBatteryDevice(string id, string kind, Func<BatteryStatus?> read, bool hasBattery = true)
        : ILightingDevice, IBatteryDevice
    {
        public string Id => id;
        public string Name => $"Device {id}";
        public string Kind => kind;
        public string Firmware => "";
        public string? Blocked => null;
        public IReadOnlyList<AuraZone> Zones { get; } = [];
        public void Apply(AuraEffect effect, byte red, byte green, byte blue) { }
        public void Release() { }
        public System.Text.Json.Nodes.JsonObject Diagnostics() => new();
        public bool HasBattery => hasBattery;
        public string Connection => SensorSidecar.Control.Lighting.Connection.Radio;
        public int Reads { get; private set; }
        public BatteryStatus? ReadBattery() { Reads++; return read(); }
    }

    private static PeripheralBatteryMonitor Monitor() => new(null!, new PeripheralStatusStore());

    [Fact]
    public void Reads_on_the_interval_or_when_the_device_list_changes()
    {
        var t0 = new DateTime(2026, 10, 9, 18, 0, 0, DateTimeKind.Utc);
        string[] ids = ["kb", "mouse"];

        Assert.False(PeripheralBatteryMonitor.ShouldRead(t0, t0.AddSeconds(5), ids, ids));
        Assert.True(PeripheralBatteryMonitor.ShouldRead(t0, t0.AddSeconds(20), ids, ids));
        Assert.True(PeripheralBatteryMonitor.ShouldRead(t0, t0.AddSeconds(5), ids, ["kb"])); // unplugged
        Assert.True(PeripheralBatteryMonitor.ShouldRead(t0, t0.AddSeconds(5), ids, ["kb2", "mouse"])); // switched
    }

    [Fact]
    public void Reads_devices_with_a_battery_and_skips_the_rest()
    {
        var keyboard = new FakeBatteryDevice("kb", "keyboard", () => new BatteryStatus(80, false));
        var wired = new FakeBatteryDevice("wired", "keyboard", () => new BatteryStatus(1, false), hasBattery: false);

        var result = Monitor().ReadAll([keyboard, wired]);

        Assert.Equal(new PeripheralStatus("kb", "Device kb", "keyboard", 80, false, "2.4ghz"), Assert.Single(result));
        Assert.Equal(0, wired.Reads);
    }

    [Fact]
    public void A_silent_device_keeps_its_last_reading_until_it_disconnects()
    {
        BatteryStatus? next = new BatteryStatus(60, false);
        var mouse = new FakeBatteryDevice("mouse", "mouse", () => next);
        var monitor = Monitor();
        monitor.ReadAll([mouse]);

        next = null; // asleep
        Assert.Equal(60, Assert.Single(monitor.ReadAll([mouse])).Battery);

        Assert.Empty(monitor.ReadAll([]));      // unplugged
        Assert.Empty(monitor.ReadAll([mouse])); // back, still asleep: no stale value
    }

    [Fact]
    public void A_reading_goes_stale_after_ten_silent_rounds()
    {
        BatteryStatus? next = new BatteryStatus(80, true);
        var keyboard = new FakeBatteryDevice("kb", "keyboard", () => next);
        var monitor = Monitor();
        monitor.ReadAll([keyboard]);

        next = null; // switched to its cable: the receiver entry stops answering
        for (var round = 1; round < PeripheralBatteryMonitor.StaleRounds; round++)
            Assert.Single(monitor.ReadAll([keyboard]));
        Assert.Empty(monitor.ReadAll([keyboard]));

        next = new BatteryStatus(81, true); // answers again
        Assert.Equal(81, Assert.Single(monitor.ReadAll([keyboard])).Battery);
    }

    [Fact]
    public void One_failing_device_does_not_hide_the_others()
    {
        var broken = new FakeBatteryDevice("broken", "headset", () => throw new IOException("gone"));
        var ok = new FakeBatteryDevice("ok", "headset", () => new BatteryStatus(30, true));

        var result = Monitor().ReadAll([broken, ok]);

        Assert.Equal("ok", Assert.Single(result).Id);
    }
}

/// The telemetry line's `peripherals` field, against the example the Rust
/// reader checks too (`rigstats-backend/src/lhm.rs`) — so neither side can
/// rename a field alone.
public class TelemetryContractTests
{
    [Fact]
    public void Peripherals_serialize_as_the_shared_contract_example()
    {
        var payload = new SensorPayload(
            61.5f, 88.25f,
            [new GpuDevice("AMD Radeon RX 9070 XT", "gpu-amd", 12, 48, 61, 54, 1200, 1250, 45.5f, 0, 2048, 16304, null, null)],
            new Dictionary<string, float> { ["Samsung SSD 990 PRO 2TB"] = 44 },
            39,
            [new MbFan("Fan #2", 1210)],
            [new MbTemp("System", 36)],
            [new MbVoltage("Vcore", 1.1f)],
            "Nuvoton NCT6799D",
            [
                new PeripheralStatus("asus-keyboard-1ace-1", "ROG Azoth X", "keyboard", 82, false, "2.4ghz"),
                new PeripheralStatus("asus-headset-1afa-1", "ROG Delta II", "headset", 24, true, "2.4ghz"),
            ],
            new ActiveProfileStatus("balanced", "Balanced"));

        var json = JsonSerializer.Serialize(payload, HardwareHost.TelemetryJsonOptions);
        var expected = File.ReadAllText(Path.Combine(RepoRoot(), "sensor-sidecar.Tests", "contract", "telemetry-peripherals.json")).Trim();

        Assert.Equal(expected, json);
    }

    [Fact]
    public void Without_peripherals_the_field_is_left_out()
    {
        var payload = new SensorPayload(null, null, [], [], null, [], [], [], null);

        Assert.DoesNotContain("peripherals", JsonSerializer.Serialize(payload, HardwareHost.TelemetryJsonOptions));
    }

    private static string RepoRoot()
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
            if (Directory.Exists(Path.Combine(dir.FullName, "sensor-sidecar.Tests", "contract")))
                return dir.FullName;
        throw new DirectoryNotFoundException("repo root with sensor-sidecar.Tests/contract");
    }
}
