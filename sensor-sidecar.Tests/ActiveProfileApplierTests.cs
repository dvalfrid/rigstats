using NSubstitute;
using NSubstitute.ExceptionExtensions;
using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// The stored active profile is re-applied when the service starts, so fan
/// curves survive a reboot without the UI.
/// </summary>
public sealed class ActiveProfileApplierTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "rigstats-applier-tests-" + Guid.NewGuid());
    private readonly ProfileStore _store;

    public ActiveProfileApplierTests()
    {
        Directory.CreateDirectory(_dir);
        _store = new ProfileStore(Path.Combine(_dir, "profiles.json"));
    }

    public void Dispose()
    {
        try { Directory.Delete(_dir, recursive: true); } catch { }
    }

    private static IControlProvider Provider(string domain)
    {
        var p = Substitute.For<IControlProvider>();
        p.Domain.Returns(domain);
        p.Validate(Arg.Any<ProfilePart>()).Returns(ValidationResult.Success());
        p.Capture().Returns(new Snapshot { Domain = domain });
        p.Verify(Arg.Any<ProfilePart>()).Returns(true);
        return p;
    }

    [Fact]
    public async Task Start_applies_the_stored_active_profile()
    {
        await _store.SetActiveAsync("gaming", CancellationToken.None);
        var powerPlan = Provider("power_plan");
        var applier = new ActiveProfileApplier(_store, new ControlBroker([powerPlan]));

        await applier.StartAsync(CancellationToken.None);

        powerPlan.Received(1).Apply(Arg.Is<ProfilePart>(p => p.PowerPlan == "high_performance"));
    }

    [Fact]
    public async Task A_failing_provider_does_not_fail_service_start()
    {
        var fan = Provider("fan");
        fan.When(p => p.Apply(Arg.Any<ProfilePart>())).Throw(new InvalidOperationException("boom"));
        var applier = new ActiveProfileApplier(_store, new ControlBroker([fan]));

        await applier.StartAsync(CancellationToken.None); // must not throw

        fan.Received(1).Restore(Arg.Any<Snapshot>());
    }
}
