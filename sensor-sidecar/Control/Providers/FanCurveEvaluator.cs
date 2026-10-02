namespace SensorSidecar.Control.Providers;

/// Pure fan-curve control logic — no I/O, no LHM types, fully unit-testable
/// without hardware fakes. `FanCurveLoop` is the thin ~1 Hz shell that reads
/// real sensors and calls this each tick. The critical-temperature override
/// (doc: "CPU or GPU above a hard threshold → all controlled fans to 100%")
/// is a global decision spanning every header, so `FanCurveLoop` applies it
/// on top of `EvaluateHeader`, using `IsCritical` from here.
public static class FanCurveEvaluator
{
    // Confirmed with the owner: common near-throttle BIOS defaults, not
    // user-configurable in phase 1.
    public const double CriticalCpuTempC = 95.0;
    public const double CriticalGpuTempC = 90.0;

    public const string CpuSource = "cpu_package";
    public const string GpuSource = "gpu";
    public const string MotherboardSourcePrefix = "mb:";

    /// Linear interpolation between the two curve points bracketing `tempC`,
    /// clamped to the end duty below/above the curve's range. An empty curve
    /// fails safe to 100% (never silently idle a fan with no configuration).
    public static double InterpolateDuty(IReadOnlyList<IReadOnlyList<double>> curve, double tempC)
    {
        if (curve.Count == 0)
            return 100.0;
        if (curve.Count == 1)
            return curve[0][1];
        if (tempC <= curve[0][0])
            return curve[0][1];
        if (tempC >= curve[^1][0])
            return curve[^1][1];

        for (var i = 0; i < curve.Count - 1; i++)
        {
            var (t0, d0) = (curve[i][0], curve[i][1]);
            var (t1, d1) = (curve[i + 1][0], curve[i + 1][1]);
            if (tempC < t0 || tempC > t1)
                continue;
            if (t1 - t0 < 1e-9)
                return d1;
            var frac = (tempC - t0) / (t1 - t0);
            return d0 + frac * (d1 - d0);
        }

        return curve[^1][1]; // Unreachable for a validated ascending curve; fail safe.
    }

    /// One header's decision for this tick, and the temperature it was made
    /// at — threaded back in as `previous` next tick so hysteresis has
    /// something to compare against. `DecisionTempC` is NaN for a
    /// sensor-loss override, so it is never held by hysteresis once the
    /// sensor comes back.
    public readonly record struct HeaderDecision(double Duty, double DecisionTempC);

    /// Evaluates one header: sensor-loss override (no source reading →
    /// 100%), then hysteresis, then curve interpolation. Hysteresis only
    /// damps a *falling* temperature — the previous decision is held until
    /// the source has dropped `HysteresisC` below the temperature that
    /// produced it — while a rising temperature always re-evaluates at once,
    /// so cooling never lags behind heat.
    public static HeaderDecision EvaluateHeader(FanHeaderConfig config, double? sourceTempC, HeaderDecision? previous)
    {
        if (sourceTempC is not { } temp)
            return new HeaderDecision(100.0, double.NaN);

        if (previous is { } prev
            && temp < prev.DecisionTempC
            && prev.DecisionTempC - temp < config.HysteresisC)
            return prev;

        return new HeaderDecision(InterpolateDuty(config.Curve, temp), temp);
    }

    /// The temperature a header's `Source` refers to in this sample, or null
    /// when it is missing (unknown source, sensor gone, or filtered as
    /// unreadable by `SensorReader` — e.g. CPU 0 °C without PawnIO).
    /// `"gpu"` is the hottest GPU, so an idle iGPU can't mask a hot dGPU.
    public static double? ResolveSource(SensorPayload sample, string source) => source switch
    {
        CpuSource => sample.CpuTemp,
        GpuSource => HottestGpu(sample),
        _ when source.StartsWith(MotherboardSourcePrefix, StringComparison.Ordinal) =>
            sample.MbTemps.FirstOrDefault(t => t.Label == source[MotherboardSourcePrefix.Length..])?.Celsius,
        _ => null,
    };

    /// Every source id a curve may reference in this sample — reported in
    /// the fan capability set so the UI only offers sources that exist.
    public static IEnumerable<string> AvailableSources(SensorPayload sample)
    {
        if (sample.CpuTemp is not null)
            yield return CpuSource;
        if (HottestGpu(sample) is not null)
            yield return GpuSource;
        foreach (var t in sample.MbTemps)
            yield return MotherboardSourcePrefix + t.Label;
    }

    /// Non-null reason when CPU or GPU is above its critical threshold.
    public static string? CriticalReason(SensorPayload sample)
    {
        var cpu = sample.CpuTemp;
        var gpu = HottestGpu(sample);
        if (cpu > CriticalCpuTempC)
            return $"CPU {cpu:F0}°C above the critical threshold ({CriticalCpuTempC:F0}°C)";
        if (gpu > CriticalGpuTempC)
            return $"GPU {gpu:F0}°C above the critical threshold ({CriticalGpuTempC:F0}°C)";
        return null;
    }

    private static double? HottestGpu(SensorPayload sample) =>
        sample.GpuDevices.Select(g => g.Temp).Where(t => t is not null).Max();
}
