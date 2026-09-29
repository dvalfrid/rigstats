using SensorSidecar.Control;
using SensorSidecar.Control.Providers;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Exercises <see cref="PowerPlanProvider"/> against a fake
/// <see cref="IPowerPlanApi"/> — no real <c>powrprof.dll</c> calls, per the
/// design doc's testing strategy ("driven by IHardwareHost fakes", the same
/// idea applied to this provider's own OS-API seam).
/// </summary>
public class PowerPlanProviderTests
{
    private static readonly Guid PowerSaver = new("a1841308-3541-4fab-bc81-f71556f20b4a");
    private static readonly Guid Balanced = new("381b4222-f694-41f0-9685-ff5bb260df2e");
    private static readonly Guid HighPerformance = new("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c");

    private sealed class FakePowerPlanApi : IPowerPlanApi
    {
        public Guid Active { get; set; } = Balanced;
        public List<(Guid, string)> Schemes { get; } =
        [
            (PowerSaver, "Power saver"),
            (Balanced, "Balanced"),
            (HighPerformance, "High performance"),
        ];

        public Guid GetActiveScheme() => Active;
        public void SetActiveScheme(Guid scheme) => Active = scheme;
        public IReadOnlyList<(Guid Guid, string Name)> EnumerateSchemes() => Schemes;
    }

    [Fact]
    public void Probe_reports_supported_with_available_schemes()
    {
        var provider = new PowerPlanProvider(new FakePowerPlanApi());
        var caps = provider.Probe();

        Assert.Equal("power_plan", caps.Domain);
        Assert.True(caps.Supported);
    }

    [Fact]
    public void Validate_accepts_known_well_known_name()
    {
        var provider = new PowerPlanProvider(new FakePowerPlanApi());
        var result = provider.Validate(new ProfilePart { PowerPlan = "high_performance" });
        Assert.True(result.Ok);
    }

    [Fact]
    public void Validate_rejects_unknown_name()
    {
        var provider = new PowerPlanProvider(new FakePowerPlanApi());
        var result = provider.Validate(new ProfilePart { PowerPlan = "turbo_nonsense" });
        Assert.False(result.Ok);
        Assert.NotNull(result.Reason);
    }

    [Fact]
    public void Validate_is_a_noop_when_power_plan_is_not_set()
    {
        var provider = new PowerPlanProvider(new FakePowerPlanApi());
        var result = provider.Validate(new ProfilePart());
        Assert.True(result.Ok);
    }

    [Fact]
    public void Apply_sets_the_resolved_scheme()
    {
        var api = new FakePowerPlanApi();
        var provider = new PowerPlanProvider(api);

        provider.Apply(new ProfilePart { PowerPlan = "high_performance" });

        Assert.Equal(HighPerformance, api.Active);
    }

    [Fact]
    public void Verify_true_only_when_active_scheme_matches()
    {
        var api = new FakePowerPlanApi { Active = HighPerformance };
        var provider = new PowerPlanProvider(api);

        Assert.True(provider.Verify(new ProfilePart { PowerPlan = "high_performance" }));
        Assert.False(provider.Verify(new ProfilePart { PowerPlan = "power_saver" }));
    }

    [Fact]
    public void Capture_then_restore_round_trips_the_active_scheme()
    {
        var api = new FakePowerPlanApi { Active = Balanced };
        var provider = new PowerPlanProvider(api);

        var snapshot = provider.Capture();
        provider.Apply(new ProfilePart { PowerPlan = "high_performance" });
        Assert.Equal(HighPerformance, api.Active);

        provider.Restore(snapshot);
        Assert.Equal(Balanced, api.Active);
    }
}
