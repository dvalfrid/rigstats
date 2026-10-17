using System.Globalization;
using System.Collections.Concurrent;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Providers;

/// One RPM sensor that sped up while a header was being identified.
public sealed record FanResponder(string Label, double BeforeRpm, double PeakRpm);

/// Which RPM sensors a control header actually drives, measured by `identify_fan`.
public sealed record FanIdentifyResult(string Header, IReadOnlyList<FanResponder> Responders);

/// `IControlProvider` for fan headers (#188, Control Center phase 1).
/// Unlike `PowerPlanProvider`'s one-shot apply, a fan "apply" just replaces
/// which curves are active — the actual duty values are recomputed every
/// tick by `FanCurveLoop` against live temperatures, using
/// `FanCurveEvaluator`'s pure logic (`FanCurveLoop` calls back into
/// `ApplyDuty` here to do the actual write). This class owns the active
/// configuration `FanCurveLoop` reads, and is the only thing that touches
/// `IControl` directly (through `IHardwareHost`, never a raw `Computer`).
/// With `dryRun`, every write is skipped and `Verify` trusts the request.
/// `channelMapPath` persists which fans each header drives (null in tests).
/// `conflict` names another program that sets the fans (Armoury Crate's fan
/// service, `OtherSoftware.FansTaken`), or null: while it runs the domain is
/// reported as taken and an apply skips the fans rather than fight it (#193).
public sealed class FanProvider(IHardwareHost host, bool dryRun, string? channelMapPath = null, Func<string?>? conflict = null) : IControlProvider
{
    // Which RPM sensors each header drives, as last measured by identify
    // (#207) — persisted so the Motherboard panel can open the right curve
    // after a restart, and reported by Probe() so it is in the diagnostics
    // ZIP's capabilities too.
    private readonly ConcurrentDictionary<string, string[]> _drives = LoadDrives(channelMapPath);

    /// Readback may differ from the commanded duty by the 0–255 PWM register
    /// rounding (~0.4 %) plus firmware step granularity; anything further
    /// off means firmware overrode the write.
    internal const double VerifyTolerancePct = 5.0;

    public string Domain => "fan";

    private readonly ConcurrentDictionary<string, FanHeaderConfig> _active = new();

    /// How long `identify_fan` holds a header at full speed.
    public static readonly TimeSpan IdentifyDuration = TimeSpan.FromSeconds(5);

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
        if (headers.Count > 0 && conflict?.Invoke() is { } other)
        {
            // Flagged, so the Fans page explains it instead of disappearing.
            return new CapabilitySet
            {
                Domain = Domain,
                Supported = false,
                Reason = $"{other} controls the fans. Uninstall it to control the fans here.",
                Problem = ControlProblems.FansTakenBy(other),
                Details = new JsonObject { ["conflict"] = true },
            };
        }
        var sample = host.GetSampleAsync(CancellationToken.None).GetAwaiter().GetResult();
        var details = new JsonObject
        {
            ["headers"] = new JsonArray(headers
                .Select(h =>
                {
                    var header = new JsonObject
                    {
                        ["id"] = h.Id,
                        ["label"] = h.Label,
                        ["min"] = h.Min,
                        ["max"] = h.Max,
                    };
                    if (_drives.TryGetValue(h.Id, out var drives))
                        header["drives"] = new JsonArray(drives.Select(d => (JsonNode)JsonValue.Create(d)).ToArray());
                    return (JsonNode)header;
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
        if (conflict?.Invoke() is { } other)
        {
            // The rest of the profile applies; the fans stay with the other
            // program (and with the BIOS, as far as RIGStats is concerned).
            SidecarLog.Log($"[rigstats-control] Fan curves skipped: {other} controls the fans");
            throw new DomainUnavailableException($"{other} controls the fans, so the fan curves were skipped.");
        }

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

        string? failure = null;
        foreach (var id in part.Fan.Headers.Keys)
        {
            // The PWM register itself is re-read (`ReadFan`), so firmware
            // that silently overrides the write is reported as not controllable.
            var readback = host.ReadFan(id);
            if (!readback.Found)
                failure = $"{id} not found";
            else if (!readback.Software)
                failure = $"{id} is not under software control";
            else if (!commanded.TryGetValue(id, out var duty))
                failure = $"{id} was never commanded";
            else if (readback.Value is not { } actual || Math.Abs(actual - duty) > VerifyTolerancePct)
                failure = $"{id} commanded={duty:F0}% readback={readback.Value?.ToString("F0", CultureInfo.InvariantCulture) ?? "none"}% (firmware override?)";
            if (failure is not null)
                break;
        }

        if (failure is not null)
            SidecarLog.Log($"[rigstats-control] Fan verify failed: {failure}");
        return failure is null;
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

    public string? ReleaseToFirmware()
    {
        List<string> ids;
        lock (_writeLock)
            ids = _active.Keys.Union(_identifying.Keys).ToList();
        foreach (var id in ids)
            ReleaseHeader(id);
        if (ids.Count == 0)
            return null;
        var names = ids.Order(StringComparer.Ordinal).Select(HeaderName);
        return $"{ids.Count} header{(ids.Count == 1 ? "" : "s")} to the BIOS ({string.Join(", ", names)})";
    }

    /// A header's name for the log: its LHM label ("Fan #2"), else its id.
    private string HeaderName(string id) =>
        _headerCache?.FirstOrDefault(h => h.Id == id).Label is { Length: > 0 } label ? label : id;

    public bool HasHeader(string headerId) => Headers().Any(h => h.Id == headerId);

    /// `identify_fan`: spins one header to full speed for `duration` so the
    /// user can see/hear which fan it is (LHM can't tell which header is the
    /// CPU cooler), then hands it back — to its curve if a profile controls
    /// it, otherwise to firmware. Works on headers no profile controls yet,
    /// which is the point: labelling comes before writing a curve.
    ///
    /// While spinning, it also measures which RPM sensors respond (#207):
    /// LHM's control channels and tach sensors don't always map 1:1 (one
    /// channel can drive several fans). Returns null when the identify was
    /// superseded or released before it finished.
    public async Task<FanIdentifyResult?> IdentifyAsync(
        string headerId, TimeSpan duration, CancellationToken ct, TimeSpan? sampleEvery = null)
    {
        var header = Headers().FirstOrDefault(h => h.Id == headerId);
        if (header.Id is null)
            return null; // Unknown header — the pipe checks HasHeader first.
        var interval = sampleEvery ?? TimeSpan.FromSeconds(1);

        var before = await FanRpm(ct);
        long token;
        lock (_writeLock)
        {
            token = ++_identifyToken;
            _identifying[headerId] = token;
            WriteSoftware(headerId, header.Max);
        }

        var peak = new Dictionary<string, double>(before);
        bool stillOwner;
        try
        {
            var end = DateTime.UtcNow + duration;
            for (var left = duration; left > TimeSpan.Zero; left = end - DateTime.UtcNow)
            {
                await Task.Delay(left < interval ? left : interval, ct);
                foreach (var (label, rpm) in await FanRpm(ct))
                    peak[label] = Math.Max(peak.GetValueOrDefault(label), rpm);
            }
        }
        finally
        {
            lock (_writeLock)
            {
                stillOwner = _identifying.TryGetValue(headerId, out var owner) && owner == token;
                if (stillOwner)
                {
                    _identifying.Remove(headerId);
                    if (_commanded.TryGetValue(headerId, out var duty))
                        WriteSoftware(headerId, duty);
                    else
                        WriteDefault(headerId);
                }
            }
        }
        if (!stillOwner)
            return null;
        var result = new FanIdentifyResult(headerId, Responders(before, peak));
        // Dry-run never spun anything, so its "measurement" means nothing.
        if (!dryRun)
        {
            _drives[headerId] = result.Responders.Select(r => r.Label).ToArray();
            SaveDrives();
        }
        return result;
    }

    private static ConcurrentDictionary<string, string[]> LoadDrives(string? path)
    {
        try
        {
            if (path is not null && File.Exists(path)
                && JsonSerializer.Deserialize<Dictionary<string, string[]>>(File.ReadAllText(path)) is { } map)
                return new ConcurrentDictionary<string, string[]>(map);
        }
        catch (Exception e) when (e is IOException or JsonException or UnauthorizedAccessException)
        {
            SidecarLog.Log($"[rigstats-control] Ignoring unreadable fan channel map: {e.Message}");
        }
        return new ConcurrentDictionary<string, string[]>();
    }

    private static readonly JsonSerializerOptions IndentedJson = new() { WriteIndented = true };

    private void SaveDrives()
    {
        if (channelMapPath is null)
            return;
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(channelMapPath)!);
            File.WriteAllText(channelMapPath, JsonSerializer.Serialize(
                new SortedDictionary<string, string[]>(_drives), IndentedJson));
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            SidecarLog.Log($"[rigstats-control] Could not save the fan channel map: {e.Message}");
        }
    }

    /// A tach sensor "responded" when its peak rose clearly above where it
    /// started — enough to rule out normal BIOS-curve drift on other fans.
    internal const double MinRiseRpm = 150;
    internal const double MinRiseFraction = 0.15;

    internal static List<FanResponder> Responders(
        IReadOnlyDictionary<string, double> before, IReadOnlyDictionary<string, double> peak) =>
        peak
            .Select(kv => new FanResponder(kv.Key, before.GetValueOrDefault(kv.Key), kv.Value))
            .Where(r => r.PeakRpm - r.BeforeRpm >= Math.Max(MinRiseRpm, r.BeforeRpm * MinRiseFraction))
            .OrderByDescending(r => r.PeakRpm - r.BeforeRpm)
            .ToList();

    private async Task<Dictionary<string, double>> FanRpm(CancellationToken ct) =>
        (await host.GetSampleAsync(ct)).MbFans
            .GroupBy(f => f.Label)
            .ToDictionary(g => g.Key, g => (double)g.First().Rpm);

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
        host.SetFan(id, duty);
    }

    private void WriteDefault(string id)
    {
        if (dryRun)
            return;
        host.SetFanDefault(id);
    }

    private List<FanHeaderInfo> Headers(bool forceRefresh = false)
    {
        if (!forceRefresh && _headerCache is not null)
            return _headerCache;
        var headers = host.FanHeaders().ToList();
        _headerCache = headers;
        return headers;
    }

    private static bool IsKnownSourceKind(string source) =>
        source is FanCurveEvaluator.CpuSource or FanCurveEvaluator.GpuSource
        || (source.StartsWith(FanCurveEvaluator.MotherboardSourcePrefix, StringComparison.Ordinal)
            && source.Length > FanCurveEvaluator.MotherboardSourcePrefix.Length);

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
