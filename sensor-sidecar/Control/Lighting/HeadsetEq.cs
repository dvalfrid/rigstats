namespace SensorSidecar.Control.Lighting;

/// A headset's equalizer as the headset keeps it (#483): on or off, and ten
/// band gains (32 Hz … 16 kHz) on the model's own scale, flat in the middle.
public sealed record HeadsetEq(bool On, int[] Gains)
{
    /// Same switch and the same gains on the bands the model uses.
    public bool SameAs(HeadsetEq other, HeadsetEqLimits limits) =>
        On == other.On
        && Gains.Skip(limits.FirstBand).SequenceEqual(other.Gains.Skip(limits.FirstBand));
}

/// A preset: a name and its ten gains on the model's scale.
public sealed record HeadsetEqPreset(string Name, int[] Gains);

/// One model's equalizer: gains 0–`Max` (flat = Max / 2), the bands from
/// `FirstBand` up shown and written (the Delta II family leaves 32 and 64 Hz
/// flat, as Gear Link and G-Helper do), and its presets.
public sealed record HeadsetEqLimits(int Max, int FirstBand, IReadOnlyList<HeadsetEqPreset> Presets)
{
    public int Flat => Max / 2;
}

/// A headset whose equalizer RIGStats can change (#483).
public interface IHeadsetEqDevice
{
    string Id { get; }
    string Name { get; }

    /// Null when this model's equalizer isn't known.
    HeadsetEqLimits? EqLimits { get; }

    /// The equalizer seen working on this model.
    bool EqVerified { get; }

    /// The equalizer now, or null when the headset doesn't answer.
    HeadsetEq? ReadEq();

    /// Writes what differs, reads it back and returns it; throws when the
    /// headset didn't take it.
    HeadsetEq WriteEq(HeadsetEq wanted);
}

/// The equalizer frames of the GearLink "pattern 2" headsets with the legacy
/// schema — Gear Link's `equalizerStatus` / `equalizerConfig` for the Delta
/// II, KJP, PBZ and Pelta, and G-Helper's `AsusHeadset` agree
/// (docs/headset-eq-research.md). Nothing else is sent. Pure, so tested.
public static class HeadsetEqCommands
{
    public const byte Get = 0x12;
    public const byte Set41 = 0x41;
    public const byte StatusGet = 0x21;   // 12 21 → on at byte 5, gains at 6–15
    public const byte StatusSet = 0x03;   // 41 03 <on>
    public const byte BandSet = 0x06;     // 41 06 <band> <gain>
    public const int Bands = 10;

    /// The band frequencies, for labels.
    public static readonly string[] BandNames = ["32 Hz", "64 Hz", "125 Hz", "250 Hz", "500 Hz", "1 kHz", "2 kHz", "4 kHz", "8 kHz", "16 kHz"];

    /// `12 21` reply (report id at 0): on/off and ten gains, each kept
    /// inside the model's scale. Null when it isn't that reply.
    public static HeadsetEq? Parse(byte[]? reply, byte reportId, HeadsetEqLimits limits) =>
        reply is { Length: >= 6 + Bands } r && r[0] == reportId && r[1] == Get && r[2] == StatusGet
            ? new HeadsetEq(r[5] == 1, r.Skip(6).Take(Bands).Select(g => Math.Min((int)g, limits.Max)).ToArray())
            : null;

    /// `wanted` with ten gains inside the model's scale, the bands the model
    /// doesn't use at flat.
    public static HeadsetEq Clamp(HeadsetEq wanted, HeadsetEqLimits limits) => wanted with
    {
        Gains = Enumerable.Range(0, Bands)
            .Select(b => b < limits.FirstBand ? limits.Flat : Math.Clamp(b < wanted.Gains.Length ? wanted.Gains[b] : limits.Flat, 0, limits.Max))
            .ToArray(),
    };

    /// The writes from `now` to `wanted` (clamped): on/off first, then each
    /// band that differs on its own — G-Helper found the all-bands frame
    /// (`41 04`) ignored right after switching on, and Gear Link sends a
    /// slider's band alone too.
    public static IReadOnlyList<(byte Command, byte Key, byte[] Data)> Writes(HeadsetEq now, HeadsetEq wanted, HeadsetEqLimits limits)
    {
        var writes = new List<(byte, byte, byte[])>();
        if (now.On != wanted.On)
            writes.Add((Set41, StatusSet, [(byte)(wanted.On ? 1 : 0)]));
        for (var band = limits.FirstBand; band < Bands; band++)
        {
            if (now.Gains[band] != wanted.Gains[band])
                writes.Add((Set41, BandSet, [(byte)band, (byte)wanted.Gains[band]]));
        }
        return writes;
    }

    /// G-Helper's presets for the Delta II family (0–100, 50 flat; bands 0
    /// and 1 flat). Not ASUS's own numbers — Gear Link's presets come with
    /// its Windows audio service, not in its modules.
    public static readonly IReadOnlyList<HeadsetEqPreset> DeltaPresets =
    [
        new("Default", [50, 50, 50, 50, 50, 50, 50, 50, 50, 50]),
        new("Classic", [50, 50, 100, 100, 50, 50, 50, 50, 66, 66]),
        new("Hip hop", [50, 50, 90, 58, 40, 40, 50, 50, 82, 82]),
        new("Jazz", [50, 50, 50, 74, 74, 74, 50, 66, 82, 82]),
        new("Metal", [50, 50, 50, 50, 50, 50, 74, 50, 74, 58]),
        new("Rock", [50, 50, 66, 74, 40, 40, 50, 50, 82, 82]),
        new("Techno", [50, 50, 82, 40, 40, 32, 50, 50, 90, 90]),
        new("Vocal", [50, 50, 66, 58, 50, 50, 50, 50, 32, 8]),
    ];

    /// G-Helper's presets for the Pelta (0–24, 12 flat).
    public static readonly IReadOnlyList<HeadsetEqPreset> PeltaPresets =
    [
        new("Default", [12, 12, 12, 12, 12, 12, 12, 12, 12, 12]),
        new("Classic", [12, 12, 16, 16, 12, 12, 12, 12, 14, 14]),
        new("Hip hop", [11, 16, 17, 13, 12, 12, 12, 12, 16, 16]),
        new("Jazz", [12, 12, 12, 15, 15, 15, 12, 14, 16, 16]),
        new("Metal", [10, 12, 12, 12, 12, 12, 15, 12, 15, 13]),
        new("Rock", [11, 13, 14, 15, 11, 11, 12, 12, 16, 16]),
        new("Techno", [10, 13, 16, 11, 11, 10, 12, 12, 16, 16]),
        new("Vocal", [10, 12, 14, 13, 12, 12, 12, 12, 10, 7]),
        new("Communication", [2, 10, 10, 11, 13, 14, 14, 13, 8, 9]),
        new("Immersive", [14, 14, 15, 14, 12, 13, 13, 14, 13, 13]),
        new("Movie", [15, 16, 15, 13, 11, 14, 14, 14, 11, 11]),
        new("Music", [14, 14, 14, 12, 11, 12, 13, 14, 14, 13]),
        new("CS2", [12, 12, 12, 15, 12, 13, 14, 18, 13, 13]),
        new("FPS", [0, 8, 10, 9, 12, 13, 15, 15, 10, 10]),
    ];

    public static readonly HeadsetEqLimits Delta = new(100, 2, DeltaPresets);
    public static readonly HeadsetEqLimits Pelta = new(24, 0, PeltaPresets);

    /// The preset named `name` on this model, or null when it has none.
    public static HeadsetEqPreset? Preset(HeadsetEqLimits limits, string name) =>
        limits.Presets.FirstOrDefault(p => string.Equals(p.Name, name, StringComparison.OrdinalIgnoreCase));
}
