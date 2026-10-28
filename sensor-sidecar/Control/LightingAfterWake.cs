using System.Runtime.InteropServices;
using System.Threading.Channels;

namespace SensorSidecar.Control;

/// Writes the active profile's lighting again when the display comes back on
/// (#670). A device that lost power in sleep — the ROG Omni receiver and the
/// keyboard behind it, a laptop's own keyboard — shows its own effect after
/// the wake, and nothing else re-applied the profile until the service
/// restarted. The display turning on is when the user is back after either
/// kind of sleep (S3, Modern Standby); a screen that only timed out gets the
/// same lighting written again, which changes nothing. The notification is
/// `GUID_CONSOLE_DISPLAY_STATE` through powrprof's callback form, so it
/// works in the service and in the dev sidecar's console alike.
public sealed class LightingAfterWake(ProfileStore profiles, ControlBroker broker) : GuardedBackgroundService
{
    // USB devices are still coming back right after the display does; a
    // device not there yet is tried again by the broker (#489).
    private static readonly TimeSpan Settle = TimeSpan.FromSeconds(3);

    private static readonly Guid ConsoleDisplayState = new("6FE69556-704A-47A0-8F24-C28D936FDA47");
    private const uint DeviceNotifyCallback = 2;
    private const uint PbtPowerSettingChange = 0x8013;

    private readonly Channel<bool> _cameOn = Channel.CreateBounded<bool>(
        new BoundedChannelOptions(1) { FullMode = BoundedChannelFullMode.DropWrite });

    private int? _state;

    // Kept in a field: the native side holds only a pointer to it.
    private NotifyCallback? _callback;

    /// Off (0) → on (1) only: the first notification, sent at registration,
    /// is the state now, and dimmed (2) → on never lost power.
    internal static bool CameOn(int? before, int now) => before == 0 && now == 1;

    protected override async Task RunAsync(CancellationToken stoppingToken)
    {
        _callback = OnNotify;
        var setting = ConsoleDisplayState;
        var recipient = new SubscribeParameters { Callback = _callback, Context = IntPtr.Zero };
        var error = PowerSettingRegisterNotification(ref setting, DeviceNotifyCallback, ref recipient, out var handle);
        if (error != 0)
        {
            SidecarLog.Log($"[rigstats-control] Lighting after wake: display notifications not available (error {error}); the lighting is written again only when a profile is applied.");
            return;
        }
        try
        {
            await foreach (var _ in _cameOn.Reader.ReadAllAsync(stoppingToken))
            {
                await Task.Delay(Settle, stoppingToken);
                await ReapplyAsync(stoppingToken);
            }
        }
        finally
        {
            _ = PowerSettingUnregisterNotification(handle);
        }
    }

    private async Task ReapplyAsync(CancellationToken ct)
    {
        var id = await profiles.GetActiveIdAsync(ct);
        if (id is null || await profiles.GetAsync(id, ct) is not { Part.Aura: not null } profile)
            return;
        var result = await broker.ReapplyLightingAsync(profile, ct);
        SidecarLog.Log(result switch
        {
            null => "[rigstats-control] Display on: lighting left as it is during a preview.",
            { Ok: true } => $"[rigstats-control] Display on: lighting of profile '{id}' written again.",
            _ => $"[rigstats-control] Display on: lighting of profile '{id}' not written again: {result.Message}",
        });
    }

    private uint OnNotify(IntPtr context, uint type, IntPtr setting)
    {
        // POWERBROADCAST_SETTING: the setting's GUID, DataLength, then the DWORD.
        if (type != PbtPowerSettingChange || setting == IntPtr.Zero || Marshal.ReadInt32(setting, 16) < 4)
            return 0;
        var now = Marshal.ReadInt32(setting, 20);
        if (CameOn(_state, now))
            _cameOn.Writer.TryWrite(true);
        _state = now;
        return 0;
    }

    [UnmanagedFunctionPointer(CallingConvention.Winapi)]
    private delegate uint NotifyCallback(IntPtr context, uint type, IntPtr setting);

    [StructLayout(LayoutKind.Sequential)]
    private struct SubscribeParameters
    {
        public NotifyCallback Callback;
        public IntPtr Context;
    }

    [DllImport("powrprof.dll")]
    private static extern uint PowerSettingRegisterNotification(ref Guid settingGuid, uint flags,
        ref SubscribeParameters recipient, out IntPtr registrationHandle);

    [DllImport("powrprof.dll")]
    private static extern uint PowerSettingUnregisterNotification(IntPtr registrationHandle);
}
