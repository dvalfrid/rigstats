using System.Text.Json;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control;

public sealed partial class ControlPipeWorker
{
    // The device pages' answers, built here for the pipe and for the shared
    // contract examples the app's reader checks too (#679).
    internal static JsonObject HeadsetsReply(Lighting.LightingProvider lighting) => new() { ["headsets"] = lighting.Headsets() };
    internal static JsonObject KeyboardsReply(Lighting.LightingProvider lighting) => new() { ["keyboards"] = lighting.KeyboardSettingsList() };
    internal static JsonObject MiceReply(Mice.MouseProvider mice) => new() { ["mice"] = mice.List() };

    /// `{"aura": {...}}` — sets the lights at once without saving anything:
    /// the Lighting tab's live preview while a colour is being picked.
    /// Harmless, so it skips the broker transaction.
    private ControlResponse HandleAuraPreview(ControlRequest request)
    {
        var aura = ObjectParam<AuraPart>(request, "aura");
        lighting.Preview(aura);
        return ControlResponse.Ok(request.Id, new { ok = true });
    }

    /// Switches the desk lamp(s) off or back on (the tray). Like the
    /// preview, harmless and unsaved, so it skips the broker transaction.
    private ControlResponse HandleLampToggle(ControlRequest request) =>
        lighting.ToggleLamp() is { } on
            ? ControlResponse.Ok(request.Id, new { ok = true, on })
            : ControlResponse.Fail(request.Id, "no_lamp", "No lighting device with a lamp is connected.");

    /// `{id, settings}`: change a mouse's settings in its active onboard
    /// profile (#431) — answered with the mouse as read back.
    private ControlResponse HandleMouseSet(ControlRequest request)
    {
        var id = StringParam(request, "id");
        var settings = Param(request, "settings") is { ValueKind: JsonValueKind.Object } value
            ? ObjectParam<Mice.MouseSettings>(value, "settings")
            : throw new BadRequestException("'settings' must be an object.");
        return ControlResponse.Ok(request.Id, new { mouse = mice.Set(id, settings) });
    }

    /// `{id, settings}`: change a headset's sidetone and noise reduction
    /// (#428) — answered with the headset as read back.
    private ControlResponse HandleHeadsetSet(ControlRequest request)
    {
        var id = StringParam(request, "id");
        var settings = Param(request, "settings") is { ValueKind: JsonValueKind.Object } value
            ? ObjectParam<Lighting.HeadsetSettings>(value, "settings")
            : throw new BadRequestException("'settings' must be an object.");
        return ControlResponse.Ok(request.Id, new { headset = lighting.SetHeadset(id, settings) });
    }

    private ControlResponse HandleKeyboardSet(ControlRequest request)
    {
        var id = StringParam(request, "id");
        var settings = Param(request, "settings") is { ValueKind: JsonValueKind.Object } value
            ? ObjectParam<Lighting.KeyboardSettings>(value, "settings")
            : throw new BadRequestException("'settings' must be an object.");
        return ControlResponse.Ok(request.Id, new { keyboard = lighting.SetKeyboard(id, settings) });
    }

    /// `{"id": …, "eq": {"on": true, "gains": [10 × gain]}}` — one headset's
    /// equalizer (#483); answers the headset as `headset_settings` lists it.
    private ControlResponse HandleHeadsetEqSet(ControlRequest request)
    {
        var id = StringParam(request, "id");
        var eq = Param(request, "eq") is { ValueKind: JsonValueKind.Object } value
            ? ObjectParam<Lighting.HeadsetEq>(value, "eq")
            : throw new BadRequestException("'eq' must be an object.");
        if (eq.Gains is null || eq.Gains.Length != Lighting.HeadsetEqCommands.Bands)
            throw new BadRequestException($"'eq.gains' must have {Lighting.HeadsetEqCommands.Bands} values.");
        return ControlResponse.Ok(request.Id, new { headset = lighting.SetHeadsetEq(id, eq) });
    }

    /// `{"id": "lamparray-0b05-1ace-2"}` — lets the device follow lighting
    /// writes (#236). Changes a setting saved in the device, so only on the
    /// user's request from the Lighting tab.
    private ControlResponse HandleAuraEnableWdl(ControlRequest request)
    {
        var id = StringParam(request, "id");
        lighting.EnableWdl(id);
        return ControlResponse.Ok(request.Id, new { ok = true });
    }
}
