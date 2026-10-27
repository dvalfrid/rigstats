using System.Runtime.Intrinsics.X86;

namespace SensorSidecar.Control.Providers;

/// RAPL units from MSR 0x606: power 1 W / 2^PU, energy 1 J / 2^ESU, time
/// 1 s / 2^TU. Never assumed — read on every machine (docs/intel-power-limits.md).
public readonly record struct RaplUnits(int PowerBits, int EnergyBits, int TimeBits)
{
    public double PowerW => 1.0 / (1L << PowerBits);
    public double TimeS => 1.0 / (1L << TimeBits);
}

/// MSR 0x610 (`MSR_PKG_POWER_LIMIT`), field by field, in raw units.
public readonly record struct PkgPowerLimit(
    uint Pl1Raw, bool Pl1Enabled, bool Pl1Clamp, int Tau1Raw,
    uint Pl2Raw, bool Pl2Enabled, bool Pl2Clamp, int Tau2Raw,
    bool Locked);

/// MSR 0x614 (`MSR_PKG_POWER_INFO`) in raw power units; 0 = not reported.
public readonly record struct PkgPowerInfo(uint ThermalSpecRaw, uint MinRaw, uint MaxRaw, int MaxWindowRaw);

/// Intel package power limits (PL1/PL2, #209). Pure decode/encode of the RAPL
/// MSRs per Intel SDM vol. 3B §17.10 — layouts, sources and test vectors in
/// docs/intel-power-limits.md — plus a read-only snapshot logged at start-up.
/// Nothing here writes to hardware yet: the write path waits for a read from
/// a real Intel CPU (live-test plan, step 1).
public static class IntelRapl
{
    public const uint MsrRaplPowerUnit = 0x606;
    public const uint MsrPkgPowerLimit = 0x610;
    public const uint MsrPkgPerfStatus = 0x613;
    public const uint MsrPkgPowerInfo = 0x614;
    public const uint MsrPackageThermStatus = 0x1B1;

    /// SHA-256 of `IntelMSR.bin` in LibreHardwareMonitorLib 0.9.7-pre749 — the
    /// signed PawnIO.Modules 0.2.11 build, the first with `ioctl_write_msr`.
    /// Pinned so an LHM change can't silently swap the module under us.
    public const string IntelMsrSha256 = "d6ed85d65ab17a22f813ef98207d6d537155ee2ded5976a21cb48413c9b92e5f";

    private const uint FieldMask = 0x7FFF;

    public static bool IsGenuineIntel((int Eax, int Ebx, int Ecx, int Edx) leaf0) =>
        leaf0 is { Ebx: 0x756E6547, Edx: 0x49656E69, Ecx: 0x6C65746E };

    public static RaplUnits Units(ulong raw606) => new(
        (int)(raw606 & 0xF),
        (int)((raw606 >> 8) & 0x1F),
        (int)((raw606 >> 16) & 0xF));

    public static PkgPowerLimit Decode(ulong raw610) => new(
        (uint)(raw610 & FieldMask),
        (raw610 & (1UL << 15)) != 0,
        (raw610 & (1UL << 16)) != 0,
        (int)((raw610 >> 17) & 0x7F),
        (uint)((raw610 >> 32) & FieldMask),
        (raw610 & (1UL << 47)) != 0,
        (raw610 & (1UL << 48)) != 0,
        (int)((raw610 >> 49) & 0x7F),
        (raw610 & (1UL << 63)) != 0);

    public static PkgPowerInfo DecodeInfo(ulong raw614) => new(
        (uint)(raw614 & FieldMask),
        (uint)((raw614 >> 16) & FieldMask),
        (uint)((raw614 >> 32) & FieldMask),
        (int)((raw614 >> 48) & 0x3F));

    public static double Watts(uint raw, RaplUnits units) => raw * units.PowerW;

    /// A 7-bit time window: 2^Y × (1 + Z/4) time units, Y = bits 4:0, Z = bits 6:5.
    public static double TimeWindowSeconds(int raw, RaplUnits units) =>
        (1L << (raw & 0x1F)) * (1 + ((raw >> 5) & 0x3) / 4.0) * units.TimeS;

    /// Watts to raw power units, rounded down so a request never ends above
    /// what was asked; capped to the 15-bit field.
    public static uint PowerToRaw(double watts, RaplUnits units) =>
        (uint)Math.Clamp(Math.Floor(watts / units.PowerW), 0, FieldMask);

    /// `raw610` with only the two limit fields replaced and both enable bits
    /// set; the clamp bits and time windows stay as the BIOS set them, and the
    /// lock bit is always cleared — RIGStats never locks the register.
    public static ulong WithLimits(ulong raw610, uint pl1Raw, uint pl2Raw)
    {
        var value = raw610 & ~((ulong)FieldMask | ((ulong)FieldMask << 32) | (1UL << 63));
        value |= (pl1Raw & FieldMask) | (1UL << 15);
        value |= ((ulong)(pl2Raw & FieldMask) << 32) | (1UL << 47);
        return value;
    }

    public static string Describe(PkgPowerLimit l, RaplUnits u) =>
        $"PL1 {Watts(l.Pl1Raw, u):0.###} W {(l.Pl1Enabled ? "on" : "off")}{(l.Pl1Clamp ? " clamp" : "")} " +
        $"Tau {TimeWindowSeconds(l.Tau1Raw, u):0.###} s; " +
        $"PL2 {Watts(l.Pl2Raw, u):0.###} W {(l.Pl2Enabled ? "on" : "off")}{(l.Pl2Clamp ? " clamp" : "")}; " +
        (l.Locked ? "locked by the BIOS" : "unlocked");

    /// Logs the RAPL registers once, read-only, on an Intel CPU, so a
    /// diagnostics export shows the units, the BIOS limits and the lock bit
    /// (live-test plan step 1). Only MSRs on IntelMSR's read list; never
    /// throws; does nothing on other CPUs.
    public static void LogSnapshot()
    {
        try
        {
            if (!X86Base.IsSupported || !IsGenuineIntel(X86Base.CpuId(0, 0)))
                return;
            var (eax, _, _, _) = X86Base.CpuId(1, 0);
            var family = (uint)((eax >> 8) & 0xF) + (uint)((eax >> 20) & 0xFF);
            var model = (uint)((eax >> 4) & 0xF) | (uint)(((eax >> 16) & 0xF) << 4);
            var cpu = $"family 0x{family:X} model 0x{model:X}";

            using var module = PawnIoModule.LoadFromLhm("IntelMSR.bin", IntelMsrSha256);
            ulong? Read(uint msr)
            {
                try
                {
                    return unchecked((ulong)module.Execute("ioctl_read_msr", [msr], 1)[0]);
                }
                catch (Exception e)
                {
                    SidecarLog.Log($"[rigstats-control] Intel RAPL: MSR 0x{msr:X} not read: {e.Message}");
                    return null;
                }
            }

            var raw = new[] { MsrRaplPowerUnit, MsrPkgPowerLimit, MsrPkgPowerInfo, MsrPackageThermStatus, MsrPkgPerfStatus }
                .Select(msr => (Msr: msr, Value: Read(msr)))
                .ToArray();
            var line = string.Join(" ", raw.Select(r => $"0x{r.Msr:X}={(r.Value is { } v ? $"0x{v:X16}" : "-")}"));
            SidecarLog.Log($"[rigstats-control] Intel RAPL ({cpu}), read-only: {line}");

            if (raw[0].Value is not { } unitsRaw)
                return;
            var units = Units(unitsRaw);
            var decoded = $"units {units.PowerW:0.######} W, {units.TimeS * 1e6:0.####} µs";
            if (raw[1].Value is { } limit)
                decoded += $"; {Describe(Decode(limit), units)}";
            if (raw[2].Value is { } infoRaw)
            {
                var info = DecodeInfo(infoRaw);
                decoded += $"; power info TDP {Watts(info.ThermalSpecRaw, units):0.###} W, " +
                    $"min {Watts(info.MinRaw, units):0.###} W, max {Watts(info.MaxRaw, units):0.###} W";
            }
            SidecarLog.Log($"[rigstats-control] Intel RAPL decoded: {decoded}");
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] Intel RAPL: not read: {e.Message}");
        }
    }
}
