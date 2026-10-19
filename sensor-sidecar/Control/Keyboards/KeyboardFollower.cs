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
            lock (_listening)
            {
                foreach (var cts in _listening.Values)
                    cts.Cancel();
                _listening.Clear();
            }
        }
    }

    /// The collections a keyboard's own events may arrive on: its product
    /// id's input-only vendor collections (usage page FFxx) with room for the
    /// event. The event arrived on page FFC0 both through the Omni receiver
    /// (mi_03&col04, report 4) and by cable (1C24's mi_02&col03, report 3,
    /// 2026-10-18). The standard keyboard collections (page 1 — by cable a
    /// second one, mi_03, that Windows keeps and can't be opened) and the
    /// consumer-control one are never read.
    public static IReadOnlyList<HidDeviceInfo> EventCollections(
        IReadOnlyList<IKeyboardProfileDevice> keyboards, IReadOnlyList<HidDeviceInfo> hid)
    {
        var products = keyboards.Select(k => k.ProductId).ToHashSet();
        return hid.Where(h => h.VendorId == AuraUsb.AsusVendorId && products.Contains(h.ProductId)
                && h.UsagePage >= 0xFF00 && h.OutputReportLength == 0 && h.InputReportLength >= 5)
            .ToList();
    }

    // `_listening` is touched by the scan loop and by each listener's end,
    // so always under its own lock: concurrent writes once corrupted it
    // (IndexOutOfRangeException, the service restarting the follower and
    // missing a switch — by cable, #459).
    private void Watch(IReadOnlyList<HidDeviceInfo> collections, CancellationToken stoppingToken)
    {
        var paths = collections.Select(c => c.Path).ToHashSet();
        lock (_listening)
        {
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
            lock (_listening)
            {
                if (_listening.TryGetValue(info.Path, out var cts) && cts.Token == ct)
                    _listening.Remove(info.Path);
            }
        }
        catch (OperationCanceledException) { }
    }

    private async Task FollowAsync(int onboard, CancellationToken ct)
    {
        // Fn + n loaded the keyboard's own saved energy settings: put the
        // chosen ones back (#497), whichever profile follows.
        try
        {
            lighting.KeepKeyboardEnergy();
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] Keyboard energy settings not set again: {e.Message}");
        }
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
