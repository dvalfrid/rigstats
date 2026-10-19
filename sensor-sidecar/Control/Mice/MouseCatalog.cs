namespace SensorSidecar.Control.Mice;

/// How a mouse reaches the PC: its cable, its own 2.4 GHz dongle, or the ROG
/// Omni receiver (where it is found through the receiver's paired list).
public enum MouseLink { Wired, Dongle, Omni }

/// What one ROG mouse model offers on the common protocol (#431) — every
/// setting is optional, a model without it never gets the command. Values
/// are the model's own: `Dpi` (min, max, step), `PollingHz` the rates it
/// accepts (plus 2000–8000 Hz when `Booster` and the Omni receiver reports
/// its booster on), `Debounce` the button-response codes (ms = 4 × code + 4),
/// `AngleTuning` (min, max, step) in degrees, `Acceleration`/`Deceleration`
/// their maximum (0 = not offered), `LowBatteryWarning` (step, max) in %.
public sealed record MouseSpec(
    string Name,
    int Profiles,
    int DpiStages,
    (int Min, int Max, int Step) Dpi,
    bool XyDpi,
    bool DpiColors,
    int[] PollingHz,
    bool Booster,
    (int Min, int Max)? Debounce,
    bool LiftOff,
    bool AngleSnapping,
    (int Min, int Max, int Step)? AngleTuning,
    int Acceleration,
    int Deceleration,
    bool MotionSync,
    bool AutoPowerOff,
    (int Step, int Max)? LowBatteryWarning)
{
    /// Its buttons can be bound (#433): the eight of G-Helper's default
    /// layout (`MouseProtocol.Buttons`). False for models whose layout differs.
    public bool Buttons { get; init; } = true;

    /// Where the DPI button sits, when known from the hardware or an ASUS
    /// manual (#435): "underside" or "top"; null = not known, so the Mouse
    /// page doesn't place it in its drawing.
    public string? DpiButton { get; init; }

    /// Where this model's frames differ from the common protocol (#432).
    public MouseFrames Frames { get; init; } = MouseFrames.Common;

    /// The DPI level in use can be picked from the PC (G-Helper's
    /// `CanChangeDPIProfile`); false = only the mouse's DPI button picks it.
    public bool CanSwitchStage { get; init; } = true;
}

/// How the lift-off height is read and written: `Calibration` — read at 8,
/// written as GearLink's `setCalibration` with the other fields as read;
/// `Plain` — read at 8, written `51 35 FF 00 FF <h>` (G-Helper's `AsusMouse`);
/// `HeightOnly` — read at 5, written `51 35 00 00 <h>` (G-Helper's Keris Wireless,
/// Strix Impact II Wireless, Chakram Core).
public enum LiftOffFrame { Calibration, Plain, HeightOnly }

/// Where an older ROG mouse family's frames differ from the common protocol
/// (#432) — G-Helper's packet overrides per family, transcribed (read as
/// documentation). Positions count with the report id at 0; `…Sub` is the
/// `51 31 <sub>` write. The common protocol is the default of every field.
public sealed record MouseFrames
{
    public int PollingAt { get; init; } = 13;
    public byte PollingSub { get; init; } = 0x04;
    public int DebounceAt { get; init; } = 15;
    public byte DebounceSub { get; init; } = 0x05;
    public int SnapAt { get; init; } = 17;
    public byte SnapSub { get; init; } = 0x06;

    /// `12 00 00`: the onboard profile and the active DPI stage.
    public int ProfileAt { get; init; } = 11;
    public int StageAt { get; init; } = 12;

    /// The active stage is counted from 0 on the wire (TUF Gaming M3 Gen II).
    public bool StageFromZero { get; init; }

    /// `12 07`: auto power off.
    public int PowerOffAt { get; init; } = 6;

    /// The low-battery warning is kept in quarters: 1 = 25 %.
    public bool BatteryQuarters { get; init; }

    public LiftOffFrame LiftOff { get; init; } = LiftOffFrame.Calibration;

    /// Energy written as G-Helper's `51 37 00 00 <off> 00 <low>`, not
    /// GearLink's `setPowerConfig` (which only the common models' GearLink
    /// modules document).
    public bool PlainEnergy { get; init; }

    public static readonly MouseFrames Common = new();

    /// G-Helper's base frames where they differ from GearLink's: older
    /// families without other changes (Strix Impact II, Gladius II Core).
    public static readonly MouseFrames GHelper = new() { LiftOff = LiftOffFrame.Plain, PlainEnergy = true };

    /// `12 04 00` with polling at 9, debounce at 11, angle snapping at 13,
    /// written with subs 2, 3, 4 (Gladius II, Gladius II Wireless, Strix
    /// Impact, TUF Gaming M5).
    public static readonly MouseFrames OldConfig = GHelper with
    {
        PollingAt = 9,
        PollingSub = 0x02,
        DebounceAt = 11,
        DebounceSub = 0x03,
        SnapAt = 13,
        SnapSub = 0x04,
    };

    /// OldConfig, with profile at 10, stage at 11 and power off at 5.
    public static readonly MouseFrames StrixCarry = OldConfig with { ProfileAt = 10, StageAt = 11, PowerOffAt = 5 };

    /// Polling read at 9 but written with the common sub 4 (G-Helper's Pugio).
    public static readonly MouseFrames Pugio = GHelper with { PollingAt = 9 };

    /// Low-battery warning in quarters (Pugio II, Chakram).
    public static readonly MouseFrames Quarters = GHelper with { BatteryQuarters = true };

    /// Quarters and the short lift-off frame (Keris Wireless, Strix Impact II Wireless).
    public static readonly MouseFrames QuartersShortLift = Quarters with { LiftOff = LiftOffFrame.HeightOnly };

    /// The short lift-off frame (Chakram Core).
    public static readonly MouseFrames ShortLift = GHelper with { LiftOff = LiftOffFrame.HeightOnly };

    /// The active stage from 0 (TUF Gaming M3 Gen II; otherwise as its common TUF Gaming M3).
    public static readonly MouseFrames TufM3GenII = Common with { StageFromZero = true };
}

/// One mouse as it appears to the PC: its id (the Omni receiver's paired-list
/// id for `Omni`), link, the HID interface and report id of its settings
/// channel, and its spec.
public sealed record MouseEntry(ushort ProductId, MouseLink Link, string Endpoint, byte ReportId, MouseSpec Spec);

/// Every ROG mouse on the common protocol (#431). Transcribed as facts from
/// G-Helper's mouse model files (`app/Peripherals/Mouse/Models`, read as
/// documentation, no code copied) with its base-class defaults, by a
/// resolver that follows each class's constructor chain and merges its
/// overrides; the Omni ids are G-Helper's `MouseFromOmniPid`. The commands
/// themselves are cross-checked with GearLink's module for the Harpe Ace Aim
/// Lab Edition (see `MouseProtocol`). The older families with their own
/// packet variants carry them as `MouseFrames` (#432), classified by the
/// resolver from the class right under `AsusMouse`. Regenerate by the same
/// means — never edit a row by guess.
public static class MouseCatalog
{
    private static MouseEntry Row(ushort productId, MouseLink link, string endpoint, byte reportId, MouseSpec spec) =>
        new(productId, link, endpoint, reportId, spec);

    /// Models whose G-Helper class has its own button layout with write-only
    /// slots (Chakram X joystick, Spatha X side buttons) — not bound until
    /// that layout is built (#433). Declared before `Entries`, which reads it.
    private static readonly HashSet<ushort> OwnButtonLayout = [0x1A1A, 0x1A18, 0x1979, 0x1977];

    /// Where each model's DPI button sits, seen on the hardware or in an ASUS
    /// manual (#435) — the Harpe Ace Aim Lab Edition (wireless 1A94, wired
    /// 1A92) on its underside, beside PAIR and the BT / off / 2.4 GHz switch
    /// (owner, 2026-10-18).
    private static readonly Dictionary<ushort, string> DpiButtonPlace = new()
    {
        [0x1A94] = "underside",
        [0x1A92] = "underside",
    };

    /// ASUS's own product names for G-Helper's (#446), from each model's page
    /// on rog.asus.com / asus.com (2026-10-18). G-Helper's name is kept where
    /// ASUS uses it too or no ASUS page names the model: "ROG Gladius III"
    /// (its three ids are the wired and the Wireless model, not told apart by
    /// G-Helper's class names), "TX Gaming Mouse" / "TX Gaming Mouse Mini"
    /// (China-market models, G-Helper `TUFM4WirelssCN` / `TXGamingMini`; only
    /// the casing follows ASUS's).
    private static readonly Dictionary<string, string> AsusNames = new()
    {
        ["ROG Gladius III Aimpoint"] = "ROG Gladius III Wireless AimPoint",
        ["ROG Gladius III Eva 2"] = "ROG Gladius III Wireless AimPoint EVA-02 Edition",
        ["Harpe Ace Mini"] = "ROG Harpe Ace Mini",
        ["ROG HARPE II ACE"] = "ROG Harpe II Ace",
        ["ROG Keris II Origin KJP"] = "ROG Keris II Origin (KJP)", // as OmniMouse and GearLink name it
        ["ROG Keris Wireless Aimpoint"] = "ROG Keris Wireless AimPoint",
        ["Keris Wireless Aimpoint"] = "ROG Keris Wireless AimPoint",
        ["ASUS Mouse MD200"] = "ASUS SmartO Mouse MD200 Silent Plus",
        ["Strix Evolve"] = "ROG Strix Evolve",
        ["Strix Impact III Wireless"] = "ROG Strix Impact III Wireless",
        ["TUF GAMING M3"] = "TUF Gaming M3",
        ["TUF GAMING M4 Air"] = "TUF Gaming M4 Air",
        ["TUF GAMING M4"] = "TUF Gaming M4 Wireless",
        ["TUF GAMING Mini Miku Edition"] = "ASUS TUF GAMING Mini Wireless Mouse Hatsune Miku Edition",
        ["TX GAMING MOUSE"] = "TX Gaming Mouse",
        ["TX GAMING MOUSE Mini"] = "TX Gaming Mouse Mini",
        ["Gladius II Origin"] = "ROG Gladius II Origin",
        ["Gladius II PNK LTD"] = "ROG Gladius II Origin PNK LTD",
        ["Gladius II Wireless"] = "ROG Gladius II Wireless",
        ["Strix Impact"] = "ROG Strix Impact",
        ["TUF GAMING M3 (Gen II)"] = "ASUS TUF Gaming M3 Gen II",
        ["TUF GAMING M5"] = "TUF Gaming M5",
        ["ROG Keris EVA Edition"] = "ROG Keris Wireless EVA Edition",
    };

    /// Where G-Helper gives two ASUS products one name, told apart by its
    /// class (#432): the Gladius II (`GladiusII`, named "Gladius II Origin"
    /// after its parent), the Keris Wireless (`KerisWireless` /
    /// `KerisWirelessWired`, both "ROG Keris" like the wired Keris) and the
    /// Strix Impact II Wireless (`StrixImpactIIWireless` / `…Wired`).
    /// "Gladius II Origin COD" is G-Helper's name: no ASUS page names it.
    private static readonly Dictionary<ushort, string> AsusNamesById = new()
    {
        [0x1845] = "ROG Gladius II",
        [0x1960] = "ROG Keris Wireless",
        [0x195E] = "ROG Keris Wireless",
        [0x1949] = "ROG Strix Impact II Wireless",
        [0x1947] = "ROG Strix Impact II Wireless",
    };

    public static readonly IReadOnlyList<MouseEntry> Entries = Rows().Select(e => e with
    {
        Spec = e.Spec with
        {
            Name = AsusNamesById.GetValueOrDefault(e.ProductId) ?? AsusNames.GetValueOrDefault(e.Spec.Name, e.Spec.Name),
            // The older families get no bindings until one is seen working (#432).
            Buttons = e.Spec.Buttons && !OwnButtonLayout.Contains(e.ProductId),
            DpiButton = DpiButtonPlace.GetValueOrDefault(e.ProductId),
            // Kept in quarters: G-Helper offers 10 % steps on some of these but
            // sends value / 25, so only multiples of 25 reach the mouse (#432).
            LowBatteryWarning = e.Spec.Frames.BatteryQuarters && e.Spec.LowBatteryWarning is { } lb
                ? (25, Math.Max(25, lb.Max - lb.Max % 25)) : e.Spec.LowBatteryWarning,
        },
    }).ToList();

    private static IReadOnlyList<MouseEntry> Rows() =>
    [
        Row(0x1A1A, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Chakram X", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper ChakramX
        Row(0x1A18, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Chakram X", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [250, 500, 1000, 2000, 4000, 8000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper ChakramXWired
        Row(0x1A72, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Gladius III Aimpoint", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper GladiusIIIAimpoint
        Row(0x1A70, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Gladius III Aimpoint", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper GladiusIIIAimpointWired
        Row(0x1B0C, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Gladius III Eva 2", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper GladiusIIIAimpointEva2
        Row(0x1B0A, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Gladius III Eva 2", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper GladiusIIIAimpointEva2Wired
        Row(0x1A72, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("ROG Gladius III Aimpoint", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper GladiusIIIAimpointOmni
        Row(0x197F, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Gladius III", Profiles: 5, DpiStages: 4, Dpi: (100, 26000, 50), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper GladiusIIIWireless
        Row(0x197D, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Gladius III", Profiles: 5, DpiStages: 4, Dpi: (100, 26000, 50), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper GladiusIIIWired
        Row(0x197B, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Gladius III", Profiles: 5, DpiStages: 4, Dpi: (100, 26000, 50), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null)), // G-Helper GladiusIII
        Row(0x1A94, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Harpe Ace Aim Lab Edition", Profiles: 5, DpiStages: 4, Dpi: (50, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-30, 30, 1), Acceleration: 9, Deceleration: 9, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper HarpeAceAimLabEdition
        Row(0x1A92, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Harpe Ace Aim Lab Edition", Profiles: 5, DpiStages: 4, Dpi: (50, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-30, 30, 1), Acceleration: 9, Deceleration: 9, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper HarpeAceAimLabEditionWired
        Row(0x1A94, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("ROG Harpe Ace Aim Lab Edition", Profiles: 5, DpiStages: 4, Dpi: (50, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-30, 30, 1), Acceleration: 9, Deceleration: 9, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper HarpeAceAimLabEditionOmni
        Row(0x1B67, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Harpe Ace Extreme", Profiles: 5, DpiStages: 4, Dpi: (50, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-30, 30, 1), Acceleration: 9, Deceleration: 9, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper HarpeAceExtremeWeird
        Row(0x1B68, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("ROG Harpe Ace Extreme", Profiles: 5, DpiStages: 4, Dpi: (50, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-30, 30, 1), Acceleration: 9, Deceleration: 9, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper HarpeAceExtremeOmni
        Row(0x1B69, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("ROG Harpe Ace Extreme", Profiles: 5, DpiStages: 4, Dpi: (50, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-30, 30, 1), Acceleration: 9, Deceleration: 9, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper HarpeAceExtremeOmni
        Row(0x1B63, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("Harpe Ace Mini", Profiles: 5, DpiStages: 4, Dpi: (100, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 5), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper HarpeAceMiniWired
        Row(0x1B65, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("Harpe Ace Mini", Profiles: 5, DpiStages: 4, Dpi: (100, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 5), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper HarpeAceMiniOmni
        Row(0x1C69, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG HARPE II ACE", Profiles: 5, DpiStages: 4, Dpi: (100, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000, 2000, 4000, 8000], Booster: false, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: true, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper HarpeIIAceWired
        Row(0x1AD0, MouseLink.Dongle, "mi_02&col03", 0x03, new MouseSpec("ROG HARPE II ACE", Profiles: 5, DpiStages: 4, Dpi: (100, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000, 2000, 4000, 8000], Booster: false, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: true, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper HarpeIIAceWireless
        Row(0x1B16, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Keris II Ace", Profiles: 5, DpiStages: 4, Dpi: (100, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 5), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper KerisIIAceWired
        Row(0x1B1A, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("ROG Keris II Ace", Profiles: 5, DpiStages: 4, Dpi: (100, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 5), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper KerisAceIIOmni
        Row(0x1B18, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("ROG Keris II Ace", Profiles: 5, DpiStages: 4, Dpi: (100, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 5), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper KerisAceIIOmni
        Row(0x1C0C, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Keris II Origin", Profiles: 5, DpiStages: 4, Dpi: (100, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 5), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper KerisIIOriginWired
        Row(0x1C0E, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("ROG Keris II Origin", Profiles: 5, DpiStages: 4, Dpi: (100, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 5), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper KerisIIOriginOmni
        Row(0x1D4C, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Keris II Origin KJP", Profiles: 5, DpiStages: 4, Dpi: (100, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 5), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper KerisIIOriginKJPWired
        Row(0x1D4E, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("ROG Keris II Origin KJP", Profiles: 5, DpiStages: 4, Dpi: (100, 42000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 5), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper KerisIIOriginKJPOmni
        Row(0x1A68, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Keris Wireless Aimpoint", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper KerisWirelssAimpoint
        Row(0x1A66, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Keris Wireless Aimpoint", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper KerisWirelssAimpointWired
        Row(0x1A68, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("Keris Wireless Aimpoint", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper KerisWirelssAimpointOmni
        Row(0x1A6A, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("Keris Wireless Aimpoint", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: true, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper KerisWirelssAimpointOmni
        Row(0x1A24, MouseLink.Dongle, "mi_02", 0x00, new MouseSpec("ASUS Mouse MD200", Profiles: 4, DpiStages: 2, Dpi: (100, 4200, 50), XyDpi: false, DpiColors: false, PollingHz: [125, 250], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: false, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null)), // G-Helper MD200
        Row(0x1979, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Spatha X", Profiles: 5, DpiStages: 4, Dpi: (100, 19000, 50), XyDpi: false, DpiColors: true, PollingHz: [250, 500, 1000], Booster: false, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper SpathaX
        Row(0x1977, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Spatha X", Profiles: 5, DpiStages: 4, Dpi: (100, 19000, 50), XyDpi: false, DpiColors: true, PollingHz: [250, 500, 1000], Booster: false, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper SpathaXWired
        Row(0x185B, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("Strix Evolve", Profiles: 2, DpiStages: 2, Dpi: (50, 7200, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null)), // G-Helper StrixEvolve
        Row(0x1A88, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Strix Impact III", Profiles: 3, DpiStages: 4, Dpi: (100, 12000, 50), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: null, LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null)), // G-Helper StrixImpactIII
        Row(0x1AD7, MouseLink.Omni, "mi_02&col03", 0x03, new MouseSpec("Strix Impact III Wireless", Profiles: 5, DpiStages: 4, Dpi: (100, 36000, 50), XyDpi: true, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: (-20, 20, 5), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper StrixImpactIIIWirelessOmni
        Row(0x1910, MouseLink.Wired, "mi_01", 0x00, new MouseSpec("TUF GAMING M3", Profiles: 1, DpiStages: 4, Dpi: (100, 7000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null)), // G-Helper TUFM3
        Row(0x1A03, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("TUF GAMING M4 Air", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null)), // G-Helper TUFM4Air
        Row(0x19F4, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("TUF GAMING M4", Profiles: 3, DpiStages: 4, Dpi: (100, 12000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper TUFM4Wirelss
        Row(0x1A8D, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("TX GAMING MOUSE", Profiles: 3, DpiStages: 4, Dpi: (100, 12000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper TUFM4WirelssCN
        Row(0x1AF5, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("TX GAMING MOUSE Mini", Profiles: 3, DpiStages: 4, Dpi: (100, 12000, 50), XyDpi: true, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper TXGamingMini
        Row(0x1AF3, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("TX GAMING MOUSE Mini", Profiles: 3, DpiStages: 4, Dpi: (100, 12000, 50), XyDpi: true, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper TXGamingMiniWired
        Row(0x1C57, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("TUF GAMING Mini Miku Edition", Profiles: 3, DpiStages: 4, Dpi: (100, 12000, 50), XyDpi: true, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper TUFGamingMiniMiku
        Row(0x1C56, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("TUF GAMING Mini Miku Edition", Profiles: 3, DpiStages: 4, Dpi: (100, 12000, 50), XyDpi: true, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50))), // G-Helper TUFGamingMiniMikuWired
        // #432: the older families, G-Helper rows with their frames (MouseFrames).
        Row(0x18E5, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Chakram", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (25, 100)) { Frames = MouseFrames.Quarters, CanSwitchStage = false, Buttons = false }), // G-Helper Chakram
        Row(0x18E3, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Chakram", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (25, 100)) { Frames = MouseFrames.Quarters, CanSwitchStage = false, Buttons = false }), // G-Helper ChakramWired
        Row(0x1958, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Chakram Core", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.ShortLift, CanSwitchStage = false, Buttons = false }), // G-Helper ChakramCore
        Row(0x1877, MouseLink.Wired, "mi_02", 0x00, new MouseSpec("Gladius II Origin", Profiles: 1, DpiStages: 2, Dpi: (100, 12000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.OldConfig, Buttons = false }), // G-Helper GladiusIIOrigin
        Row(0x1845, MouseLink.Wired, "mi_02", 0x00, new MouseSpec("Gladius II Origin", Profiles: 3, DpiStages: 2, Dpi: (100, 12000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.OldConfig, Buttons = false }), // G-Helper GladiusII
        Row(0x18CD, MouseLink.Wired, "mi_02", 0x00, new MouseSpec("Gladius II PNK LTD", Profiles: 3, DpiStages: 2, Dpi: (100, 12000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.OldConfig, Buttons = false }), // G-Helper GladiusIIOriginPink
        Row(0x18B1, MouseLink.Wired, "mi_02", 0x00, new MouseSpec("Gladius II Origin COD", Profiles: 1, DpiStages: 2, Dpi: (100, 12000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.OldConfig, Buttons = false }), // G-Helper GladiusIIOriginCOD
        Row(0x18DD, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Gladius II Core", Profiles: 3, DpiStages: 4, Dpi: (100, 6200, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.GHelper, Buttons = false }), // G-Helper GladiusIICore
        Row(0x18A0, MouseLink.Dongle, "mi_02", 0x00, new MouseSpec("Gladius II Wireless", Profiles: 1, DpiStages: 2, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: (-20, 20, 1), Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (25, 50)) { Frames = MouseFrames.OldConfig, Buttons = false }), // G-Helper GladiusIIWireless
        Row(0x1960, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Keris", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50)) { Frames = MouseFrames.QuartersShortLift, CanSwitchStage = false, Buttons = false }), // G-Helper KerisWireless
        Row(0x195C, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Keris", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.QuartersShortLift, CanSwitchStage = false, Buttons = false }), // G-Helper Keris
        Row(0x195E, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Keris", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50)) { Frames = MouseFrames.QuartersShortLift, CanSwitchStage = false, Buttons = false }), // G-Helper KerisWirelessWired
        Row(0x1A59, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Keris EVA Edition", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50)) { Frames = MouseFrames.QuartersShortLift, CanSwitchStage = false, Buttons = false }), // G-Helper KerisWirelessEvaEdition
        Row(0x1A57, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Keris EVA Edition", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50)) { Frames = MouseFrames.QuartersShortLift, CanSwitchStage = false, Buttons = false }), // G-Helper KerisWirelessEvaEditionWired
        Row(0x1846, MouseLink.Wired, "mi_02", 0x00, new MouseSpec("ROG Pugio", Profiles: 3, DpiStages: 2, Dpi: (50, 7200, 50), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: null, LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.Pugio, Buttons = false }), // G-Helper Pugio
        Row(0x1908, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Pugio II", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (25, 100)) { Frames = MouseFrames.Quarters, CanSwitchStage = false, Buttons = false }), // G-Helper PugioII
        Row(0x1906, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Pugio II", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (25, 100)) { Frames = MouseFrames.Quarters, CanSwitchStage = false, Buttons = false }), // G-Helper PugioIIWired
        Row(0x18B4, MouseLink.Dongle, "mi_01", 0x00, new MouseSpec("ROG Strix Carry", Profiles: 3, DpiStages: 2, Dpi: (50, 7200, 50), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: null) { Frames = MouseFrames.StrixCarry, CanSwitchStage = false, Buttons = false }), // G-Helper StrixCarry
        Row(0x1847, MouseLink.Wired, "mi_02", 0x00, new MouseSpec("Strix Impact", Profiles: 1, DpiStages: 2, Dpi: (100, 5000, 50), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: false, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.OldConfig, Buttons = false }), // G-Helper StrixImpact
        Row(0x18E1, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Strix Impact II", Profiles: 3, DpiStages: 4, Dpi: (100, 6200, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.GHelper, Buttons = false }), // G-Helper StrixImpactII
        Row(0x1956, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Strix Impact II Electro Punk", Profiles: 3, DpiStages: 4, Dpi: (100, 6200, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.GHelper, Buttons = false }), // G-Helper StrixImpactIIElectroPunk
        Row(0x19D2, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Strix Impact II Moonlight White", Profiles: 3, DpiStages: 4, Dpi: (100, 6200, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.GHelper, Buttons = false }), // G-Helper StrixImpactIIMoonlightWhite
        Row(0x1949, MouseLink.Dongle, "mi_00", 0x00, new MouseSpec("ROG Strix Impact II", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50)) { Frames = MouseFrames.QuartersShortLift, CanSwitchStage = false, Buttons = false }), // G-Helper StrixImpactIIWireless
        Row(0x1947, MouseLink.Wired, "mi_00", 0x00, new MouseSpec("ROG Strix Impact II", Profiles: 3, DpiStages: 4, Dpi: (100, 16000, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: true, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: true, LowBatteryWarning: (10, 50)) { Frames = MouseFrames.QuartersShortLift, CanSwitchStage = false, Buttons = false }), // G-Helper StrixImpactIIWirelessWired
        Row(0x1A9B, MouseLink.Wired, "mi_02", 0x00, new MouseSpec("TUF GAMING M3 (Gen II)", Profiles: 1, DpiStages: 4, Dpi: (100, 8000, 50), XyDpi: false, DpiColors: true, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.TufM3GenII }), // G-Helper TUFM3GenII
        Row(0x1898, MouseLink.Wired, "mi_02", 0x00, new MouseSpec("TUF GAMING M5", Profiles: 3, DpiStages: 2, Dpi: (100, 6200, 100), XyDpi: false, DpiColors: false, PollingHz: [125, 250, 500, 1000], Booster: false, Debounce: (2, 7), LiftOff: false, AngleSnapping: true, AngleTuning: null, Acceleration: 0, Deceleration: 0, MotionSync: false, AutoPowerOff: false, LowBatteryWarning: null) { Frames = MouseFrames.OldConfig, Buttons = false }), // G-Helper TUFM5
    ];

    /// Mice whose settings were seen working on hardware, by id and link: the
    /// Harpe Ace Aim Lab Edition on the Omni receiver (2026-10-17 — every
    /// setting and the onboard-profile switch read back as written; 2026-10-18
    /// the button bindings too: double click, disabled, back to default), and
    /// the Harpe Ace Mini on a second Omni receiver (2026-10-19 — onboard
    /// profile 1 → 2 → 1, DPI level in use, DPI level 1 400 → 800, each read
    /// back as written, #296).
    public static readonly IReadOnlySet<(ushort ProductId, MouseLink Link)> Verified =
        new HashSet<(ushort, MouseLink)> { (0x1A94, MouseLink.Omni), (0x1B65, MouseLink.Omni) };

    /// The entry for a mouse found by `productId` on `link`, or null.
    public static MouseEntry? Find(ushort productId, MouseLink link) =>
        Entries.FirstOrDefault(e => e.ProductId == productId && e.Link == link);
}
