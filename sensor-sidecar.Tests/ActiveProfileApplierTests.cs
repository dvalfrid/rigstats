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

    private BootCrashGuard Guard() =>
        new(Path.Combine(_dir, "pending-apply"), BootCrashGuard.DefaultStableAfter);

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
        var applier = new ActiveProfileApplier(_store, new ControlBroker([powerPlan]), Guard());

        await applier.StartAsync(CancellationToken.None);

        powerPlan.Received(1).Apply(Arg.Is<ProfilePart>(p => p.PowerPlan == "high_performance"));
    }

    [Fact]
    public async Task A_failing_provider_does_not_fail_service_start()
    {
        var fan = Provider("fan");
        fan.When(p => p.Apply(Arg.Any<ProfilePart>())).Throw(new InvalidOperationException("boom"));
        var applier = new ActiveProfileApplier(_store, new ControlBroker([fan]), Guard());

        await applier.StartAsync(CancellationToken.None); // must not throw

        fan.Received(1).Restore(Arg.Any<Snapshot>());
    }

    [Fact]
    public async Task After_a_tripped_crash_guard_cpu_limits_go_back_to_bios_and_the_rest_is_applied()
    {
        var limited = new Profile
        {
            Id = "limited",
            Name = "Limited",
            Part = new ProfilePart { PowerPlan = "balanced", CpuLimit = new CpuLimitPart { Amd = new AmdCpuLimit { PptW = 88 } } },
        };
        await _store.SaveProfileAsync(limited, CancellationToken.None);
        await _store.SetActiveAsync("limited", CancellationToken.None);
        var marker = Path.Combine(_dir, "pending-apply");
        File.WriteAllText(marker, "x"); // the previous run went down mid-window.
        var powerPlan = Provider("power_plan");
        var cpuLimit = Provider("cpu_limit");
        var applier = new ActiveProfileApplier(_store, new ControlBroker([powerPlan, cpuLimit]), Guard());

        await applier.StartAsync(CancellationToken.None);

        powerPlan.Received(1).Apply(Arg.Any<ProfilePart>());
        // Put back to BIOS values (an empty part), never the stored 88 W.
        cpuLimit.Received(1).Apply(Arg.Is<ProfilePart>(p => p.CpuLimit != null && p.CpuLimit.Amd == null));
        cpuLimit.DidNotReceive().Apply(Arg.Is<ProfilePart>(p => p.CpuLimit != null && p.CpuLimit.Amd != null));
    }

    [Fact]
    public async Task Without_a_marker_cpu_limits_are_re_applied()
    {
        var limited = new Profile
        {
            Id = "limited",
            Name = "Limited",
            Part = new ProfilePart { CpuLimit = new CpuLimitPart { Amd = new AmdCpuLimit { PptW = 88 } } },
        };
        await _store.SaveProfileAsync(limited, CancellationToken.None);
        await _store.SetActiveAsync("limited", CancellationToken.None);
        var cpuLimit = Provider("cpu_limit");
        var applier = new ActiveProfileApplier(_store, new ControlBroker([cpuLimit]), Guard());

        await applier.StartAsync(CancellationToken.None);

        cpuLimit.Received(1).Apply(Arg.Is<ProfilePart>(p => p.CpuLimit!.Amd!.PptW == 88));
    }
}
