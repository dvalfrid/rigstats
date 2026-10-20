using System.Diagnostics;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// A HyperX Alloy Origins Core keyboard (#511), per-key RGB over its vendor
/// collection (interface 2, usage page 0xFF13, 65-byte reports).
///
/// The protocol comes from OpenRGB's HyperX Alloy Origins Core controller
/// (GPL-2, `Controllers/HyperXKeyboardController/HyperXAlloyOriginsCoreController`)
/// — read as documentation, no code copied:
/// - colours: 380 bytes in reports `00 A2 <seq> 00 <len> <data…>`, 60 bytes
///   each (seven reports); the data is 94 key slots grouped in segments of
///   48 bytes — per 12 keys, 6 green + 2 zero, 6 green + 2 zero, then red,
///   then blue the same way;
/// - brightness: `00 A7 00 00 01 <0–255>`;
/// - the keyboard only shows colours while they keep coming: OpenRGB sends
///   the frame again whenever 50 ms passed without one. So does this
///   device, on its own thread, until the profile changes or the service
///   stops — then the keyboard goes back to its own effect.
///
/// Nothing is asked of the keyboard (OpenRGB's layout query is not used:
/// every key gets the same colour). `Verified = false` until a user confirms.
public sealed class HyperXKeyboardDevice : ILightingDevice, IDisposable
{
    public const ushort KingstonVendorId = 0x0951;
    public const ushort HpVendorId = 0x03F0;
    public const ushort VendorUsagePage = 0xFF13;
    public const int Interface = 2;
    public const int ReportLength = 65;

    public const int KeySlots = 94;
    public const int ColourBytes = 380;
    public const int PayloadPerReport = 60;
    public const byte ColourCommand = 0xA2;
    public const byte BrightnessCommand = 0xA7;

    /// OpenRGB re-sends after 50 ms without a frame.
    public static readonly TimeSpan KeepAlive = TimeSpan.FromMilliseconds(50);

    public sealed record Model(string Name, bool Verified = false);

    /// Every model, by (vendor id, product id), as OpenRGB's detector lists
    /// them for this controller.
    public static readonly IReadOnlyDictionary<(ushort Vendor, ushort Product), Model> Models =
        new Dictionary<(ushort, ushort), Model>
        {
            [(KingstonVendorId, 0x16E6)] = new("HyperX Alloy Origins Core"),
            [(HpVendorId, 0x098F)] = new("HyperX Alloy Origins Core (HP)"),
        };

    /// HyperX's own software — while it runs it owns the lights. Process
    /// names not yet seen on a machine.
    public static readonly IReadOnlyList<string> HyperXSoftware = ["NGenuity2", "NGENUITY", "HyperX NGENUITY"];

    public static bool IsChannel(HidDeviceInfo h) =>
        Models.ContainsKey((h.VendorId, h.ProductId)) && h.UsagePage == VendorUsagePage && h.Interface == Interface;

    private readonly HidDeviceInfo _info;
    private readonly Model _model;
    private readonly IHidDevice _device;
    private readonly Func<string?> _ownSoftware;
    private readonly SoftwareKeepAlive _loop;
    private readonly object _write = new();

    public HyperXKeyboardDevice(HidDeviceInfo info, Model model, IHidDevice device,
        Func<string?>? ownSoftware = null, TimeSpan? keepAlive = null)
    {
        _info = info;
        _model = model;
        _device = device;
        _ownSoftware = ownSoftware ?? (() => Running(HyperXSoftware, "HyperX NGENUITY"));
        _loop = new SoftwareKeepAlive(model.Name, keepAlive ?? KeepAlive, Draw, problem => Problem = problem);
    }

    public string Id => $"hyperx-{_info.VendorId:x4}-{_info.ProductId:x4}";
    public string Name => _model.Name;
    public string Kind => "keyboard";
    public string Firmware => "";
    public IReadOnlyList<AuraZone> Zones { get; } = [new AuraZone("keys", "Keys", Addressable: false, EffectChannel: 0, Leds: KeySlots)];
    public string? Blocked => _ownSoftware() is { } program ? $"{program} is running" : null;
    public string? Problem { get; private set; }

    public static IReadOnlyList<HyperXKeyboardDevice> Discover(IReadOnlyList<HidDeviceInfo> hid,
        Func<HidDeviceInfo, IHidDevice>? open = null)
    {
        open ??= Hid.Open;
        var found = new List<HyperXKeyboardDevice>();
        foreach (var info in hid.Where(IsChannel).OrderBy(h => h.Path, StringComparer.OrdinalIgnoreCase))
        {
            var model = Models[(info.VendorId, info.ProductId)];
            try
            {
                found.Add(new HyperXKeyboardDevice(info, model, open(info)));
                LightingLog.Discovery($"[rigstats-control] Lighting: {model.Name} {info.VendorId:X4}:{info.ProductId:X4} (HyperX protocol).");
            }
            catch (Exception e) when (e is IOException or System.ComponentModel.Win32Exception or UnauthorizedAccessException)
            {
                LightingLog.Discovery($"[rigstats-control] Lighting: {model.Name} couldn't be opened: {e.Message}");
            }
        }
        return found;
    }

    /// Never throws (not confirmed yet): a failure shows as `Problem`.
    public void Apply(AuraEffect effect, byte red, byte green, byte blue)
    {
        try
        {
            lock (_write)
                _device.Write(BrightnessReport(255));
            _loop.Start(effect, red, green, blue);
            Problem = null;
        }
        catch (Exception e) when (e is IOException or System.ComponentModel.Win32Exception or InvalidOperationException or OperationCanceledException)
        {
            _loop.Stop();
            Problem = e.Message;
            SidecarLog.Log($"[rigstats-control] Lighting on {Name} failed: {e.Message}");
        }
    }

    /// Stops sending: the keyboard goes back to its own effect.
    public bool Release()
    {
        _loop.Stop();
        return true;
    }

    public JsonObject Diagnostics() => new()
    {
        ["vendor_id"] = $"{_info.VendorId:X4}",
        ["product_id"] = $"{_info.ProductId:X4}",
        ["interface"] = _info.Interface,
        ["product"] = _info.Product,
        ["protocol"] = "hyperx_alloy_origins_core",
        ["verified"] = _model.Verified,
    };

    public void Dispose()
    {
        _loop.Dispose();
        _device.Dispose();
    }

    // Every 40th frame (~2 s) the loop checks whether HyperX's own software
    // started meanwhile, and leaves the keyboard to it — never fights it.
    private const int OwnSoftwareCheckEvery = 40;
    private int _frames;

    private void Draw(byte red, byte green, byte blue)
    {
        if (++_frames % OwnSoftwareCheckEvery == 0 && _ownSoftware() is { } program)
            throw new InvalidOperationException($"{program} is running — the keyboard is left to it");
        lock (_write)
        {
            foreach (var report in ColourReports(red, green, blue))
                _device.Write(report);
        }
    }

    // ── Pure (unit-tested) ────────────────────────────────────────────

    /// The 380 colour bytes with every key slot in one colour.
    public static byte[] ColourBuffer(byte red, byte green, byte blue)
    {
        var buffer = new byte[ColourBytes];
        for (var key = 0; key < KeySlots; key++)
        {
            var at = key / 12 * 48 + (key / 6 & 1) * 8 + key % 6;
            buffer[at] = green;
            buffer[at + 16] = red;
            buffer[at + 32] = blue;
        }
        return buffer;
    }

    /// The seven reports that carry one frame.
    public static IReadOnlyList<byte[]> ColourReports(byte red, byte green, byte blue)
    {
        var buffer = ColourBuffer(red, green, blue);
        var reports = new List<byte[]>();
        for (int sent = 0, seq = 0; sent < buffer.Length; seq++)
        {
            var size = Math.Min(PayloadPerReport, buffer.Length - sent);
            var report = new byte[ReportLength];
            report[1] = ColourCommand;
            report[2] = (byte)seq;
            report[4] = (byte)size;
            Array.Copy(buffer, sent, report, 5, size);
            reports.Add(report);
            sent += size;
        }
        return reports;
    }

    public static byte[] BrightnessReport(byte brightness)
    {
        var report = new byte[ReportLength];
        report[1] = BrightnessCommand;
        report[4] = 0x01;
        report[5] = brightness;
        return report;
    }

    internal static string? Running(IReadOnlyList<string> processes, string shownAs)
    {
        foreach (var name in processes)
        {
            var found = Process.GetProcessesByName(name);
            var running = found.Length > 0;
            foreach (var p in found)
                p.Dispose();
            if (running)
                return shownAs;
        }
        return null;
    }
}

/// Sends a device's frame again and again — for devices that show colours
/// only while they keep coming. Every frame follows `SoftwareEffect`, so
/// breathing and spectrum animate on the same beat. A frame that fails
/// stops the loop and reports why; the next `Start` tries again.
public sealed class SoftwareKeepAlive : IDisposable
{
    private static readonly TimeSpan StopTimeout = TimeSpan.FromSeconds(1);

    private readonly string _name;
    private readonly TimeSpan _interval;
    private readonly Action<byte, byte, byte> _draw;
    private readonly Action<string> _failed;
    private readonly object _gate = new();
    private CancellationTokenSource? _running;
    private Thread? _thread;

    public SoftwareKeepAlive(string name, TimeSpan interval, Action<byte, byte, byte> draw, Action<string> failed)
    {
        _name = name;
        _interval = interval;
        _draw = draw;
        _failed = failed;
    }

    public bool Running
    {
        get
        {
            lock (_gate)
                return _running is not null;
        }
    }

    /// Draws the first frame at once (a failure throws to the caller), then
    /// keeps drawing every interval on its own thread.
    public void Start(AuraEffect effect, byte red, byte green, byte blue)
    {
        lock (_gate)
        {
            StopLocked();
            var (r, g, b) = SoftwareEffect.Frame(effect, red, green, blue, TimeSpan.Zero);
            _draw(r, g, b);
            var cts = new CancellationTokenSource();
            _running = cts;
            var started = DateTime.UtcNow;
            _thread = new Thread(() =>
            {
                while (!cts.Token.WaitHandle.WaitOne(_interval))
                {
                    var frame = SoftwareEffect.Frame(effect, red, green, blue, DateTime.UtcNow - started);
                    try
                    {
                        _draw(frame.Red, frame.Green, frame.Blue);
                    }
                    catch (Exception e)
                    {
                        SidecarLog.Log($"[rigstats-control] Lighting on {_name} stopped: {e.Message}");
                        _failed(e.Message);
                        return;
                    }
                }
            })
            {
                IsBackground = true,
                Name = $"lighting-{_name}",
            };
            _thread.Start();
        }
    }

    public void Stop()
    {
        lock (_gate)
            StopLocked();
    }

    private void StopLocked()
    {
        var cts = _running;
        var thread = _thread;
        _running = null;
        _thread = null;
        if (cts is null)
            return;
        cts.Cancel();
        if (thread is null || thread == Thread.CurrentThread || thread.Join(StopTimeout))
        {
            cts.Dispose();
            return;
        }
        SidecarLog.Log($"[rigstats-control] Lighting on {_name} did not stop within {StopTimeout.TotalSeconds:0} s.");
    }

    public void Dispose() => Stop();
}
