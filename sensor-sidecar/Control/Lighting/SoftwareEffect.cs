namespace SensorSidecar.Control.Lighting;

/// Effects for devices that only have a direct mode (ASUS Aura monitors):
/// the service draws each frame itself. Pure frame maths here, unit-tested;
/// <see cref="SoftwareEffectLoop"/> runs it.
public static class SoftwareEffect
{
    public static readonly TimeSpan BreathingPeriod = TimeSpan.FromSeconds(4);
    public static readonly TimeSpan SpectrumPeriod = TimeSpan.FromSeconds(8);

    /// Whether `effect` needs frames over time (otherwise one frame is enough).
    public static bool IsAnimated(AuraEffect effect) =>
        effect is AuraEffect.Breathing or AuraEffect.SpectrumCycle;

    /// The colour at time `t` since the effect started.
    public static (byte Red, byte Green, byte Blue) Frame(AuraEffect effect, byte red, byte green, byte blue, TimeSpan t)
    {
        switch (effect)
        {
            case AuraEffect.Off:
                return (0, 0, 0);
            case AuraEffect.Breathing:
                {
                    // 0 → full → 0 over one period, smooth at both ends.
                    var phase = t.TotalSeconds % BreathingPeriod.TotalSeconds / BreathingPeriod.TotalSeconds;
                    var level = 0.5 - 0.5 * Math.Cos(2 * Math.PI * phase);
                    return ((byte)Math.Round(red * level), (byte)Math.Round(green * level), (byte)Math.Round(blue * level));
                }
            case AuraEffect.SpectrumCycle:
                {
                    var hue = t.TotalSeconds % SpectrumPeriod.TotalSeconds / SpectrumPeriod.TotalSeconds * 360.0;
                    return Hue(hue);
                }
            default:
                return (red, green, blue);
        }
    }

    /// Full-saturation, full-value colour at `hue` degrees.
    public static (byte Red, byte Green, byte Blue) Hue(double hue)
    {
        var h = (hue % 360 + 360) % 360 / 60.0;
        var x = 1 - Math.Abs(h % 2 - 1);
        var (r, g, b) = (int)h switch
        {
            0 => (1.0, x, 0.0),
            1 => (x, 1.0, 0.0),
            2 => (0.0, 1.0, x),
            3 => (0.0, x, 1.0),
            4 => (x, 0.0, 1.0),
            _ => (1.0, 0.0, x),
        };
        return ((byte)Math.Round(r * 255), (byte)Math.Round(g * 255), (byte)Math.Round(b * 255));
    }
}

/// Draws a software effect on one device until replaced or stopped: one
/// frame for off/static, ~25 frames a second for breathing and spectrum.
public sealed class SoftwareEffectLoop : IDisposable
{
    private static readonly TimeSpan FrameInterval = TimeSpan.FromMilliseconds(40);

    private readonly Action<byte, byte, byte> _draw;
    private readonly string _name;
    private CancellationTokenSource? _running;

    public SoftwareEffectLoop(string name, Action<byte, byte, byte> draw)
    {
        _name = name;
        _draw = draw;
    }

    /// Replaces whatever is running. The first frame is drawn at once, so
    /// a failing device throws to the caller.
    public void Start(AuraEffect effect, byte red, byte green, byte blue)
    {
        Stop();
        var (r, g, b) = SoftwareEffect.Frame(effect, red, green, blue, TimeSpan.Zero);
        _draw(r, g, b);
        if (!SoftwareEffect.IsAnimated(effect))
            return;

        var cts = new CancellationTokenSource();
        _running = cts;
        var started = DateTime.UtcNow;
        var thread = new Thread(() =>
        {
            while (!cts.Token.WaitHandle.WaitOne(FrameInterval))
            {
                var frame = SoftwareEffect.Frame(effect, red, green, blue, DateTime.UtcNow - started);
                try
                {
                    _draw(frame.Red, frame.Green, frame.Blue);
                }
                catch (Exception e)
                {
                    // Unplugged mid-animation: stop quietly; the next apply
                    // tries again.
                    SidecarLog.Log($"[rigstats-control] Lighting animation on {_name} stopped: {e.Message}");
                    return;
                }
            }
        })
        {
            IsBackground = true,
            Name = $"lighting-{_name}",
        };
        thread.Start();
    }

    public void Stop()
    {
        _running?.Cancel();
        _running = null;
    }

    public void Dispose() => Stop();
}
