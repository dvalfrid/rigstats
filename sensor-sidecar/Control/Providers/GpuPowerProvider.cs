using System.Text.Json;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Providers;

/// `IControlProvider` for GPU power limits (#190, Control Center phase 3).
/// AMD through ADLX for now (`AdlxGpuPower`); NVIDIA (NVML) is #210. The
/// limit is a percentage offset from the driver default, within the
/// driver's own range — the same knob as Adrenalin's Power Limit.
///
/// "Original" is the value in force before RIGStats first changed an
/// adapter (the user's own Adrenalin setting, usually 0). It is recorded at
/// the first write and persisted, so it survives a crash or reboot, and is
/// what a profile without a value, a release and a service stop go back to.
/// Any power-limit change switches Adrenalin's tuning from "Default" to
/// "Custom"; when the GPU was at factory settings before RIGStats, going
/// back to the original value is a factory reset, so Adrenalin shows
/// "Default" again instead of "Custom" with default values.
public sealed class GpuPowerProvider(IGpuPowerApi? api, bool dryRun, string? originalsPath) : IControlProvider
{
    private readonly object _lock = new();
    /// The value in force before RIGStats first changed an adapter, and
    /// whether all its tuning was at factory settings then.
    public sealed record GpuOriginal(int PowerLimitPct, bool AtFactory);

    private readonly Dictionary<string, GpuOriginal> _originals = LoadOriginals(originalsPath);
    private Dictionary<string, int> _lastTargets = [];

    public string Domain => "gpu";

    public CapabilitySet Probe()
    {
        IReadOnlyList<GpuPowerAdapter> adapters;
        try
        {
            adapters = api?.Adapters() ?? [];
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] GPU power: probe failed: {e.Message}");
            adapters = [];
        }
        if (adapters.Count == 0)
        {
            return new CapabilitySet
            {
                Domain = Domain,
                Supported = false,
                Reason = "No GPU with power limit control found (AMD Radeon with the Adrenalin driver).",
            };
        }
        return new CapabilitySet
        {
            Domain = Domain,
            Supported = true,
            Details = new JsonObject
            {
                ["adapters"] = new JsonArray(adapters.Select(a => (JsonNode)new JsonObject
                {
                    ["id"] = a.Id,
                    ["name"] = a.Name,
                    ["min"] = a.Min,
                    ["max"] = a.Max,
                    ["step"] = a.Step,
                    ["default"] = a.Default,
                    ["original"] = Original(a),
                    ["current"] = api!.GetPowerLimit(a.Id),
                }).ToArray()),
            },
        };
    }

    // Adapter ids that aren't present (a swapped GPU) are skipped, not
    // rejected: a stale entry must not block the rest of a profile.
    public ValidationResult Validate(ProfilePart part) => ValidationResult.Success();

    public Snapshot Capture()
    {
        if (api is null)
            return new Snapshot { Domain = Domain };
        var state = new JsonObject();
        foreach (var adapter in api.Adapters())
            state[adapter.Id] = api.GetPowerLimit(adapter.Id);
        return new Snapshot { Domain = Domain, State = state };
    }

    public void Apply(ProfilePart part)
    {
        if (part.Gpu is null || api is null)
            return;
        var targets = new Dictionary<string, int>();
        foreach (var adapter in api.Adapters())
        {
            GpuAdapterConfig? config = null;
            part.Gpu.Adapters?.TryGetValue(adapter.Id, out config);
            targets[adapter.Id] = Resolve(config?.PowerLimitPct, Original(adapter), adapter);
        }
        lock (_lock)
            _lastTargets = targets;
        foreach (var (id, target) in targets)
            Write(id, target);
    }

    public bool Verify(ProfilePart part)
    {
        if (part.Gpu is null || api is null || dryRun)
            return true;
        Dictionary<string, int> targets;
        lock (_lock)
            targets = _lastTargets;
        foreach (var (id, target) in targets)
        {
            var actual = api.GetPowerLimit(id);
            if (actual != target)
            {
                SidecarLog.Log($"[rigstats-control] GPU power verify failed: {id} set {target} %, readback {actual} %.");
                return false;
            }
        }
        return true;
    }

    public void Restore(Snapshot snapshot)
    {
        if (snapshot.Domain != Domain || snapshot.State is not JsonObject state || api is null)
            return;
        foreach (var (id, value) in state)
        {
            if (value is not null)
                Write(id, value.GetValue<int>());
        }
    }

    /// Every adapter RIGStats changed goes back to its original value, and
    /// the record of originals is cleared.
    public void ReleaseToFirmware()
    {
        if (api is null)
            return;
        Dictionary<string, GpuOriginal> originals;
        lock (_lock)
            originals = new Dictionary<string, GpuOriginal>(_originals);
        if (originals.Count == 0)
            return;
        var present = api.Adapters().Select(a => a.Id).ToHashSet();
        foreach (var (id, original) in originals)
        {
            if (present.Contains(id))
                Write(id, original.PowerLimitPct);
        }
        lock (_lock)
        {
            _originals.Clear();
            SaveOriginals();
        }
    }

    private void Write(string id, int target)
    {
        GpuOriginal? original;
        lock (_lock)
            _originals.TryGetValue(id, out original);

        // Back to a factory-state original: reset, so Adrenalin leaves
        // "Custom" too — even when the value alone already matches.
        if (original is { AtFactory: true } && target == original.PowerLimitPct)
        {
            if (api!.IsAtFactory(id))
                return;
            if (dryRun)
            {
                SidecarLog.Log($"[rigstats-control] dry-run: GPU {id} reset to factory");
                return;
            }
            api.ResetToFactory(id);
            return;
        }

        var current = api!.GetPowerLimit(id);
        if (current == target)
            return;
        if (dryRun)
        {
            SidecarLog.Log($"[rigstats-control] dry-run: GPU {id} power limit {current} % -> {target} %");
            return;
        }
        if (original is null)
        {
            lock (_lock)
            {
                _originals[id] = new GpuOriginal(current, api.IsAtFactory(id));
                SaveOriginals();
            }
        }
        api.SetPowerLimit(id, target);
    }

    private int Original(GpuPowerAdapter adapter)
    {
        lock (_lock)
        {
            if (_originals.TryGetValue(adapter.Id, out var original))
                return original.PowerLimitPct;
        }
        return api!.GetPowerLimit(adapter.Id); // never changed: what's in force is the original.
    }

    /// What to write: the requested value (or the original), clamped to the
    /// driver's range and snapped to its step.
    public static int Resolve(int? requested, int original, GpuPowerAdapter adapter)
    {
        var value = Math.Clamp(requested ?? original, adapter.Min, adapter.Max);
        var steps = (int)Math.Round((value - adapter.Min) / (double)adapter.Step);
        return Math.Min(adapter.Min + steps * adapter.Step, adapter.Max);
    }

    private static Dictionary<string, GpuOriginal> LoadOriginals(string? path)
    {
        if (path is null || !File.Exists(path))
            return [];
        try
        {
            return JsonSerializer.Deserialize<Dictionary<string, GpuOriginal>>(File.ReadAllText(path), ControlJson.Options) ?? [];
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] GPU power originals unreadable: {e.Message}");
            return [];
        }
    }

    // Caller holds _lock.
    private void SaveOriginals()
    {
        if (originalsPath is null)
            return;
        try
        {
            if (_originals.Count == 0)
            {
                File.Delete(originalsPath);
                return;
            }
            Directory.CreateDirectory(Path.GetDirectoryName(originalsPath)!);
            File.WriteAllText(originalsPath, JsonSerializer.Serialize(_originals, ControlJson.Options));
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] GPU power originals not saved: {e.Message}");
        }
    }
}
