using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using Microsoft.Win32;
using Microsoft.Win32.SafeHandles;

namespace SensorSidecar.Control.Lighting;

/// Any HID LampArray device (#212) — the vendor-neutral standard behind
/// Windows Dynamic Lighting (HID usage page 0x59, "Lighting and
/// Illumination"). One implementation for every current keyboard, mouse or
/// accessory that supports Dynamic Lighting, whatever its brand: e.g. the
/// ROG Harpe Ace through the ROG Omni receiver. Report ids and field layout
/// come from the device's own descriptor via Windows' HidP functions, so no
/// model is hard-coded.
///
/// The service takes control by clearing AutonomousMode and hands it back
/// on release, so the device returns to its own effect. While Windows'
/// Dynamic Lighting is on, Windows drives these devices itself — the device
/// then reports itself blocked instead of fighting it.
public sealed class LampArrayDevice : ILightingDevice, IWdlDevice, IBatteryDevice, IDisposable
{
    public const ushort LightingPage = 0x59;
    private const ushort LampArrayUsage = 0x01;
    private const ushort LampCount = 0x03;
    private const ushort LampArrayKindUsage = 0x07;
    private const ushort MinUpdateInterval = 0x08;
    private const ushort RedUpdateChannel = 0x51;
    private const ushort GreenUpdateChannel = 0x52;
    private const ushort BlueUpdateChannel = 0x53;
    private const ushort IntensityUpdateChannel = 0x54;
    private const ushort LampUpdateFlags = 0x55;
    private const ushort LampIdStart = 0x61;
    private const ushort LampIdEnd = 0x62;
    private const ushort AutonomousMode = 0x71;
    private const int UpdateComplete = 0x01;

    private readonly object _lock = new();
    private readonly SafeFileHandle _handle;
    private readonly IntPtr _preparsed;
    private readonly ReportField _rangeUpdate;
    private readonly ReportField _control;
    private readonly int _featureLength;
    private readonly int _outputLength;
    private readonly int _lamps;
    private readonly SoftwareEffectLoop _loop;
    private readonly Func<bool> _dynamicLightingOn;
    private bool _hostControlled;
    private readonly JsonObject _diagnostics;
    private readonly string _baseName;
    private readonly OmniMouse? _omni;

    public string Id { get; }
    public string Name { get; }
    public string Kind { get; }
    public string Firmware => "";
    public IReadOnlyList<AuraZone> Zones { get; }

    public string? Blocked => _dynamicLightingOn()
        ? "Controlled by Windows Dynamic Lighting. Turn it off in Settings → Personalization → Dynamic Lighting to control it here."
        : null;

    /// Where a usage lives: report type (output or feature) and report id.
    private readonly record struct ReportField(HidReportType Type, byte ReportId);

    private enum HidReportType
    {
        Output = 1,
        Feature = 2,
    }

    private LampArrayDevice(SafeFileHandle handle, IntPtr preparsed, HidpCaps caps, string id, string baseName,
        string name, string kind, int lamps, ReportField rangeUpdate, ReportField control,
        Func<bool> dynamicLightingOn, JsonObject diagnostics, OmniMouse? omni)
    {
        _omni = omni;
        _baseName = baseName;
        _diagnostics = diagnostics;
        _handle = handle;
        _preparsed = preparsed;
        _featureLength = caps.FeatureReportByteLength;
        _outputLength = caps.OutputReportByteLength;
        _lamps = lamps;
        _rangeUpdate = rangeUpdate;
        _control = control;
        _dynamicLightingOn = dynamicLightingOn;
        Id = id;
        Name = name;
        Kind = kind;
        Zones = [new AuraZone("lamps", lamps == 1 ? "1 lamp" : $"{lamps} lamps", Addressable: true, 0, lamps)];
        _loop = new SoftwareEffectLoop(name, Draw);
    }

    /// LampArrayKind (HID Usage Tables, Lighting and Illumination page).
    public static string KindName(uint kind) => kind switch
    {
        1 => "keyboard",
        2 => "mouse",
        3 => "game_controller",
        4 => "peripheral",
        5 => "scene",
        6 => "notification",
        7 => "chassis",
        8 => "wearable",
        9 => "furniture",
        10 => "art",
        _ => "peripheral",
    };

    /// The name shown in the Lighting tab. A receiver's product string names
    /// the dongle, not the device behind it, and laptop keyboards report the
    /// controller chip — both say what the user actually sees instead.
    /// `pairedModel`: the mouse the receiver reports, when known (#236).
    public static string DisplayName(ushort vendorId, ushort productId, string? product, string kind,
        string? pairedModel = null)
    {
        if (vendorId == AuraUsb.AsusVendorId && productId == OmniMouse.ReceiverProductId)
            return pairedModel ?? $"{char.ToUpperInvariant(kind[0])}{kind[1..].Replace('_', ' ')} via ROG Omni receiver";
        if (kind == "keyboard" && product?.StartsWith("ITE Device", StringComparison.OrdinalIgnoreCase) == true)
            return "Laptop keyboard";
        return string.IsNullOrWhiteSpace(product) ? $"LampArray {vendorId:X4}:{productId:X4}" : product;
    }

    /// Every LampArray collection that can be opened and described.
    public static IReadOnlyList<LampArrayDevice> Discover(IReadOnlyList<HidDeviceInfo> hid, Func<bool> dynamicLightingOn)
    {
        var found = new List<LampArrayDevice>();
        var lampArrays = hid.Where(d => d.UsagePage == LightingPage && d.Usage == LampArrayUsage).ToList();
        // The receiver's LampArray is its paired mouse; the receiver says which.
        OmniMouse? omni = null;
        if (lampArrays.Count(d => d.VendorId == AuraUsb.AsusVendorId && d.ProductId == OmniMouse.ReceiverProductId) == 1)
        {
            try
            {
                omni = OmniMouse.Find(hid);
            }
            catch (Exception e)
            {
                LightingLog.Discovery($"[rigstats-control] Lighting: Omni receiver mouse not read: {e.Message}");
            }
        }
        foreach (var info in lampArrays)
        {
            var receiver = info.VendorId == AuraUsb.AsusVendorId && info.ProductId == OmniMouse.ReceiverProductId;
            try
            {
                if (Open(info, found.Count + 1, found.Select(f => f._baseName).ToList(), dynamicLightingOn,
                        receiver ? omni : null) is { } device)
                    found.Add(device);
            }
            catch (Exception e)
            {
                LightingLog.Discovery($"[rigstats-control] Lighting: LampArray 0x{info.VendorId:X4}:0x{info.ProductId:X4} not usable: {e.Message}");
            }
        }
        return found;
    }

    /// `nth` keeps the device id stable across rescans (LightingProvider
    /// matches on it); the shown name is numbered only among equal names.
    private static LampArrayDevice? Open(HidDeviceInfo info, int nth, IReadOnlyList<string> namesSoFar,
        Func<bool> dynamicLightingOn, OmniMouse? omni)
    {
        var handle = CreateFile(info.Path, GenericRead | GenericWrite, FileShareReadWrite, IntPtr.Zero, OpenExisting, 0, IntPtr.Zero);
        if (handle.IsInvalid)
            return null;
        if (!HidD_GetPreparsedData(handle, out var preparsed))
        {
            handle.Dispose();
            return null;
        }
        var ok = false;
        try
        {
            HidP_GetCaps(preparsed, out var caps);
            var fields = Fields(preparsed, caps);
            if (!fields.TryGetValue(LampCount, out var attributes)
                || !fields.TryGetValue(LampIdStart, out var rangeUpdate)
                || !fields.TryGetValue(AutonomousMode, out var control))
            {
                LightingLog.Discovery($"[rigstats-control] Lighting: LampArray 0x{info.ProductId:X4} lacks a range update or control report.");
                return null;
            }

            var report = GetFeature(handle, preparsed, attributes.ReportId, caps.FeatureReportByteLength);
            var lamps = (int)Value(preparsed, HidReportType.Feature, LampCount, report);
            var kind = KindName(Value(preparsed, HidReportType.Feature, LampArrayKindUsage, report));
            var interval = Value(preparsed, HidReportType.Feature, MinUpdateInterval, report);
            var product = Product(handle) ?? $"LampArray {info.VendorId:X4}:{info.ProductId:X4}";
            var baseName = DisplayName(info.VendorId, info.ProductId, product, kind, omni?.Name);
            var same = namesSoFar.Count(n => n == baseName);
            var name = same == 0 ? baseName : $"{baseName} ({same + 1})";
            LightingLog.Discovery($"[rigstats-control] Lighting: LampArray '{product}' 0x{info.VendorId:X4}:0x{info.ProductId:X4}, " +
                $"{kind}, {lamps} lamp(s), min update {interval} µs.");
            ok = true;
            var diagnostics = new JsonObject
            {
                ["vendor_id"] = $"{info.VendorId:X4}",
                ["product_id"] = $"{info.ProductId:X4}",
                ["interface"] = info.Interface,
                ["product"] = product,
                ["lamp_array_kind"] = kind,
                ["lamp_count"] = lamps,
                ["min_update_interval_us"] = interval,
                ["bounding_box_um"] = new JsonArray(
                    Value(preparsed, HidReportType.Feature, 0x04, report),
                    Value(preparsed, HidReportType.Feature, 0x05, report),
                    Value(preparsed, HidReportType.Feature, 0x06, report)),
                // Which report carries each usage — the descriptor layout a fix needs.
                ["reports"] = new JsonObject(fields.OrderBy(f => f.Key).Select(f =>
                    new KeyValuePair<string, JsonNode?>($"0x{f.Key:X2}", $"{f.Value.Type} 0x{f.Value.ReportId:X2}"))),
            };
            return new LampArrayDevice(handle, preparsed, caps, $"lamparray-{info.VendorId:x4}-{info.ProductId:x4}-{nth}",
                baseName, name, kind, Math.Max(1, lamps), rangeUpdate, control, dynamicLightingOn, diagnostics, omni);
        }
        finally
        {
            if (!ok)
            {
                HidD_FreePreparsedData(preparsed);
                handle.Dispose();
            }
        }
    }

    public void Apply(AuraEffect effect, byte red, byte green, byte blue)
    {
        lock (_lock)
        {
            SetAutonomous(false);
            _hostControlled = true;
        }
        _loop.Start(effect, red, green, blue);
    }

    public JsonObject Diagnostics()
    {
        var diagnostics = _diagnostics.DeepClone().AsObject();
        if (_omni is not null)
            diagnostics["omni_mouse"] = _omni.Diagnostics();
        return diagnostics;
    }

    public bool HasWdl => _omni?.HasWdl == true;

    public bool KnownModel => _omni?.Name is not null;

    public bool? WdlOn() => _omni?.WdlOn();

    /// The battery of the mouse behind the Omni receiver; other LampArray
    /// devices have none here.
    public bool HasBattery => _omni?.HasBattery == true;

    public BatteryStatus? ReadBattery() => _omni?.ReadBattery();

    /// Only the Omni receiver's mouse reports a battery here — 2.4 GHz.
    public string Connection => Lighting.Connection.Radio;

    public void EnableWdl() =>
        (_omni ?? throw new InvalidOperationException($"{Name} has no lighting mode to switch.")).SetWdl(true);

    /// Back to the device's own effect.
    public void Release()
    {
        _loop.Stop();
        lock (_lock)
        {
            if (!_hostControlled)
                return;
            SetAutonomous(true);
            _hostControlled = false;
        }
    }

    private void Draw(byte red, byte green, byte blue)
    {
        lock (_lock)
        {
            var report = NewReport(_rangeUpdate);
            Set(_rangeUpdate.Type, LampUpdateFlags, UpdateComplete, report);
            Set(_rangeUpdate.Type, LampIdStart, 0, report);
            Set(_rangeUpdate.Type, LampIdEnd, (uint)(_lamps - 1), report);
            Set(_rangeUpdate.Type, RedUpdateChannel, red, report);
            Set(_rangeUpdate.Type, GreenUpdateChannel, green, report);
            Set(_rangeUpdate.Type, BlueUpdateChannel, blue, report);
            Set(_rangeUpdate.Type, IntensityUpdateChannel, 0xFF, report);
            Send(_rangeUpdate.Type, report);
        }
    }

    // Caller holds _lock.
    private void SetAutonomous(bool autonomous)
    {
        var report = NewReport(_control);
        Set(_control.Type, AutonomousMode, autonomous ? 1u : 0u, report);
        Send(_control.Type, report);
    }

    private byte[] NewReport(ReportField field)
    {
        var report = new byte[field.Type == HidReportType.Feature ? _featureLength : _outputLength];
        Check(HidP_InitializeReportForID((int)field.Type, field.ReportId, _preparsed, report, report.Length), "HidP_InitializeReportForID");
        return report;
    }

    private void Set(HidReportType type, ushort usage, uint value, byte[] report) =>
        Check(HidP_SetUsageValue((int)type, LightingPage, 0, usage, value, _preparsed, report, report.Length), $"HidP_SetUsageValue(0x{usage:X2})");

    private void Send(HidReportType type, byte[] report)
    {
        var ok = type == HidReportType.Feature
            ? HidD_SetFeature(_handle, report, report.Length)
            : HidD_SetOutputReport(_handle, report, report.Length);
        if (!ok)
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), $"{Name}: report not accepted");
    }

    public void Dispose()
    {
        try
        {
            Release();
        }
        catch
        {
            // Unplugged: nothing to hand back.
        }
        HidD_FreePreparsedData(_preparsed);
        _handle.Dispose();
    }

    // ── Descriptor helpers ─────────────────────────────────────────────

    /// Which report (type + id) carries each LampArray usage, from the
    /// value caps of the output and feature reports.
    private static Dictionary<ushort, ReportField> Fields(IntPtr preparsed, HidpCaps caps)
    {
        var fields = new Dictionary<ushort, ReportField>();
        foreach (var (type, count) in new[] { (HidReportType.Output, caps.NumberOutputValueCaps), (HidReportType.Feature, caps.NumberFeatureValueCaps) })
        {
            if (count == 0)
                continue;
            var n = count;
            var values = new HidpValueCaps[n];
            if (HidP_GetValueCaps((int)type, values, ref n, preparsed) != HidpStatusSuccess)
                continue;
            foreach (var v in values.Take(n).Where(v => v.UsagePage == LightingPage))
            {
                for (var usage = v.UsageMin; usage <= (v.IsRange != 0 ? v.UsageMax : v.UsageMin); usage++)
                    fields.TryAdd(usage, new ReportField(type, v.ReportID));
            }
        }
        return fields;
    }

    private static byte[] GetFeature(SafeFileHandle handle, IntPtr preparsed, byte reportId, int length)
    {
        var report = new byte[length];
        Check(HidP_InitializeReportForID((int)HidReportType.Feature, reportId, preparsed, report, length), "HidP_InitializeReportForID");
        if (!HidD_GetFeature(handle, report, length))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "LampArray attributes not read");
        return report;
    }

    private static uint Value(IntPtr preparsed, HidReportType type, ushort usage, byte[] report) =>
        HidP_GetUsageValue((int)type, LightingPage, 0, usage, out var value, preparsed, report, report.Length) == HidpStatusSuccess
            ? value
            : 0;

    private static string? Product(SafeFileHandle handle)
    {
        var buffer = new byte[256];
        return HidD_GetProductString(handle, buffer, buffer.Length)
            ? System.Text.Encoding.Unicode.GetString(buffer).TrimEnd('\0').Trim()
            : null;
    }

    private static void Check(int status, string call)
    {
        if (status != HidpStatusSuccess)
            throw new InvalidOperationException($"{call} failed (0x{status:X8}).");
    }

    // ── Windows Dynamic Lighting ───────────────────────────────────────

    /// Whether any signed-in user has Windows Dynamic Lighting on (Settings
    /// → Personalization → Dynamic Lighting → "Use Dynamic Lighting on my
    /// devices"). The service runs as SYSTEM, so it reads the loaded user
    /// hives under HKEY_USERS rather than HKCU.
    public static bool WindowsDynamicLightingOn()
    {
        try
        {
            foreach (var sid in Registry.Users.GetSubKeyNames().Where(s => s.StartsWith("S-1-5-21-") && !s.EndsWith("_Classes")))
            {
                using var key = Registry.Users.OpenSubKey($@"{sid}\Software\Microsoft\Lighting");
                if (key?.GetValue("AmbientLightingEnabled") is int enabled && enabled != 0)
                    return true;
            }
        }
        catch
        {
            // Can't tell — assume off rather than hide devices.
        }
        return false;
    }

    // ── Win32 ──────────────────────────────────────────────────────────

    private const uint GenericRead = 0x80000000;
    private const uint GenericWrite = 0x40000000;
    private const uint FileShareReadWrite = 3;
    private const uint OpenExisting = 3;
    private const int HidpStatusSuccess = 0x00110000;

    [StructLayout(LayoutKind.Sequential)]
    private struct HidpCaps
    {
        public ushort Usage;
        public ushort UsagePage;
        public ushort InputReportByteLength;
        public ushort OutputReportByteLength;
        public ushort FeatureReportByteLength;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 17)]
        public ushort[] Reserved;
        public ushort NumberLinkCollectionNodes;
        public ushort NumberInputButtonCaps;
        public ushort NumberInputValueCaps;
        public ushort NumberInputDataIndices;
        public ushort NumberOutputButtonCaps;
        public ushort NumberOutputValueCaps;
        public ushort NumberOutputDataIndices;
        public ushort NumberFeatureButtonCaps;
        public ushort NumberFeatureValueCaps;
        public ushort NumberFeatureDataIndices;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct HidpValueCaps
    {
        public ushort UsagePage;
        public byte ReportID;
        public byte IsAlias;
        public ushort BitField;
        public ushort LinkCollection;
        public ushort LinkUsage;
        public ushort LinkUsagePage;
        public byte IsRange;
        public byte IsStringRange;
        public byte IsDesignatorRange;
        public byte IsAbsolute;
        public byte HasNull;
        public byte Reserved;
        public ushort BitSize;
        public ushort ReportCount;
        public ushort Reserved2a, Reserved2b, Reserved2c, Reserved2d, Reserved2e;
        public uint UnitsExp;
        public uint Units;
        public int LogicalMin, LogicalMax, PhysicalMin, PhysicalMax;
        public ushort UsageMin, UsageMax, StringMin, StringMax, DesignatorMin, DesignatorMax, DataIndexMin, DataIndexMax;
    }

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern SafeFileHandle CreateFile(string name, uint access, uint share, IntPtr security,
        uint disposition, uint flags, IntPtr template);

    [DllImport("hid.dll", SetLastError = true)]
    private static extern bool HidD_GetPreparsedData(SafeFileHandle device, out IntPtr preparsed);

    [DllImport("hid.dll")]
    private static extern bool HidD_FreePreparsedData(IntPtr preparsed);

    [DllImport("hid.dll", SetLastError = true)]
    private static extern bool HidD_GetFeature(SafeFileHandle device, byte[] report, int length);

    [DllImport("hid.dll", SetLastError = true)]
    private static extern bool HidD_SetFeature(SafeFileHandle device, byte[] report, int length);

    [DllImport("hid.dll", SetLastError = true)]
    private static extern bool HidD_SetOutputReport(SafeFileHandle device, byte[] report, int length);

    [DllImport("hid.dll", SetLastError = true)]
    private static extern bool HidD_GetProductString(SafeFileHandle device, byte[] buffer, int length);

    [DllImport("hid.dll")]
    private static extern int HidP_GetCaps(IntPtr preparsed, out HidpCaps caps);

    [DllImport("hid.dll")]
    private static extern int HidP_GetValueCaps(int type, [Out] HidpValueCaps[] caps, ref ushort length, IntPtr preparsed);

    [DllImport("hid.dll")]
    private static extern int HidP_InitializeReportForID(int type, byte reportId, IntPtr preparsed, byte[] report, int length);

    [DllImport("hid.dll")]
    private static extern int HidP_SetUsageValue(int type, ushort page, ushort link, ushort usage, uint value,
        IntPtr preparsed, byte[] report, int length);

    [DllImport("hid.dll")]
    private static extern int HidP_GetUsageValue(int type, ushort page, ushort link, ushort usage, out uint value,
        IntPtr preparsed, byte[] report, int length);
}
