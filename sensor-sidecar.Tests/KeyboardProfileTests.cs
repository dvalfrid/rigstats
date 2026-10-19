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
    public void A_profile_switched_on_the_keyboard_is_read_from_its_event()
    {
        // Fn + 1 / 2 / 3 on the Azoth X through the Omni receiver (#457).
        byte[] Event(string hex) => Convert.FromHexString(hex.PadRight(42, '0'));
        Assert.Equal(1, KeyboardProfileCommands.ProfileFromEvent(Event("0470000002"), 21));
        Assert.Equal(2, KeyboardProfileCommands.ProfileFromEvent(Event("0470000004"), 21));
        Assert.Equal(3, KeyboardProfileCommands.ProfileFromEvent(Event("0470000008"), 21));
        // Key-state reports, other bytes or several bits are not a switch.
        Assert.Null(KeyboardProfileCommands.ProfileFromEvent(Event("0471000108"), 21));
        Assert.Null(KeyboardProfileCommands.ProfileFromEvent(Event("0470000006"), 21));
        Assert.Null(KeyboardProfileCommands.ProfileFromEvent(Event("0470000001"), 21));
        Assert.Null(KeyboardProfileCommands.ProfileFromEvent(Event("0470010004"), 21));
        Assert.Null(KeyboardProfileCommands.ProfileFromEvent(null, 0));
    }

    [Fact]
    public void The_first_profile_using_an_onboard_profile_follows_it()
    {
        Profile P(string id, int? keyboard) => new() { Id = id, Name = id, Part = new ProfilePart { Keyboard = keyboard is null ? null : new KeyboardPart { Profile = keyboard } } };
        var list = new[] { P("silent", null), P("gaming", 2), P("eco", 2), P("balanced", 1) };
        Assert.Equal("gaming", KeyboardFollower.PickProfile(list, 2)?.Id);
        Assert.Equal("balanced", KeyboardFollower.PickProfile(list, 1)?.Id);
        Assert.Null(KeyboardFollower.PickProfile(list, 5));
    }

    [Fact]
    public void Events_are_listened_for_on_the_keyboards_input_only_collections()
    {
        HidDeviceInfo Hid(string mi, ushort page, int input, int output, ushort pid = 0x1ACE) =>
            new($@"\\?\hid#vid_0b05&pid_{pid:x4}&{mi}#x", 0x0B05, pid, page, 1, output, input);
        var hid = new[]
        {
            Hid("mi_00&col01", 0x0001, 9, 2),   // the system keyboard
            Hid("mi_02&col02", 0xFF00, 64, 64), // the command channel
            Hid("mi_03&col04", 0xFFC0, 21, 0),  // events (verified)
            Hid("mi_03&col01", 0x000C, 4, 0),   // consumer keys, too short
            Hid("mi_03&col04", 0xFFC0, 21, 0, pid: 0x1AFA), // another device
            Hid("mi_03", 0x0001, 16, 0),        // a second Windows keyboard collection (by cable) — never read
        };
        var found = KeyboardFollower.EventCollections([new FakeKeyboard()], hid);
        Assert.Single(found);
        Assert.Contains("mi_03&col04", found[0].Path, StringComparison.Ordinal);
        Assert.Equal(0x1ACE, found[0].ProductId);
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
    public void A_silent_keyboard_doesnt_keep_the_others_from_switching()
    {
        var asleep = new FakeKeyboard { Asleep = true };
        var awake = new FakeKeyboard();
        var provider = new KeyboardProvider(() => [asleep, awake], dryRun: false);

        var e = Assert.Throws<DeviceAsleepException>(() =>
            provider.Apply(new ProfilePart { Keyboard = new KeyboardPart { Profile = 4 } }));

        Assert.Equal(4, awake.Current);
        Assert.Contains("ROG Azoth X", e.Message, StringComparison.Ordinal);
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
        Assert.Throws<DeviceAsleepException>(() =>
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
        public ushort ProductId => 0x1ACE;
        public int Current { get; private set; } = 1;
        public bool Asleep { get; init; }
        public KeyboardProfiles? FoundProfiles => new(6, 1);
        public bool HasBattery => true;
        public bool SettingsVerified => false;
        public KeyboardSettings Settings { get; private set; } = new(2, 1, 20);

        public KeyboardSettings? ReadSettings() => Asleep ? null : Settings;

        public KeyboardSettings WriteSettings(KeyboardSettings wanted) =>
            Asleep ? throw new IOException("ROG Azoth X didn't answer.") : Settings = KeyboardSettingsCommands.Clamp(wanted);

        public KeyboardProfiles? ReadProfiles() => Asleep ? null : new KeyboardProfiles(6, Current);

        public void SwitchProfile(int profile)
        {
            if (Asleep)
                throw new IOException("ROG Azoth X didn't answer.");
            Current = Math.Clamp(profile, 1, 6);
        }
    }
}
