using System.Runtime.Intrinsics.X86;

namespace SensorSidecar.Control.Providers;

/// AMD package limits: PPT (W), TDC (A), EDC (A).
public readonly record struct AmdLimits(double PptW, double TdcA, double EdcA);

/// Curve Optimizer mailbox ids (RSMU) for one generation.
public sealed record CurveOptimizerCommands(uint SetPerCore, uint SetAll, uint Get);

/// One physical core: CCD index and core index within it.
public readonly record struct PhysicalCore(int Ccd, int Core);

/// RSMU mailbox message ids for one CPU generation. The argument is the
/// limit in mW / mA.
public sealed record SmuLimitCommands(uint SetPpt, uint SetTdc, uint SetEdc);

/// Where the limits sit in one PM table version, as float indices.
public sealed record PmTableLayout(int PptLimit, int TdcLimit, int EdcLimit)
{
    public int FloatsNeeded => Math.Max(PptLimit, Math.Max(TdcLimit, EdcLimit)) + 1;
}

/// What is known to work, per CPU generation and per PM table version.
/// Both must be known before CPU limits are offered: the command ids differ
/// between generations (Zen 4's SetPPT 0x56 is Zen 2's SetTctlMax), and the
/// PM table — the only readback — moves its fields between versions. Command
/// ids are protocol facts from public documentation (ZenStates-Core, GPL,
/// used as documentation only — no code copied). Table layouts are added
/// only after being checked on real hardware: a diagnostics export from an
/// unlisted CPU logs its table version and head (see `RyzenSmu.TryOpen`).
public static class AmdSmuMap
{
    private static readonly SmuLimitCommands Zen2And3 = new(0x53, 0x54, 0x55);
    private static readonly SmuLimitCommands Zen4And5 = new(0x56, 0x57, 0x58);

    /// Desktop Ryzen only — mobile, Threadripper and server parts use other
    /// mailboxes or limits and are not offered.
    public static (string Name, SmuLimitCommands Commands)? Generation(uint family, uint model, uint pkgType) =>
        ((family << 8) | model) switch
        {
            0x1771 => ("Matisse", Zen2And3),
            0x1920 or 0x1921 => ("Vermeer", Zen2And3),
            0x1961 when pkgType != 1 => ("Raphael", Zen4And5), // pkg 1 = Dragon Range (mobile)
            0x1A44 => ("Granite Ridge", Zen4And5),
            _ => null,
        };

    /// Verified on hardware:
    /// - 0x620105 — Ryzen 7 9800X3D (Granite Ridge, SMU 98.78.0): PPT limit
    ///   [2], TDC limit [8], EDC limit [63]; stock 162 W / 120 A / 180 A.
    public static PmTableLayout? Layout(uint tableVersion) => tableVersion switch
    {
        0x620105 => new PmTableLayout(2, 8, 63),
        _ => null,
    };

    /// CPUID leaf 0 spells the vendor in EBX, EDX, ECX: "Auth" "enti" "cAMD".
    public static bool IsAuthenticAmd((int Eax, int Ebx, int Ecx, int Edx) leaf0) =>
        leaf0 is { Ebx: 0x68747541, Edx: 0x69746E65, Ecx: 0x444D4163 };

    /// Curve Optimizer (#191) RSMU ids, only for generations where they were
    /// checked on hardware: Zen 4/5 SetDldoPsmMargin 0x6 (per core),
    /// SetAllDldoPsmMargin 0x7, GetDldoPsmMargin 0xD5 — verified on a
    /// Ryzen 7 9800X3D. (ZenStates-Core marks the Zen 3 ids "not sure".)
    public static CurveOptimizerCommands? CurveOptimizer(string generation) => generation switch
    {
        "Granite Ridge" => new CurveOptimizerCommands(SetPerCore: 0x6, SetAll: 0x7, Get: 0xD5),
        _ => null,
    };

    /// Zen 3+ core mask: [31:28] CCD, [23:20] physical core within the CCD.
    public static uint CoreMask(int ccd, int core) => ((uint)ccd << 28) | ((uint)(core % 8) << 20);

    /// Core mask in the top 12 bits, the offset as 16-bit two's complement.
    public static uint CurveOptimizerArg(uint coreMask, int offset) =>
        (coreMask & 0xFFF00000) | ((uint)offset & 0xFFFF);

    /// GetDldoPsmMargin's answer: the offset in the low 16 bits, signed.
    public static int CurveOptimizerValue(uint raw) => (short)(raw & 0xFFFF);

    /// SMU limit argument: W or A to mW or mA.
    public static uint ToSmuArg(double value) => (uint)Math.Round(value * 1000.0);

    /// One float out of the PM table, which the module hands back packed two
    /// per 64-bit value (low half first).
    public static float PmFloat(long[] table, int index)
    {
        var q = table[index / 2];
        var bits = (int)(index % 2 == 0 ? q & 0xFFFFFFFF : (q >> 32) & 0xFFFFFFFF);
        return BitConverter.Int32BitsToSingle(bits);
    }
}

/// The SMU operations `CpuLimitProvider` needs — a seam for tests.
public interface IRyzenSmu
{
    /// "Granite Ridge", ...
    string Generation { get; }
    uint PmTableVersion { get; }

    /// The limits currently in force, read back from a fresh PM table.
    AmdLimits ReadLimits();

    void SetLimits(AmdLimits limits);
}

/// Curve Optimizer through the same SMU — a seam for tests.
public interface ICurveOptimizerSmu
{
    /// The cores the SMU answers for, in CCD/core order.
    IReadOnlyList<PhysicalCore> Cores();

    int GetOffset(PhysicalCore core);

    void SetOffset(PhysicalCore core, int offset);

    void SetAllOffsets(int offset);
}

/// RSMU mailbox + PM table through LHM's signed `RyzenSMU` PawnIO module —
/// the "AMD SMU module" of the design doc. SMU limits are not persistent:
/// they reset to the BIOS values at every reboot.
public sealed class RyzenSmu : IRyzenSmu, ICurveOptimizerSmu, IDisposable
{
    // Desktop Ryzen has at most two CCDs of eight cores.
    private const int MaxCcds = 2;
    private const int CoresPerCcd = 8;

    // Shared with LHM (and other tools) for every SMU/PCI access — LHM reads
    // the same PM table for telemetry from its own module instance.
    private const string PciMutexName = @"Global\Access_PCI";
    private static readonly TimeSpan MutexTimeout = TimeSpan.FromSeconds(2);

    private readonly IPawnIoModule _module;
    private readonly SmuLimitCommands _commands;
    private readonly PmTableLayout _layout;
    private readonly Mutex _pciMutex = new(false, PciMutexName);

    public string Generation { get; }
    public uint PmTableVersion { get; }

    private RyzenSmu(IPawnIoModule module, string generation, SmuLimitCommands commands, uint tableVersion, PmTableLayout layout)
    {
        _module = module;
        Generation = generation;
        _commands = commands;
        PmTableVersion = tableVersion;
        _layout = layout;
    }

    /// Null with a user-facing reason when this machine isn't supported.
    public static RyzenSmu? TryOpen(out string reason)
    {
        if (!X86Base.IsSupported)
        {
            reason = "CPU power limits need an x64 CPU.";
            return null;
        }
        if (!AmdSmuMap.IsAuthenticAmd(X86Base.CpuId(0, 0)))
        {
            reason = "CPU power limits are only available on AMD Ryzen for now.";
            SidecarLog.Log("[rigstats-control] CPU limits: not an AMD CPU.");
            return null;
        }
        var (eax, _, _, _) = X86Base.CpuId(1, 0);
        var family = (uint)((eax >> 8) & 0xF) + (uint)((eax >> 20) & 0xFF);
        var model = (uint)((eax >> 4) & 0xF) | (uint)(((eax >> 16) & 0xF) << 4);
        var (_, extEbx, _, _) = X86Base.CpuId(unchecked((int)0x80000001), 0);
        var pkgType = (uint)((extEbx >> 28) & 0xF);
        var cpu = $"family 0x{family:X} model 0x{model:X} package {pkgType}";

        if (AmdSmuMap.Generation(family, model, pkgType) is not { } generation)
        {
            reason = "CPU power limits are not supported on this CPU.";
            SidecarLog.Log($"[rigstats-control] CPU limits: unsupported CPU ({cpu}).");
            return null;
        }

        IPawnIoModule module;
        try
        {
            module = PawnIoModule.LoadFromLhm("RyzenSMU.bin");
        }
        catch (Exception e)
        {
            reason = "CPU power limits need the PawnIO driver.";
            SidecarLog.Log($"[rigstats-control] CPU limits: RyzenSMU module not loaded: {e.Message}");
            return null;
        }

        using var pciMutex = new Mutex(false, PciMutexName);
        var locked = Acquire(pciMutex);
        try
        {
            var tableVersion = (uint)module.Execute("ioctl_resolve_pm_table", [], 2)[0];
            if (AmdSmuMap.Layout(tableVersion) is { } layout)
            {
                reason = "";
                SidecarLog.Log($"[rigstats-control] CPU limits: {generation.Name} ({cpu}), PM table 0x{tableVersion:X}.");
                return new RyzenSmu(module, generation.Name, generation.Commands, tableVersion, layout);
            }

            // Unknown layout: record the head of the table so a diagnostics
            // export is enough to add this version to AmdSmuMap.Layout.
            module.Execute("ioctl_update_pm_table", [], 0);
            var head = module.Execute("ioctl_read_pm_table", [], 64);
            var floats = Enumerable.Range(0, 128).Select(i => $"[{i}]={AmdSmuMap.PmFloat(head, i):F1}");
            SidecarLog.Log($"[rigstats-control] CPU limits: {generation.Name} ({cpu}) PM table 0x{tableVersion:X} " +
                $"not verified yet; head: {string.Join(" ", floats)}");
            reason = $"CPU power limits are not verified for this CPU yet (PM table 0x{tableVersion:X}).";
        }
        catch (Exception e)
        {
            reason = "CPU power limits are unavailable: the SMU did not answer.";
            SidecarLog.Log($"[rigstats-control] CPU limits: PM table probe failed: {e.Message}");
        }
        finally
        {
            if (locked)
                pciMutex.ReleaseMutex();
        }
        module.Dispose();
        return null;
    }

    // STATUS_INTERNAL_ERROR: the module's mapping of the SMU's 0xFD
    // "command rejected, prerequisite not met".
    private const int ErrorInternalError = 1359;
    private const int TransferAttempts = 5;

    public AmdLimits ReadLimits() => WithPciLock(() =>
    {
        RefreshPmTable();
        var table = _module.Execute("ioctl_read_pm_table", [], (_layout.FloatsNeeded + 1) / 2);
        return new AmdLimits(
            AmdSmuMap.PmFloat(table, _layout.PptLimit),
            AmdSmuMap.PmFloat(table, _layout.TdcLimit),
            AmdSmuMap.PmFloat(table, _layout.EdcLimit));
    });

    public void SetLimits(AmdLimits limits) => WithPciLock(() =>
    {
        Send(_commands.SetPpt, limits.PptW);
        Send(_commands.SetTdc, limits.TdcA);
        Send(_commands.SetEdc, limits.EdcA);
        return 0;
    });

    /// Seen on the 9800X3D: TransferTableToDram is rejected (0xFD) once the
    /// table address resolved earlier has gone stale — LHM opens its own
    /// module instance after ours. Resolving again right before the
    /// transfer is what made the probe tool work, so do that and retry.
    private void RefreshPmTable()
    {
        for (var attempt = 1; ; attempt++)
        {
            try
            {
                _module.Execute("ioctl_update_pm_table", [], 0);
                if (attempt > 1)
                    SidecarLog.Log($"[rigstats-control] CPU limits: PM table transfer needed {attempt} attempts.");
                return;
            }
            catch (System.ComponentModel.Win32Exception e)
                when (e.NativeErrorCode == ErrorInternalError && attempt < TransferAttempts)
            {
                _module.Execute("ioctl_resolve_pm_table", [], 2);
                Thread.Sleep(20 * attempt);
            }
        }
    }

    private void Send(uint message, double value) =>
        _module.Execute("ioctl_send_smu_command", [message, AmdSmuMap.ToSmuArg(value), 0, 0, 0, 0, 0], 6);

    // ── Curve Optimizer (#191) ──────────────────────────────────────────

    /// Null when this generation's CO ids aren't verified.
    public ICurveOptimizerSmu? CurveOptimizer => AmdSmuMap.CurveOptimizer(Generation) is null ? null : this;

    private IReadOnlyList<PhysicalCore>? _cores;

    /// Asks GetDldoPsmMargin for every possible core; the SMU rejects
    /// cores (and CCDs) that don't exist — checked on a 9800X3D, where
    /// CCD 0 cores 0–7 answer and CCD 1 is rejected. Cached: cores don't
    /// change at runtime.
    public IReadOnlyList<PhysicalCore> Cores() => _cores ??= WithPciLock(() =>
    {
        var cores = new List<PhysicalCore>();
        for (var ccd = 0; ccd < MaxCcds; ccd++)
        {
            for (var core = 0; core < CoresPerCcd; core++)
            {
                try
                {
                    SendCo(Co.Get, AmdSmuMap.CoreMask(ccd, core));
                    cores.Add(new PhysicalCore(ccd, core));
                }
                catch (System.ComponentModel.Win32Exception)
                {
                    // Not present.
                }
            }
        }
        return (IReadOnlyList<PhysicalCore>)cores;
    });

    public int GetOffset(PhysicalCore core) => WithPciLock(() =>
        AmdSmuMap.CurveOptimizerValue(SendCo(Co.Get, AmdSmuMap.CoreMask(core.Ccd, core.Core))));

    public void SetOffset(PhysicalCore core, int offset) => WithPciLock(() =>
        SendCo(Co.SetPerCore, AmdSmuMap.CurveOptimizerArg(AmdSmuMap.CoreMask(core.Ccd, core.Core), offset)));

    public void SetAllOffsets(int offset) => WithPciLock(() =>
        SendCo(Co.SetAll, AmdSmuMap.CurveOptimizerArg(0, offset)));

    private CurveOptimizerCommands Co => AmdSmuMap.CurveOptimizer(Generation)
        ?? throw new InvalidOperationException($"Curve Optimizer is not verified for {Generation}.");

    /// One RSMU command with a raw argument; returns the first result word.
    private uint SendCo(uint message, uint arg) =>
        (uint)_module.Execute("ioctl_send_smu_command", [message, arg, 0, 0, 0, 0, 0], 6)[0];

    private static bool Acquire(Mutex mutex)
    {
        try
        {
            return mutex.WaitOne(MutexTimeout);
        }
        catch (AbandonedMutexException)
        {
            return true; // a crashed holder — the mutex is ours now.
        }
    }

    private T WithPciLock<T>(Func<T> action)
    {
        if (!Acquire(_pciMutex))
            throw new TimeoutException("The SMU is busy (PCI bus mutex not acquired).");
        try
        {
            return action();
        }
        finally
        {
            _pciMutex.ReleaseMutex();
        }
    }

    public void Dispose()
    {
        _module.Dispose();
        _pciMutex.Dispose();
    }
}
