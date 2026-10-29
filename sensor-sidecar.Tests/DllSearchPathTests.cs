using System.Runtime.InteropServices;
using SensorSidecar;
using Xunit;

namespace SensorSidecar.Tests;

/// #675: a DLL planted in a PATH directory must not be loaded by bare name.
/// `Restrict` changes the whole test process for good, which is harmless:
/// the tests load nothing from outside the app folder and System32.
public class DllSearchPathTests
{
    private const string Planted = "rigstats-planted-675.dll";

    // LibreHardwareMonitor loads the vendor libraries with [DllImport] by
    // bare name; these two stand in for that.
    [DllImport(Planted, EntryPoint = "GetFileVersionInfoSizeW", CharSet = CharSet.Unicode)]
    private static extern uint PlantedExport(string file, out uint handle);

    [DllImport("wtsapi32.dll")]
    private static extern void WTSFreeMemory(IntPtr memory);

    [Fact]
    public void A_dll_on_path_is_found_before_and_not_after_the_search_is_restricted()
    {
        // A renamed copy of a small Windows DLL stands in for a planted one.
        var dir = Directory.CreateTempSubdirectory("rigstats-dll-plant-");
        File.Copy(Path.Combine(Environment.SystemDirectory, "version.dll"), Path.Combine(dir.FullName, Planted));
        var path = Environment.GetEnvironmentVariable("PATH");
        try
        {
            Environment.SetEnvironmentVariable("PATH", $"{dir.FullName};{path}");

            Assert.True(NativeLibrary.TryLoad(Planted, out var planted), "the plant is found through PATH");
            NativeLibrary.Free(planted);

            DllSearchPath.Restrict();

            Assert.False(DllSearchPath.TryLoadSystem(Planted, out _), "the plant is no longer found");
            Assert.Throws<DllNotFoundException>(() => PlantedExport("x", out _));

            Assert.True(DllSearchPath.TryLoadSystem("version.dll", out var system32), "System32 still is");
            NativeLibrary.Free(system32);
            WTSFreeMemory(IntPtr.Zero); // [DllImport] still finds System32
        }
        finally
        {
            Environment.SetEnvironmentVariable("PATH", path);
            try { dir.Delete(recursive: true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        }
    }
}
