using System.Text.Json;

namespace SensorSidecar.Control;

public sealed partial class ControlPipeWorker
{
    /// An IP address from the client (#378): parsed, so nothing but an address
    /// becomes the host of an HTTPS request or goes into the log.
    private static string IpParam(string text) =>
        System.Net.IPAddress.TryParse(text.Trim(), out var address)
            && address.AddressFamily is System.Net.Sockets.AddressFamily.InterNetwork or System.Net.Sockets.AddressFamily.InterNetworkV6
            ? address.ToString()
            : throw new BadRequestException($"'{Printable(text)}' is not an IP address.");

    /// `{"ip": "192.168.1.20"}` (optional) — the Hue Bridges on the network
    /// (mDNS), or only the one at `ip`.
    private static async Task<ControlResponse> HandleHueDiscoverAsync(ControlRequest request, CancellationToken ct)
    {
        string? ip = null;
        if (request.Params is { ValueKind: JsonValueKind.Object } p && p.TryGetProperty("ip", out var value)
            && value.ValueKind != JsonValueKind.Null)
        {
            var text = value.ValueKind == JsonValueKind.String
                ? value.GetString()!
                : throw new BadRequestException("'ip' must be a string.");
            if (!string.IsNullOrWhiteSpace(text))
                ip = IpParam(text);
        }
        var bridges = await Lighting.HueLink.DiscoverAsync(ip, ct);
        SidecarLog.Log($"[rigstats-control] Hue: search{(ip is null ? "" : $" at {ip}")} found " +
            (bridges.Count == 0 ? "no bridge." : string.Join(", ", bridges.Select(b => $"{b.Name} ({b.Model}, {b.Ip})")) + "."));
        return ControlResponse.Ok(request.Id, new
        {
            bridges = bridges.Select(b => new { id = b.Id, name = b.Name, model = b.Model, ip = b.Ip }),
        });
    }

    /// `{"ip": "..."}` — pairs once the bridge's link button was pressed.
    private async Task<ControlResponse> HandleHuePairAsync(ControlRequest request, CancellationToken ct)
    {
        var ip = IpParam(StringParam(request, "ip"));
        if (!await hue.PairAsync(ip, ct))
            return ControlResponse.Fail(request.Id, "link_button", "Press the round link button on the Hue Bridge, then Pair again.");
        lighting.Rediscover();
        return ControlResponse.Ok(request.Id, new { ok = true });
    }

    private async Task<ControlResponse> HandleHueRefreshAsync(ControlRequest request, CancellationToken ct)
    {
        await hue.RefreshGroupsAsync(ct);
        lighting.Rediscover();
        return ControlResponse.Ok(request.Id, new { ok = true });
    }

    /// `{"groups": ["<room or zone id>", ...]}` — what follows the rig.
    private ControlResponse HandleHueChoose(ControlRequest request)
    {
        var groups = ObjectParam<List<string>>(request, "groups");
        hue.Choose(groups);
        lighting.Rediscover();
        return ControlResponse.Ok(request.Id, new { ok = true });
    }

    private ControlResponse HandleHueUnpair(ControlRequest request)
    {
        hue.Unpair();
        lighting.Rediscover();
        return ControlResponse.Ok(request.Id, new { ok = true });
    }
}
