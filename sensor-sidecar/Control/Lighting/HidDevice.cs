using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace SensorSidecar.Control.Lighting;

/// One HID collection as Windows enumerates it.
public sealed record HidDeviceInfo(string Path, ushort VendorId, ushort ProductId, ushort UsagePage, ushort Usage,
    int OutputReportLength, int InputReportLength, int FeatureReportLength = 0, string? Product = null,
    byte? OutputReportId = null)
{
    /// The USB interface number from the path ("mi_02" → 2), or null.
    public int? Interface
    {
        get
        {
            var at = Path.IndexOf("&mi_", StringComparison.OrdinalIgnoreCase);
            return at >= 0 && at + 6 <= Path.Length && int.TryParse(Path.AsSpan(at + 4, 2), System.Globalization.NumberStyles.HexNumber, null, out var mi)
                ? mi
                : null;
        }
    }
}

/// One open HID device: fixed-size reports out, with a timed read back.
/// A seam so the Aura protocol is testable against a fake device.
public interface IHidDevice : IDisposable
{
    /// Writes one output report (first byte = report id).
    void Write(byte[] report);

    /// The next input report, or null when none arrives within `timeout`.
    byte[]? Read(TimeSpan timeout);
}

/// HID through the Windows HID class driver (`hid.dll` + SetupAPI) — no
/// hidapi, no extra binary. Thin: enumeration, open, and overlapped I/O.
public static class Hid
{
    /// Every HID collection present, with its VID/PID and top-level usage.
    public static IReadOnlyList<HidDeviceInfo> Enumerate()
    {
        HidD_GetHidGuid(out var hidGuid);
        var set = SetupDiGetClassDevs(ref hidGuid, IntPtr.Zero, IntPtr.Zero, DigcfPresent | DigcfDeviceInterface);
        if (set == new IntPtr(-1))
            return [];
        var found = new List<HidDeviceInfo>();
        try
        {
            var iface = new SpDeviceInterfaceData { CbSize = Marshal.SizeOf<SpDeviceInterfaceData>() };
            for (uint index = 0; SetupDiEnumDeviceInterfaces(set, IntPtr.Zero, ref hidGuid, index, ref iface); index++)
            {
                if (DevicePath(set, ref iface) is not { } path)
                    continue;
                if (Describe(path) is { } info)
                    found.Add(info);
            }
        }
        finally
        {
            SetupDiDestroyDeviceInfoList(set);
        }
        return found;
    }

    public static IHidDevice Open(HidDeviceInfo info) => new WindowsHidDevice(info);

    private static string? DevicePath(IntPtr set, ref SpDeviceInterfaceData iface)
    {
        SetupDiGetDeviceInterfaceDetail(set, ref iface, IntPtr.Zero, 0, out var size, IntPtr.Zero);
        if (size == 0)
            return null;
        var buffer = Marshal.AllocHGlobal((int)size);
        try
        {
            // SP_DEVICE_INTERFACE_DETAIL_DATA_W.cbSize: 8 on x64.
            Marshal.WriteInt32(buffer, IntPtr.Size == 8 ? 8 : 6);
            if (!SetupDiGetDeviceInterfaceDetail(set, ref iface, buffer, size, out _, IntPtr.Zero))
                return null;
            return Marshal.PtrToStringUni(buffer + 4);
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }

    /// Attributes and capabilities without read/write access (works even
    /// when another process has the device open).
    private static HidDeviceInfo? Describe(string path)
    {
        using var handle = CreateFile(path, 0, FileShareReadWrite, IntPtr.Zero, OpenExisting, 0, IntPtr.Zero);
        if (handle.IsInvalid)
            return null;
        var attributes = new HiddAttributes { Size = Marshal.SizeOf<HiddAttributes>() };
        if (!HidD_GetAttributes(handle, ref attributes))
            return null;
        if (!HidD_GetPreparsedData(handle, out var preparsed))
            return null;
        try
        {
            if (HidP_GetCaps(preparsed, out var caps) != HidpStatusSuccess)
                return null;
            return new HidDeviceInfo(path, attributes.VendorId, attributes.ProductId, caps.UsagePage, caps.Usage,
                caps.OutputReportByteLength, caps.InputReportByteLength, caps.FeatureReportByteLength, Product(handle),
                OutputReportId(preparsed, caps));
        }
        finally
        {
            HidD_FreePreparsedData(preparsed);
        }
    }

    /// The report id of the collection's output report (from its first
    /// output value), or null when it has none.
    private static byte? OutputReportId(IntPtr preparsed, HidpCaps caps)
    {
        var count = caps.NumberOutputValueCaps;
        if (count == 0)
            return null;
        var values = new HidpValueCaps[count];
        return HidP_GetValueCaps(HidpOutput, values, ref count, preparsed) == HidpStatusSuccess && count > 0
            ? values[0].ReportID
            : null;
    }

    private static string? Product(SafeFileHandle handle)
    {
        var buffer = new byte[256];
        if (!HidD_GetProductString(handle, buffer, buffer.Length))
            return null;
        var text = System.Text.Encoding.Unicode.GetString(buffer).TrimEnd('\0').Trim();
        return text.Length > 0 ? text : null;
    }

    private sealed class WindowsHidDevice : IHidDevice
    {
        private readonly FileStream _stream;
        private readonly HidDeviceInfo _info;

        public WindowsHidDevice(HidDeviceInfo info)
        {
            _info = info;
            var handle = CreateFile(info.Path, GenericRead | GenericWrite, FileShareReadWrite, IntPtr.Zero,
                OpenExisting, FileFlagOverlapped, IntPtr.Zero);
            if (handle.IsInvalid)
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), $"HID device {info.Path} not opened");
            _stream = new FileStream(handle, FileAccess.ReadWrite, 0, isAsync: true);
        }

        public void Write(byte[] report)
        {
            // Windows wants exactly the device's output report length.
            var buffer = new byte[Math.Max(_info.OutputReportLength, report.Length)];
            report.CopyTo(buffer, 0);
            _stream.Write(buffer, 0, _info.OutputReportLength > 0 ? _info.OutputReportLength : buffer.Length);
        }

        public byte[]? Read(TimeSpan timeout)
        {
            var buffer = new byte[Math.Max(_info.InputReportLength, 65)];
            using var cts = new CancellationTokenSource(timeout);
            try
            {
                var read = _stream.ReadAsync(buffer, 0, _info.InputReportLength > 0 ? _info.InputReportLength : buffer.Length, cts.Token)
                    .GetAwaiter().GetResult();
                return read > 0 ? buffer[..read] : null;
            }
            catch (OperationCanceledException)
            {
                return null;
            }
        }

        public void Dispose() => _stream.Dispose();
    }

    // ── Win32 ──────────────────────────────────────────────────────────

    private const uint DigcfPresent = 0x2;
    private const uint DigcfDeviceInterface = 0x10;
    private const uint GenericRead = 0x80000000;
    private const uint GenericWrite = 0x40000000;
    private const uint FileShareReadWrite = 3;
    private const uint OpenExisting = 3;
    private const uint FileFlagOverlapped = 0x40000000;
    private const int HidpStatusSuccess = 0x00110000;

    [StructLayout(LayoutKind.Sequential)]
    private struct SpDeviceInterfaceData
    {
        public int CbSize;
        public Guid InterfaceClassGuid;
        public int Flags;
        public IntPtr Reserved;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct HiddAttributes
    {
        public int Size;
        public ushort VendorId;
        public ushort ProductId;
        public ushort VersionNumber;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct HidpCaps
    {
        public ushort Usage;
        public ushort UsagePage;
        public ushort InputReportByteLength;
        public ushort OutputReportByteLength;
        public ushort FeatureReportByteLength;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 17)]
        public ushort[] Reserved;
        public ushort NumberLinkCollectionNodes;
        public ushort NumberInputButtonCaps;
        public ushort NumberInputValueCaps;
        public ushort NumberInputDataIndices;
        public ushort NumberOutputButtonCaps;
        public ushort NumberOutputValueCaps;
        public ushort NumberOutputDataIndices;
        public ushort NumberFeatureButtonCaps;
        public ushort NumberFeatureValueCaps;
        public ushort NumberFeatureDataIndices;
    }

    private const int HidpOutput = 1;

    [StructLayout(LayoutKind.Sequential)]
    private struct HidpValueCaps
    {
        public ushort UsagePage;
        public byte ReportID;
        public byte IsAlias;
        public ushort BitField;
        public ushort LinkCollection;
        public ushort LinkUsage;
        public ushort LinkUsagePage;
        public byte IsRange;
        public byte IsStringRange;
        public byte IsDesignatorRange;
        public byte IsAbsolute;
        public byte HasNull;
        public byte Reserved;
        public ushort BitSize;
        public ushort ReportCount;
        public ushort Reserved2a, Reserved2b, Reserved2c, Reserved2d, Reserved2e;
        public uint UnitsExp;
        public uint Units;
        public int LogicalMin, LogicalMax, PhysicalMin, PhysicalMax;
        public ushort UsageMin, UsageMax, StringMin, StringMax, DesignatorMin, DesignatorMax, DataIndexMin, DataIndexMax;
    }

    [DllImport("hid.dll")]
    private static extern void HidD_GetHidGuid(out Guid guid);

    [DllImport("hid.dll", SetLastError = true)]
    private static extern bool HidD_GetProductString(SafeFileHandle device, byte[] buffer, int length);

    [DllImport("hid.dll", SetLastError = true)]
    private static extern bool HidD_GetAttributes(SafeFileHandle device, ref HiddAttributes attributes);

    [DllImport("hid.dll", SetLastError = true)]
    private static extern bool HidD_GetPreparsedData(SafeFileHandle device, out IntPtr preparsed);

    [DllImport("hid.dll")]
    private static extern bool HidD_FreePreparsedData(IntPtr preparsed);

    [DllImport("hid.dll")]
    private static extern int HidP_GetCaps(IntPtr preparsed, out HidpCaps caps);

    [DllImport("hid.dll")]
    private static extern int HidP_GetValueCaps(int type, [Out] HidpValueCaps[] caps, ref ushort length, IntPtr preparsed);

    [DllImport("setupapi.dll", SetLastError = true)]
    private static extern IntPtr SetupDiGetClassDevs(ref Guid classGuid, IntPtr enumerator, IntPtr parent, uint flags);

    [DllImport("setupapi.dll", SetLastError = true)]
    private static extern bool SetupDiEnumDeviceInterfaces(IntPtr set, IntPtr devInfo, ref Guid classGuid, uint index,
        ref SpDeviceInterfaceData iface);

    [DllImport("setupapi.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern bool SetupDiGetDeviceInterfaceDetail(IntPtr set, ref SpDeviceInterfaceData iface,
        IntPtr detail, uint detailSize, out uint requiredSize, IntPtr devInfo);

    [DllImport("setupapi.dll")]
    private static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security,
        uint disposition, uint flags, IntPtr template);
}
