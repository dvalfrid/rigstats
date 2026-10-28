using System.Diagnostics;
using System.Runtime.InteropServices;
using Microsoft.Win32;
using Microsoft.Win32.SafeHandles;

namespace SensorSidecar.Control.Providers;

/// ASUS laptops through `\\.\ATKACPI` (#234). For now only step 1 of the
/// live-test plan in docs/asus-laptop-acpi.md: a read-only snapshot logged
/// once at start — `DSTS` (get) on the documented ids, never `DEVS` (set),
/// `INIT` or any id outside <see cref="ReadIds"/> / <see cref="CurveIds"/>.
/// Does nothing on a PC without the device.
public static class AsusAcpi
{
    private const string DevicePath = @"\\.\ATKACPI";
    private const uint Ioctl = 0x0022240C;
    public const uint Dsts = 0x53545344; // "DSTS"
    public const uint PresenceBit = 0x00010000;

    /// Performance mode (ROG/TUF, Vivobook), fan RPM (CPU, GPU, middle) and
    /// the power-limit ids — each read with argument 0, as Linux's presence
    /// probe does.
    public static readonly uint[] ReadIds =
    [
        0x00120075, 0x00110019,
        0x00110013, 0x00110014, 0x00110031,
        0x001200A3, 0x001200A0, 0x001200C1, 0x001200B0, 0x001200B1,
        0x001200C0, 0x001200C2, 0x00120099, 0x00120098,
    ];

    /// Fan curves (CPU, GPU, middle): read with the mode as argument
    /// (0 Balanced, 1 Silent, 2 Turbo); the 16 bytes are the firmware curve.
    public static readonly uint[] CurveIds = [0x00110024, 0x00110025, 0x00110032];

    /// The `DSTS` input buffer: method, argument length (8), id, argument.
    public static byte[] DstsInput(uint id, uint arg)
    {
        var buf = new byte[16];
        BitConverter.TryWriteBytes(buf.AsSpan(0), Dsts);
        BitConverter.TryWriteBytes(buf.AsSpan(4), 8u);
        BitConverter.TryWriteBytes(buf.AsSpan(8), id);
        BitConverter.TryWriteBytes(buf.AsSpan(12), arg);
        return buf;
    }

    /// Present when bit 16 is set; 0xFFFFFFFE (method unsupported) and
    /// 0xFFFFFFFF (no device) have it set too, so they are excluded.
    public static bool IsPresent(uint raw) =>
        raw is not (0xFFFFFFFE or 0xFFFFFFFF) && (raw & PresenceBit) != 0;

    /// Logs the snapshot; never throws.
    public static void LogSnapshot()
    {
        try
        {
            using var device = CreateFileW(DevicePath, GenericRead | GenericWrite, FileShareReadWrite,
                IntPtr.Zero, OpenExisting, 0, IntPtr.Zero);
            if (device.IsInvalid)
                return; // not an ASUS laptop (or no ASUS System Control Interface)

            var board = Registry.GetValue(@"HKEY_LOCAL_MACHINE\HARDWARE\DESCRIPTION\System\BIOS", "BaseBoardProduct", null);
            var bios = Registry.GetValue(@"HKEY_LOCAL_MACHINE\HARDWARE\DESCRIPTION\System\BIOS", "BIOSVersion", null);
            var asusSoftware = Process.GetProcesses()
                .Select(p => p.ProcessName)
                .Where(n => n.StartsWith("ArmouryCrate", StringComparison.OrdinalIgnoreCase)
                    || n.Equals("GHelper", StringComparison.OrdinalIgnoreCase)
                    || n.StartsWith("Asus", StringComparison.OrdinalIgnoreCase))
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Order();
            SidecarLog.Log($"[rigstats-control] ASUS ACPI (board {board}, BIOS {bios}), read-only; " +
                $"ASUS software running: {string.Join(", ", asusSoftware).NullIfEmpty() ?? "none"}");

            var words = ReadIds.Select(id =>
            {
                var raw = Read(device, id, 0, 4);
                return raw is null
                    ? $"0x{id:X8}=fail"
                    : $"0x{id:X8}=0x{BitConverter.ToUInt32(raw):X8}{(IsPresent(BitConverter.ToUInt32(raw)) ? "" : "(absent)")}";
            });
            SidecarLog.Log($"[rigstats-control] ASUS ACPI DSTS: {string.Join(" ", words)}");

            foreach (var id in CurveIds)
            {
                var curves = Enumerable.Range(0, 3).Select(mode =>
                {
                    var raw = Read(device, id, (uint)mode, 32);
                    return raw is null ? $"[{mode}] fail" : $"[{mode}] {Convert.ToHexString(raw, 0, 16)}";
                });
                SidecarLog.Log($"[rigstats-control] ASUS ACPI curve 0x{id:X8} (arg 0 Balanced, 1 Silent, 2 Turbo): {string.Join(" ", curves)}");
            }
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] ASUS ACPI: not read: {e.Message}");
        }
    }

    private static byte[]? Read(SafeFileHandle device, uint id, uint arg, int outSize)
    {
        var input = DstsInput(id, arg);
        var output = new byte[Math.Max(outSize, 16)];
        if (!DeviceIoControl(device, Ioctl, input, (uint)input.Length, output, (uint)output.Length, out var returned, IntPtr.Zero))
        {
            SidecarLog.Log($"[rigstats-control] ASUS ACPI: DSTS 0x{id:X8} arg {arg} failed: error {Marshal.GetLastWin32Error()}");
            return null;
        }
        return returned >= 4 ? output : null;
    }

    private static string? NullIfEmpty(this string s) => s.Length == 0 ? null : s;

    private const uint GenericRead = 0x80000000;
    private const uint GenericWrite = 0x40000000;
    private const uint FileShareReadWrite = 3;
    private const uint OpenExisting = 3;

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern SafeFileHandle CreateFileW(string name, uint access, uint share, IntPtr security,
        uint disposition, uint flags, IntPtr template);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool DeviceIoControl(SafeFileHandle device, uint code, byte[] inBuf, uint inSize,
        byte[] outBuf, uint outSize, out uint returned, IntPtr overlapped);
}
