# Prints every HID collection of a device and what its report descriptor
# declares — usage page/usage, report lengths, and each input/output/feature
# button and value usage with its report id. Read-only: the collections are
# opened without access rights and nothing is sent to the device. Used to
# find out what a device can be asked or told through documented HID usages
# before writing any code for it (docs/control-architecture.md, "Finding a
# lighting protocol"; the ROG Delta II's telephony LEDs in "Peripherals:
# battery and settings").
#
#   pwsh -File tools\hid-caps.ps1 -Match "vid_0b05&pid_1afa"
param([Parameter(Mandatory = $true)][string]$Match)
$code = @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class HidCaps {
    [StructLayout(LayoutKind.Sequential)] public struct SP_DEVICE_INTERFACE_DATA { public int cbSize; public Guid g; public int flags; public IntPtr r; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct SP_DEVICE_INTERFACE_DETAIL_DATA { public int cbSize; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 512)] public string path; }
    [DllImport("setupapi.dll", CharSet = CharSet.Unicode)] static extern IntPtr SetupDiGetClassDevs(ref Guid g, IntPtr e, IntPtr h, int flags);
    [DllImport("setupapi.dll")] static extern bool SetupDiEnumDeviceInterfaces(IntPtr h, IntPtr d, ref Guid g, int i, ref SP_DEVICE_INTERFACE_DATA data);
    [DllImport("setupapi.dll", CharSet = CharSet.Unicode)] static extern bool SetupDiGetDeviceInterfaceDetail(IntPtr h, ref SP_DEVICE_INTERFACE_DATA d, ref SP_DEVICE_INTERFACE_DETAIL_DATA detail, int size, out int req, IntPtr di);
    [DllImport("setupapi.dll")] static extern bool SetupDiDestroyDeviceInfoList(IntPtr h);
    [DllImport("hid.dll")] static extern void HidD_GetHidGuid(out Guid g);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateFile(string p, uint a, uint s, IntPtr sec, uint c, uint f, IntPtr t);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("hid.dll")] static extern bool HidD_GetPreparsedData(IntPtr h, out IntPtr pp);
    [DllImport("hid.dll")] static extern bool HidD_FreePreparsedData(IntPtr pp);
    [StructLayout(LayoutKind.Sequential)] public struct HIDP_CAPS { public ushort Usage, UsagePage, InputReportByteLength, OutputReportByteLength, FeatureReportByteLength; [MarshalAs(UnmanagedType.ByValArray, SizeConst = 17)] public ushort[] Reserved; public ushort NumberLinkCollectionNodes, NumberInputButtonCaps, NumberInputValueCaps, NumberInputDataIndices, NumberOutputButtonCaps, NumberOutputValueCaps, NumberOutputDataIndices, NumberFeatureButtonCaps, NumberFeatureValueCaps, NumberFeatureDataIndices; }
    [DllImport("hid.dll")] static extern int HidP_GetCaps(IntPtr pp, out HIDP_CAPS caps);
    // HIDP_BUTTON_CAPS / HIDP_VALUE_CAPS are 72 bytes; the fields used are read by offset.
    [DllImport("hid.dll")] static extern int HidP_GetButtonCaps(int type, byte[] caps, ref ushort len, IntPtr pp);
    [DllImport("hid.dll")] static extern int HidP_GetValueCaps(int type, byte[] caps, ref ushort len, IntPtr pp);
    static string Caps(int type, string label, ushort n, IntPtr pp, bool values) {
        if (n == 0) return "";
        var buf = new byte[72 * n]; ushort len = n;
        if (values) HidP_GetValueCaps(type, buf, ref len, pp); else HidP_GetButtonCaps(type, buf, ref len, pp);
        var sb = new StringBuilder();
        for (int i = 0; i < len; i++) {
            int o = i * 72;
            ushort page = BitConverter.ToUInt16(buf, o); byte reportId = buf[o + 2]; byte isRange = buf[o + 3];
            ushort u0 = BitConverter.ToUInt16(buf, o + 56), u1 = BitConverter.ToUInt16(buf, o + 58);
            sb.AppendFormat("    {0} page=0x{1:X4} report=0x{2:X2} usage={3}\n", label, page, reportId, isRange != 0 ? string.Format("0x{0:X2}..0x{1:X2}", u0, u1) : string.Format("0x{0:X2}", u0));
        }
        return sb.ToString();
    }
    public static string Run(string match) {
        var sb = new StringBuilder(); Guid g; HidD_GetHidGuid(out g);
        IntPtr set = SetupDiGetClassDevs(ref g, IntPtr.Zero, IntPtr.Zero, 0x12);
        var data = new SP_DEVICE_INTERFACE_DATA(); data.cbSize = Marshal.SizeOf(data);
        for (int i = 0; SetupDiEnumDeviceInterfaces(set, IntPtr.Zero, ref g, i, ref data); i++) {
            var detail = new SP_DEVICE_INTERFACE_DETAIL_DATA(); detail.cbSize = IntPtr.Size == 8 ? 8 : 6; int req;
            if (!SetupDiGetDeviceInterfaceDetail(set, ref data, ref detail, Marshal.SizeOf(detail), out req, IntPtr.Zero)) continue;
            if (detail.path.IndexOf(match, StringComparison.OrdinalIgnoreCase) < 0) continue;
            IntPtr h = CreateFile(detail.path, 0, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
            if (h == new IntPtr(-1)) { sb.AppendLine(detail.path + "  (open failed " + Marshal.GetLastWin32Error() + ")"); continue; }
            IntPtr pp;
            if (HidD_GetPreparsedData(h, out pp)) {
                HIDP_CAPS c; HidP_GetCaps(pp, out c);
                sb.AppendFormat("{0}\n  usagePage=0x{1:X4} usage=0x{2:X4} in={3} out={4} feat={5}\n", detail.path, c.UsagePage, c.Usage, c.InputReportByteLength, c.OutputReportByteLength, c.FeatureReportByteLength);
                sb.Append(Caps(0, "IN   button", c.NumberInputButtonCaps, pp, false));
                sb.Append(Caps(0, "IN   value ", c.NumberInputValueCaps, pp, true));
                sb.Append(Caps(1, "OUT  button", c.NumberOutputButtonCaps, pp, false));
                sb.Append(Caps(1, "OUT  value ", c.NumberOutputValueCaps, pp, true));
                sb.Append(Caps(2, "FEAT button", c.NumberFeatureButtonCaps, pp, false));
                sb.Append(Caps(2, "FEAT value ", c.NumberFeatureValueCaps, pp, true));
                HidD_FreePreparsedData(pp);
            }
            CloseHandle(h);
        }
        SetupDiDestroyDeviceInfoList(set);
        return sb.Length == 0 ? "no HID collection matches '" + match + "'" : sb.ToString();
    }
}
'@
Add-Type -TypeDefinition $code
[HidCaps]::Run($Match)
