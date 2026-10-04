using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// One Hue room or zone the user chose to follow the rig (#215), as a
/// lighting device: Aura Sync's effect goes to its grouped light. Lights the
/// user didn't choose are never touched.
///
/// The bridge takes about one group command a second, so nothing is drawn
/// frame by frame: static and off are one command, and breathing and
/// spectrum cycle are one command every <see cref="AnimationStep"/> that
/// tells the bridge to fade to the effect's next point over that time — a
/// slow, smooth pulse or colour cycle, not in step with the rig's own LEDs.
///
/// Commands go out on the device's own thread, newest wins: a live preview
/// or a slow bridge never holds up the control pipe or the other devices,
/// so a bridge that can't be reached is logged rather than failing the
/// profile.
public sealed class HueRoomDevice : ILightingDevice, IDisposable
{
    public static readonly TimeSpan AnimationStep = TimeSpan.FromSeconds(2);
    private static readonly TimeSpan StaticFade = TimeSpan.FromMilliseconds(400);

    /// The dimmest point of a breath — 0 % would switch the lights off.
    private const double BreathingFloor = 1.0;

    private readonly HueGroup _group;
    private readonly HueBridgeInfo _bridge;
    private readonly Action<string, JsonObject> _send;
    private readonly object _gate = new();
    private (AuraEffect Effect, byte Red, byte Green, byte Blue, DateTime Started)? _wanted;
    private int _version;
    private bool _disposed;
    private Thread? _thread;
    private volatile string? _problem;

    public HueRoomDevice(HueGroup group, HueBridgeInfo bridge, Action<string, JsonObject> send)
    {
        _group = group;
        _bridge = bridge;
        _send = send;
        Zones = [new AuraZone("lights", $"{group.Kind} lights", false, 0, 0)];
    }

    public string Id => $"hue-{_group.Id}";
    public string Name => $"Hue: {_group.Name}";
    public string Kind => $"hue_{_group.Kind}";
    public string Firmware => _bridge.Firmware;
    public string? Blocked => null;
    public string? Problem => _problem;
    public IReadOnlyList<AuraZone> Zones { get; }

    public void Apply(AuraEffect effect, byte red, byte green, byte blue)
    {
        lock (_gate)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            _wanted = (effect, red, green, blue, DateTime.UtcNow);
            _version++;
            if (_thread is null)
            {
                _thread = new Thread(Run) { IsBackground = true, Name = $"lighting-hue-{_group.Name}" };
                _thread.Start();
            }
            Monitor.PulseAll(_gate);
        }
    }

    /// The lights stay as they are; a running animation stops.
    public void Release()
    {
        lock (_gate)
        {
            _wanted = null;
            _version++;
            Monitor.PulseAll(_gate);
        }
    }

    public JsonObject Diagnostics() => new()
    {
        ["bridge_id"] = _bridge.Id,
        ["bridge_model"] = _bridge.Model,
        ["bridge_firmware"] = _bridge.Firmware,
        ["group"] = _group.Name,
        ["group_kind"] = _group.Kind,
    };

    public void Dispose()
    {
        Thread? thread;
        lock (_gate)
        {
            _disposed = true;
            Monitor.PulseAll(_gate);
            thread = _thread;
        }
        thread?.Join(TimeSpan.FromSeconds(1));
    }

    private void Run()
    {
        var failing = false;
        var since = DateTime.Now;
        while (true)
        {
            (AuraEffect Effect, byte Red, byte Green, byte Blue, DateTime Started) wanted;
            int version;
            lock (_gate)
            {
                while (!_disposed && _wanted is null)
                    Monitor.Wait(_gate);
                if (_disposed)
                    return;
                wanted = _wanted!.Value;
                version = _version;
            }

            var sentAt = DateTime.UtcNow;
            try
            {
                _send(_group.GroupedLightId, Command(wanted.Effect, wanted.Red, wanted.Green, wanted.Blue, sentAt - wanted.Started));
                if (failing)
                    SidecarLog.Log($"[rigstats-control] Lighting on {Name} works again.");
                failing = false;
                _problem = null;
            }
            catch (Exception e)
            {
                if (!failing)
                {
                    SidecarLog.Log($"[rigstats-control] Lighting on {Name} failed: {e.Message}");
                    since = DateTime.Now;
                }
                failing = true;
                _problem = $"Not working since {since:HH:mm}: {e.Message}";
            }

            lock (_gate)
            {
                if (_version != version)
                    continue; // something newer to send
                if (!SoftwareEffect.IsAnimated(wanted.Effect))
                {
                    // Shown; sleep until the next change.
                    while (!_disposed && _version == version)
                        Monitor.Wait(_gate);
                    continue;
                }
                var next = AnimationStep - (DateTime.UtcNow - sentAt);
                if (next > TimeSpan.Zero)
                    Monitor.Wait(_gate, next);
            }
        }
    }

    /// The command that shows `effect` at `elapsed` since it started: for
    /// an animation, a fade to where the effect is one step later.
    public static JsonObject Command(AuraEffect effect, byte red, byte green, byte blue, TimeSpan elapsed)
    {
        var level = Math.Max(red, Math.Max(green, blue)) / 255.0;
        if (effect == AuraEffect.Off || level == 0)
            return Off();
        switch (effect)
        {
            case AuraEffect.Breathing:
                {
                    var (x, y, full) = HueColor.FromRgb(red, green, blue);
                    var (r, g, b) = SoftwareEffect.Frame(effect, red, green, blue, elapsed + AnimationStep);
                    var breath = Math.Max(r, Math.Max(g, b)) / 255.0 / level;
                    return On(x, y, Math.Max(BreathingFloor, Math.Round(full * breath, 1)), AnimationStep);
                }
            case AuraEffect.SpectrumCycle:
                {
                    var (r, g, b) = SoftwareEffect.Frame(effect, red, green, blue, elapsed + AnimationStep);
                    var (x, y, _) = HueColor.FromRgb(r, g, b);
                    return On(x, y, Math.Round(level * 100, 1), AnimationStep);
                }
            default:
                {
                    var (x, y, brightness) = HueColor.FromRgb(red, green, blue);
                    return On(x, y, brightness, StaticFade);
                }
        }
    }

    private static JsonObject Off() => new()
    {
        ["on"] = new JsonObject { ["on"] = false },
        ["dynamics"] = new JsonObject { ["duration"] = (int)StaticFade.TotalMilliseconds },
    };

    private static JsonObject On(double x, double y, double brightness, TimeSpan fade) => new()
    {
        ["on"] = new JsonObject { ["on"] = true },
        ["dimming"] = new JsonObject { ["brightness"] = brightness },
        ["color"] = new JsonObject { ["xy"] = new JsonObject { ["x"] = x, ["y"] = y } },
        ["dynamics"] = new JsonObject { ["duration"] = (int)fade.TotalMilliseconds },
    };
}
