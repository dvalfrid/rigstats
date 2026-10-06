namespace SensorSidecar.Control.Providers;

/// Several GPU drivers behind one `IGpuPowerApi` — AMD (ADLX) and NVIDIA
/// (NVML) in the same machine (#210). Each adapter is handled by the driver
/// that listed it. A driver that fails only hides its own adapters: an AMD
/// iGPU whose ADLX won't initialize (#241) must not hide an NVIDIA card.
public sealed class GpuPowerApis : IGpuPowerApi
{
    private readonly IReadOnlyList<IGpuPowerApi> _apis;
    private readonly object _lock = new();
    private Dictionary<string, IGpuPowerApi> _owners = [];

    private GpuPowerApis(IReadOnlyList<IGpuPowerApi> apis) => _apis = apis;

    /// The drivers that are installed, combined; null when none is.
    public static IGpuPowerApi? Of(params IGpuPowerApi?[] apis)
    {
        var present = apis.OfType<IGpuPowerApi>().ToList();
        return present.Count switch
        {
            0 => null,
            1 => present[0],
            _ => new GpuPowerApis(present),
        };
    }

    /// Every driver's adapters. Throws only when all drivers failed, so the
    /// GPU tab says the driver did not answer rather than "no GPU".
    public IReadOnlyList<GpuPowerAdapter> Adapters()
    {
        var adapters = new List<GpuPowerAdapter>();
        var owners = new Dictionary<string, IGpuPowerApi>();
        Exception? failure = null;
        var answered = false;
        foreach (var api in _apis)
        {
            try
            {
                foreach (var adapter in api.Adapters())
                {
                    adapters.Add(adapter);
                    owners[adapter.Id] = api;
                }
                answered = true;
            }
            catch (Exception e)
            {
                failure ??= e; // the driver logs its own failures
            }
        }
        lock (_lock)
            _owners = owners;
        if (!answered && failure is not null)
            throw failure;
        return adapters;
    }

    public int GetPowerLimit(string adapterId) => Owner(adapterId).GetPowerLimit(adapterId);

    public void SetPowerLimit(string adapterId, int percent) => Owner(adapterId).SetPowerLimit(adapterId, percent);

    public bool IsAtFactory(string adapterId) => Owner(adapterId).IsAtFactory(adapterId);

    public void ResetToFactory(string adapterId) => Owner(adapterId).ResetToFactory(adapterId);

    public void EndSession()
    {
        foreach (var api in _apis)
        {
            try
            {
                api.EndSession();
            }
            catch (Exception e)
            {
                SidecarLog.Log($"[rigstats-control] GPU power: ending a driver session failed: {e.Message}");
            }
        }
    }

    private IGpuPowerApi Owner(string adapterId)
    {
        lock (_lock)
        {
            if (_owners.TryGetValue(adapterId, out var owner))
                return owner;
        }
        Adapters(); // not listed yet (a call before any probe): look again
        lock (_lock)
        {
            return _owners.TryGetValue(adapterId, out var owner)
                ? owner
                : throw new InvalidOperationException($"GPU '{adapterId}' has no power tuning.");
        }
    }
}
