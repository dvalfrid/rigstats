using NSubstitute;
using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// After sleep the active profile's lighting is written again when the
/// display comes back on — only the lighting, never during a preview (#670).
/// </summary>
public sealed class LightingAfterWakeTests : IDisposable
{
    private readonly string _dir = Path.Combine(Path.GetTempPath(), "rigstats-wake-tests-" + Guid.NewGuid());
    private readonly ProfileStore _store;

    public LightingAfterWakeTests()
    {
        Directory.CreateDirectory(_dir);
        _store = new ProfileStore(Path.Combine(_dir, "profiles.json"));
    }

    public void Dispose()
    {
        try { Directory.Delete(_dir, recursive: true); } catch { }
    }

    private static readonly Profile Balanced = new()
    {
        Id = "wake-test",
        Name = "Wake test",
        Part = new ProfilePart
        {
            PowerPlan = "balanced",
            Keyboard = new KeyboardPart { Profile = 2 },
            Aura = new AuraPart { Effect = "static", Color = "#0000ff" },
        },
    };

    private static IControlProvider Provider(string domain)
    {
        var p = Substitute.For<IControlProvider>();
        p.Domain.Returns(domain);
        p.Validate(Arg.Any<ProfilePart>()).Returns(ValidationResult.Success());
        p.Capture().Returns(new Snapshot { Domain = domain });
        p.Verify(Arg.Any<ProfilePart>()).Returns(true);
        return p;
    }

    [Theory]
    [InlineData(null, 1, false)] // the state at registration
    [InlineData(0, 1, true)]
    [InlineData(2, 1, false)] // dimmed never lost power
    [InlineData(1, 1, false)]
    [InlineData(1, 0, false)]
    public void Only_off_to_on_counts(int? before, int now, bool expected) =>
        Assert.Equal(expected, LightingAfterWake.CameOn(before, now));

    [Fact]
    public async Task Only_the_lighting_is_written_again()
    {
        var power = Provider("power_plan");
        var keyboard = Provider("keyboard");
        var aura = Provider("aura");
        var broker = new ControlBroker([power, keyboard, aura]);

        var result = await broker.ReapplyLightingAsync(Balanced, CancellationToken.None);

        Assert.True(result!.Ok);
        aura.Received(1).Apply(Arg.Any<ProfilePart>());
        power.DidNotReceive().Apply(Arg.Any<ProfilePart>());
        keyboard.DidNotReceive().Apply(Arg.Any<ProfilePart>());
    }

    [Fact]
    public async Task A_keyboard_not_awake_yet_gets_the_lighting_when_it_answers()
    {
        var aura = Provider("aura");
        var tries = 0;
        aura.When(a => a.Apply(Arg.Any<ProfilePart>())).Do(_ =>
        {
            if (tries++ < 2)
                throw DomainUnavailableException.Silent(["ROG Azoth X"]);
        });
        var broker = new ControlBroker([aura])
        {
            RetryEvery = TimeSpan.FromMilliseconds(20),
            RetryFor = TimeSpan.FromSeconds(10),
        };
        var caughtUp = new TaskCompletionSource();
        broker.ApplyResultChanged += r =>
        {
            if (r.Ok && r.Message is null)
                caughtUp.TrySetResult();
        };

        var result = await broker.ReapplyLightingAsync(Balanced, CancellationToken.None);

        Assert.True(result!.Ok);
        await caughtUp.Task.WaitAsync(TimeSpan.FromSeconds(5));
        aura.Received(3).Apply(Arg.Any<ProfilePart>());
    }

    [Fact]
    public async Task A_pending_preview_keeps_its_lighting()
    {
        var aura = Provider("aura");
        var broker = new ControlBroker([aura]);
        await broker.PreviewAsync(Balanced, TimeSpan.FromMinutes(1), CancellationToken.None);
        aura.ClearReceivedCalls();

        var result = await broker.ReapplyLightingAsync(Balanced, CancellationToken.None);

        Assert.Null(result);
        aura.DidNotReceive().Apply(Arg.Any<ProfilePart>());
        await broker.EndPreviewAsync(keep: false, CancellationToken.None);
    }

    // Three writes a few milliseconds apart stand in for 3, 10 and 30 s.
    private LightingAfterWake Wake(ControlBroker broker) => new(_store, broker)
    {
        WriteAt = [TimeSpan.FromMilliseconds(10), TimeSpan.FromMilliseconds(20), TimeSpan.FromMilliseconds(30)],
    };

    private async Task MakeActiveAsync()
    {
        Assert.True(await _store.SaveProfileAsync(Balanced, CancellationToken.None));
        await _store.SetActiveAsync(Balanced.Id, CancellationToken.None);
    }

    [Fact]
    public async Task The_lighting_is_written_at_each_time_after_display_on()
    {
        await MakeActiveAsync();
        var aura = Provider("aura");
        var broker = new ControlBroker([aura]);

        await Wake(broker).WriteAfterWakeAsync(CancellationToken.None);

        aura.Received(3).Apply(Arg.Any<ProfilePart>());
    }

    [Fact]
    public async Task A_profile_applied_in_between_ends_the_writes()
    {
        await MakeActiveAsync();
        var aura = Provider("aura");
        var broker = new ControlBroker([aura]);
        var firstWrite = new TaskCompletionSource();
        aura.When(a => a.Apply(Arg.Any<ProfilePart>())).Do(_ => firstWrite.TrySetResult());
        // Wide gaps after the first write, so the switch is surely in between.
        var wake = new LightingAfterWake(_store, broker)
        {
            WriteAt = [TimeSpan.FromMilliseconds(10), TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(2)],
        };

        var writes = wake.WriteAfterWakeAsync(CancellationToken.None);
        await firstWrite.Task.WaitAsync(TimeSpan.FromSeconds(5));
        await broker.ApplyProfileAsync(Balanced, CancellationToken.None);
        await broker.Settled;
        await writes;

        // The wake's first write and the switch's own; not the wake's later two.
        aura.Received(2).Apply(Arg.Any<ProfilePart>());
    }

    [Fact]
    public async Task Nothing_is_written_without_lighting_in_the_active_profile()
    {
        var noLighting = new Profile
        {
            Id = "wake-test",
            Name = "Wake test",
            Part = new ProfilePart { PowerPlan = "balanced" },
        };
        Assert.True(await _store.SaveProfileAsync(noLighting, CancellationToken.None));
        await _store.SetActiveAsync(noLighting.Id, CancellationToken.None);
        var aura = Provider("aura");

        await Wake(new ControlBroker([aura])).WriteAfterWakeAsync(CancellationToken.None);

        aura.DidNotReceive().Apply(Arg.Any<ProfilePart>());
    }
}
