using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace SensorSidecar.Control.Providers;

/// Runs functions of one loaded PawnIO module. A seam so the SMU logic in
/// `RyzenSmu` is unit-testable without the driver.
public interface IPawnIoModule : IDisposable
{
    /// Calls the module's `fn` with `input`; returns `outCount` values.
    /// Throws `Win32Exception` with the module's NTSTATUS-derived error.
    long[] Execute(string fn, long[] input, int outCount);
}

/// One handle to the PawnIO driver with one module loaded into it, talking
/// to `\\.\PawnIO` directly (same IOCTLs as LHM's own `PawnIo` class, whose
/// module loader isn't public). The module blob is LHM's own signed
/// resource, so this adds no new binary: PawnIO only runs modules signed by
/// its author. No logic beyond marshaling.
public sealed class PawnIoModule : IPawnIoModule
{
    private const uint IoctlLoadBinary = 0xA1B22084;
    private const uint IoctlExecute = 0xA1B22104;
    private const int FnNameLength = 32;

    private readonly SafeFileHandle _device;

    private PawnIoModule(SafeFileHandle device) => _device = device;

    /// Opens the driver and loads `LibreHardwareMonitor.Resources.PawnIo.<name>`.
    /// Throws when PawnIO isn't installed or the caller isn't elevated.
    public static PawnIoModule LoadFromLhm(string name)
    {
        using var blob = typeof(LibreHardwareMonitor.Hardware.Computer).Assembly
            .GetManifestResourceStream($"LibreHardwareMonitor.Resources.PawnIo.{name}")
            ?? throw new InvalidOperationException($"PawnIO module {name} not found in LibreHardwareMonitorLib.");
        using var bytes = new MemoryStream();
        blob.CopyTo(bytes);

        var device = CreateFileW(@"\\.\PawnIO", GenericRead | GenericWrite, FileShareReadWrite,
            IntPtr.Zero, OpenExisting, 0, IntPtr.Zero);
        if (device.IsInvalid)
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "PawnIO driver not available");

        var module = new PawnIoModule(device);
        try
        {
            module.Io(IoctlLoadBinary, bytes.ToArray(), 0);
        }
        catch
        {
            module.Dispose();
            throw;
        }
        return module;
    }

    public long[] Execute(string fn, long[] input, int outCount)
    {
        var inBuf = new byte[FnNameLength + input.Length * sizeof(long)];
        System.Text.Encoding.ASCII.GetBytes(fn, 0, Math.Min(fn.Length, FnNameLength - 1), inBuf, 0);
        Buffer.BlockCopy(input, 0, inBuf, FnNameLength, input.Length * sizeof(long));
        var outBuf = Io(IoctlExecute, inBuf, outCount * sizeof(long));
        var result = new long[outCount];
        Buffer.BlockCopy(outBuf, 0, result, 0, outCount * sizeof(long));
        return result;
    }

    private byte[] Io(uint code, byte[] input, int outSize)
    {
        var output = new byte[Math.Max(outSize, 1)];
        if (!DeviceIoControl(_device, code, input, (uint)input.Length, output, (uint)outSize, out _, IntPtr.Zero))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        return output;
    }

    public void Dispose() => _device.Dispose();

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
