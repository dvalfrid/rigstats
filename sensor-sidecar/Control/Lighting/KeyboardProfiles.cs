namespace SensorSidecar.Control.Lighting;

/// A keyboard's onboard profiles (#458): how many it has and which is in use,
/// both counted from 1 as the keyboard and its Fn + 1…6 keys count.
public sealed record KeyboardProfiles(int Count, int Current);

/// A lighting device that is a keyboard with onboard profiles.
public interface IKeyboardProfileDevice
{
    string Id { get; }
    string Name { get; }

    /// Its onboard profiles as discovery read them (no device I/O); null when
    /// that reply didn't carry them.
    KeyboardProfiles? FoundProfiles { get; }

    /// Its onboard profiles now, or null when it doesn't answer.
    KeyboardProfiles? ReadProfiles();

    /// Switches to onboard profile `profile` (1-based) and confirms it.
    void SwitchProfile(int profile);
}

/// The onboard-profile frames of ASUS keyboards — Gear Link's
/// `getKeyboardDeviceInfo` / `changeProfile` and G-Helper's `AsusKeyboard`
/// agree (docs/keyboard-research.md). Pure, so they are unit-tested.
public static class KeyboardProfileCommands
{
    /// `12 00` reply (report id at 0): byte 9 = number of profiles, byte 11 =
    /// the current one (1-based; verified on the ROG Azoth X: 6 profiles,
    /// profile 1, 2026-10-18). Null when it isn't that reply or is out of range.
    public static KeyboardProfiles? Parse(byte[]? reply, byte reportId) =>
        reply is { Length: >= 12 } r && r[0] == reportId && r[1] == 0x12 && r[2] == 0x00 && r[3] == 0x00
        && r[9] is >= 1 and <= 16 && r[11] >= 1 && r[11] <= r[9]
            ? new KeyboardProfiles(r[9], r[11])
            : null;

    /// `51 00 00 00 <p>` switches the onboard profile; the keyboard echoes the
    /// frame (Gear Link's switch to 2 and 1 on the Azoth X through the Omni
    /// receiver: `02 51 00 00 00 02`, then `12 00` byte 11 = 2, 2026-10-18). G-Helper sends its
    /// own index 0–5, where the keyboard's last profile (6) is 0 — so
    /// profile n goes out as n mod count. To be confirmed on hardware.
    public static byte[] Switch(byte reportId, int length, int profile, int count)
    {
        var report = new byte[Math.Max(length, 6)];
        report[0] = reportId;
        report[1] = 0x51;
        report[5] = (byte)(profile % count);
        return report;
    }
}
