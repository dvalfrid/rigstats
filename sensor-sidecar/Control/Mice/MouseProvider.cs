using System.Text.Json.Nodes;
using SensorSidecar.Control.Lighting;

namespace SensorSidecar.Control.Mice;

/// ROG mice (#431): the `mouse` profile domain — which onboard profile a
/// RIGStats profile uses — and the Mouse page's settings (`mouse_settings` /
/// `mouse_set`), which live in the mouse. Mice are found by their id in
/// <see cref="MouseCatalog"/> only: their own collection (cable, dongle) or
/// the Omni receiver's paired list; nothing is sent to an unknown device.
/// Found again when the set of HID collections changes.
public sealed class MouseProvider : IControlProvider, IAffectsPart, IDisposable
{
    private readonly Func<IReadOnlyList<HidDeviceInfo>> _enumerate;
    private readonly Func<IReadOnlyList<HidDeviceInfo>, IReadOnlyList<IMouseDevice>> _discover;
    private readonly bool _dryRun;
    private readonly object _scanLock = new();
    private IReadOnlyList<IMouseDevice> _mice = [];
    private string? _fingerprint;

    public MouseProvider(
        Func<IReadOnlyList<HidDeviceInfo>> enumerate,
        Func<IReadOnlyList<HidDeviceInfo>, IReadOnlyList<IMouseDevice>> discover,
        bool dryRun)
    {
        _enumerate = enumerate;
        _discover = discover;
        _dryRun = dryRun;
    }

    /// A fixed list (tests).
    public MouseProvider(IReadOnlyList<IMouseDevice> mice, bool dryRun)
        : this(() => [], _ => mice, dryRun)
    {
        _mice = mice;
        _fingerprint = "";
    }

    public string Domain => "mouse";

    public bool Affects(ProfilePart part) => part.Mouse?.Profile is not null;

    public CapabilitySet Probe()
    {
        var mice = Mice();
        if (mice.Count == 0)
            return new CapabilitySet { Domain = Domain, Supported = false, Reason = "No ROG mouse with known settings found." };
        return new CapabilitySet
        {
            Domain = Domain,
            Supported = true,
            Details = new JsonObject
            {
                ["mice"] = new JsonArray(mice.Select(m => (JsonNode)new JsonObject
                {
                    ["id"] = m.Id,
                    ["name"] = m.Entry.Spec.Name,
                    ["profiles"] = m.Entry.Spec.Profiles,
                }).ToArray()),
            },
        };
    }

    public ValidationResult Validate(ProfilePart part) =>
        part.Mouse?.Profile is { } p && p < 1
            ? ValidationResult.Failure("The mouse profile must be 1 or more.")
            : ValidationResult.Success();

    /// The onboard profile each mouse uses now.
    public Snapshot Capture()
    {
        var state = new JsonObject();
        foreach (var mouse in Mice())
        {
            if (mouse.Read() is { } now)
                state[mouse.Id] = now.Profile;
        }
        return new Snapshot { Domain = Domain, State = state };
    }

    public void Apply(ProfilePart part)
    {
        if (part.Mouse?.Profile is not { } profile)
            return;
        var mice = Mice();
        if (mice.Count == 0)
            throw new DomainUnavailableException("No ROG mouse is connected, so its profile was skipped.");
        // Every mouse is tried; one that doesn't answer (asleep, out of range)
        // doesn't keep the others from switching (#484). The rest of the
        // profile still applies.
        var silent = new List<string>();
        foreach (var mouse in mice)
        {
            // A mouse with one onboard profile (Gladius II Origin, Strix Impact, #432) has nothing to switch.
            if (mouse.Entry.Spec.Profiles <= 1)
                continue;
            var p = Math.Clamp(profile, 1, mouse.Entry.Spec.Profiles);
            if (_dryRun)
            {
                SidecarLog.Log($"[rigstats-control] dry-run: {mouse.Entry.Spec.Name} -> onboard profile {p}");
                continue;
            }
            try
            {
                mouse.SwitchProfile(p);
                SidecarLog.Log($"[rigstats-control] Mouse: {mouse.Entry.Spec.Name} on onboard profile {p}.");
            }
            catch (IOException e)
            {
                SidecarLog.Log($"[rigstats-control] Mouse: {mouse.Entry.Spec.Name} not switched: {e.Message}");
                silent.Add(mouse.Entry.Spec.Name);
            }
        }
        if (silent.Count > 0)
            throw DomainUnavailableException.Silent(silent);
    }

    public bool Verify(ProfilePart part)
    {
        if (part.Mouse?.Profile is not { } profile || _dryRun)
            return true;
        return Mice().All(m => m.Read() is not { } now || now.Profile == Math.Clamp(profile, 1, m.Entry.Spec.Profiles));
    }

    public void Restore(Snapshot snapshot)
    {
        if (snapshot.Domain != Domain || snapshot.State is not JsonObject state || _dryRun)
            return;
        foreach (var mouse in Mice())
        {
            if (state[mouse.Id]?.GetValue<int>() is { } profile)
            {
                try
                {
                    mouse.SwitchProfile(profile);
                }
                catch (IOException)
                {
                    // Best effort: the mouse keeps whichever profile it has.
                }
            }
        }
    }

    /// The mouse keeps its onboard profile — nothing RIGStats holds.
    public string? ReleaseToFirmware() => null;

    /// Every known mouse with its spec and current settings (`mouse_settings`).
    public JsonArray List()
    {
        return new JsonArray(Mice().Select(m => (JsonNode)Json(m, SafeRead(m))).ToArray());
    }

    /// Changes one mouse's settings in its active onboard profile and answers
    /// with what it read back (`mouse_set`).
    public JsonObject Set(string id, MouseSettings wanted)
    {
        if (Mice().FirstOrDefault(m => m.Id == id) is not { } mouse)
            throw new InvalidOperationException($"No mouse '{id}' with settings to change.");
        if (_dryRun)
        {
            SidecarLog.Log($"[rigstats-control] dry-run: {mouse.Entry.Spec.Name} settings -> {AsusMouseDevice.Describe(wanted)}");
            return Json(mouse, wanted);
        }
        var back = mouse.Write(wanted);
        SidecarLog.Log($"[rigstats-control] Mouse: {mouse.Entry.Spec.Name} {AsusMouseDevice.Describe(back)}.");
        return Json(mouse, back);
    }

    private static MouseSettings? SafeRead(IMouseDevice mouse)
    {
        try
        {
            return mouse.Read();
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] Mouse: {mouse.Entry.Spec.Name} settings not read: {e.Message}");
            return null;
        }
    }

    private static JsonObject Json(IMouseDevice mouse, MouseSettings? settings)
    {
        var spec = mouse.Entry.Spec;
        return new JsonObject
        {
            ["id"] = mouse.Id,
            ["name"] = spec.Name,
            ["link"] = mouse.Entry.Link.ToString().ToLowerInvariant(),
            ["verified"] = MouseCatalog.Verified.Contains((mouse.Entry.ProductId, mouse.Entry.Link)),
            ["spec"] = new JsonObject
            {
                ["profiles"] = spec.Profiles,
                ["dpi_stages"] = spec.DpiStages,
                ["dpi_min"] = spec.Dpi.Min,
                ["dpi_max"] = spec.Dpi.Max,
                ["dpi_step"] = spec.Dpi.Step,
                ["dpi_colors"] = spec.DpiColors,
                ["polling_hz"] = new JsonArray(MouseProtocol.Rates(spec, mouse.Booster).Select(r => (JsonNode)r).ToArray()),
                ["debounce"] = spec.Debounce is { } d ? new JsonArray(d.Min, d.Max) : null,
                ["lift_off"] = spec.LiftOff,
                ["angle_snapping"] = spec.AngleSnapping,
                ["angle_tuning"] = spec.AngleTuning is { } a ? new JsonArray(a.Min, a.Max, a.Step) : null,
                ["acceleration"] = spec.Acceleration,
                ["deceleration"] = spec.Deceleration,
                ["motion_sync"] = spec.MotionSync,
                ["auto_power_off"] = spec.AutoPowerOff,
                ["low_battery_warning"] = spec.LowBatteryWarning is { } lb ? new JsonArray(lb.Step, lb.Max) : null,
                // Bindable buttons and what they can do (#433).
                ["buttons"] = spec.Buttons
                    ? new JsonArray(MouseProtocol.Buttons.Select(b => (JsonNode)new JsonObject { ["source"] = b.Source, ["name"] = b.Name }).ToArray())
                    : null,
                ["dpi_button"] = spec.DpiButton,
                ["can_switch_stage"] = spec.CanSwitchStage,
                ["actions"] = spec.Buttons
                    ? new JsonArray(MouseProtocol.Actions.Select(a => (JsonNode)new JsonObject { ["code"] = a.Code, ["name"] = a.Name }).ToArray())
                    : null,
            },
            ["settings"] = settings is null ? null : new JsonObject
            {
                ["profile"] = settings.Profile,
                ["active_stage"] = settings.ActiveStage,
                ["stages"] = new JsonArray(settings.Stages.Select(s => (JsonNode)new JsonObject { ["dpi"] = s.Dpi, ["color"] = s.Color }).ToArray()),
                ["polling_hz"] = settings.PollingHz,
                ["debounce"] = settings.Debounce,
                ["angle_snapping"] = settings.AngleSnapping,
                ["angle_tuning"] = settings.AngleTuning,
                ["acceleration"] = settings.Acceleration,
                ["deceleration"] = settings.Deceleration,
                ["motion_sync"] = settings.MotionSync,
                ["lift_off"] = settings.LiftOff,
                ["power_off"] = settings.PowerOff,
                ["low_battery_warning"] = settings.LowBatteryWarning,
                ["buttons"] = settings.Buttons is { } buttons ? new JsonArray(buttons.Select(b => (JsonNode)b).ToArray()) : null,
            },
        };
    }

    private IReadOnlyList<IMouseDevice> Mice()
    {
        lock (_scanLock)
        {
            var hid = _enumerate();
            var fingerprint = string.Join("|", hid.Where(h => h.VendorId == AuraUsb.AsusVendorId).Select(h => h.Path).Order());
            if (fingerprint == _fingerprint)
                return _mice;
            foreach (var old in _mice.OfType<IDisposable>())
                old.Dispose();
            _mice = _discover(hid);
            _fingerprint = fingerprint;
            foreach (var mouse in _mice)
                SidecarLog.Log($"[rigstats-control] Mouse: {mouse.Entry.Spec.Name} 0x{mouse.Entry.ProductId:X4} ({mouse.Entry.Link.ToString().ToLowerInvariant()})"
                    + (mouse.Booster ? ", booster on." : "."));
            return _mice;
        }
    }

    /// The known mice among `hid`: by id on their own collection (the
    /// interface, report id and vendor page the catalog names), and on the
    /// Omni receiver by its paired list.
    public static IReadOnlyList<IMouseDevice> Discover(IReadOnlyList<HidDeviceInfo> hid) =>
        Plan(hid).Select(c => (IMouseDevice)Open(c.Channel, c.Entry, c.Id)).ToList();

    /// A mouse to open: its settings collection, catalog entry and id.
    public sealed record Candidate(HidDeviceInfo Channel, MouseEntry Entry, string Id);

    /// What <see cref="Discover"/> opens, without opening anything (tested).
    /// Every mouse of a model is found — two identical mice on cable are two
    /// collections on two USB devices — the second and later with `-2`, `-3`
    /// on its id (#484). A mouse on its cable whose model the Omni receiver
    /// also lists is taken once, by cable, as keyboards are (#484; that a
    /// charging mouse stays in the receiver's list is assumed from the
    /// keyboard, not yet seen with a mouse).
    public static IReadOnlyList<Candidate> Plan(IReadOnlyList<HidDeviceInfo> hid)
    {
        var found = new List<Candidate>();
        foreach (var group in hid.Where(h => h.VendorId == AuraUsb.AsusVendorId).GroupBy(h => h.ProductId))
        {
            foreach (var entry in MouseCatalog.Entries.Where(e => e.ProductId == group.Key && e.Link != MouseLink.Omni))
            {
                // One channel per USB device: a device's collections share
                // its instance, so a second matching collection on the same
                // device isn't a second mouse.
                var channels = group.Where(h => IsChannel(h, entry))
                    .GroupBy(h => DeviceInstance(h.Path)).Select(g => g.OrderBy(h => h.Path, StringComparer.Ordinal).First())
                    .OrderBy(h => h.Path, StringComparer.Ordinal).ToList();
                if (channels.Count == 0)
                    continue;
                var id = $"asus-mouse-{entry.ProductId:x4}-{entry.Link.ToString().ToLowerInvariant()}";
                for (var i = 0; i < channels.Count; i++)
                    found.Add(new Candidate(channels[i], entry, i == 0 ? id : $"{id}-{i + 1}"));
                break;
            }
        }
        // Every Omni receiver's own paired list (#296); a second mouse of the
        // same model on another receiver gets `-2` on its id.
        foreach (var receiver in hid.Any(h => h.VendorId == AuraUsb.AsusVendorId && h.ProductId == OmniMouse.ReceiverProductId)
                     ? OmniMouse.Receivers(hid) : [])
        {
            foreach (var paired in OmniMouse.PairedDevices(receiver))
            {
                if (MouseCatalog.Find(paired.ProductId, MouseLink.Omni) is not { } entry
                    || receiver.Channel(paired.ReportId) is not { } channel)
                    continue;
                if (CabledToo(found, entry))
                {
                    SidecarLog.Log($"[rigstats-control] Mouse: {entry.Spec.Name} is connected by cable too — the receiver's entry is left out.");
                    continue;
                }
                var id = $"asus-mouse-{entry.ProductId:x4}-omni";
                var same = found.Count(c => c.Id == id || c.Id.StartsWith(id + "-", StringComparison.Ordinal));
                found.Add(new Candidate(channel, entry with { ReportId = paired.ReportId }, same == 0 ? id : $"{id}-{same + 1}"));
            }
        }
        return found;
    }

    /// The receiver's mouse `omni` is already found on its cable — the same
    /// model among the wired ones. Two different mice of one model, one by
    /// cable and one by receiver, are taken as one (as for keyboards).
    public static bool CabledToo(IEnumerable<Candidate> found, MouseEntry omni) =>
        found.Any(c => c.Entry.Link == MouseLink.Wired && c.Entry.Spec.Name == omni.Spec.Name);

    /// The USB device a HID collection belongs to: its instance id without
    /// the collection's own last part — `…#8&31cacbe7&0&0002#…` → `8&31cacbe7&0`.
    public static string DeviceInstance(string path) => UsbParent.Instance(path);

    /// `h` is the settings collection of a mouse on its own cable or dongle:
    /// the catalog's interface and report id, on a vendor page.
    public static bool IsChannel(HidDeviceInfo h, MouseEntry entry) =>
        h.VendorId == AuraUsb.AsusVendorId && h.ProductId == entry.ProductId && entry.Link != MouseLink.Omni
        && h.Interface == Interface(entry.Endpoint) && h.UsagePage >= 0xFF00
        && h.OutputReportLength > 0 && (h.OutputReportId ?? 0) == entry.ReportId;

    private static AsusMouseDevice Open(HidDeviceInfo channel, MouseEntry entry, string id) => new(channel, entry, id);

    /// "mi_00" → 0, "mi_02&col03" → 2.
    public static int? Interface(string endpoint) =>
        endpoint.StartsWith("mi_", StringComparison.OrdinalIgnoreCase)
        && int.TryParse(endpoint.AsSpan(3, 2), System.Globalization.NumberStyles.HexNumber, null, out var mi) ? mi : null;

    public void Dispose()
    {
        lock (_scanLock)
        {
            foreach (var mouse in _mice.OfType<IDisposable>())
                mouse.Dispose();
            _mice = [];
        }
    }
}
