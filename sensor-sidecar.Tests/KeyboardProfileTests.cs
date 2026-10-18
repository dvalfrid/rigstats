using SensorSidecar.Control;
using SensorSidecar.Control.Keyboards;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Keyboard onboard profiles (#458): the <c>12 00</c> reply and the
/// <c>51 00</c> switch frame, and <see cref="KeyboardProvider"/> with a fake
/// keyboard.
/// </summary>
public class KeyboardProfileTests
{
    // The ROG Azoth X's 12 00 reply through the Omni receiver (report 2),
    // read live on 2026-10-18 (#457): 6 profiles, profile 1.
    private static readonly byte[] AzothX = Convert.FromHexString("02120000000800070006000124000100");

    [Fact]
    public void The_reply_gives_the_number_of_profiles_and_the_current_one()
    {
        Assert.Equal(new KeyboardProfiles(6, 1), KeyboardProfileCommands.Parse(AzothX, 0x02));
        Assert.Null(KeyboardProfileCommands.Parse(AzothX, 0x00)); // another report id
        Assert.Null(KeyboardProfileCommands.Parse(Convert.FromHexString("021201000000430201001443"), 0x02)); // 12 01
        Assert.Null(KeyboardProfileCommands.Parse(null, 0x02));
    }

    [Fact]
    public void A_profile_goes_out_as_g_helpers_index()
    {
        Assert.Equal("025100000002", Convert.ToHexString(KeyboardProfileCommands.Switch(0x02, 6, 2, 6)));
        Assert.Equal("025100000000", Convert.ToHexString(KeyboardProfileCommands.Switch(0x02, 6, 6, 6))); // the last is 0
        Assert.Equal(64, KeyboardProfileCommands.Switch(0x02, 64, 1, 6).Length);
    }

    [Fact]
    public void Apply_switches_every_keyboard_and_verify_reads_it_back()
    {
        var keyboard = new FakeKeyboard();
        var provider = new KeyboardProvider(() => [keyboard], dryRun: false);
        var part = new ProfilePart { Keyboard = new KeyboardPart { Profile = 3 } };

        provider.Apply(part);

        Assert.Equal(3, keyboard.Current);
        Assert.True(provider.Verify(part));
        Assert.True(provider.Affects(part));
        Assert.False(provider.Affects(new ProfilePart()));
    }

    [Fact]
    public void Dry_run_and_restore()
    {
        var keyboard = new FakeKeyboard();
        new KeyboardProvider(() => [keyboard], dryRun: true).Apply(new ProfilePart { Keyboard = new KeyboardPart { Profile = 4 } });
        Assert.Equal(1, keyboard.Current);

        var provider = new KeyboardProvider(() => [keyboard], dryRun: false);
        var before = provider.Capture();
        provider.Apply(new ProfilePart { Keyboard = new KeyboardPart { Profile = 5 } });
        provider.Restore(before);
        Assert.Equal(1, keyboard.Current);
    }

    [Fact]
    public void No_keyboard_or_no_answer_skips_the_domain()
    {
        var part = new ProfilePart { Keyboard = new KeyboardPart { Profile = 2 } };
        Assert.Throws<DomainUnavailableException>(() => new KeyboardProvider(() => [], dryRun: false).Apply(part));
        Assert.Throws<DomainUnavailableException>(() =>
            new KeyboardProvider(() => [new FakeKeyboard { Asleep = true }], dryRun: false).Apply(part));
        Assert.False(new KeyboardProvider(() => [], dryRun: false).Probe().Supported);
        // An asleep keyboard still shows: the probe uses what discovery read.
        Assert.True(new KeyboardProvider(() => [new FakeKeyboard { Asleep = true }], dryRun: false).Probe().Supported);
        Assert.False(new KeyboardProvider(() => [], dryRun: false).Validate(new ProfilePart { Keyboard = new KeyboardPart { Profile = 0 } }).Ok);
    }

    private sealed class FakeKeyboard : IKeyboardProfileDevice
    {
        public string Id => "asus-kb-1ace-02";
        public string Name => "ROG Azoth X";
        public int Current { get; private set; } = 1;
        public bool Asleep { get; init; }
        public KeyboardProfiles? FoundProfiles => new(6, 1);

        public KeyboardProfiles? ReadProfiles() => Asleep ? null : new KeyboardProfiles(6, Current);

        public void SwitchProfile(int profile)
        {
            if (Asleep)
                throw new IOException("ROG Azoth X didn't answer.");
            Current = Math.Clamp(profile, 1, 6);
        }
    }
}
