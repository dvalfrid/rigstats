using System.Runtime.InteropServices;

namespace SensorSidecar.Control.Providers;

/// One GPU whose power limit can be tuned. The limit is a percentage offset
/// from the driver's default (0 = default), within [Min, Max].
public sealed record GpuPowerAdapter(string Id, string Name, int Min, int Max, int Step, int Default);

/// The GPU power-limit operations `GpuPowerProvider` needs — a seam for
/// tests and for a future NVIDIA (NVML, #210) backend.
public interface IGpuPowerApi
{
    /// Adapters that support power tuning; empty when none (or no driver).
    IReadOnlyList<GpuPowerAdapter> Adapters();

    int GetPowerLimit(string adapterId);

    void SetPowerLimit(string adapterId, int percent);

    /// Whether all of the GPU's tuning is at factory settings (Adrenalin
    /// shows "Default" rather than "Custom").
    bool IsAtFactory(string adapterId);

    /// Back to factory settings — all of the GPU's tuning, not just the
    /// power limit.
    void ResetToFactory(string adapterId);
}

/// AMD GPUs through ADLX (`amdadlx64.dll`, installed with the Adrenalin
/// driver) — "manual power tuning", the same setting as Adrenalin's
/// Performance → Tuning → Power Limit. Calls the SDK's C interface (plain
/// vtables) through delegates; no native wrapper, no `unsafe`. Works from
/// session 0 as SYSTEM (checked on an RX 9070 XT, ADLX runtime 1.5).
/// Adapters are identified by their PNP device instance path, stable
/// across reboots. Vtable slots are from the ADLX SDK headers
/// (GPUOpen-LibrariesAndSDKs/ADLX, `SDK/Include`).
public sealed class AdlxGpuPower : IGpuPowerApi, IDisposable
{
    private const int AdlxOk = 0;

    // IADLXSystem
    private const int SystemGetGpus = 1;
    private const int SystemGetGpuTuningServices = 8;
    // IADLXInterface (every other interface starts with these)
    private const int Release = 1;
    private const int QueryInterface = 2;
    // IADLXGPUList
    private const int ListSize = 3;
    private const int ListAtGpu = 11;
    // IADLXGPU
    private const int GpuName = 7;
    private const int GpuPnpString = 9;
    // IADLXGPUTuningServices
    private const int TuningIsAtFactory = 4;
    private const int TuningResetToFactory = 5;
    private const int TuningIsSupportedManualPower = 11;
    private const int TuningGetManualPower = 17;
    // IADLXManualPowerTuning / IADLXManualPowerTuning1
    private const int PowerGetRange = 3;
    private const int PowerGet = 4;
    private const int PowerSet = 5;
    private const int PowerGetDefault = 10;

    [StructLayout(LayoutKind.Sequential)]
    private struct IntRange
    {
        public int Min, Max, Step;
    }

    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int QueryFullVersionFn(out ulong version);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int InitializeFn(ulong version, out IntPtr system);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int TerminateFn();

    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate long ReleaseFn(IntPtr self);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int OutPtrFn(IntPtr self, out IntPtr result);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int OutIntFn(IntPtr self, out int value);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int InIntFn(IntPtr self, int value);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate uint SizeFn(IntPtr self);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int AtFn(IntPtr self, uint index, out IntPtr item);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int GpuOutIntFn(IntPtr self, IntPtr gpu, out int value);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int GpuFn(IntPtr self, IntPtr gpu);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int GpuOutPtrFn(IntPtr self, IntPtr gpu, out IntPtr value);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int QueryInterfaceFn(IntPtr self, [MarshalAs(UnmanagedType.LPWStr)] string iid, out IntPtr value);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    private delegate int RangeFn(IntPtr self, out IntRange range);

    private readonly object _lock = new();
    private readonly IntPtr _library;
    private readonly InitializeFn? _initialize;
    private readonly TerminateFn? _terminate;
    private readonly ulong _version;
    private IntPtr _system;

    private AdlxGpuPower(IntPtr library)
    {
        _library = library;
        Marshal.GetDelegateForFunctionPointer<QueryFullVersionFn>(
            NativeLibrary.GetExport(library, "ADLXQueryFullVersion"))(out _version);
        _initialize = Marshal.GetDelegateForFunctionPointer<InitializeFn>(NativeLibrary.GetExport(library, "ADLXInitialize"));
        _terminate = Marshal.GetDelegateForFunctionPointer<TerminateFn>(NativeLibrary.GetExport(library, "ADLXTerminate"));
    }

    /// Null when the AMD driver (ADLX) isn't installed.
    public static AdlxGpuPower? TryLoad()
    {
        if (!NativeLibrary.TryLoad("amdadlx64.dll", out var library))
            return null;
        try
        {
            return new AdlxGpuPower(library);
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] GPU power: ADLX unusable: {e.Message}");
            NativeLibrary.Free(library);
            return null;
        }
    }

    public IReadOnlyList<GpuPowerAdapter> Adapters() => WithGpus(gpus =>
        gpus.Where(g => g.Power != IntPtr.Zero) // GPUs without power tuning (e.g. an iGPU)
            .Select(g => Describe(g, g.Power))
            .ToList());

    public int GetPowerLimit(string adapterId) => WithPowerTuning(adapterId, power =>
    {
        Check(Fn<OutIntFn>(power, PowerGet)(power, out var value), "GetPowerLimit");
        return value;
    });

    public void SetPowerLimit(string adapterId, int percent) => WithPowerTuning(adapterId, power =>
    {
        Check(Fn<InIntFn>(power, PowerSet)(power, percent), "SetPowerLimit");
        return 0;
    });

    public bool IsAtFactory(string adapterId) => WithGpu(adapterId, (gpu, tuning) =>
    {
        Check(Fn<GpuOutIntFn>(tuning, TuningIsAtFactory)(tuning, gpu.Handle, out var atFactory), "IsAtFactory");
        return atFactory != 0;
    });

    public void ResetToFactory(string adapterId) => WithGpu(adapterId, (gpu, tuning) =>
    {
        Check(Fn<GpuFn>(tuning, TuningResetToFactory)(tuning, gpu.Handle), "ResetToFactory");
        return 0;
    });

    private sealed record Gpu(IntPtr Handle, string Name, string Id, IntPtr Power, IntPtr Tuning);

    private static GpuPowerAdapter Describe(Gpu gpu, IntPtr power)
    {
        Check(Fn<RangeFn>(power, PowerGetRange)(power, out var range), "GetPowerLimitRange");
        // IADLXManualPowerTuning1 (newer drivers) knows the default; the
        // range is an offset around it, so 0 is the default otherwise.
        var defaultValue = 0;
        if (Fn<QueryInterfaceFn>(power, QueryInterface)(power, "IADLXManualPowerTuning1", out var power1) == AdlxOk)
        {
            if (Fn<OutIntFn>(power1, PowerGetDefault)(power1, out var d) == AdlxOk)
                defaultValue = d;
            ReleaseObject(power1);
        }
        return new GpuPowerAdapter(gpu.Id, gpu.Name, range.Min, range.Max, Math.Max(1, range.Step), defaultValue);
    }

    private T WithPowerTuning<T>(string adapterId, Func<IntPtr, T> action) =>
        WithGpu(adapterId, (gpu, _) => action(gpu.Power));

    private T WithGpu<T>(string adapterId, Func<Gpu, IntPtr, T> action) => WithGpus(gpus =>
    {
        var gpu = gpus.FirstOrDefault(g => g.Id == adapterId && g.Power != IntPtr.Zero)
            ?? throw new InvalidOperationException($"GPU '{adapterId}' has no power tuning.");
        return action(gpu, gpu.Tuning);
    });

    /// One pass over the GPU list with everything acquired released after.
    /// A failure drops the ADLX session, so the next call starts fresh
    /// (e.g. after a driver update or reset).
    private T WithGpus<T>(Func<List<Gpu>, T> action)
    {
        lock (_lock)
        {
            var acquired = new List<IntPtr>();
            try
            {
                var system = System();
                Check(Fn<OutPtrFn>(system, SystemGetGpus)(system, out var list), "GetGPUs");
                acquired.Add(list);
                Check(Fn<OutPtrFn>(system, SystemGetGpuTuningServices)(system, out var tuning), "GetGPUTuningServices");
                acquired.Add(tuning);

                var gpus = new List<Gpu>();
                var count = Fn<SizeFn>(list, ListSize)(list);
                for (uint i = 0; i < count; i++)
                {
                    if (Fn<AtFn>(list, ListAtGpu)(list, i, out var gpu) != AdlxOk)
                        continue;
                    acquired.Add(gpu);
                    var power = IntPtr.Zero;
                    if (Fn<GpuOutIntFn>(tuning, TuningIsSupportedManualPower)(tuning, gpu, out var supported) == AdlxOk
                        && supported != 0
                        && Fn<GpuOutPtrFn>(tuning, TuningGetManualPower)(tuning, gpu, out power) == AdlxOk)
                    {
                        acquired.Add(power);
                    }
                    gpus.Add(new Gpu(gpu, Text(gpu, GpuName), Text(gpu, GpuPnpString), power, tuning));
                }
                var result = action(gpus);
                ReleaseAll(acquired);
                return result;
            }
            catch
            {
                // Release before terminating: ADLXTerminate frees every
                // object, so releasing after it would touch freed memory.
                ReleaseAll(acquired);
                TerminateLocked();
                throw;
            }
        }
    }

    private IntPtr System()
    {
        if (_system != IntPtr.Zero)
            return _system;
        // Ask for exactly the runtime's version: the vtables used here exist
        // in every ADLX release that has manual power tuning.
        Check(_initialize!(_version, out var system), "ADLXInitialize");
        _system = system;
        return system;
    }

    private void TerminateLocked()
    {
        if (_system == IntPtr.Zero)
            return;
        _system = IntPtr.Zero;
        try
        {
            _terminate!();
        }
        catch
        {
            // Nothing more to do; the next call re-initializes.
        }
    }

    private static string Text(IntPtr obj, int slot) =>
        Fn<OutPtrFn>(obj, slot)(obj, out var text) == AdlxOk ? Marshal.PtrToStringAnsi(text) ?? "" : "";

    private static void ReleaseAll(List<IntPtr> acquired)
    {
        for (var i = acquired.Count - 1; i >= 0; i--)
            ReleaseObject(acquired[i]);
        acquired.Clear(); // never released twice
    }

    private static void ReleaseObject(IntPtr obj)
    {
        if (obj != IntPtr.Zero)
            Fn<ReleaseFn>(obj, Release)(obj);
    }

    private static T Fn<T>(IntPtr obj, int slot) where T : Delegate =>
        Marshal.GetDelegateForFunctionPointer<T>(Marshal.ReadIntPtr(Marshal.ReadIntPtr(obj), slot * IntPtr.Size));

    private static void Check(int result, string call)
    {
        if (result != AdlxOk)
            throw new InvalidOperationException($"ADLX {call} failed ({AdlxResultName(result)}).");
    }

    /// `ADLX_RESULT` names, for the service log.
    public static string AdlxResultName(int result) => result switch
    {
        0 => "ADLX_OK",
        1 => "ADLX_ALREADY_ENABLED",
        2 => "ADLX_ALREADY_INITIALIZED",
        3 => "ADLX_FAIL",
        4 => "ADLX_INVALID_ARGS",
        5 => "ADLX_BAD_VER",
        6 => "ADLX_UNKNOWN_INTERFACE",
        7 => "ADLX_TERMINATED",
        8 => "ADLX_ADL_INIT_ERROR",
        9 => "ADLX_NOT_FOUND",
        10 => "ADLX_INVALID_OBJECT",
        11 => "ADLX_ORPHAN_OBJECTS",
        12 => "ADLX_NOT_SUPPORTED",
        13 => "ADLX_PENDING_OPERATION",
        14 => "ADLX_GPU_INACTIVE",
        15 => "ADLX_GPU_IN_USE",
        16 => "ADLX_TIMEOUT_OPERATION",
        17 => "ADLX_NOT_ACTIVE",
        18 => "ADLX_RESET_NEEDED",
        _ => result.ToString(),
    };

    public void Dispose()
    {
        lock (_lock)
        {
            TerminateLocked();
            NativeLibrary.Free(_library);
        }
    }
}
