namespace SensorSidecar.Control.Lighting;

/// A headset's own settings that RIGStats can change (#428): sidetone (hear
/// your own voice) and microphone noise reduction. They live in the
/// headset's memory, like GearLink's — RIGStats reads them, writes only what
/// changed and reads them back; nothing is kept in a profile.
public sealed record HeadsetSettings(bool SidetoneOn, int SidetoneLevel, bool NoiseReductionOn, int NoiseReductionLevel);

/// What one headset model offers: sidetone 0–`SidetoneMax`, noise reduction
/// with `NoiseLevels` levels (0 = on/off only).
public sealed record HeadsetLimits(int SidetoneMax, int NoiseLevels);

/// A lighting device that also has headset settings.
public interface IHeadsetSettingsDevice
{
    string Id { get; }
    string Name { get; }

    /// Null when this model's settings aren't known.
    HeadsetLimits? Limits { get; }

    /// Whether the settings were seen working on this model.
    bool SettingsVerified { get; }

    /// The headset's settings now, or null when it doesn't answer.
    HeadsetSettings? ReadSettings();

    /// Writes what differs from the headset's current settings, reads them
    /// back and returns them; throws when the headset didn't take them.
    HeadsetSettings WriteSettings(HeadsetSettings wanted);
}

/// The GearLink "pattern 2" commands for these settings — each documented by
/// both GearLink's schema for the ROG Delta II (`sideToneStatus`,
/// `sideToneConfig`, `ecnrStatus`, `ecnrConfig`) and G-Helper's
/// `AsusHeadset` (read as documentation, no code copied). Nothing else is
/// sent. Pure, so the framing is unit-tested without a headset.
public static class HeadsetCommands
{
    public const byte Get = 0x12;
    /// Settings writes (GearLink command 65) — and the noise-reduction read,
    /// which GearLink asks with the same command.
    public const byte Set41 = 0x41;
    /// The sidetone level write (GearLink command 97).
    public const byte Set61 = 0x61;

    public const byte SidetoneStatusGet = 36;   // 12 24 → status at byte 5
    public const byte SidetoneLevelGet = 25;    // 12 19 → level at byte 5
    public const byte SidetoneSet = 17;         // 41 11 <on> / 61 11 <level>
    public const byte NoiseGet = 32;            // 41 20 → status at 5, level at 6
    public const byte NoiseStatusSet = 2;       // 41 02 <on>
    public const byte NoiseLevelSet = 16;       // 41 10 <level>

    /// The questions that read every setting, in order.
    public static readonly (byte Command, byte Key)[] Reads =
        [(Get, SidetoneStatusGet), (Get, SidetoneLevelGet), (Set41, NoiseGet)];

    /// The settings from the three replies to <see cref="Reads"/> (each
    /// starting with the report id), clamped to the model's limits; null
    /// when a reply is too short.
    public static HeadsetSettings? Parse(byte[] sidetoneStatus, byte[] sidetoneLevel, byte[] noise, HeadsetLimits limits)
    {
        if (sidetoneStatus.Length < 6 || sidetoneLevel.Length < 6 || noise.Length < 7)
            return null;
        return new HeadsetSettings(
            sidetoneStatus[5] == 1,
            Math.Min((int)sidetoneLevel[5], limits.SidetoneMax),
            noise[5] == 1,
            limits.NoiseLevels > 0 ? Math.Min((int)noise[6], limits.NoiseLevels - 1) : 0);
    }

    /// `wanted` kept inside the model's limits.
    public static HeadsetSettings Clamp(HeadsetSettings wanted, HeadsetLimits limits) => wanted with
    {
        SidetoneLevel = Math.Clamp(wanted.SidetoneLevel, 0, limits.SidetoneMax),
        NoiseReductionLevel = limits.NoiseLevels > 0 ? Math.Clamp(wanted.NoiseReductionLevel, 0, limits.NoiseLevels - 1) : 0,
    };

    /// The writes that take the headset from `now` to `wanted` (already
    /// clamped) — only for what differs, a switch before its level.
    public static IReadOnlyList<(byte Command, byte Key, byte[] Data)> Writes(HeadsetSettings now, HeadsetSettings wanted, HeadsetLimits limits)
    {
        var writes = new List<(byte, byte, byte[])>();
        if (now.SidetoneOn != wanted.SidetoneOn)
            writes.Add((Set41, SidetoneSet, [(byte)(wanted.SidetoneOn ? 1 : 0)]));
        if (now.SidetoneLevel != wanted.SidetoneLevel)
            writes.Add((Set61, SidetoneSet, [(byte)wanted.SidetoneLevel]));
        if (now.NoiseReductionOn != wanted.NoiseReductionOn)
            writes.Add((Set41, NoiseStatusSet, [(byte)(wanted.NoiseReductionOn ? 1 : 0)]));
        if (limits.NoiseLevels > 0 && now.NoiseReductionLevel != wanted.NoiseReductionLevel)
            writes.Add((Set41, NoiseLevelSet, [(byte)wanted.NoiseReductionLevel]));
        return writes;
    }
}
