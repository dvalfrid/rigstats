namespace SensorSidecar.Control.Mice;

/// One DPI stage: its value and, on models with stage colours, "#rrggbb"
/// (empty otherwise).
public sealed record DpiStage(int Dpi, string Color);

/// A mouse's settings in its active onboard profile (#431). `Profile` and
/// `ActiveStage` count from 1, as the UI shows them. A setting the model
/// doesn't offer is null.
public sealed record MouseSettings(
    int Profile,
    int ActiveStage,
    IReadOnlyList<DpiStage> Stages,
    int? PollingHz,
    int? Debounce,
    bool? AngleSnapping,
    int? AngleTuning,
    int? Acceleration,
    int? Deceleration,
    bool? MotionSync,
    int? LiftOff,
    int? PowerOff,
    int? LowBatteryWarning,
    IReadOnlyList<int>? Buttons = null)
{
    /// Value equality including the stages (a record compares lists by reference).
    public bool SameAs(MouseSettings other) =>
        this with { Stages = [], Buttons = null } == other with { Stages = [], Buttons = null }
        && Stages.SequenceEqual(other.Stages)
        && (Buttons ?? []).SequenceEqual(other.Buttons ?? []);
}

/// The common ROG mouse protocol (#431) — pure, so every frame is unit-tested.
/// Frames are `[report id, command, key, index, 0, data…]`; a reply repeats
/// `command key` (and `index` for `12 04 xx`). Every command here appears in
/// G-Helper's `AsusMouse` and — unless noted — in GearLink's mouse module for
/// the Harpe Ace Aim Lab Edition (both read as documentation, 2026-10-17):
///
/// | Read | Reply bytes (with the report id at 0) |
/// |---|---|
/// | `12 00 00` | 11 onboard profile (from 0), 12 active DPI stage (from 1) |
/// | `12 04 00` | 13 polling (low 3 bits; booster: high nibble when set), 15 debounce, 17 angle snapping, 19–20 angle tuning (int16); DPI at 5 + 2·i on non-XY models |
/// | `12 04 02` | XY models: DPI X at 5 + 4·i |
/// | `12 04 03` | stage colours, R G B at 5 + 3·i |
/// | `12 04 01` | 5 acceleration, 7 deceleration (G-Helper and GearLink's reader) |
/// | `12 04 04` | 5 motion sync |
/// | `12 06` | 6 type, 7 material, 8 lift-off height, 9–11 k-values (GearLink `getCalibrationInfo`) |
/// | `12 07` | 6 auto power off, 7 low-battery warning, 11 battery type, 12 full-charge effect |
/// | `12 05 00` | button bindings: button i's action code at 5 + 2·i (#433; GearLink reads `12 05 <group>`) |
///
/// Writes: `51 31 <stage−1> 00 <dpi lo hi> [R G B]`, `51 31 09 00 <stage>`,
/// `51 31 04 00 <rate>`, `05` debounce, `06` angle snapping, `0B` angle
/// tuning (int16), `07`/`08` acceleration / deceleration (G-Helper only —
/// GearLink's module reads them but has no writer), `12` motion sync;
/// `51 35 <type> 00 <material> <height> 00 <k0 k1 k2>` (GearLink's
/// `setCalibration`: what was read, with the new height — G-Helper sends
/// `FF` instead), `51 37 00 00 <sleep> 00 <low> <battery type> <full-charge>`
/// (GearLink's `setPowerConfig`: the last two as read — G-Helper leaves them
/// 0), `51 21 00 00 <button lo hi> <action lo hi>` to bind a button (G-Helper
/// and GearLink's `mapButton`, #433), `50 02 <profile−1>` to switch profile
/// and `50 03` to save.
public static class MouseProtocol
{
    public const byte Get = 0x12;
    public const byte Set = 0x51;
    public const byte Profile = 0x50;

    /// Polling-rate codes 0–6.
    public static readonly int[] PollingCodes = [125, 250, 500, 1000, 2000, 4000, 8000];

    /// Auto-power-off codes: 0 → 1 min, 1 → 2, 2 → 3, 3 → 5, 4 → 10, 0xFF → never.
    public static readonly IReadOnlyDictionary<int, int?> PowerOffMinutes = new Dictionary<int, int?>
    {
        [0] = 1,
        [1] = 2,
        [2] = 3,
        [3] = 5,
        [4] = 10,
        [0xFF] = null,
    };

    /// The Omni receiver's booster question on the mouse's channel (G-Helper's
    /// `DedectOmniMouse`; GearLink asks `7D 20` too): booster on when byte 5 is 1.
    public static readonly byte[] BoosterQuestion = [0x7D, 0x20, 0x02];

    public static readonly byte[] Save = [Profile, 0x03];

    /// The buttons that can be bound (#433), in the order `12 05 00` reports
    /// them (code at 5 + 2·i, little-endian) — G-Helper's default layout. Each
    /// is identified by its own action code, which is also its default.
    public static readonly IReadOnlyList<(int Source, string Name)> Buttons =
    [
        (0x01F0, "Left click"), (0x01F1, "Right click"), (0x01F2, "Scroll click"),
        (0x01E4, "Side back"), (0x01E5, "Side forward"), (0x01E6, "DPI button"),
        (0x01E8, "Scroll up"), (0x01E9, "Scroll down"),
    ];

    /// What a button can be bound to — the mouse functions whose codes
    /// GearLink's `MS_…` table and G-Helper's `MouseBindings` agree on, and
    /// "disabled" (G-Helper's 0). Keys and media come once a code is seen
    /// working (#433).
    public static readonly IReadOnlyList<(int Code, string Name)> Actions =
    [
        (0x01F0, "Left click"), (0x01F1, "Right click"), (0x01F2, "Middle click"),
        (0x01E3, "Double click"), (0x01E4, "Back"), (0x01E5, "Forward"),
        (0x01E8, "Scroll up"), (0x01E9, "Scroll down"), (0x0000, "Disabled"),
    ];

    /// Whether button `slot` may be bound to `code`: a known action, or the
    /// button's own default (the DPI button's 0x01E6 is not an action).
    public static bool IsAction(int slot, int code) =>
        Actions.Any(a => a.Code == code) || (slot < Buttons.Count && Buttons[slot].Source == code);

    /// The questions that read `spec`'s settings, as `[command, key, index]`.
    public static IReadOnlyList<byte[]> Reads(MouseSpec spec)
    {
        var reads = new List<byte[]> { new byte[] { Get, 0x00, 0x00 }, new byte[] { Get, 0x04, 0x00 } };
        if (spec.XyDpi)
            reads.Add([Get, 0x04, 0x02]);
        if (spec.DpiColors)
            reads.Add([Get, 0x04, 0x03]);
        if (spec.Acceleration > 0 || spec.Deceleration > 0)
            reads.Add([Get, 0x04, 0x01]);
        if (spec.MotionSync)
            reads.Add([Get, 0x04, 0x04]);
        if (spec.LiftOff)
            reads.Add([Get, 0x06, 0x00]);
        if (spec.AutoPowerOff || spec.LowBatteryWarning is not null)
            reads.Add([Get, 0x07, 0x00]);
        if (spec.Buttons)
            reads.Add([Get, 0x05, 0x00]);
        return reads;
    }

    /// Whether `reply` answers `question`. A read's reply repeats all three
    /// bytes: the mouse also sends reports of its own that start like one —
    /// `12 00 09 00 01` arrived while `12 00 00` was asked and was read as
    /// DPI stage 1 (seen live on the Harpe Ace over Omni, #442), while every
    /// real reply there repeats its third byte.
    public static bool Answers(byte[] reply, byte reportId, byte[] question) =>
        reply.Length >= 4 && reply[0] == reportId && reply[1] == question[0] && reply[2] == question[1]
        && (question[0] != Get || question.Length < 3 || reply[3] == question[2]);

    /// The polling rates this mouse accepts now.
    public static int[] Rates(MouseSpec spec, bool booster) =>
        spec.Booster && booster ? [.. spec.PollingHz, 2000, 4000, 8000] : spec.PollingHz;

    /// The settings from the replies to <see cref="Reads"/> (keyed by
    /// `command key index` in hex), or null when one is missing or short.
    public static MouseSettings? Parse(MouseSpec spec, IReadOnlyDictionary<string, byte[]> replies, bool booster)
    {
        byte[]? R(byte key, byte index) =>
            replies.TryGetValue(Key([Get, key, index]), out var r) && r.Length >= 21 ? r : null;
        if (R(0x00, 0x00) is not { } profile || R(0x04, 0x00) is not { } main)
            return null;
        var xy = spec.XyDpi ? R(0x04, 0x02) : main;
        var colors = spec.DpiColors ? R(0x04, 0x03) : null;
        if (xy is null || (spec.DpiColors && colors is null))
            return null;

        var stages = new List<DpiStage>();
        for (var i = 0; i < spec.DpiStages; i++)
        {
            var at = 5 + i * (spec.XyDpi ? 4 : 2);
            if (at + 1 >= xy.Length)
                return null;
            var dpi = (xy[at] | (xy[at + 1] << 8)) * spec.Dpi.Step + spec.Dpi.Step;
            var color = colors is null ? "" : $"#{colors[5 + i * 3]:x2}{colors[6 + i * 3]:x2}{colors[7 + i * 3]:x2}";
            stages.Add(new DpiStage(dpi, color));
        }

        var f = spec.Frames;
        int? polling = null;
        if (spec.PollingHz.Length > 0)
        {
            var raw = main[f.PollingAt];
            var high = (raw >> 4) & 0x0F;
            // The older families keep the plain code (G-Helper reads it as is).
            var code = f.PollingAt != MouseFrames.Common.PollingAt ? raw
                : booster && spec.Booster && high > 0 ? high : raw & 0x07;
            polling = code < PollingCodes.Length ? PollingCodes[code] : null;
        }

        var accel = spec.Acceleration > 0 || spec.Deceleration > 0 ? R(0x04, 0x01) : null;
        var motion = spec.MotionSync ? R(0x04, 0x04) : null;
        var lift = spec.LiftOff ? R(0x06, 0x00) : null;
        var power = spec.AutoPowerOff || spec.LowBatteryWarning is not null ? R(0x07, 0x00) : null;
        if ((spec.Acceleration > 0 || spec.Deceleration > 0) && accel is null
            || spec.MotionSync && motion is null || spec.LiftOff && lift is null
            || (spec.AutoPowerOff || spec.LowBatteryWarning is not null) && power is null)
            return null;

        IReadOnlyList<int>? buttons = null;
        if (spec.Buttons)
        {
            if (R(0x05, 0x00) is not { } b)
                return null;
            buttons = Enumerable.Range(0, Buttons.Count).Select(i => b[5 + 2 * i] | (b[6 + 2 * i] << 8)).ToList();
        }

        return new MouseSettings(
            Profile: profile[f.ProfileAt] + 1,
            ActiveStage: Math.Clamp(profile[f.StageAt] + (f.StageFromZero ? 1 : 0), 1, spec.DpiStages),
            Stages: stages,
            PollingHz: polling,
            Debounce: spec.Debounce is not null ? main[f.DebounceAt] : null,
            AngleSnapping: spec.AngleSnapping ? main[f.SnapAt] == 1 : null,
            AngleTuning: spec.AngleTuning is not null ? (short)(main[19] | (main[20] << 8)) : null,
            Acceleration: spec.Acceleration > 0 ? accel![5] : null,
            Deceleration: spec.Deceleration > 0 ? accel![7] : null,
            MotionSync: spec.MotionSync ? motion![5] == 1 : null,
            LiftOff: spec.LiftOff ? lift![f.LiftOff == LiftOffFrame.HeightOnly ? 5 : 8] : null,
            PowerOff: spec.AutoPowerOff ? power![f.PowerOffAt] : null,
            LowBatteryWarning: spec.LowBatteryWarning is not null ? power![7] * (f.BatteryQuarters ? 25 : 1) : null,
            Buttons: buttons);
    }

    /// `wanted` kept inside `spec`'s limits; settings the model lacks stay as
    /// they are in `now`. Profile and stage count can't change here.
    public static MouseSettings Clamp(MouseSpec spec, MouseSettings now, MouseSettings wanted, bool booster)
    {
        int Dpi(int v) => Math.Clamp((int)Math.Round(v / (double)spec.Dpi.Step) * spec.Dpi.Step, spec.Dpi.Min, spec.Dpi.Max);
        string Color(string c, string fallback) =>
            spec.DpiColors && c.Length == 7 && c[0] == '#' && int.TryParse(c.AsSpan(1), System.Globalization.NumberStyles.HexNumber, null, out _)
                ? c.ToLowerInvariant() : fallback;
        var stages = now.Stages.Select((s, i) => i < wanted.Stages.Count
            ? new DpiStage(Dpi(wanted.Stages[i].Dpi), Color(wanted.Stages[i].Color, s.Color))
            : s).ToList();
        var rates = Rates(spec, booster);
        return now with
        {
            ActiveStage = spec.CanSwitchStage ? Math.Clamp(wanted.ActiveStage, 1, spec.DpiStages) : now.ActiveStage,
            Stages = stages,
            PollingHz = wanted.PollingHz is { } hz && rates.Contains(hz) ? hz : now.PollingHz,
            Debounce = spec.Debounce is { } d && wanted.Debounce is { } w ? Math.Clamp(w, d.Min, d.Max) : now.Debounce,
            AngleSnapping = spec.AngleSnapping ? wanted.AngleSnapping ?? now.AngleSnapping : now.AngleSnapping,
            AngleTuning = spec.AngleTuning is { } a && wanted.AngleTuning is { } t
                ? Math.Clamp((int)Math.Round(t / (double)a.Step) * a.Step, a.Min, a.Max) : now.AngleTuning,
            Acceleration = spec.Acceleration > 0 && wanted.Acceleration is { } acc ? Math.Clamp(acc, 0, spec.Acceleration) : now.Acceleration,
            Deceleration = spec.Deceleration > 0 && wanted.Deceleration is { } dec ? Math.Clamp(dec, 0, spec.Deceleration) : now.Deceleration,
            MotionSync = spec.MotionSync ? wanted.MotionSync ?? now.MotionSync : now.MotionSync,
            LiftOff = spec.LiftOff && wanted.LiftOff is 0 or 1 ? wanted.LiftOff : now.LiftOff,
            PowerOff = spec.AutoPowerOff && wanted.PowerOff is { } p && PowerOffMinutes.ContainsKey(p) ? p : now.PowerOff,
            LowBatteryWarning = spec.LowBatteryWarning is { } lb && wanted.LowBatteryWarning is { } l
                ? Math.Clamp((int)Math.Round(l / (double)lb.Step) * lb.Step, lb.Step, lb.Max) : now.LowBatteryWarning,
            Buttons = spec.Buttons && now.Buttons is { } nowButtons && wanted.Buttons is { } wantedButtons
                ? nowButtons.Select((b, i) => i < wantedButtons.Count && i < Buttons.Count && IsAction(i, wantedButtons[i]) ? wantedButtons[i] : b).ToList()
                : now.Buttons,
        };
    }

    /// The writes that take the mouse from `now` to `wanted` (already
    /// clamped), as `[command, key, index, 0, data…]` without the report id —
    /// only for what differs. `liftReply` / `powerReply` are the `12 06` /
    /// `12 07` replies, whose other fields are written back as they were.
    public static IReadOnlyList<byte[]> Writes(MouseSpec spec, MouseSettings now, MouseSettings wanted, byte[]? liftReply, byte[]? powerReply)
    {
        var writes = new List<byte[]>();
        var f = spec.Frames;
        byte[] Sub(byte sub, params byte[] data) => [Set, 0x31, sub, 0x00, .. data];
        for (var i = 0; i < Math.Min(now.Stages.Count, wanted.Stages.Count); i++)
        {
            if (now.Stages[i] == wanted.Stages[i])
                continue;
            var code = (wanted.Stages[i].Dpi - spec.Dpi.Step) / spec.Dpi.Step;
            byte[] dpi = [(byte)(code & 0xFF), (byte)((code >> 8) & 0xFF)];
            writes.Add(spec.DpiColors ? Sub((byte)i, [.. dpi, .. Rgb(wanted.Stages[i].Color)]) : Sub((byte)i, dpi));
        }
        if (now.ActiveStage != wanted.ActiveStage)
            writes.Add(Sub(0x09, (byte)(wanted.ActiveStage - (f.StageFromZero ? 1 : 0))));
        if (wanted.PollingHz is { } hz && hz != now.PollingHz)
            writes.Add(Sub(f.PollingSub, (byte)Array.IndexOf(PollingCodes, hz)));
        if (wanted.Debounce is { } d && d != now.Debounce)
            writes.Add(Sub(f.DebounceSub, (byte)d));
        if (wanted.AngleSnapping is { } snap && snap != now.AngleSnapping)
            writes.Add(Sub(f.SnapSub, (byte)(snap ? 1 : 0)));
        if (wanted.AngleTuning is { } angle && angle != now.AngleTuning)
            writes.Add(Sub(0x0B, (byte)(angle & 0xFF), (byte)((angle >> 8) & 0xFF)));
        if (wanted.Acceleration is { } acc && acc != now.Acceleration)
            writes.Add(Sub(0x07, (byte)acc));
        if (wanted.Deceleration is { } dec && dec != now.Deceleration)
            writes.Add(Sub(0x08, (byte)dec));
        if (wanted.MotionSync is { } sync && sync != now.MotionSync)
            writes.Add(Sub(0x12, (byte)(sync ? 1 : 0)));
        if (wanted.LiftOff is { } height && height != now.LiftOff)
        {
            if (f.LiftOff == LiftOffFrame.Plain)
                writes.Add([Set, 0x35, 0xFF, 0x00, 0xFF, (byte)height]);
            else if (f.LiftOff == LiftOffFrame.HeightOnly)
                writes.Add([Set, 0x35, 0x00, 0x00, (byte)height]);
            else if (liftReply is { Length: >= 12 } lift)
                writes.Add([Set, 0x35, lift[6], 0x00, lift[7], (byte)height, 0x00, lift[9], lift[10], lift[11]]);
        }
        if (wanted.PowerOff != now.PowerOff || wanted.LowBatteryWarning != now.LowBatteryWarning)
        {
            if (f.PlainEnergy)
                writes.Add([Set, 0x37, 0x00, 0x00, (byte)(wanted.PowerOff ?? 0), 0x00,
                    (byte)((wanted.LowBatteryWarning ?? 0) / (f.BatteryQuarters ? 25 : 1))]);
            else if (powerReply is { Length: >= 13 } power)
                writes.Add([Set, 0x37, 0x00, 0x00, (byte)(wanted.PowerOff ?? power[6]), 0x00,
                    (byte)(wanted.LowBatteryWarning ?? power[7]), power[11], power[12]]);
        }
        if (wanted.Buttons is { } wantedButtons && now.Buttons is { } nowButtons)
        {
            for (var i = 0; i < Math.Min(Buttons.Count, Math.Min(wantedButtons.Count, nowButtons.Count)); i++)
            {
                if (wantedButtons[i] == nowButtons[i])
                    continue;
                var (source, dest) = (Buttons[i].Source, wantedButtons[i]);
                writes.Add([Set, 0x21, 0x00, 0x00, (byte)(source & 0xFF), (byte)(source >> 8), (byte)(dest & 0xFF), (byte)(dest >> 8)]);
            }
        }
        return writes;
    }

    /// `[50 02 <profile−1>]` — switch the onboard profile (1-based in).
    public static byte[] SwitchProfile(int profile) => [Profile, 0x02, (byte)(profile - 1)];

    /// The reply key for a question: "12 04 02".
    public static string Key(byte[] question) => Convert.ToHexString(question.AsSpan(0, Math.Min(3, question.Length)));

    private static byte[] Rgb(string color) => color.Length == 7
        ? [Convert.ToByte(color[1..3], 16), Convert.ToByte(color[3..5], 16), Convert.ToByte(color[5..7], 16)]
        : [0, 0, 0];
}
