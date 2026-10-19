using System.Runtime.InteropServices;

namespace SensorSidecar.Control.Lighting;

/// The USB device a HID collection belongs to (#296): Windows' device tree,
/// read with cfgmgr32 — no device I/O. A receiver's interfaces (its vendor
/// channels on mi_02, its LampArray on mi_05, …) each have their own HID
/// instance, but all hang under one USB device, so that is what tells two
/// identical ROG Omni receivers apart.
public static class UsbParent
{
    /// The USB device's instance id (`USB\VID_0B05&PID_1ACE\…`), or the
    /// collection's own interface instance when the tree can't be read — two
    /// receivers' channels still differ by it, only the LampArray can't be
    /// matched then.
    public static string Of(string hidPath)
    {
        try
        {
            if (DeviceId(hidPath) is { } id && CM_Locate_DevNodeW(out var node, id, 0) == 0)
            {
                // HID collection → USB interface (…&MI_02) → USB device.
                for (var level = 0; level < 4 && CM_Get_Parent(out var parent, node, 0) == 0; level++)
                {
                    node = parent;
                    if (Id(node) is { } parentId && parentId.StartsWith(@"USB\", StringComparison.OrdinalIgnoreCase)
                        && !parentId.Contains("&MI_", StringComparison.OrdinalIgnoreCase))
                        return parentId.ToUpperInvariant();
                }
            }
        }
        catch (Exception e) when (e is DllNotFoundException or EntryPointNotFoundException)
        {
        }
        return Instance(hidPath);
    }

    /// `\\?\hid#vid_0b05&pid_1ace&mi_02&col01#a&17fdc3f8&0&0000#{…}` →
    /// `HID\VID_0B05&PID_1ACE&MI_02&COL01\A&17FDC3F8&0&0000`.
    public static string? DeviceId(string hidPath)
    {
        var parts = hidPath.Split('#');
        return parts.Length >= 3 && parts[0].EndsWith("hid", StringComparison.OrdinalIgnoreCase)
            ? $@"HID\{parts[1]}\{parts[2]}".ToUpperInvariant()
            : null;
    }

    /// The interface instance without the collection's own last part —
    /// `…#a&17fdc3f8&0&0002#…` → `a&17fdc3f8&0`.
    public static string Instance(string hidPath)
    {
        var parts = hidPath.Split('#');
        if (parts.Length < 3)
            return hidPath;
        var last = parts[2].LastIndexOf('&');
        return last > 0 ? parts[2][..last] : parts[2];
    }

    private static string? Id(uint node)
    {
        var buffer = new char[400];
        return CM_Get_Device_IDW(node, buffer, buffer.Length, 0) == 0 ? new string(buffer).TrimEnd('\0') : null;
    }

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    private static extern int CM_Locate_DevNodeW(out uint inst, string id, uint flags);

    [DllImport("cfgmgr32.dll")]
    private static extern int CM_Get_Parent(out uint parent, uint inst, uint flags);

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode)]
    private static extern int CM_Get_Device_IDW(uint inst, char[] buffer, int length, uint flags);
}
