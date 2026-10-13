using System.Runtime.InteropServices;

namespace SensorSidecar.Control;

/// Battery of Bluetooth devices from Windows' own device properties (#291):
/// Windows reads the standard battery level of a paired device (GATT Battery
/// Service for Bluetooth LE, HFP for headsets) and stores it on the device's
/// BTHLE / BTHENUM node — the percentage Settings › Bluetooth shows. Reading
/// it is a configuration-manager property query; nothing is sent to the
/// device. Windows doesn't expose charging for Bluetooth devices.
public static class BluetoothBatteries
{
    /// One paired Bluetooth device that has a battery level.
    public sealed record Node(string Address, string Name, string Category, int Battery, bool Connected);

    /// The present BTHLE and BTHENUM nodes with a battery level, one per
    /// address (a classic device has a node per service).
    public static IReadOnlyList<Node> Read()
    {
        var result = new Dictionary<string, Node>(StringComparer.OrdinalIgnoreCase);
        foreach (var enumerator in new[] { "BTHLE", "BTHENUM" })
        {
            foreach (var id in DeviceIds(enumerator))
            {
                if (CM_Locate_DevNodeW(out var inst, id, 0) != 0)
                    continue;
                if (Property(inst, BatteryLevel) is not [var level, ..] || level > 100)
                    continue;
                var address = Text(Property(inst, Address)) is { Length: > 0 } a ? a : id;
                if (result.ContainsKey(address))
                    continue;
                var connected = CM_Get_DevNode_Status(out var status, out _, inst, 0) == 0
                    && (status & DnDeviceDisconnected) == 0;
                result[address] = new Node(
                    address,
                    Text(Property(inst, FriendlyName)) is { Length: > 0 } n ? n : "Bluetooth device",
                    Text(Property(inst, Category)),
                    level,
                    connected);
            }
        }
        return [.. result.Values];
    }

    /// The panel's kind from the device container's category ("Input.Mouse",
    /// "Audio.Headphone", …).
    public static string KindOf(string category) => category switch
    {
        _ when category.StartsWith("Input.Mouse", StringComparison.OrdinalIgnoreCase) => "mouse",
        _ when category.StartsWith("Input.Keyboard", StringComparison.OrdinalIgnoreCase) => "keyboard",
        _ when category.StartsWith("Audio.", StringComparison.OrdinalIgnoreCase) => "headset",
        _ => "device",
    };

    /// The ROG readings plus the Bluetooth ones not already among them (a ROG
    /// device on Bluetooth is read by its own protocol, which also knows
    /// charging).
    public static IReadOnlyList<PeripheralStatus> Merge(IReadOnlyList<PeripheralStatus> rog, IReadOnlyList<PeripheralStatus> bluetooth)
    {
        var names = rog.Select(p => p.Name.Trim()).ToHashSet(StringComparer.OrdinalIgnoreCase);
        return [.. rog, .. bluetooth.Where(p => !names.Contains(p.Name.Trim()))];
    }

    private static IEnumerable<string> DeviceIds(string enumerator)
    {
        const uint flags = 0x1 | 0x100; // CM_GETIDLIST_FILTER_ENUMERATOR | _PRESENT
        if (CM_Get_Device_ID_List_SizeW(out var length, enumerator, flags) != 0 || length == 0)
            return [];
        var buffer = new char[length];
        if (CM_Get_Device_ID_ListW(enumerator, buffer, length, flags) != 0)
            return [];
        return new string(buffer).Split('\0', StringSplitOptions.RemoveEmptyEntries);
    }

    private static byte[]? Property(uint inst, DevPropKey key)
    {
        uint size = 0;
        if (CM_Get_DevNode_PropertyW(inst, ref key, out _, null, ref size, 0) != CrBufferSmall || size == 0)
            return null;
        var buffer = new byte[size];
        return CM_Get_DevNode_PropertyW(inst, ref key, out _, buffer, ref size, 0) == 0 ? buffer : null;
    }

    /// A string, or a string list's entries joined by commas.
    private static string Text(byte[]? bytes) =>
        bytes is null ? "" : string.Join(",", System.Text.Encoding.Unicode.GetString(bytes).Split('\0', StringSplitOptions.RemoveEmptyEntries));

    private const int CrBufferSmall = 0x1A;
    private const uint DnDeviceDisconnected = 0x02000000;

    // DEVPKEY_Bluetooth_Battery (byte, percent), DEVPKEY_Bluetooth_DeviceAddress,
    // DEVPKEY_NAME, DEVPKEY_DeviceContainer_Category.
    private static readonly DevPropKey BatteryLevel = new(new Guid("104EA319-6EE2-4701-BD47-8DDBF425BBE5"), 2);
    private static readonly DevPropKey Address = new(new Guid("2BD67D8B-8BEB-48D5-87E0-6CDA3428040A"), 1);
    private static readonly DevPropKey FriendlyName = new(new Guid("A45C254E-DF1C-4EFD-8020-67D146A850E0"), 14);
    private static readonly DevPropKey Category = new(new Guid("78C34FC8-104A-4ACA-9EA4-524D52996E57"), 90);

    [StructLayout(LayoutKind.Sequential)]
    private readonly record struct DevPropKey(Guid Fmtid, uint Pid);

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    private static extern int CM_Get_Device_ID_List_SizeW(out uint length, string filter, uint flags);

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    private static extern int CM_Get_Device_ID_ListW(string filter, char[] buffer, uint length, uint flags);

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    private static extern int CM_Locate_DevNodeW(out uint inst, string id, uint flags);

    [DllImport("cfgmgr32.dll")]
    private static extern int CM_Get_DevNode_PropertyW(uint inst, ref DevPropKey key, out uint type, byte[]? buffer, ref uint size, uint flags);

    [DllImport("cfgmgr32.dll")]
    private static extern int CM_Get_DevNode_Status(out uint status, out uint problem, uint inst, uint flags);
}

/// Which Bluetooth readings the panel shows: connected devices, and one that
/// disconnected (switched off, asleep, out of range — a Bluetooth mouse drops
/// its link after a few idle minutes) with its last level for
/// <see cref="KeepAfterDisconnect"/>, like a silent ROG device. A device
/// already disconnected when the service starts isn't shown: its level is old.
public sealed class BluetoothBatteryTracker
{
    public static readonly TimeSpan KeepAfterDisconnect = TimeSpan.FromMinutes(10);

    private readonly Dictionary<string, DateTime> _lastConnected = [];
    private readonly Dictionary<string, int> _logged = [];

    public IReadOnlyList<PeripheralStatus> Update(IReadOnlyList<BluetoothBatteries.Node> nodes, DateTime now)
    {
        var result = new List<PeripheralStatus>();
        foreach (var node in nodes)
        {
            if (node.Connected)
                _lastConnected[node.Address] = now;
            if (!_lastConnected.TryGetValue(node.Address, out var seen) || now - seen >= KeepAfterDisconnect)
                continue;
            // Logged when first shown and when the level changes by 10 or more,
            // not every read.
            if (!_logged.TryGetValue(node.Address, out var before) || Math.Abs(before - node.Battery) >= 10)
            {
                SidecarLog.Log($"[rigstats-control] Battery: {node.Name} {node.Battery} % (Bluetooth).");
                _logged[node.Address] = node.Battery;
            }
            result.Add(new PeripheralStatus($"bt:{node.Address}", node.Name, BluetoothBatteries.KindOf(node.Category),
                node.Battery, false, Lighting.Connection.Bluetooth));
        }
        foreach (var gone in _lastConnected.Where(e => now - e.Value >= KeepAfterDisconnect).Select(e => e.Key).ToList())
        {
            _lastConnected.Remove(gone);
            _logged.Remove(gone);
        }
        return result;
    }
}
