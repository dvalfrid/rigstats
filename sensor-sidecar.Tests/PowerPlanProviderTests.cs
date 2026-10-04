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
    private static readonly Guid UltimatePerformance = new("e9a42b02-d5df-448d-aa00-03f14749eb61");

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
    public void Unknown_name_falls_back_to_balanced()
    {
        var api = new FakePowerPlanApi { Active = PowerSaver };
        var provider = new PowerPlanProvider(api);
        var part = new ProfilePart { PowerPlan = "turbo_nonsense" };

        Assert.True(provider.Validate(part).Ok);
        provider.Apply(part);
        Assert.Equal(Balanced, api.Active);
        Assert.True(provider.Verify(part));
    }

    // Modern Standby laptops (ASUS G14) ship with Balanced only: Gaming's
    // high_performance must not fail the whole profile.
    [Theory]
    [InlineData("high_performance")]
    [InlineData("power_saver")]
    public void Missing_plan_on_a_balanced_only_pc_uses_balanced(string plan)
    {
        var api = new FakePowerPlanApi();
        api.Schemes.Clear();
        api.Schemes.Add((Balanced, "Balanced"));
        var provider = new PowerPlanProvider(api);
        var part = new ProfilePart { PowerPlan = plan };

        Assert.True(provider.Validate(part).Ok);
        provider.Apply(part);
        Assert.Equal(Balanced, api.Active);
        Assert.True(provider.Verify(part));
    }

    [Fact]
    public void Missing_high_performance_prefers_ultimate_performance()
    {
        var api = new FakePowerPlanApi();
        api.Schemes.RemoveAll(s => s.Item1 == HighPerformance);
        api.Schemes.Add((UltimatePerformance, "Ultimate Performance"));
        var provider = new PowerPlanProvider(api);

        provider.Apply(new ProfilePart { PowerPlan = "high_performance" });

        Assert.Equal(UltimatePerformance, api.Active);
    }

    [Fact]
    public void Custom_plan_resolves_by_its_guid()
    {
        var custom = Guid.NewGuid();
        var api = new FakePowerPlanApi();
        api.Schemes.Add((custom, "My plan"));
        var provider = new PowerPlanProvider(api);
        var part = new ProfilePart { PowerPlan = custom.ToString() };

        provider.Apply(part);

        Assert.Equal(custom, api.Active);
        Assert.True(provider.Verify(part));
    }

    [Fact]
    public void No_plans_at_all_leaves_the_active_plan_untouched()
    {
        var api = new FakePowerPlanApi { Active = PowerSaver };
        api.Schemes.Clear();
        var provider = new PowerPlanProvider(api);
        var part = new ProfilePart { PowerPlan = "high_performance" };

        Assert.True(provider.Validate(part).Ok);
        provider.Apply(part);
        Assert.Equal(PowerSaver, api.Active);
        Assert.True(provider.Verify(part));
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
