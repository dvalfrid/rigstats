using System.Collections.Concurrent;
using System.Text.Json.Nodes;
using LibreHardwareMonitor.Hardware;

namespace SensorSidecar.Control.Providers;

/// One controllable fan header, as discovered by enumerating `/lpc/`
/// `SensorType.Control` sensors (siblings of the read-only `SensorType.Fan`
/// RPM sensors `SensorReader` already extracts).
internal readonly record struct FanHeaderInfo(string Id, string Label, double Min, double Max);

/// `IControlProvider` for fan headers (#188, Control Center phase 1).
/// Unlike `PowerPlanProvider`'s one-shot apply, a fan "apply" just replaces
/// which curves are active — the actual duty values are recomputed every
/// tick by `FanCurveLoop` against live temperatures, using
/// `FanCurveEvaluator`'s pure logic (`FanCurveLoop` calls back into
/// `ApplyDuty` here to do the actual write). This class owns the active
/// configuration `FanCurveLoop` reads, and is the only thing that touches
/// `IControl` directly (through `IHardwareHost`, never a raw `Computer`).
/// With `dryRun`, every write is skipped and `Verify` trusts the request.
public sealed class FanProvider(IHardwareHost host, bool dryRun) : IControlProvider
{
    /// Readback may differ from the commanded duty by the 0–255 PWM register
    /// rounding (~0.4 %) plus firmware step granularity; anything further
    /// off means firmware overrode the write.
    internal const double VerifyTolerancePct = 5.0;

    public string Domain => "fan";

    private readonly ConcurrentDictionary<string, FanHeaderConfig> _active = new();

    /// How long `identify_fan` holds a header at full speed.
    public static readonly TimeSpan IdentifyDuration = TimeSpan.FromSeconds(3);

    // Serialises ApplyDuty against release, so a FanCurveLoop tick that
    // computed its duty just before ReleaseToFirmware can't re-claim a
    // header right after it was handed back to the BIOS.
    private readonly object _writeLock = new();
    private readonly Dictionary<string, double> _commanded = new();

    // Headers currently held at full speed by `identify_fan`, each with the
    // token of the identify call that owns it — a newer identify of the same
    // header, or a release, makes an older call's end a no-op.
    private readonly Dictionary<string, long> _identifying = new();
    private long _identifyToken;

    // Super I/O control headers don't appear/disappear at runtime in
    // practice, so the header list (ids + min/max) is cheap to cache after
    // the first enumeration instead of walking the whole hardware tree on
    // every single duty write — `FanCurveLoop` calls `ApplyDuty` once per
    // active header every ~1s tick.
    private List<FanHeaderInfo>? _headerCache;

    /// Read by `FanCurveLoop` each tick.
    public IReadOnlyDictionary<string, FanHeaderConfig> GetActiveConfig() => _active;

    public CapabilitySet Probe()
    {
        var headers = Headers(forceRefresh: true);
        var sample = host.GetSampleAsync(CancellationToken.None).GetAwaiter().GetResult();
        var details = new JsonObject
        {
            ["headers"] = new JsonArray(headers
                .Select(h => (JsonNode)new JsonObject
                {
                    ["id"] = h.Id,
                    ["label"] = h.Label,
                    ["min"] = h.Min,
                    ["max"] = h.Max,
                })
                .ToArray()),
            ["sources"] = new JsonArray(FanCurveEvaluator.AvailableSources(sample)
                .Select(s => (JsonNode)JsonValue.Create(s))
                .ToArray()),
        };
        return new CapabilitySet
        {
            Domain = Domain,
            Supported = headers.Count > 0,
            Reason = headers.Count > 0 ? null : "No writable fan headers found on this motherboard.",
            Details = details,
        };
    }

    public ValidationResult Validate(ProfilePart part)
    {
        if (part.Fan?.Headers is null)
            return ValidationResult.Success(); // domain untouched — not this provider's concern.

        var known = Headers().ToDictionary(h => h.Id);
        foreach (var (id, config) in part.Fan.Headers)
        {
            if (!known.ContainsKey(id))
                return ValidationResult.Failure($"Unknown fan header '{id}'.");
            if (!IsKnownSourceKind(config.Source))
                return ValidationResult.Failure($"Fan header '{id}' has an unknown temperature source '{config.Source}'.");
            if (!double.IsFinite(config.HysteresisC) || config.HysteresisC < 0)
                return ValidationResult.Failure($"Fan header '{id}' hysteresis must be a non-negative number.");
            if (config.Curve.Count == 0)
                return ValidationResult.Failure($"Fan header '{id}' has an empty curve.");
            for (var i = 0; i < config.Curve.Count; i++)
            {
                if (config.Curve[i].Count != 2 || !config.Curve[i].All(double.IsFinite))
                    return ValidationResult.Failure($"Fan header '{id}' curve point {i} must be [tempC, dutyPct].");
                if (i > 0 && config.Curve[i][0] < config.Curve[i - 1][0])
                    return ValidationResult.Failure($"Fan header '{id}' curve must be sorted ascending by temperature.");
            }
        }
        // Duty values themselves are clamped to probed hardware limits at
        // write time (ApplyDuty), not here — "hard limits always clamped,
        // UI values never trusted" is enforced at the point of the actual
        // SetSoftware() call, not just on this one-time check, since Apply
        // has no guarantee it only ever sees already-validated data.
        return ValidationResult.Success();
    }

    public Snapshot Capture()
    {
        var state = new JsonObject
        {
            ["headers"] = new JsonObject(_active.Select(kv =>
                new KeyValuePair<string, JsonNode?>(kv.Key, SerializeHeader(kv.Value)))),
        };
        return new Snapshot { Domain = Domain, State = state };
    }

    public void Apply(ProfilePart part)
    {
        if (part.Fan?.Headers is null)
            return;

        // Headers no longer present in the new profile are released back to
        // firmware rather than left spinning at their last commanded duty.
        foreach (var id in _active.Keys.Where(id => !part.Fan.Headers.ContainsKey(id)).ToList())
            ReleaseHeader(id);

        Activate(part.Fan.Headers);
    }

    public bool Verify(ProfilePart part)
    {
        if (part.Fan?.Headers is null || dryRun)
            return true;

        Dictionary<string, double> commanded;
        lock (_writeLock)
            commanded = new Dictionary<string, double>(_commanded);

        return host.WithHardwareLockAsync(computer =>
        {
            var controlSensors = FindControlSensors(computer).ToDictionary(s => s.Identifier.ToString());
            foreach (var id in part.Fan.Headers.Keys)
            {
                if (!controlSensors.TryGetValue(id, out var sensor) || sensor.Control is null)
                    return false;
                if (sensor.Control.ControlMode != ControlMode.Software)
                    return false;
                // ControlMode is only LHM's own bookkeeping — re-read the
                // PWM register itself, so firmware that silently overrides
                // the write is reported as not controllable.
                sensor.Hardware.Update();
                if (!commanded.TryGetValue(id, out var duty)
                    || sensor.Value is not { } actual
                    || Math.Abs(actual - duty) > VerifyTolerancePct)
                    return false;
            }
            return true;
        }, CancellationToken.None).GetAwaiter().GetResult();
    }

    public void Restore(Snapshot snapshot)
    {
        if (snapshot.Domain != Domain)
            return;

        var restored = new Dictionary<string, FanHeaderConfig>();
        if (snapshot.State?["headers"] is JsonObject headers)
        {
            foreach (var (id, node) in headers)
            {
                if (DeserializeHeader(node) is { } config)
                    restored[id] = config;
            }
        }

        foreach (var id in _active.Keys.Where(id => !restored.ContainsKey(id)).ToList())
            ReleaseHeader(id);

        Activate(restored);
    }

    public void ReleaseToFirmware()
    {
        List<string> ids;
        lock (_writeLock)
            ids = _active.Keys.Union(_identifying.Keys).ToList();
        foreach (var id in ids)
            ReleaseHeader(id);
    }

    public bool HasHeader(string headerId) => Headers().Any(h => h.Id == headerId);

    /// `identify_fan`: spins one header to full speed for `duration` so the
    /// user can see/hear which fan it is (LHM can't tell which header is the
    /// CPU cooler), then hands it back — to its curve if a profile controls
    /// it, otherwise to firmware. Works on headers no profile controls yet,
    /// which is the point: labelling comes before writing a curve.
    public async Task IdentifyAsync(string headerId, TimeSpan duration, CancellationToken ct)
    {
        var header = Headers().FirstOrDefault(h => h.Id == headerId);
        if (header.Id is null)
            return; // Unknown header — the pipe checks HasHeader first.

        long token;
        lock (_writeLock)
        {
            token = ++_identifyToken;
            _identifying[headerId] = token;
            WriteSoftware(headerId, header.Max);
        }

        try
        {
            await Task.Delay(duration, ct);
        }
        finally
        {
            lock (_writeLock)
            {
                if (_identifying.TryGetValue(headerId, out var owner) && owner == token)
                {
                    _identifying.Remove(headerId);
                    if (_commanded.TryGetValue(headerId, out var duty))
                        WriteSoftware(headerId, duty);
                    else
                        WriteDefault(headerId);
                }
            }
        }
    }

    /// Clamps `duty` to the header's probed hardware min/max and writes it —
    /// the single point every duty write (first-apply, restore, and every
    /// `FanCurveLoop` tick) funnels through, so the "never trust an unclamped
    /// value" guarantee holds regardless of caller. Headers that aren't
    /// active (never applied, or already released) are never written.
    public void ApplyDuty(string headerId, double duty)
    {
        var known = Headers().ToDictionary(h => h.Id);
        if (!known.TryGetValue(headerId, out var header))
            return;
        var clamped = Math.Clamp(duty, header.Min, header.Max);

        lock (_writeLock)
        {
            if (!_active.ContainsKey(headerId))
                return;
            _commanded[headerId] = clamped;
            // An identify in progress keeps full speed; it restores this
            // duty when it ends.
            if (!_identifying.ContainsKey(headerId))
                WriteSoftware(headerId, clamped);
        }
    }

    /// Makes `configs` active and claims each header right away with the
    /// duty its curve gives at the current source temperature, rather than
    /// leaving it on firmware control for up to ~1s until FanCurveLoop's
    /// next tick — Verify (called right after Apply, same transaction)
    /// needs something real to check.
    private void Activate(IReadOnlyDictionary<string, FanHeaderConfig> configs)
    {
        if (configs.Count == 0)
            return;
        var known = Headers().ToDictionary(h => h.Id);
        var sample = host.GetSampleAsync(CancellationToken.None).GetAwaiter().GetResult();
        foreach (var (id, config) in configs)
        {
            if (!known.ContainsKey(id))
                continue; // Already rejected by Validate in the same transaction; defensive skip here.
            _active[id] = config;
            var temp = FanCurveEvaluator.ResolveSource(sample, config.Source);
            ApplyDuty(id, FanCurveEvaluator.EvaluateHeader(config, temp, previous: null).Duty);
        }
    }

    private void ReleaseHeader(string id)
    {
        lock (_writeLock)
        {
            _active.TryRemove(id, out _);
            _commanded.Remove(id);
            _identifying.Remove(id);
            WriteDefault(id);
        }
    }

    // The only two places that write hardware. Callers hold _writeLock.
    private void WriteSoftware(string id, double duty)
    {
        if (dryRun)
            return;
        host.WithHardwareLockAsync(computer =>
        {
            FindControlSensor(computer, id)?.Control?.SetSoftware((float)duty);
            return 0;
        }, CancellationToken.None).GetAwaiter().GetResult();
    }

    private void WriteDefault(string id)
    {
        if (dryRun)
            return;
        host.WithHardwareLockAsync(computer =>
        {
            FindControlSensor(computer, id)?.Control?.SetDefault();
            return 0;
        }, CancellationToken.None).GetAwaiter().GetResult();
    }

    private List<FanHeaderInfo> Headers(bool forceRefresh = false)
    {
        if (!forceRefresh && _headerCache is not null)
            return _headerCache;

        var headers = host.WithHardwareLockAsync(computer => FindControlSensors(computer)
            .Select(s => new FanHeaderInfo(
                s.Identifier.ToString(),
                s.Name,
                s.Control!.MinSoftwareValue,
                s.Control!.MaxSoftwareValue))
            .ToList(), CancellationToken.None).GetAwaiter().GetResult();

        _headerCache = headers;
        return headers;
    }

    private static bool IsKnownSourceKind(string source) =>
        source is FanCurveEvaluator.CpuSource or FanCurveEvaluator.GpuSource
        || (source.StartsWith(FanCurveEvaluator.MotherboardSourcePrefix, StringComparison.Ordinal)
            && source.Length > FanCurveEvaluator.MotherboardSourcePrefix.Length);

    /// `/lpc/<chip>/0/control/<n>` sensors — the writable siblings of the
    /// read-only `/lpc/<chip>/0/fan/<n>` RPM sensors `SensorReader` already
    /// extracts (same Super I/O sub-hardware, different `SensorType`).
    private static IEnumerable<ISensor> FindControlSensors(IComputer computer) =>
        computer.Hardware
            .Where(hw => hw.HardwareType == HardwareType.Motherboard)
            .SelectMany(hw => hw.SubHardware)
            .SelectMany(sub => sub.Sensors)
            .Where(s => s.SensorType == SensorType.Control
                && s.Identifier.ToString().StartsWith("/lpc/", StringComparison.Ordinal)
                && s.Control is not null);

    private static ISensor? FindControlSensor(IComputer computer, string id) =>
        FindControlSensors(computer).FirstOrDefault(s => s.Identifier.ToString() == id);

    private static JsonNode SerializeHeader(FanHeaderConfig config) => new JsonObject
    {
        ["label"] = config.Label,
        ["source"] = config.Source,
        ["curve"] = new JsonArray(config.Curve
            .Select(p => (JsonNode)new JsonArray(p.Select(v => (JsonNode)JsonValue.Create(v)).ToArray()))
            .ToArray()),
        ["hysteresis_c"] = config.HysteresisC,
    };

    private static FanHeaderConfig? DeserializeHeader(JsonNode? node)
    {
        if (node is not JsonObject obj || obj["curve"] is not JsonArray curveArray || obj["source"] is not { } sourceNode)
            return null;
        var curve = curveArray
            .Select(point => point as JsonArray)
            .Where(point => point is not null)
            .Select(point => point!.Select(v => v!.GetValue<double>()).ToList())
            .ToList();
        return new FanHeaderConfig
        {
            Label = obj["label"]?.GetValue<string>(),
            Source = sourceNode.GetValue<string>(),
            Curve = curve,
            HysteresisC = obj["hysteresis_c"]?.GetValue<double>() ?? 3.0,
        };
    }
}
