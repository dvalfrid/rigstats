using NSubstitute;
using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// After sleep the active profile's lighting is written again when the
/// display comes back on — only the lighting, never during a preview (#670).
/// </summary>
public class LightingAfterWakeTests
{
    private static readonly Profile Balanced = new()
    {
        Id = "balanced",
        Name = "Balanced",
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
}
