using SensorSidecar.Control.Lighting;

namespace SensorSidecar.Control.Keyboards;

/// Follows a profile switched on the keyboard itself (#459): Fn + 1…6 on an
/// ASUS keyboard sends `70 00 00 <bit>` on one of its input-only collections,
/// and RIGStats switches to the Control Center profile that uses that onboard
/// profile (its keyboard part, #458) — the first in the list when several
/// do, nothing when none does. Only listens: nothing is sent to the keyboard
/// here. The same collection carries the key-state reports (`71`, every key
/// press); they are dropped unread, never logged or kept. A switch from the
/// host raises no event, so RIGStats' own switch doesn't come back here.
public sealed class KeyboardFollower(LightingProvider lighting, ProfileStore profiles, ControlBroker broker)
    : GuardedBackgroundService
{
    internal static readonly TimeSpan ScanInterval = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan ReadTimeout = TimeSpan.FromSeconds(1);

    private readonly Dictionary<string, CancellationTokenSource> _listening = [];

    /// A profile was switched to because of the keyboard — its id, for the
    /// app (`profile_changed`).
    public event Action<string>? ProfileSwitched;

    protected override async Task RunAsync(CancellationToken stoppingToken)
    {
        try
        {
            while (!stoppingToken.IsCancellationRequested)
            {
                Watch(EventCollections(lighting.Keyboards(), Hid.Enumerate()), stoppingToken);
                await Task.Delay(ScanInterval, stoppingToken);
            }
        }
        catch (OperationCanceledException) { }
        finally
        {
            foreach (var cts in _listening.Values)
                cts.Cancel();
            _listening.Clear();
        }
    }

    /// The collections a keyboard's own events may arrive on: its product
    /// id's input-only collections with room for the event, the standard
    /// keyboard / pointer interface (mi_00) left out. On the Omni receiver
    /// that is mi_03 (pages FFC0–FFC2; the event arrived on FFC0, report 4).
    public static IReadOnlyList<HidDeviceInfo> EventCollections(
        IReadOnlyList<IKeyboardProfileDevice> keyboards, IReadOnlyList<HidDeviceInfo> hid)
    {
        var products = keyboards.Select(k => k.ProductId).ToHashSet();
        return hid.Where(h => h.VendorId == AuraUsb.AsusVendorId && products.Contains(h.ProductId)
                && h.OutputReportLength == 0 && h.InputReportLength >= 5 && h.Interface != 0)
            .ToList();
    }

    private void Watch(IReadOnlyList<HidDeviceInfo> collections, CancellationToken stoppingToken)
    {
        var paths = collections.Select(c => c.Path).ToHashSet();
        foreach (var gone in _listening.Keys.Where(p => !paths.Contains(p)).ToList())
        {
            _listening[gone].Cancel();
            _listening.Remove(gone);
        }
        foreach (var info in collections.Where(c => !_listening.ContainsKey(c.Path)))
        {
            var cts = CancellationTokenSource.CreateLinkedTokenSource(stoppingToken);
            _listening[info.Path] = cts;
            _ = Task.Run(() => ListenAsync(info, cts.Token), CancellationToken.None);
        }
    }

    private async Task ListenAsync(HidDeviceInfo info, CancellationToken ct)
    {
        try
        {
            using var device = Hid.Open(info);
            while (!ct.IsCancellationRequested)
            {
                var report = device.Read(ReadTimeout);
                if (KeyboardProfileCommands.ProfileFromEvent(report, report?.Length ?? 0) is { } onboard)
                    await FollowAsync(onboard, ct);
            }
        }
        catch (Exception e) when (e is IOException or System.ComponentModel.Win32Exception or ObjectDisposedException)
        {
            // Unplugged or re-enumerated: the next scan opens it again.
            if (_listening.TryGetValue(info.Path, out var cts) && cts.Token == ct)
                _listening.Remove(info.Path);
        }
        catch (OperationCanceledException) { }
    }

    private async Task FollowAsync(int onboard, CancellationToken ct)
    {
        var target = PickProfile(await profiles.ListAsync(ct), onboard);
        if (target is null)
        {
            SidecarLog.Log($"[rigstats-control] Keyboard: switched to its profile {onboard} on the keyboard; no profile uses it.");
            return;
        }
        if (await profiles.GetActiveIdAsync(ct) == target.Id)
            return;
        var result = await broker.ApplyProfileAsync(target, ct);
        SidecarLog.Log($"[rigstats-control] Keyboard: its profile {onboard} switched on the keyboard -> '{target.Id}' ({(result.Ok ? "applied" : result.Message)}).");
        if (result.Ok)
        {
            await profiles.SetActiveAsync(target.Id, ct);
            ProfileSwitched?.Invoke(target.Id);
        }
    }

    /// The profile that uses onboard profile `onboard` — the first in the
    /// list when several do.
    public static Profile? PickProfile(IReadOnlyList<Profile> list, int onboard) =>
        list.FirstOrDefault(p => p.Part.Keyboard?.Profile == onboard);
}
