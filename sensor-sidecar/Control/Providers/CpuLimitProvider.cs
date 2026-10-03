using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Providers;

/// `IControlProvider` for CPU package limits (#189, Control Center phase 2).
/// AMD only for now: PPT/TDC/EDC through the SMU (`IRyzenSmu`). Limits can
/// only be lowered — the ceiling is what the BIOS set at boot (the
/// "baseline"), the floor is <see cref="Floor"/>. A null value in a profile
/// means "the BIOS value". SMU limits reset at every reboot, so the baseline
/// is read once per boot, before any write, and persisted with the boot time
/// so a service restart within the same boot doesn't mistake an applied
/// limit for the BIOS one.
public sealed class CpuLimitProvider(
    IRyzenSmu? smu,
    string unsupportedReason,
    BootCrashGuard crashGuard,
    bool dryRun,
    string? baselinePath,
    DateTimeOffset bootTime) : IControlProvider
{
    /// Lowest limits offered — AMD's own 45 W Eco Mode sits at 61 W / 45 A / 65 A.
    public static readonly AmdLimits Floor = new(45, 30, 45);

    /// PM table readback is a float of what the SMU accepted in mW/mA.
    internal const double VerifyTolerance = 1.0;

    private static readonly TimeSpan SameBootTolerance = TimeSpan.FromMinutes(2);

    private readonly object _lock = new();
    private AmdLimits? _baseline;
    private AmdLimits? _lastTarget;
    private bool _touched;

    public string Domain => "cpu_limit";

    public CapabilitySet Probe()
    {
        if (smu is null)
            return new CapabilitySet { Domain = Domain, Supported = false, Reason = unsupportedReason };

        AmdLimits baseline, current;
        try
        {
            baseline = Baseline();
            current = smu.ReadLimits();
        }
        catch (Exception e)
        {
            // One domain's SMU trouble must not break the whole capability
            // list (and with it the Control Center).
            SidecarLog.Log($"[rigstats-control] CPU limits: SMU read failed: {e.Message}");
            return new CapabilitySet
            {
                Domain = Domain,
                Supported = false,
                Reason = "CPU power limits are unavailable: the SMU did not answer.",
            };
        }
        var min = MinFor(baseline);
        return new CapabilitySet
        {
            Domain = Domain,
            Supported = true,
            Details = new JsonObject
            {
                ["vendor"] = "amd",
                ["generation"] = smu.Generation,
                ["pm_table"] = $"0x{smu.PmTableVersion:X}",
                ["stock"] = LimitsJson(baseline),
                ["min"] = LimitsJson(min),
                ["current"] = LimitsJson(current),
            },
        };
    }

    public ValidationResult Validate(ProfilePart part)
    {
        if (part.CpuLimit?.Amd is not { } amd)
            return ValidationResult.Success();
        foreach (var (name, value) in new[] { ("PPT", amd.PptW), ("TDC", amd.TdcA), ("EDC", amd.EdcA) })
        {
            if (value is { } v && (!double.IsFinite(v) || v <= 0))
                return ValidationResult.Failure($"CPU {name} limit must be a positive number.");
        }
        // Unsupported (e.g. a BIOS update changed the PM table version):
        // skipped in Apply rather than failing the whole profile — fans and
        // the power plan should still switch.
        if (smu is null)
            return ValidationResult.Success();
        var target = Resolve(amd, Baseline());
        return ValidationResult.Success(LimitsJson(target));
    }

    public Snapshot Capture() => new()
    {
        Domain = Domain,
        State = smu is null ? null : LimitsJson(smu.ReadLimits()),
    };

    public void Apply(ProfilePart part)
    {
        if (part.CpuLimit is null)
            return;
        if (smu is null)
        {
            if (HasValues(part.CpuLimit))
                SidecarLog.Log($"[rigstats-control] CPU limits skipped: {unsupportedReason}");
            return;
        }
        var target = Resolve(part.CpuLimit.Amd, Baseline());
        lock (_lock)
            _lastTarget = target;
        Write(target);
    }

    public bool Verify(ProfilePart part)
    {
        if (part.CpuLimit is null || smu is null || dryRun)
            return true;
        AmdLimits? target;
        lock (_lock)
            target = _lastTarget;
        if (target is not { } expected)
            return true;
        var actual = smu.ReadLimits();
        if (Matches(actual, expected))
            return true;
        SidecarLog.Log($"[rigstats-control] CPU limit verify failed: set {Describe(expected)}, readback {Describe(actual)}.");
        return false;
    }

    public void Restore(Snapshot snapshot)
    {
        if (snapshot.Domain != Domain || snapshot.State is null || smu is null)
            return;
        Write(ParseLimits(snapshot.State));
    }

    /// Back to the BIOS values — only if this service changed anything, so
    /// a machine where nobody uses CPU limits never sees an SMU write.
    public void ReleaseToFirmware()
    {
        if (smu is null)
            return;
        bool touched;
        lock (_lock)
            touched = _touched;
        if (touched)
            Write(Baseline());
    }

    private void Write(AmdLimits target)
    {
        if (smu is null)
            return;
        if (Matches(smu.ReadLimits(), target))
            return; // already in force — no write, no crash-guard window.
        if (dryRun)
        {
            SidecarLog.Log($"[rigstats-control] dry-run: CPU limits -> {Describe(target)}");
            return;
        }
        // Back at the BIOS values is not a risky state worth guarding.
        if (!Matches(target, Baseline()))
            crashGuard.Arm();
        lock (_lock)
            _touched = true;
        smu.SetLimits(target);
    }

    /// The BIOS limits for this boot: from the baseline file when it was
    /// written during this same boot, otherwise read now (before any write).
    private AmdLimits Baseline()
    {
        lock (_lock)
        {
            if (_baseline is { } cached)
                return cached;
            var baseline = LoadBaseline() ?? smu!.ReadLimits();
            SaveBaseline(baseline);
            _baseline = baseline;
            return baseline;
        }
    }

    private AmdLimits? LoadBaseline()
    {
        if (baselinePath is null || !File.Exists(baselinePath))
            return null;
        try
        {
            var node = JsonNode.Parse(File.ReadAllText(baselinePath));
            var boot = DateTimeOffset.Parse(node!["boot_utc"]!.GetValue<string>());
            if ((boot - bootTime).Duration() > SameBootTolerance)
                return null; // an earlier boot — the SMU has reset since.
            return ParseLimits(node["limits"]!);
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] CPU limit baseline unreadable, re-reading: {e.Message}");
            return null;
        }
    }

    private void SaveBaseline(AmdLimits baseline)
    {
        if (baselinePath is null)
            return;
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(baselinePath)!);
            var node = new JsonObject
            {
                ["boot_utc"] = bootTime.ToString("O"),
                ["limits"] = LimitsJson(baseline),
            };
            File.WriteAllText(baselinePath, node.ToJsonString());
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] CPU limit baseline not saved: {e.Message}");
        }
    }

    // ── Pure helpers (unit-tested) ─────────────────────────────────────

    /// The floor, but never above the BIOS value itself.
    public static AmdLimits MinFor(AmdLimits baseline) => new(
        Math.Min(Floor.PptW, baseline.PptW),
        Math.Min(Floor.TdcA, baseline.TdcA),
        Math.Min(Floor.EdcA, baseline.EdcA));

    /// What to write for a requested part: null → BIOS value, anything else
    /// clamped to [floor, BIOS value] and rounded to whole W/A.
    public static AmdLimits Resolve(AmdCpuLimit? requested, AmdLimits baseline)
    {
        var min = MinFor(baseline);
        return new AmdLimits(
            Clamp(requested?.PptW, min.PptW, baseline.PptW),
            Clamp(requested?.TdcA, min.TdcA, baseline.TdcA),
            Clamp(requested?.EdcA, min.EdcA, baseline.EdcA));
    }

    private static double Clamp(double? value, double min, double max) =>
        value is { } v ? Math.Clamp(Math.Round(v), min, max) : max;

    public static bool Matches(AmdLimits a, AmdLimits b) =>
        Math.Abs(a.PptW - b.PptW) <= VerifyTolerance
        && Math.Abs(a.TdcA - b.TdcA) <= VerifyTolerance
        && Math.Abs(a.EdcA - b.EdcA) <= VerifyTolerance;

    private static bool HasValues(CpuLimitPart part) =>
        part.Amd is { } a && (a.PptW is not null || a.TdcA is not null || a.EdcA is not null);

    internal static JsonObject LimitsJson(AmdLimits l) => new()
    {
        ["ppt_w"] = l.PptW,
        ["tdc_a"] = l.TdcA,
        ["edc_a"] = l.EdcA,
    };

    internal static AmdLimits ParseLimits(JsonNode node) => new(
        node["ppt_w"]!.GetValue<double>(),
        node["tdc_a"]!.GetValue<double>(),
        node["edc_a"]!.GetValue<double>());

    private static string Describe(AmdLimits l) => $"PPT {l.PptW:F0} W, TDC {l.TdcA:F0} A, EDC {l.EdcA:F0} A";

    /// When this boot started — the baseline file's key.
    public static DateTimeOffset CurrentBootTime() =>
        DateTimeOffset.UtcNow - TimeSpan.FromMilliseconds(Environment.TickCount64);
}
