namespace SensorSidecar.Control.Lighting;

/// A wireless device's battery, as the device reports it (#290).
public readonly record struct BatteryStatus(int Percent, bool Charging);

/// A lighting device that can also report its battery. Read-only: one
/// documented "get" per device type — see docs/control-architecture.md,
/// "Peripherals: battery and settings".
public interface IBatteryDevice
{
    /// Whether this model has a battery, from the model tables. False means
    /// the question is never sent.
    bool HasBattery { get; }

    /// The battery now, or null when the device doesn't answer (asleep, off,
    /// out of range).
    BatteryStatus? ReadBattery();
}

/// Battery reply layouts per device type, byte positions including the
/// report id — cross-checked between ASUS GearLink's device modules and
/// G-Helper. The same command number means different things per type, so
/// each type is only ever asked its own question.
public static class BatteryReplies
{
    /// Headset `12 07`: sleep timer, battery %, low-battery warning, prompt.
    public static int? HeadsetPercent(byte[] reply) => reply.Length > 6 ? Percent(reply[6]) : null;

    /// Headset `12 08`: 1 = charging.
    public static bool HeadsetCharging(byte[]? reply) => reply is { Length: > 5 } && reply[5] == 1;

    /// Headset battery from both replies; null when it reads as off.
    public static BatteryStatus? Headset(byte[] battery, byte[]? charging) =>
        HeadsetPercent(battery) is { } percent ? Status(percent, HeadsetCharging(charging)) : null;

    /// Mouse `12 07`: battery %, auto power-off, low-battery warning,
    /// voltage (2 bytes), charging (> 0).
    public static BatteryStatus? Mouse(byte[] reply) =>
        reply.Length > 10 && Percent(reply[5]) is { } percent ? Status(percent, reply[10] > 0) : null;

    /// Keyboard `12 01`: battery %, idle timeout, power saving, charging (1).
    public static BatteryStatus? Keyboard(byte[] reply) =>
        reply.Length > 9 && Percent(reply[6]) is { } percent ? Status(percent, reply[9] == 1) : null;

    // An error reply (`FF AA`) lands outside 0–100 and reads as no answer.
    private static int? Percent(byte value) => value <= 100 ? value : null;

    // A device in standby answers 0 % and not charging (G-Helper treats that
    // as "not ready") — no reading rather than a false empty battery.
    private static BatteryStatus? Status(int percent, bool charging) =>
        percent == 0 && !charging ? null : new BatteryStatus(percent, charging);
}
