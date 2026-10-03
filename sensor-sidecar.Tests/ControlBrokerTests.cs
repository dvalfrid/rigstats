using NSubstitute;
using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// The doc-mandated broker guarantee: "a provider that fails mid-transaction
/// must trigger full rollback." <see cref="ControlBroker"/> must never leave
/// a profile apply half-done.
/// </summary>
public class ControlBrokerTests
{
    private static IControlProvider FakeProvider(string domain, bool verifyOk = true)
    {
        var p = Substitute.For<IControlProvider>();
        p.Domain.Returns(domain);
        p.Validate(Arg.Any<ProfilePart>()).Returns(ValidationResult.Success());
        p.Capture().Returns(new Snapshot { Domain = domain });
        p.Verify(Arg.Any<ProfilePart>()).Returns(verifyOk);
        return p;
    }

    private static Profile ProfileWith(ProfilePart part) =>
        new() { Id = "test", Name = "Test", Part = part };

    [Fact]
    public async Task ApplyProfileAsync_happy_path_applies_and_verifies_every_affected_domain()
    {
        var powerPlan = FakeProvider("power_plan");
        var broker = new ControlBroker([powerPlan]);

        var result = await broker.ApplyProfileAsync(
            ProfileWith(new ProfilePart { PowerPlan = "balanced" }), CancellationToken.None);

        Assert.True(result.Ok);
        powerPlan.Received(1).Apply(Arg.Any<ProfilePart>());
        powerPlan.Received(1).Verify(Arg.Any<ProfilePart>());
        powerPlan.DidNotReceive().Restore(Arg.Any<Snapshot>());
    }

    [Fact]
    public async Task ApplyProfileAsync_skips_domains_with_no_registered_provider()
    {
        var broker = new ControlBroker([]); // no providers at all

        var result = await broker.ApplyProfileAsync(
            ProfileWith(new ProfilePart { PowerPlan = "balanced" }), CancellationToken.None);

        Assert.True(result.Ok); // nothing to do is not a failure.
    }

    [Fact]
    public async Task ApplyProfileAsync_rejects_up_front_without_applying_anything_when_validation_fails()
    {
        var powerPlan = FakeProvider("power_plan");
        powerPlan.Validate(Arg.Any<ProfilePart>()).Returns(ValidationResult.Failure("nope"));
        var broker = new ControlBroker([powerPlan]);

        var result = await broker.ApplyProfileAsync(
            ProfileWith(new ProfilePart { PowerPlan = "balanced" }), CancellationToken.None);

        Assert.False(result.Ok);
        powerPlan.DidNotReceive().Apply(Arg.Any<ProfilePart>());
    }

    [Fact]
    public async Task ApplyProfileAsync_rolls_back_every_already_applied_provider_in_reverse_order_on_failure()
    {
        // power_plan and cpu_limit both apply cleanly; curve_opt's Apply throws
        // (possibly after a partial write). All three must be Restored, the
        // failing one first.
        var order = new List<string>();

        var powerPlan = FakeProvider("power_plan");
        powerPlan.When(p => p.Restore(Arg.Any<Snapshot>())).Do(_ => order.Add("power_plan"));

        var cpuLimit = FakeProvider("cpu_limit");
        cpuLimit.When(p => p.Restore(Arg.Any<Snapshot>())).Do(_ => order.Add("cpu_limit"));

        var curveOpt = FakeProvider("curve_opt");
        curveOpt.When(p => p.Apply(Arg.Any<ProfilePart>())).Do(_ => throw new InvalidOperationException("locked by BIOS"));
        curveOpt.When(p => p.Restore(Arg.Any<Snapshot>())).Do(_ => order.Add("curve_opt"));

        var broker = new ControlBroker([powerPlan, cpuLimit, curveOpt]);

        var result = await broker.ApplyProfileAsync(
            ProfileWith(new ProfilePart
            {
                PowerPlan = "balanced",
                CpuLimit = new CpuLimitPart(),
                CurveOpt = System.Text.Json.Nodes.JsonValue.Create("y"),
            }),
            CancellationToken.None);

        Assert.False(result.Ok);
        Assert.Contains("locked by BIOS", result.Message);

        // Applied in fixed order (power_plan, cpu_limit, curve_opt) —
        // rollback is the reverse of what was touched.
        Assert.Equal(["curve_opt", "cpu_limit", "power_plan"], order);
    }

    [Fact]
    public async Task ApplyProfileAsync_rolls_back_when_verify_fails_even_though_apply_succeeded()
    {
        var powerPlan = FakeProvider("power_plan", verifyOk: false);
        var broker = new ControlBroker([powerPlan]);

        var result = await broker.ApplyProfileAsync(
            ProfileWith(new ProfilePart { PowerPlan = "balanced" }), CancellationToken.None);

        Assert.False(result.Ok);
        powerPlan.Received(1).Restore(Arg.Any<Snapshot>());
    }

    [Fact]
    public async Task ApplyProfileAsync_serializes_concurrent_transactions()
    {
        var concurrent = 0;
        var maxConcurrent = 0;
        var provider = Substitute.For<IControlProvider>();
        provider.Domain.Returns("power_plan");
        provider.Validate(Arg.Any<ProfilePart>()).Returns(ValidationResult.Success());
        provider.Capture().Returns(new Snapshot { Domain = "power_plan" });
        provider.Verify(Arg.Any<ProfilePart>()).Returns(true);
        provider.When(p => p.Apply(Arg.Any<ProfilePart>())).Do(_ =>
        {
            var c = Interlocked.Increment(ref concurrent);
            maxConcurrent = Math.Max(maxConcurrent, c);
            Thread.Sleep(20);
            Interlocked.Decrement(ref concurrent);
        });

        var broker = new ControlBroker([provider]);
        var profile = ProfileWith(new ProfilePart { PowerPlan = "balanced" });

        await Task.WhenAll(Enumerable.Range(0, 5)
            .Select(_ => broker.ApplyProfileAsync(profile, CancellationToken.None)));

        Assert.Equal(1, maxConcurrent);
    }

    [Fact]
    public async Task Preview_reverts_by_itself_when_not_confirmed()
    {
        var cpuLimit = FakeProvider("cpu_limit");
        var broker = new ControlBroker([cpuLimit]);
        var reverted = new TaskCompletionSource<string>();
        broker.PreviewReverted += id => reverted.TrySetResult(id);

        var result = await broker.PreviewAsync(
            ProfileWith(new ProfilePart { CpuLimit = new CpuLimitPart() }),
            TimeSpan.FromMilliseconds(50),
            CancellationToken.None);

        Assert.True(result.Ok);
        Assert.Equal("test", await reverted.Task.WaitAsync(TimeSpan.FromSeconds(5)));
        cpuLimit.Received(1).Restore(Arg.Any<Snapshot>());
        Assert.Null(await broker.EndPreviewAsync(keep: true, CancellationToken.None)); // too late.
    }

    [Fact]
    public async Task Confirmed_preview_is_kept()
    {
        var cpuLimit = FakeProvider("cpu_limit");
        var broker = new ControlBroker([cpuLimit]);
        var reverted = false;
        broker.PreviewReverted += _ => reverted = true;

        await broker.PreviewAsync(
            ProfileWith(new ProfilePart { CpuLimit = new CpuLimitPart() }),
            TimeSpan.FromMilliseconds(100),
            CancellationToken.None);
        var kept = await broker.EndPreviewAsync(keep: true, CancellationToken.None);
        await Task.Delay(300);

        Assert.Equal("test", kept?.Id);
        Assert.False(reverted);
        cpuLimit.DidNotReceive().Restore(Arg.Any<Snapshot>());
    }

    [Fact]
    public async Task Preview_can_be_undone_at_once()
    {
        var cpuLimit = FakeProvider("cpu_limit");
        var broker = new ControlBroker([cpuLimit]);

        await broker.PreviewAsync(
            ProfileWith(new ProfilePart { CpuLimit = new CpuLimitPart() }),
            TimeSpan.FromMinutes(1),
            CancellationToken.None);
        await broker.EndPreviewAsync(keep: false, CancellationToken.None);

        cpuLimit.Received(1).Restore(Arg.Any<Snapshot>());
    }

    [Fact]
    public async Task Applying_a_profile_reverts_a_pending_preview_first()
    {
        var order = new List<string>();
        var cpuLimit = FakeProvider("cpu_limit");
        cpuLimit.When(p => p.Restore(Arg.Any<Snapshot>())).Do(_ => order.Add("restore"));
        cpuLimit.When(p => p.Apply(Arg.Any<ProfilePart>())).Do(_ => order.Add("apply"));
        var broker = new ControlBroker([cpuLimit]);

        await broker.PreviewAsync(
            ProfileWith(new ProfilePart { CpuLimit = new CpuLimitPart() }),
            TimeSpan.FromMinutes(1),
            CancellationToken.None);
        await broker.ApplyProfileAsync(
            ProfileWith(new ProfilePart { CpuLimit = new CpuLimitPart() }),
            CancellationToken.None);

        Assert.Equal(["apply", "restore", "apply"], order);
    }

    [Fact]
    public async Task A_failed_preview_leaves_nothing_pending()
    {
        var cpuLimit = FakeProvider("cpu_limit", verifyOk: false);
        var broker = new ControlBroker([cpuLimit]);

        var result = await broker.PreviewAsync(
            ProfileWith(new ProfilePart { CpuLimit = new CpuLimitPart() }),
            TimeSpan.FromMinutes(1),
            CancellationToken.None);

        Assert.False(result.Ok);
        Assert.Null(await broker.EndPreviewAsync(keep: true, CancellationToken.None));
    }

    [Fact]
    public async Task A_capture_that_throws_fails_the_profile_without_touching_anything()
    {
        var powerPlan = FakeProvider("power_plan");
        var cpuLimit = FakeProvider("cpu_limit");
        cpuLimit.Capture().Returns(_ => throw new InvalidOperationException("SMU busy"));
        var broker = new ControlBroker([powerPlan, cpuLimit]);

        var result = await broker.ApplyProfileAsync(
            ProfileWith(new ProfilePart { PowerPlan = "balanced", CpuLimit = new CpuLimitPart() }),
            CancellationToken.None);

        Assert.False(result.Ok);
        Assert.Contains("SMU busy", result.Message);
        powerPlan.DidNotReceive().Apply(Arg.Any<ProfilePart>());
    }
}
