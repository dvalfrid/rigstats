using System.Runtime.InteropServices;
using SensorSidecar.Control;

namespace SensorSidecar;

/// Limits where this process looks for a DLL named without a path to its own
/// folder and System32 (#675). The service and both workers run as
/// LocalSystem, and the vendor libraries are loaded by bare name
/// (`nvml.dll`, `amdadlx64.dll`, and LibreHardwareMonitor's `nvapi64.dll`,
/// `atiadlxx.dll`, …). On a PC without that vendor the standard search goes
/// on through the current directory and every PATH directory, so a user who
/// can write to one of those would get their DLL run as SYSTEM. Every
/// library we load legitimately is in one of the two: the drivers install
/// theirs in System32, the .NET runtime is in the app folder, and the one
/// elsewhere (NVML on old drivers, under NVSMI) is loaded by full path.
///
/// Call first in `Program.cs`, before anything loads a library.
public static class DllSearchPath
{
    private const uint LoadLibrarySearchApplicationDir = 0x0000_0200;
    private const uint LoadLibrarySearchSystem32 = 0x0000_0800;

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetDefaultDllDirectories(uint directoryFlags);

    public static void Restrict()
    {
        if (!SetDefaultDllDirectories(LoadLibrarySearchApplicationDir | LoadLibrarySearchSystem32))
            SidecarLog.Log($"[rigstats-sensor] DLL search path not restricted (error {Marshal.GetLastPInvokeError()}).");
    }

    /// A driver library from System32, by bare name. Use this, not
    /// `NativeLibrary.TryLoad(name)`: that overload loads with
    /// LOAD_WITH_ALTERED_SEARCH_PATH, which after `Restrict` no longer finds
    /// a bare name at all — not even in System32 (seen in `DllSearchPathTests`).
    public static bool TryLoadSystem(string name, out IntPtr handle) =>
        NativeLibrary.TryLoad(name, typeof(DllSearchPath).Assembly, DllImportSearchPath.System32, out handle);
}
