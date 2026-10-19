using NSubstitute;
using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// A part skipped because its wireless device slept is applied as soon as
/// the device answers (#489), and the app is told so its footer clears.
/// </summary>
public class BrokerRetryTests
{
    private static readonly ProfilePart Part = new() { PowerPlan = "balanced", Keyboard = new KeyboardPart { Profile = 2 } };
    private static readonly Profile Balanced = new() { Id = "balanced", Name = "Balanced", Part = Part };

    private static IControlProvider Provider(string domain)
    {
        var p = Substitute.For<IControlProvider>();
        p.Domain.Returns(domain);
        p.Validate(Arg.Any<ProfilePart>()).Returns(ValidationResult.Success());
        p.Capture().Returns(new Snapshot { Domain = domain });
        p.Verify(Arg.Any<ProfilePart>()).Returns(true);
        return p;
    }

    // A keyboard asleep for the first `sleeps` tries.
    private static IControlProvider Keyboard(int sleeps)
    {
        var p = Provider("keyboard");
        var tries = 0;
        p.When(k => k.Apply(Arg.Any<ProfilePart>())).Do(_ =>
        {
            if (tries++ < sleeps)
                throw DomainUnavailableException.Silent(["ROG Azoth X"]);
        });
        return p;
    }

    [Fact]
    public async Task A_sleeping_keyboard_gets_its_part_when_it_wakes_and_the_app_is_told()
    {
        var keyboard = Keyboard(sleeps: 2);
        var broker = new ControlBroker([Provider("power_plan"), keyboard])
        {
            RetryEvery = TimeSpan.FromMilliseconds(20),
            RetryFor = TimeSpan.FromSeconds(10),
        };
        var pushed = new List<ApplyResult>();
        var caughtUp = new TaskCompletionSource();
        broker.ApplyResultChanged += r =>
        {
            lock (pushed)
                pushed.Add(r);
            if (r.Message is null)
                caughtUp.TrySetResult();
        };

        var first = await broker.ApplyProfileAsync(Balanced, CancellationToken.None);

        // The switch is answered before the keyboard is tried (#495); the
        // note that it slept is pushed, then its catching up.
        Assert.True(first.Ok);
        Assert.Null(first.Message);
        await caughtUp.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal("Profile Balanced applied; ROG Azoth X didn't answer — probably asleep — and gets the profile as soon as it wakes up.", pushed[0].Message);
        Assert.True(pushed[^1].Ok);
        keyboard.Received(3).Apply(Arg.Any<ProfilePart>());
    }

    [Fact]
    public async Task A_keyboard_that_stays_asleep_is_given_up_with_words_that_say_so()
    {
        var broker = new ControlBroker([Provider("power_plan"), Keyboard(sleeps: int.MaxValue)])
        {
            RetryEvery = TimeSpan.FromMilliseconds(20),
            RetryFor = TimeSpan.FromMilliseconds(100),
        };
        var gaveUp = new TaskCompletionSource<ApplyResult>();
        broker.ApplyResultChanged += r =>
        {
            if (r.Message?.Contains("stayed asleep", StringComparison.Ordinal) == true)
                gaveUp.TrySetResult(r);
        };

        await broker.ApplyProfileAsync(Balanced, CancellationToken.None);

        var later = await gaveUp.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.True(later.Ok);
        Assert.Contains("ROG Azoth X stayed asleep and kept its own settings", later.Message, StringComparison.Ordinal);
    }

    [Fact]
    public async Task The_switch_is_answered_before_the_slow_devices_and_power_waits_for_none_of_them()
    {
        var keyboardStarted = new TaskCompletionSource();
        var release = new TaskCompletionSource();
        var keyboard = Provider("keyboard");
        keyboard.When(k => k.Apply(Arg.Any<ProfilePart>())).Do(_ =>
        {
            keyboardStarted.TrySetResult();
            release.Task.Wait(TimeSpan.FromSeconds(5)); // a slow wireless keyboard
        });
        var power = Provider("power_plan");
        var broker = new ControlBroker([power, keyboard]);

        var answer = await broker.ApplyProfileAsync(Balanced, CancellationToken.None);

        Assert.True(answer.Ok);
        power.Received(1).Apply(Arg.Any<ProfilePart>());
        await keyboardStarted.Task.WaitAsync(TimeSpan.FromSeconds(5)); // started after the answer
        release.SetResult();
        await broker.Settled;
        keyboard.Received(1).Apply(Arg.Any<ProfilePart>());
    }

    [Fact]
    public async Task A_newer_switch_ends_the_retry()
    {
        var keyboard = Keyboard(sleeps: int.MaxValue);
        var broker = new ControlBroker([Provider("power_plan"), keyboard])
        {
            RetryEvery = TimeSpan.FromMilliseconds(30),
            RetryFor = TimeSpan.FromSeconds(10),
        };

        await broker.ApplyProfileAsync(Balanced, CancellationToken.None);
        await broker.ApplyProfileAsync(new Profile { Id = "eco", Name = "Eco", Part = new ProfilePart { PowerPlan = "power_saver" } }, CancellationToken.None);
        var before = keyboard.ReceivedCalls().Count(c => c.GetMethodInfo().Name == nameof(IControlProvider.Apply));
        await Task.Delay(200);

        Assert.Equal(before, keyboard.ReceivedCalls().Count(c => c.GetMethodInfo().Name == nameof(IControlProvider.Apply)));
    }
}
