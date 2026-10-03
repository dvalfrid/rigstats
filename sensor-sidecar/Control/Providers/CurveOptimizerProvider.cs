using System.Runtime.InteropServices;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Providers;

/// `IControlProvider` for AMD Curve Optimizer (#191, Control Center phase
/// 4) — per-core voltage-curve offsets through the SMU (`ICurveOptimizerSmu`).
/// The riskiest domain: a too-negative offset crashes the machine, so values
/// are bounded to <see cref="Min"/>…<see cref="Max"/> (undervolt only),
/// every write that leaves the BIOS values arms the boot-crash guard, and
/// the UI always goes through `preview`. SMU offsets reset at every reboot;
/// the BIOS values are read once per boot before any write and persisted
/// with the boot time, like `CpuLimitProvider`'s baseline.
///
/// Cores are the ones the SMU answers for, in CCD/core order. Per-core
/// values are only offered when that count matches the physical cores
/// Windows reports — then "core 3" means the same core to the user and the
/// SMU. Otherwise (fused-off cores leave gaps) only all-core is offered.
public sealed class CurveOptimizerProvider(
    ICurveOptimizerSmu? smu,
    int windowsPhysicalCores,
    string unsupportedReason,
    BootCrashGuard crashGuard,
    bool dryRun,
    string? baselinePath,
    DateTimeOffset bootTime) : IControlProvider
{
    /// Conservative bounds: the classic PBO2 range, negative only.
    public const int Min = -30;
    public const int Max = 0;

    private static readonly TimeSpan SameBootTolerance = TimeSpan.FromMinutes(2);

    private readonly object _lock = new();
    private int[]? _baseline;
    private int[]? _lastTargets;
    private bool _touched;

    public string Domain => "curve_opt";

    public CapabilitySet Probe()
    {
        if (smu is null)
            return Unsupported(unsupportedReason);
        try
        {
            var cores = smu.Cores();
            if (cores.Count == 0)
                return Unsupported("Curve Optimizer is unavailable: the SMU reported no cores.");
            var baseline = Baseline();
            return new CapabilitySet
            {
                Domain = Domain,
                Supported = true,
                Details = new JsonObject
                {
                    ["min"] = Min,
                    ["max"] = Max,
                    ["cores"] = cores.Count,
                    ["per_core"] = PerCoreSupported(cores.Count, windowsPhysicalCores),
                    ["bios"] = new JsonArray(baseline.Select(v => (JsonNode)v).ToArray()),
                    ["current"] = new JsonArray(Read().Select(v => (JsonNode)v).ToArray()),
                },
            };
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] Curve Optimizer: SMU read failed: {e.Message}");
            return Unsupported("Curve Optimizer is unavailable: the SMU did not answer.");
        }
    }

    private CapabilitySet Unsupported(string reason) =>
        new() { Domain = Domain, Supported = false, Reason = reason };

    public ValidationResult Validate(ProfilePart part)
    {
        if (part.CurveOpt?.PerCore is { } perCore)
        {
            foreach (var key in perCore.Keys)
            {
                if (!int.TryParse(key, out var index) || index < 0)
                    return ValidationResult.Failure($"Curve Optimizer core '{key}' is not a core number.");
            }
        }
        // Unsupported here (other CPU, BIOS update): skipped in Apply rather
        // than failing the whole profile, like CPU limits.
        return ValidationResult.Success();
    }

    public Snapshot Capture() => new()
    {
        Domain = Domain,
        State = smu is null ? null : new JsonArray(Read().Select(v => (JsonNode)v).ToArray()),
    };

    public void Apply(ProfilePart part)
    {
        if (part.CurveOpt is null)
            return;
        if (smu is null)
        {
            if (part.CurveOpt.AllCore is not null || part.CurveOpt.PerCore is { Count: > 0 })
                SidecarLog.Log($"[rigstats-control] Curve Optimizer skipped: {unsupportedReason}");
            return;
        }
        var cores = smu.Cores();
        var targets = Resolve(part.CurveOpt, Baseline(), PerCoreSupported(cores.Count, windowsPhysicalCores));
        lock (_lock)
            _lastTargets = targets;
        Write(targets);
    }

    public bool Verify(ProfilePart part)
    {
        if (part.CurveOpt is null || smu is null || dryRun)
            return true;
        int[]? targets;
        lock (_lock)
            targets = _lastTargets;
        if (targets is null)
            return true;
        var actual = Read();
        if (actual.SequenceEqual(targets))
            return true;
        SidecarLog.Log($"[rigstats-control] Curve Optimizer verify failed: set [{string.Join(", ", targets)}], " +
            $"readback [{string.Join(", ", actual)}].");
        return false;
    }

    public void Restore(Snapshot snapshot)
    {
        if (snapshot.Domain != Domain || snapshot.State is not JsonArray state || smu is null)
            return;
        Write(state.Select(v => v!.GetValue<int>()).ToArray());
    }

    /// Back to the BIOS values — only if this service changed anything.
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

    private int[] Read()
    {
        var cores = smu!.Cores();
        return cores.Select(smu.GetOffset).ToArray();
    }

    private void Write(int[] targets)
    {
        var cores = smu!.Cores();
        var current = Read();
        if (current.SequenceEqual(targets))
            return; // already in force — no write, no crash-guard window.
        if (dryRun)
        {
            SidecarLog.Log($"[rigstats-control] dry-run: Curve Optimizer -> [{string.Join(", ", targets)}]");
            return;
        }
        // Back at the BIOS values is not a risky state worth guarding.
        if (!targets.SequenceEqual(Baseline()))
            crashGuard.Arm();
        lock (_lock)
            _touched = true;

        if (targets.Distinct().Count() == 1)
        {
            smu.SetAllOffsets(targets[0]);
            return;
        }
        for (var i = 0; i < cores.Count; i++)
        {
            if (current[i] != targets[i])
                smu.SetOffset(cores[i], targets[i]);
        }
    }

    /// The BIOS offsets for this boot: from the baseline file when written
    /// during this same boot, otherwise read now (before any write).
    private int[] Baseline()
    {
        lock (_lock)
        {
            if (_baseline is { } cached)
                return cached;
            var baseline = LoadBaseline() ?? Read();
            SaveBaseline(baseline);
            _baseline = baseline;
            return baseline;
        }
    }

    private int[]? LoadBaseline()
    {
        if (baselinePath is null || !File.Exists(baselinePath))
            return null;
        try
        {
            var node = JsonNode.Parse(File.ReadAllText(baselinePath));
            var boot = DateTimeOffset.Parse(node!["boot_utc"]!.GetValue<string>());
            if ((boot - bootTime).Duration() > SameBootTolerance)
                return null; // an earlier boot — the SMU has reset since.
            var offsets = node["offsets"]!.AsArray().Select(v => v!.GetValue<int>()).ToArray();
            return offsets.Length == smu!.Cores().Count ? offsets : null;
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] Curve Optimizer baseline unreadable, re-reading: {e.Message}");
            return null;
        }
    }

    private void SaveBaseline(int[] baseline)
    {
        if (baselinePath is null)
            return;
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(baselinePath)!);
            var node = new JsonObject
            {
                ["boot_utc"] = bootTime.ToString("O"),
                ["offsets"] = new JsonArray(baseline.Select(v => (JsonNode)v).ToArray()),
            };
            File.WriteAllText(baselinePath, node.ToJsonString());
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] Curve Optimizer baseline not saved: {e.Message}");
        }
    }

    // ── Pure helpers (unit-tested) ─────────────────────────────────────

    public static bool PerCoreSupported(int smuCores, int windowsPhysicalCores) =>
        smuCores > 0 && smuCores == windowsPhysicalCores;

    /// Per core: its own value (when per-core is supported), else the
    /// all-core value, else the BIOS value. Requested values are clamped to
    /// <see cref="Min"/>…<see cref="Max"/>; BIOS values are kept as they are.
    public static int[] Resolve(CurveOptPart part, int[] baseline, bool perCore)
    {
        var targets = new int[baseline.Length];
        for (var i = 0; i < baseline.Length; i++)
        {
            int? requested = null;
            if (perCore && part.PerCore is { } map && map.TryGetValue(i.ToString(), out var own))
                requested = own;
            requested ??= part.AllCore;
            targets[i] = requested is { } v ? Math.Clamp(v, Min, Max) : baseline[i];
        }
        return targets;
    }

    // ── Physical core count from Windows ───────────────────────────────

    /// Physical cores as Windows sees them (RelationProcessorCore entries);
    /// 0 when it can't be read, which turns per-core off.
    public static int WindowsPhysicalCores()
    {
        uint length = 0;
        GetLogicalProcessorInformation(IntPtr.Zero, ref length);
        if (length == 0)
            return 0;
        var buffer = Marshal.AllocHGlobal((int)length);
        try
        {
            if (!GetLogicalProcessorInformation(buffer, ref length))
                return 0;
            var size = Marshal.SizeOf<SystemLogicalProcessorInformation>();
            var cores = 0;
            for (var offset = 0; offset + size <= length; offset += size)
            {
                var info = Marshal.PtrToStructure<SystemLogicalProcessorInformation>(buffer + offset);
                if (info.Relationship == RelationProcessorCore)
                    cores++;
            }
            return cores;
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }

    private const int RelationProcessorCore = 0;

    [StructLayout(LayoutKind.Sequential)]
    private struct SystemLogicalProcessorInformation
    {
        public UIntPtr ProcessorMask;
        public int Relationship;
        // The union (cache descriptor / flags / reserved) — 16 bytes.
        public ulong Reserved0;
        public ulong Reserved1;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetLogicalProcessorInformation(IntPtr buffer, ref uint returnLength);
}
