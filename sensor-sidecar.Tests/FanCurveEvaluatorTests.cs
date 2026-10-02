using SensorSidecar.Control;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// <see cref="FanCurveEvaluator"/> is pure — curve interpolation, hysteresis,
/// sensor-loss and source resolution, no hardware involved.
/// </summary>
public class FanCurveEvaluatorTests
{
    private static readonly List<List<double>> Curve = [[40, 30], [70, 60], [85, 100]];

    private static FanHeaderConfig Config(double hysteresis = 3.0) =>
        new() { Source = "cpu_package", Curve = Curve, HysteresisC = hysteresis };

    [Theory]
    [InlineData(20, 30)]  // below the curve → first duty
    [InlineData(40, 30)]
    [InlineData(55, 45)]  // halfway between 40/30 and 70/60
    [InlineData(70, 60)]
    [InlineData(95, 100)] // above the curve → last duty
    public void InterpolateDuty_is_linear_and_clamped_to_the_ends(double temp, double expected) =>
        Assert.Equal(expected, FanCurveEvaluator.InterpolateDuty(Curve, temp), precision: 6);

    [Fact]
    public void InterpolateDuty_fails_safe_to_full_speed_on_an_empty_curve() =>
        Assert.Equal(100, FanCurveEvaluator.InterpolateDuty([], 50));

    [Fact]
    public void Sensor_loss_forces_full_speed()
    {
        var decision = FanCurveEvaluator.EvaluateHeader(Config(), sourceTempC: null, previous: null);
        Assert.Equal(100, decision.Duty);
    }

    [Fact]
    public void Sensor_loss_override_is_not_held_once_the_sensor_recovers()
    {
        var lost = FanCurveEvaluator.EvaluateHeader(Config(), null, null);
        var recovered = FanCurveEvaluator.EvaluateHeader(Config(), 40, lost);
        Assert.Equal(30, recovered.Duty);
    }

    [Fact]
    public void Hysteresis_holds_a_small_temperature_drop()
    {
        var at70 = FanCurveEvaluator.EvaluateHeader(Config(), 70, null);
        var at68 = FanCurveEvaluator.EvaluateHeader(Config(), 68, at70);
        Assert.Equal(60, at68.Duty);
        Assert.Equal(70, at68.DecisionTempC);
    }

    [Fact]
    public void Hysteresis_releases_after_a_large_enough_drop()
    {
        var at70 = FanCurveEvaluator.EvaluateHeader(Config(), 70, null);
        var at67 = FanCurveEvaluator.EvaluateHeader(Config(), 67, at70);
        Assert.Equal(57, at67.Duty, precision: 6);
    }

    [Fact]
    public void A_rising_temperature_is_never_held_by_hysteresis()
    {
        var at55 = FanCurveEvaluator.EvaluateHeader(Config(), 55, null);
        var at56 = FanCurveEvaluator.EvaluateHeader(Config(), 56, at55);
        Assert.Equal(46, at56.Duty, precision: 6);
    }

    [Fact]
    public void ResolveSource_reads_cpu_hottest_gpu_and_motherboard_by_label()
    {
        var sample = FanSamples.Temps(cpu: 61, gpu: 48, mb: ("System", 33)) with
        {
            GpuDevices =
            [
                new GpuDevice("iGPU", "test", null, 40, null, null, null, null, null, null, null, null, null),
                new GpuDevice("dGPU", "test", null, 72, null, null, null, null, null, null, null, null, null),
            ],
        };

        Assert.Equal(61, FanCurveEvaluator.ResolveSource(sample, "cpu_package"));
        Assert.Equal(72, FanCurveEvaluator.ResolveSource(sample, "gpu"));
        Assert.Equal(33, FanCurveEvaluator.ResolveSource(sample, "mb:System"));
        Assert.Null(FanCurveEvaluator.ResolveSource(sample, "mb:Missing"));
        Assert.Null(FanCurveEvaluator.ResolveSource(sample, "nonsense"));
    }

    [Fact]
    public void ResolveSource_is_null_when_the_cpu_reading_is_unavailable()
    {
        // SensorReader drops a 0 °C CPU reading (e.g. PawnIO not loaded, #205).
        var sample = FanSamples.Temps(cpu: null);
        Assert.Null(FanCurveEvaluator.ResolveSource(sample, "cpu_package"));
    }

    [Fact]
    public void AvailableSources_lists_only_sources_present_in_the_sample()
    {
        var sources = FanCurveEvaluator.AvailableSources(FanSamples.Temps(cpu: 50, gpu: null, mb: ("VRM", 45))).ToList();
        Assert.Equal(["cpu_package", "mb:VRM"], sources);
    }

    [Theory]
    [InlineData(94f, 89f, false)]
    [InlineData(96f, 50f, true)]
    [InlineData(50f, 91f, true)]
    public void CriticalReason_trips_above_the_cpu_or_gpu_threshold(float cpu, float gpu, bool critical) =>
        Assert.Equal(critical, FanCurveEvaluator.CriticalReason(FanSamples.Temps(cpu, gpu)) is not null);
}
