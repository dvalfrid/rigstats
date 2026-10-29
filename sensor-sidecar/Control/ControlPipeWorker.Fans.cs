using SensorSidecar.Control.Providers;

namespace SensorSidecar.Control;

public sealed partial class ControlPipeWorker
{
    private ControlResponse HandleReleaseToFirmware(ControlRequest request)
    {
        safetyGuard.ReleaseAllToFirmware(SafetyGuard.AskedFromControlCenter);
        return ControlResponse.Ok(request.Id, new { ok = true });
    }

    /// Answers at once and runs the 3 s spin in the background — the control
    /// pipe serves one request at a time, so awaiting it would stall the UI.
    private ControlResponse HandleIdentifyFan(ControlRequest request, Connection connection)
    {
        var header = StringParam(request, "header");
        if (!fanProvider.HasHeader(header))
            return ControlResponse.Fail(request.Id, "not_found", $"fan header '{header}' not found.");

        _ = IdentifyInBackgroundAsync(header, connection);
        return ControlResponse.Ok(request.Id, new { ok = true, duration_ms = (int)FanProvider.IdentifyDuration.TotalMilliseconds });
    }

    /// The measured channel→fan mapping goes to the service log (and so the
    /// diagnostics ZIP, #207) and back to the requesting client. If that
    /// client has gone, the event just lands in its abandoned queue.
    private async Task IdentifyInBackgroundAsync(string header, Connection connection)
    {
        try
        {
            var result = await fanProvider.IdentifyAsync(header, FanProvider.IdentifyDuration, CancellationToken.None);
            if (result is null)
                return; // superseded or released before it finished.
            SidecarLog.Log($"[rigstats-control] Identify {DescribeIdentify(result)}");
            if (connection.Subscribed)
                connection.Events.Writer.TryWrite(ToIdentifiedMessage(result));
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] identify_fan {header} failed: {e}");
        }
    }

    internal static string DescribeIdentify(FanIdentifyResult result) =>
        result.Responders.Count == 0
            ? $"{result.Header} -> no RPM change (nothing connected, already at full speed, or not this channel)"
            : $"{result.Header} -> " + string.Join(", ", result.Responders.Select(r =>
                $"{r.Label} ({r.BeforeRpm:F0} -> {r.PeakRpm:F0} rpm)"));

    internal static ControlEventMessage ToIdentifiedMessage(FanIdentifyResult result) => new()
    {
        Event = "fan_identified",
        Data = new
        {
            header = result.Header,
            responders = result.Responders
                .Select(r => new { label = r.Label, before_rpm = r.BeforeRpm, peak_rpm = r.PeakRpm })
                .ToList(),
        },
    };
}
