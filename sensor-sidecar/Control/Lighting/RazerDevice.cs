using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using Microsoft.Win32.SafeHandles;

namespace SensorSidecar.Control.Lighting;

/// How a Razer model's lights are addressed.
public enum RazerLayout
{
    /// Separate LEDs (scroll wheel, logo), each with its own state, colour
    /// and effect — the older mice.
    ClassicLeds,

    /// One keyboard backlight set as a whole with the "standard matrix"
    /// effects — the Blade laptops of 2016–2019.
    StandardMatrix,
}

/// A Razer model RIGStats drives. `TransactionId` is the one byte the
/// device expects in every report, per model.
public sealed record RazerModel(string Name, string Kind, RazerLayout Layout, byte TransactionId, bool Verified = false);

/// Razer's HID protocol (#510): one 90-byte feature report each way, report
/// id 0 — status, transaction id, remaining packets, protocol type, data
/// size, command class, command id, 80 argument bytes, an XOR checksum. The
/// device answers each report with the same frame and a status byte.
///
/// The command numbers, the per-model transaction ids and which LEDs a
/// model has come from openrazer (GPL-2, https://github.com/openrazer/openrazer,
/// `driver/razercommon.c`, `driver/razerchromacommon.c`,
/// `driver/razermouse_driver.c`, `driver/razerkbd_driver.c`) — used as
/// documentation only, no code copied. Only those commands are ever sent,
/// and only the ones for the model at hand; nothing is asked of a device to
/// find out what it is.
public static class RazerProtocol
{
    public const ushort VendorId = 0x1532;

    /// Report id 0 + 90 bytes.
    public const int ReportLength = 91;

    // Offsets in the 91-byte report (byte 0 is the report id).
    public const int Status = 1;
    public const int TransactionId = 2;
    public const int DataSize = 6;
    public const int CommandClass = 7;
    public const int CommandId = 8;
    public const int Arguments = 9;
    public const int Crc = 89;

    // Reply status (`RAZER_CMD_*`).
    public const byte StatusNotTakenUp = 0x00;
    public const byte StatusBusy = 0x01;
    public const byte StatusOk = 0x02;
    public const byte StatusFailure = 0x03;
    public const byte StatusTimeout = 0x04;
    public const byte StatusNotSupported = 0x05;

    // Which store a setting goes to: VARSTORE = the one in use.
    public const byte VarStore = 0x01;

    // LED ids (`*_LED`).
    public const byte ScrollWheelLed = 0x01;
    public const byte LogoLed = 0x04;

    // Classic effects (`CLASSIC_EFFECT_*`).
    public const byte ClassicStatic = 0x00;
    public const byte ClassicBreathing = 0x02;

    // Standard matrix effects (`MATRIX_EFFECT_*`).
    public const byte MatrixOff = 0x00;
    public const byte MatrixStatic = 0x06;

    /// A report: class, id and arguments, data size = argument count,
    /// checksum over bytes 2..87 of the 90.
    public static byte[] Frame(byte transactionId, byte commandClass, byte commandId, params byte[] arguments)
    {
        if (arguments.Length > 80)
            throw new ArgumentException("At most 80 argument bytes.", nameof(arguments));
        var report = new byte[ReportLength];
        report[TransactionId] = transactionId;
        report[DataSize] = (byte)arguments.Length;
        report[CommandClass] = commandClass;
        report[CommandId] = commandId;
        arguments.CopyTo(report, Arguments);
        report[Crc] = Checksum(report);
        return report;
    }

    /// XOR of the 90-byte report's bytes 2..87 (here 3..88, after the
    /// report id).
    public static byte Checksum(byte[] report)
    {
        byte crc = 0;
        for (var i = 3; i < 89; i++)
            crc ^= report[i];
        return crc;
    }

    /// `set_led_state`: an LED on or off.
    public static byte[] LedState(byte tid, byte led, bool on) => Frame(tid, 0x03, 0x00, VarStore, led, on ? (byte)1 : (byte)0);

    /// `set_led_rgb`: an LED's colour.
    public static byte[] LedRgb(byte tid, byte led, byte r, byte g, byte b) => Frame(tid, 0x03, 0x01, VarStore, led, r, g, b);

    /// `set_led_effect`: an LED's classic effect.
    public static byte[] LedEffect(byte tid, byte led, byte effect) => Frame(tid, 0x03, 0x02, VarStore, led, effect);

    /// `standard_matrix_effect_none`: the backlight off.
    public static byte[] MatrixNone(byte tid) => Frame(tid, 0x03, 0x0A, MatrixOff);

    /// `standard_matrix_effect_static`: the whole backlight in one colour.
    public static byte[] MatrixStaticColor(byte tid, byte r, byte g, byte b) => Frame(tid, 0x03, 0x0A, MatrixStatic, r, g, b);

    /// Whether the device hasn't finished the command yet: new (not taken up
    /// yet, seen on a DeathAdder 2013 read 2 ms after the request, #538) or
    /// busy. The same reply is read again.
    public static bool Pending(byte status) => status is StatusNotTakenUp or StatusBusy;

    /// Why a reply means the command didn't take, or null when it did. The
    /// reply must answer the same command.
    public static string? Problem(byte[] request, byte[]? reply)
    {
        if (reply is null || reply.Length < ReportLength)
            return "no reply";
        if (reply[CommandClass] != request[CommandClass] || reply[CommandId] != request[CommandId])
            return $"reply to another command ({reply[CommandClass]:X2} {reply[CommandId]:X2})";
        return reply[Status] switch
        {
            StatusOk => null,
            StatusNotTakenUp => "not taken up yet",
            StatusBusy => "busy",
            StatusFailure => "failed",
            StatusTimeout => "timed out",
            StatusNotSupported => "not supported",
            var s => $"status {s:X2}",
        };
    }
}

/// One exchange with a Razer device: a report out, its reply back. A seam so
/// `RazerDevice` is testable without a device.
public interface IRazerTransport : IDisposable
{
    /// Sends `request` and returns the device's reply (null when none).
    byte[]? Exchange(byte[] request);
}

/// A Razer mouse or laptop keyboard on Razer's own HID protocol (#510): off
/// or one static colour. Breathing and spectrum are shown as the static
/// colour for now. `Verified = false` until a user with the device confirms.
public sealed class RazerDevice : ILightingDevice, IDisposable
{
    /// Every Razer model RIGStats drives, by USB product id. Ids, LEDs and
    /// transaction ids from openrazer (see `RazerProtocol`).
    public static readonly IReadOnlyDictionary<ushort, RazerModel> Models = new Dictionary<ushort, RazerModel>
    {
        // Single-colour LEDs: openrazer accepts a colour, the LEDs may show
        // their own.
        [0x0037] = new("Razer DeathAdder 2013", "mouse", RazerLayout.ClassicLeds, 0x3F),
        [0x0233] = new("Razer Blade 15 (2018)", "keyboard", RazerLayout.StandardMatrix, 0xFF),
    };

    /// Razer's own software — while it runs it owns the lights; RIGStats
    /// leaves them alone. Process names not yet seen on a machine.
    public static readonly IReadOnlyList<string> RazerSoftware =
        ["Razer Synapse 3", "Razer Synapse Service", "RazerCentralService", "RazerAppEngine"];

    public static RazerModel? Model(ushort productId) => Models.TryGetValue(productId, out var m) ? m : null;

    /// The collection that carries the protocol: a known model's 90-byte
    /// feature report.
    public static bool IsChannel(HidDeviceInfo h) =>
        h.VendorId == RazerProtocol.VendorId && Model(h.ProductId) is not null && h.FeatureReportLength == RazerProtocol.ReportLength;

    private readonly HidDeviceInfo _info;
    private readonly RazerModel _model;
    private readonly IRazerTransport _transport;
    private readonly Func<string?> _razerSoftware;

    public RazerDevice(HidDeviceInfo info, RazerModel model, IRazerTransport transport, Func<string?>? razerSoftware = null)
    {
        _info = info;
        _model = model;
        _transport = transport;
        _razerSoftware = razerSoftware ?? RunningRazerSoftware;
        Zones = model.Layout == RazerLayout.ClassicLeds
            ?
            [
                new AuraZone("scroll_wheel", "Scroll wheel", Addressable: false, EffectChannel: RazerProtocol.ScrollWheelLed, Leds: 1),
                new AuraZone("logo", "Logo", Addressable: false, EffectChannel: RazerProtocol.LogoLed, Leds: 1),
            ]
            : [new AuraZone("backlight", "Keyboard", Addressable: false, EffectChannel: 0, Leds: 1)];
    }

    public string Id => $"razer-{_info.VendorId:x4}-{_info.ProductId:x4}";
    public string Name => _model.Name;
    public string Kind => _model.Kind;
    public string Firmware => "";
    public IReadOnlyList<AuraZone> Zones { get; }
    public string? Blocked => _razerSoftware() is { } program ? $"{program} is running" : null;

    /// Every Razer device among the HID collections that speaks the protocol.
    /// Opens the collection only — nothing is sent until a profile asks.
    public static IReadOnlyList<RazerDevice> Discover(IReadOnlyList<HidDeviceInfo> hid,
        Func<HidDeviceInfo, IRazerTransport?>? open = null)
    {
        open ??= WindowsRazerTransport.Open;
        var found = new List<RazerDevice>();
        foreach (var info in hid.Where(IsChannel).OrderBy(h => h.ProductId).ThenBy(h => h.Path, StringComparer.OrdinalIgnoreCase))
        {
            // A model that also shows a LampArray is driven by
            // `LampArrayDevice` — never twice.
            if (hid.Any(h => h.VendorId == info.VendorId && h.ProductId == info.ProductId && h.UsagePage == LampArrayDevice.LightingPage))
                continue;
            if (open(info) is not { } transport)
            {
                LightingLog.Discovery($"[rigstats-control] Lighting: {Model(info.ProductId)!.Name} 0x{info.ProductId:X4} couldn't be opened.");
                continue;
            }
            LightingLog.Discovery($"[rigstats-control] Lighting: {Model(info.ProductId)!.Name} 0x{info.ProductId:X4} (Razer protocol).");
            found.Add(new RazerDevice(info, Model(info.ProductId)!, transport));
        }
        return found;
    }

    /// Why the last write failed, for the Lighting tab, or null.
    public string? Problem { get; private set; }
    public bool StaticOnly => true;

    /// Never throws: these models aren't confirmed yet, so a write that
    /// fails is shown on the device (`Problem`) and logged — it must not
    /// fail a profile switch, the fans and the rest of the lighting with it.
    public void Apply(AuraEffect effect, byte red, byte green, byte blue)
    {
        try
        {
            Write(effect, red, green, blue);
            Problem = null;
        }
        catch (Exception e) when (e is InvalidOperationException or System.ComponentModel.Win32Exception or IOException)
        {
            Problem = e.Message;
            SidecarLog.Log($"[rigstats-control] Lighting on {Name} failed: {e.Message}");
        }
    }

    private void Write(AuraEffect effect, byte red, byte green, byte blue)
    {
        var tid = _model.TransactionId;
        var on = effect != AuraEffect.Off;
        if (_model.Layout == RazerLayout.StandardMatrix)
        {
            Send(on ? RazerProtocol.MatrixStaticColor(tid, red, green, blue) : RazerProtocol.MatrixNone(tid));
            return;
        }
        foreach (var zone in Zones)
        {
            var led = (byte)zone.EffectChannel;
            Send(RazerProtocol.LedState(tid, led, on));
            if (!on)
                continue;
            Send(RazerProtocol.LedRgb(tid, led, red, green, blue));
            Send(RazerProtocol.LedEffect(tid, led, RazerProtocol.ClassicStatic));
        }
    }

    /// The device keeps the last colour; nothing of Razer's own to go back to.
    public bool Release() => false;

    public JsonObject Diagnostics() => new()
    {
        ["vendor_id"] = $"{_info.VendorId:X4}",
        ["product_id"] = $"{_info.ProductId:X4}",
        ["interface"] = _info.Interface,
        ["product"] = _info.Product,
        ["protocol"] = "razer",
        ["layout"] = _model.Layout.ToString(),
        ["transaction_id"] = $"{_model.TransactionId:X2}",
        ["verified"] = _model.Verified,
    };

    public void Dispose() => _transport.Dispose();

    private void Send(byte[] request)
    {
        var reply = _transport.Exchange(request);
        if (RazerProtocol.Problem(request, reply) is { } problem)
            throw new InvalidOperationException(
                $"{Name}: command {request[RazerProtocol.CommandClass]:X2} {request[RazerProtocol.CommandId]:X2} not taken — {problem}.");
    }

    private static string? RunningRazerSoftware()
    {
        foreach (var name in RazerSoftware)
        {
            var processes = Process.GetProcessesByName(name);
            var running = processes.Length > 0;
            foreach (var p in processes)
                p.Dispose();
            if (running)
                return "Razer Synapse";
        }
        return null;
    }
}

/// Feature reports through `hid.dll` on a collection Windows may hold for
/// itself (a mouse, a keyboard): opened for read/write when it can be,
/// otherwise with no access — feature reports still work on such a handle.
public sealed class WindowsRazerTransport : IRazerTransport
{
    // openrazer waits 600 µs between a report and asking for its reply;
    // Windows sleeps at least a millisecond.
    private static readonly TimeSpan ReplyDelay = TimeSpan.FromMilliseconds(2);
    private const int BusyRetries = 5;

    private readonly SafeFileHandle _handle;

    private WindowsRazerTransport(SafeFileHandle handle) => _handle = handle;

    public static IRazerTransport? Open(HidDeviceInfo info)
    {
        var handle = CreateFile(info.Path, GenericRead | GenericWrite, FileShareReadWrite, IntPtr.Zero, OpenExisting, 0, IntPtr.Zero);
        if (handle.IsInvalid)
        {
            handle.Dispose();
            handle = CreateFile(info.Path, 0, FileShareReadWrite, IntPtr.Zero, OpenExisting, 0, IntPtr.Zero);
        }
        if (handle.IsInvalid)
        {
            handle.Dispose();
            return null;
        }
        return new WindowsRazerTransport(handle);
    }

    public byte[]? Exchange(byte[] request)
    {
        if (!HidD_SetFeature(_handle, request, request.Length))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Razer report not accepted");
        for (var attempt = 0; ; attempt++)
        {
            Thread.Sleep(ReplyDelay);
            var reply = new byte[RazerProtocol.ReportLength];
            if (!HidD_GetFeature(_handle, reply, reply.Length))
                return null;
            if (!RazerProtocol.Pending(reply[RazerProtocol.Status]) || attempt >= BusyRetries)
                return reply;
        }
    }

    public void Dispose() => _handle.Dispose();

    private const uint GenericRead = 0x80000000;
    private const uint GenericWrite = 0x40000000;
    private const uint FileShareReadWrite = 0x00000003;
    private const uint OpenExisting = 3;

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security,
        uint disposition, uint flags, IntPtr template);

    [DllImport("hid.dll", SetLastError = true)]
    private static extern bool HidD_GetFeature(SafeFileHandle device, byte[] report, int length);

    [DllImport("hid.dll", SetLastError = true)]
    private static extern bool HidD_SetFeature(SafeFileHandle device, byte[] report, int length);
}
