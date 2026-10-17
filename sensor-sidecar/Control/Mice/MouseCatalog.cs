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
/// Lab Edition (see `MouseProtocol`). Families with their own packet
/// variants are not here (#432). Regenerate by the same means — never edit
/// a row by guess.
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

    public static readonly IReadOnlyList<MouseEntry> Entries = Rows().Select(e => e with
    {
        Spec = e.Spec with
        {
            Buttons = !OwnButtonLayout.Contains(e.ProductId),
            DpiButton = DpiButtonPlace.GetValueOrDefault(e.ProductId),
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
    ];

    /// Mice whose settings were seen working on hardware, by id and link: the
    /// Harpe Ace Aim Lab Edition on the Omni receiver (2026-10-17 — every
    /// setting and the onboard-profile switch read back as written; 2026-10-18
    /// the button bindings too: double click, disabled, back to default).
    public static readonly IReadOnlySet<(ushort ProductId, MouseLink Link)> Verified =
        new HashSet<(ushort, MouseLink)> { (0x1A94, MouseLink.Omni) };

    /// The entry for a mouse found by `productId` on `link`, or null.
    public static MouseEntry? Find(ushort productId, MouseLink link) =>
        Entries.FirstOrDefault(e => e.ProductId == productId && e.Link == link);
}
