using SensorSidecar.Control.Lighting;

namespace SensorSidecar.Control.Mice;

/// The settings channel of a ROG mouse (#431): what `MouseProtocol` asks and
/// writes, over the mouse's own collection (cable or dongle) or the Omni
/// receiver's mouse channel. Every change is read back and saved in the
/// mouse (`50 03`); the mouse keeps it without RIGStats.
public interface IMouseDevice
{
    string Id { get; }
    MouseEntry Entry { get; }

    /// The Omni receiver's booster is on (2000–8000 Hz offered).
    bool Booster { get; }

    /// The mouse's settings now, or null when it doesn't answer (asleep, gone).
    MouseSettings? Read();

    /// Writes what differs from the mouse's current settings, saves, reads
    /// them back and returns them; throws when the mouse didn't take them.
    MouseSettings Write(MouseSettings wanted);

    /// Switches the onboard profile (1-based) and confirms it.
    void SwitchProfile(int profile);
}

public sealed class AsusMouseDevice : IMouseDevice, IDisposable
{
    private static readonly TimeSpan ReplyTimeout = TimeSpan.FromMilliseconds(600);

    private readonly object _lock = new();
    private readonly HidDeviceInfo _channel;
    private readonly byte _reportId;
    private IHidDevice? _device;

    public string Id { get; }
    public MouseEntry Entry { get; }
    public bool Booster { get; }

    public AsusMouseDevice(HidDeviceInfo channel, MouseEntry entry, string id)
    {
        _channel = channel;
        _reportId = entry.ReportId;
        Entry = entry;
        Id = id;
        // Only the Omni receiver says whether its booster is on (G-Helper asks
        // there alone); elsewhere the model's base rates apply.
        if (entry.Link == MouseLink.Omni && entry.Spec.Booster)
        {
            lock (_lock)
                Booster = Ask(MouseProtocol.BoosterQuestion) is { Length: > 5 } reply && reply[5] == 1;
        }
    }

    public MouseSettings? Read()
    {
        lock (_lock)
            return ReadLocked().Settings;
    }

    public MouseSettings Write(MouseSettings wanted)
    {
        lock (_lock)
        {
            var (now, replies) = ReadLocked();
            if (now is null)
                throw new IOException($"{Entry.Spec.Name} didn't answer.");
            wanted = MouseProtocol.Clamp(Entry.Spec, now, wanted, Booster);
            var writes = MouseProtocol.Writes(Entry.Spec, now, wanted,
                replies.GetValueOrDefault(MouseProtocol.Key([MouseProtocol.Get, 0x06, 0x00])),
                replies.GetValueOrDefault(MouseProtocol.Key([MouseProtocol.Get, 0x07, 0x00])));
            if (writes.Count == 0)
                return now;
            foreach (var write in writes)
                Ask(write); // the acknowledgement; the read-back below decides
            Ask(MouseProtocol.Save);
            var back = ReadLocked().Settings ?? throw new IOException($"{Entry.Spec.Name} didn't answer after the change.");
            if (!back.SameAs(wanted))
                throw new IOException($"{Entry.Spec.Name} did not take the settings (read back {Describe(back)}).");
            return back;
        }
    }

    public void SwitchProfile(int profile)
    {
        lock (_lock)
        {
            profile = Math.Clamp(profile, 1, Entry.Spec.Profiles);
            Ask(MouseProtocol.SwitchProfile(profile));
            Ask(MouseProtocol.Save);
            var back = Ask([MouseProtocol.Get, 0x00, 0x00]);
            if (back is not { Length: > 11 } || back[11] + 1 != profile)
                throw new IOException($"{Entry.Spec.Name} did not switch to profile {profile}.");
        }
    }

    /// "dpi 400/800/1600/3200 #1, 1000 Hz, …" for the log.
    public static string Describe(MouseSettings s) =>
        $"profile {s.Profile}, dpi {string.Join("/", s.Stages.Select(x => x.Dpi))} (stage {s.ActiveStage})"
        + (s.PollingHz is { } hz ? $", {hz} Hz" : "")
        + (s.Debounce is { } d ? $", response {4 * d + 4} ms" : "")
        + (s.AngleSnapping is { } snap ? $", angle snapping {(snap ? "on" : "off")}" : "")
        + (s.AngleTuning is { } a ? $", angle {a}°" : "")
        + (s.Acceleration is { } acc ? $", acceleration {acc}" : "")
        + (s.Deceleration is { } dec ? $", deceleration {dec}" : "")
        + (s.MotionSync is { } m ? $", motion sync {(m ? "on" : "off")}" : "")
        + (s.LiftOff is { } l ? $", lift-off {(l == 0 ? "low" : "high")}" : "")
        + (s.PowerOff is { } p ? $", power off {p}" : "")
        + (s.LowBatteryWarning is { } lb ? $", low battery {lb} %" : "")
        + (s.Stages.Any(x => x.Color.Length > 0) ? $", colours {string.Join("/", s.Stages.Select(x => x.Color))}" : "")
        + (s.Buttons is { } b ? $", buttons {string.Join(" ", b.Select(x => $"{x:X4}"))}" : "");

    // Caller holds _lock.
    private (MouseSettings? Settings, Dictionary<string, byte[]> Replies) ReadLocked()
    {
        var replies = new Dictionary<string, byte[]>();
        foreach (var question in MouseProtocol.Reads(Entry.Spec))
        {
            if (Ask(question) is not { } reply)
                return (null, replies);
            replies[MouseProtocol.Key(question)] = reply;
        }
        return (MouseProtocol.Parse(Entry.Spec, replies, Booster), replies);
    }

    // Caller holds _lock. Asks once more when no answer came: the first
    // question after a pause sometimes goes unanswered over the Omni link
    // (seen live, #431) — each one is a documented question, safe to repeat.
    private byte[]? Ask(byte[] question)
    {
        if (AskOnce(question) is { } reply)
            return reply;
        SidecarLog.Log($"[rigstats-control] Mouse: {Entry.Spec.Name} didn't answer {Convert.ToHexString(question)} in {ReplyTimeout.TotalMilliseconds:F0} ms, asking again.");
        return AskOnce(question);
    }

    // Caller holds _lock. Sends `[report id, question…]` and returns the
    // reply that answers it, skipping other input reports; reopens once
    // (a dongle re-enumerates when the mouse wakes).
    private byte[]? AskOnce(byte[] question)
    {
        var report = new byte[Math.Max(_channel.OutputReportLength, 1 + question.Length)];
        report[0] = _reportId;
        question.CopyTo(report, 1);
        try
        {
            (_device ??= Hid.Open(_channel)).Write(report);
        }
        catch (Exception e) when (e is IOException or ObjectDisposedException or System.ComponentModel.Win32Exception)
        {
            _device?.Dispose();
            _device = Hid.Open(_channel);
            _device.Write(report);
        }
        var deadline = DateTime.UtcNow + ReplyTimeout;
        for (var left = ReplyTimeout; left > TimeSpan.Zero; left = deadline - DateTime.UtcNow)
        {
            if (_device.Read(left) is not { } reply)
                return null;
            if (MouseProtocol.Answers(reply, _reportId, question))
                return reply;
        }
        return null;
    }

    public void Dispose()
    {
        lock (_lock)
        {
            _device?.Dispose();
            _device = null;
        }
    }
}
