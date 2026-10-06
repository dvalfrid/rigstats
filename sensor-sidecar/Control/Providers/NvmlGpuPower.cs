using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Providers;

/// NVIDIA GPUs through NVML (`nvml.dll`, installed with the driver) — the
/// board power limit, the same setting as MSI Afterburner's Power Limit and
/// `nvidia-smi -pl` (#210). NVML speaks milliwatts; `GpuPowerProvider`
/// speaks a percentage offset from the driver default, so the limits are
/// converted here (`Describe`, `ToPercent`, `ToMilliwatts`) and the GPU tab
/// stays vendor-neutral. Adapters are identified by their NVML UUID, stable
/// across reboots and driver updates.
///
/// Laptop GPUs are left out: their power budget belongs to the firmware
/// (Dynamic Boost and the maker's performance modes move it above the
/// driver default — 118 W on an 80 W-default RTX 5070 Ti Laptop GPU), and
/// NVML refuses to set it there. On ASUS laptops that budget is #234's.
/// A desktop board that refuses a write anyway is left out from then on.
public sealed class NvmlGpuPower : IGpuPowerApi, IDisposable
{
    private const int NvmlSuccess = 0;
    private const int NvmlNotSupported = 3;
    private const int NvmlNoPermission = 4;
    private const int TextLength = 96; // NVML_DEVICE_NAME_V2_BUFFER_SIZE, > NVML_DEVICE_UUID_V2_BUFFER_SIZE

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int NoArgFn();
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int CountFn(out uint count);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int HandleFn(uint index, out IntPtr device);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int TextFn(IntPtr device, [Out] byte[] text, uint length);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int RangeFn(IntPtr device, out uint min, out uint max);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int OutUIntFn(IntPtr device, out uint value);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int InUIntFn(IntPtr device, uint value);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate int SystemTextFn([Out] byte[] text, uint length);

    private readonly object _lock = new();
    private readonly IntPtr _library;
    private readonly NoArgFn _init;
    private readonly NoArgFn _shutdown;
    private readonly CountFn _count;
    private readonly HandleFn _handle;
    private readonly TextFn _name;
    private readonly TextFn _uuid;
    private readonly RangeFn _constraints;
    private readonly OutUIntFn _defaultLimit;
    private readonly OutUIntFn _limit;
    private readonly InUIntFn _setLimit;
    // Diagnostics only — optional, so an old nvml.dll still loads.
    private readonly OutUIntFn? _enforcedLimit;
    private readonly SystemTextFn? _driverVersion;
    private readonly HashSet<string> _refused = [];
    private bool _initialized;
    private long _retryAfterTick;

    /// How long a failed nvmlInit is not tried again — every capabilities
    /// request and profile apply would otherwise re-run it.
    private static readonly TimeSpan InitRetryDelay = TimeSpan.FromMinutes(1);

    private NvmlGpuPower(IntPtr library)
    {
        _library = library;
        _init = Export<NoArgFn>("nvmlInit_v2");
        _shutdown = Export<NoArgFn>("nvmlShutdown");
        _count = Export<CountFn>("nvmlDeviceGetCount_v2");
        _handle = Export<HandleFn>("nvmlDeviceGetHandleByIndex_v2");
        _name = Export<TextFn>("nvmlDeviceGetName");
        _uuid = Export<TextFn>("nvmlDeviceGetUUID");
        _constraints = Export<RangeFn>("nvmlDeviceGetPowerManagementLimitConstraints");
        _defaultLimit = Export<OutUIntFn>("nvmlDeviceGetPowerManagementDefaultLimit");
        _limit = Export<OutUIntFn>("nvmlDeviceGetPowerManagementLimit");
        _setLimit = Export<InUIntFn>("nvmlDeviceSetPowerManagementLimit");
        _enforcedLimit = OptionalExport<OutUIntFn>("nvmlDeviceGetEnforcedPowerLimit");
        _driverVersion = OptionalExport<SystemTextFn>("nvmlSystemGetDriverVersion");
    }

    private T Export<T>(string name) where T : Delegate =>
        Marshal.GetDelegateForFunctionPointer<T>(NativeLibrary.GetExport(_library, name));

    private T? OptionalExport<T>(string name) where T : Delegate =>
        NativeLibrary.TryGetExport(_library, name, out var fn) ? Marshal.GetDelegateForFunctionPointer<T>(fn) : null;

    /// Null when the NVIDIA driver (NVML) isn't installed.
    public static NvmlGpuPower? TryLoad()
    {
        // System32 on current drivers; NVSMI on old ones.
        if (!NativeLibrary.TryLoad("nvml.dll", out var library)
            && !NativeLibrary.TryLoad(Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles),
                @"NVIDIA Corporation\NVSMI\nvml.dll"), out library))
        {
            return null;
        }
        try
        {
            return new NvmlGpuPower(library);
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] GPU power: NVML unusable: {e.Message}");
            NativeLibrary.Free(library);
            return null;
        }
    }

    private sealed record Device(IntPtr Handle, string Id, string Name);

    public IReadOnlyList<GpuPowerAdapter> Adapters() => WithDevices(devices =>
    {
        var adapters = new List<GpuPowerAdapter>();
        foreach (var device in devices)
        {
            if (IsLaptopGpu(device.Name) || _refused.Contains(device.Id))
                continue;
            // A limit that can't be read can't be managed — the G14's laptop
            // GPU answers NOT_SUPPORTED here while its constraints read fine.
            if (_constraints(device.Handle, out var min, out var max) != NvmlSuccess
                || _defaultLimit(device.Handle, out var defaultMw) != NvmlSuccess
                || _limit(device.Handle, out _) != NvmlSuccess)
            {
                continue; // no power management on this board
            }
            if (Describe(device.Id, device.Name, min, max, defaultMw) is { } adapter)
                adapters.Add(adapter);
        }
        return adapters;
    });

    public int GetPowerLimit(string adapterId) => WithDevice(adapterId, device =>
        ToPercent(Read(_limit, device, "GetPowerManagementLimit"), Read(_defaultLimit, device, "GetPowerManagementDefaultLimit")));

    public void SetPowerLimit(string adapterId, int percent) => WithDevice(adapterId, device =>
    {
        Check(_constraints(device.Handle, out var min, out var max), "GetPowerManagementLimitConstraints");
        Set(device, ToMilliwatts(percent, Read(_defaultLimit, device, "GetPowerManagementDefaultLimit"), min, max));
        return 0;
    });

    /// NVML has one tuning value here, the power limit: at its default is
    /// at factory.
    public bool IsAtFactory(string adapterId) => WithDevice(adapterId, device =>
        Read(_limit, device, "GetPowerManagementLimit") == Read(_defaultLimit, device, "GetPowerManagementDefaultLimit"));

    public void ResetToFactory(string adapterId) => WithDevice(adapterId, device =>
    {
        Set(device, Read(_defaultLimit, device, "GetPowerManagementDefaultLimit"));
        return 0;
    });

    /// What NVML reports, read-only, for the diagnostics export's
    /// `gpu-power.json` (#210): every NVIDIA GPU with its raw limits in mW,
    /// whether it is offered and why not — so a user's export shows whether
    /// NVML works from the service on their card. UUIDs are left out: they
    /// identify the machine and add nothing. Never throws.
    public JsonObject Diagnostics()
    {
        lock (_lock)
        {
            try
            {
                Initialize();
                Check(_count(out var count), "DeviceGetCount");
                var gpus = new JsonArray();
                for (uint i = 0; i < count; i++)
                {
                    var entry = new JsonObject { ["index"] = i };
                    gpus.Add(entry);
                    var handleResult = _handle(i, out var handle);
                    if (handleResult != NvmlSuccess)
                    {
                        entry["error"] = NvmlResultName(handleResult);
                        continue;
                    }
                    var name = Text(_name, handle);
                    var id = Text(_uuid, handle);
                    entry["name"] = name;
                    entry["laptop"] = IsLaptopGpu(name);
                    entry["refused_write"] = _refused.Contains(id);
                    var constraints = _constraints(handle, out var min, out var max);
                    entry["min_mw"] = Reading(constraints, min);
                    entry["max_mw"] = Reading(constraints, max);
                    var defaultResult = _defaultLimit(handle, out var defaultMw);
                    entry["default_mw"] = Reading(defaultResult, defaultMw);
                    var limitResult = _limit(handle, out var limit);
                    entry["limit_mw"] = Reading(limitResult, limit);
                    entry["enforced_mw"] = _enforcedLimit is null ? null : Reading(_enforcedLimit(handle, out var enforced), enforced);
                    var adapter = constraints == NvmlSuccess && defaultResult == NvmlSuccess && limitResult == NvmlSuccess
                        ? Describe(id, name, min, max, defaultMw)
                        : null;
                    entry["range_pct"] = adapter is null ? null : $"{adapter.Min}..{adapter.Max}";
                    entry["offered"] = adapter is not null && !IsLaptopGpu(name) && !_refused.Contains(id);
                }
                return new JsonObject
                {
                    ["driver_version"] = _driverVersion is null ? null : SystemText(_driverVersion),
                    ["gpus"] = gpus,
                };
            }
            catch (Exception e)
            {
                ShutdownLocked();
                return new JsonObject { ["error"] = e.Message };
            }
        }
    }

    /// The value, or NVML's answer when there is none.
    private static JsonNode Reading(int result, uint value) =>
        result == NvmlSuccess ? JsonValue.Create(value) : JsonValue.Create(NvmlResultName(result));

    private static string SystemText(SystemTextFn fn)
    {
        var buffer = new byte[TextLength];
        if (fn(buffer, TextLength) != NvmlSuccess)
            return "";
        var end = Array.IndexOf(buffer, (byte)0);
        return Encoding.UTF8.GetString(buffer, 0, end < 0 ? buffer.Length : end);
    }

    private void Set(Device device, uint milliwatts)
    {
        var result = _setLimit(device.Handle, milliwatts);
        if (result is NvmlNotSupported or NvmlNoPermission)
        {
            // Firmware-owned budget: not offered again this service run.
            _refused.Add(device.Id);
            SidecarLog.Log($"[rigstats-control] GPU power: {device.Name} refused a power limit ({NvmlResultName(result)}); no longer offered.");
        }
        Check(result, "SetPowerManagementLimit");
    }

    /// The driver's range as a percentage offset from its default: the
    /// whole range of whole percents that stays inside it. Null when the
    /// driver reports no usable range.
    public static GpuPowerAdapter? Describe(string id, string name, uint minMw, uint maxMw, uint defaultMw)
    {
        if (defaultMw == 0 || minMw > defaultMw || maxMw < defaultMw || minMw == maxMw)
            return null;
        var min = (int)Math.Ceiling((minMw / (double)defaultMw - 1) * 100 - 1e-9);
        var max = (int)Math.Floor((maxMw / (double)defaultMw - 1) * 100 + 1e-9);
        return new GpuPowerAdapter(id, name, min, max, 1, 0);
    }

    public static int ToPercent(uint limitMw, uint defaultMw) =>
        defaultMw == 0 ? 0 : (int)Math.Round((limitMw / (double)defaultMw - 1) * 100);

    public static uint ToMilliwatts(int percent, uint defaultMw, uint minMw, uint maxMw) =>
        (uint)Math.Clamp(Math.Round(defaultMw * (1 + percent / 100.0)), minMw, maxMw);

    /// NVIDIA names its notebook GPUs "… Laptop GPU" (RTX 30 and later) and
    /// "… with Max-Q Design" before that.
    public static bool IsLaptopGpu(string name) =>
        name.Contains("Laptop", StringComparison.OrdinalIgnoreCase)
        || name.Contains("Max-Q", StringComparison.OrdinalIgnoreCase)
        || name.Contains("Mobile", StringComparison.OrdinalIgnoreCase);

    private T WithDevice<T>(string adapterId, Func<Device, T> action) => WithDevices(devices =>
    {
        var device = devices.FirstOrDefault(d => d.Id == adapterId)
            ?? throw new InvalidOperationException($"NVIDIA GPU '{adapterId}' not found.");
        return action(device);
    });

    /// One pass over the GPUs. A failure ends the NVML session, so the next
    /// call starts fresh (e.g. after a driver update or reset).
    private T WithDevices<T>(Func<List<Device>, T> action)
    {
        lock (_lock)
        {
            try
            {
                Initialize();
                Check(_count(out var count), "DeviceGetCount");
                var devices = new List<Device>();
                for (uint i = 0; i < count; i++)
                {
                    if (_handle(i, out var handle) != NvmlSuccess)
                        continue; // a GPU NVML can't open (e.g. lost from the bus)
                    var id = Text(_uuid, handle);
                    if (id.Length > 0)
                        devices.Add(new Device(handle, id, Text(_name, handle)));
                }
                return action(devices);
            }
            catch
            {
                ShutdownLocked();
                throw;
            }
        }
    }

    private void Initialize()
    {
        if (_initialized)
            return;
        if (Environment.TickCount64 < _retryAfterTick)
            throw new InvalidOperationException("NVML unavailable (initialization failed recently; retried shortly).");
        var result = _init();
        if (result != NvmlSuccess)
        {
            _retryAfterTick = Environment.TickCount64 + (long)InitRetryDelay.TotalMilliseconds;
            SidecarLog.Log($"[rigstats-control] GPU power: nvmlInit answered {NvmlResultName(result)} — " +
                $"NVIDIA power limits unavailable, not tried again for {InitRetryDelay.TotalSeconds:0} s.");
            Check(result, "Init");
        }
        _retryAfterTick = 0;
        _initialized = true;
    }

    private void ShutdownLocked()
    {
        if (!_initialized)
            return;
        _initialized = false;
        try
        {
            _shutdown();
        }
        catch
        {
            // Nothing more to do; the next call initializes again.
        }
    }

    private static uint Read(OutUIntFn fn, Device device, string call)
    {
        Check(fn(device.Handle, out var value), call);
        return value;
    }

    private static string Text(TextFn fn, IntPtr device)
    {
        var buffer = new byte[TextLength];
        if (fn(device, buffer, TextLength) != NvmlSuccess)
            return "";
        var end = Array.IndexOf(buffer, (byte)0);
        return Encoding.UTF8.GetString(buffer, 0, end < 0 ? buffer.Length : end);
    }

    private static void Check(int result, string call)
    {
        if (result != NvmlSuccess)
            throw new InvalidOperationException($"NVML {call} failed ({NvmlResultName(result)}).");
    }

    /// `nvmlReturn_t` names, for the service log.
    public static string NvmlResultName(int result) => result switch
    {
        0 => "NVML_SUCCESS",
        1 => "NVML_ERROR_UNINITIALIZED",
        2 => "NVML_ERROR_INVALID_ARGUMENT",
        3 => "NVML_ERROR_NOT_SUPPORTED",
        4 => "NVML_ERROR_NO_PERMISSION",
        5 => "NVML_ERROR_ALREADY_INITIALIZED",
        6 => "NVML_ERROR_NOT_FOUND",
        7 => "NVML_ERROR_INSUFFICIENT_SIZE",
        8 => "NVML_ERROR_INSUFFICIENT_POWER",
        9 => "NVML_ERROR_DRIVER_NOT_LOADED",
        10 => "NVML_ERROR_TIMEOUT",
        12 => "NVML_ERROR_LIBRARY_NOT_FOUND",
        13 => "NVML_ERROR_FUNCTION_NOT_FOUND",
        15 => "NVML_ERROR_GPU_IS_LOST",
        17 => "NVML_ERROR_OPERATING_SYSTEM",
        18 => "NVML_ERROR_LIB_RM_VERSION_MISMATCH",
        999 => "NVML_ERROR_UNKNOWN",
        _ => result.ToString(),
    };

    public void EndSession()
    {
        lock (_lock)
            ShutdownLocked();
    }

    public void Dispose()
    {
        lock (_lock)
        {
            ShutdownLocked();
            NativeLibrary.Free(_library);
        }
    }
}
