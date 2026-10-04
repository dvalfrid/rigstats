using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Lighting;

/// The paired Hue Bridge (#215): its application key, its rooms and zones,
/// and which of them follow the rig — kept in `hue.json` beside the
/// profiles, so it survives restarts and service updates. The key is
/// encrypted with DPAPI (machine scope) and the file is readable by SYSTEM
/// and Administrators only; nothing else ever sees it (not the pipe, not the
/// diagnostics).
public sealed class HueLink : IDisposable
{
    private static readonly TimeSpan RelocateInterval = TimeSpan.FromMinutes(1);

    private readonly string _path;
    private readonly object _lock = new();
    private HueFile? _file;
    private HueBridge? _bridge;
    private long _lastRelocate = long.MinValue / 2;
    // "Not found on the network" was logged; cleared by the next command that works.
    private bool _missingLogged;

    public HueLink(string path)
    {
        _path = path;
        try
        {
            if (File.Exists(path))
                Use(JsonSerializer.Deserialize<HueFile>(File.ReadAllText(path), ControlJson.Options));
        }
        catch (Exception e)
        {
            SidecarLog.Log($"[rigstats-control] Hue: {Path.GetFileName(path)} not read ({e.Message}); pair the bridge again.");
        }
    }

    /// For the aura capability: the paired bridge and its rooms and zones
    /// (which follow the rig) — no key.
    public JsonObject Details()
    {
        lock (_lock)
        {
            if (_file is not { } file)
                return new JsonObject { ["paired"] = false };
            return new JsonObject
            {
                ["paired"] = true,
                ["bridge"] = new JsonObject
                {
                    ["id"] = file.Bridge.Id,
                    ["name"] = file.Bridge.Name,
                    ["model"] = file.Bridge.Model,
                    ["firmware"] = file.Bridge.Firmware,
                    ["ip"] = file.Bridge.Ip,
                },
                ["groups"] = new JsonArray(file.Groups.Select(g => (JsonNode)new JsonObject
                {
                    ["id"] = g.Id,
                    ["name"] = g.Name,
                    ["kind"] = g.Kind,
                    ["chosen"] = file.Chosen.Contains(g.Id),
                }).ToArray()),
            };
        }
    }

    /// A device for each chosen room and zone; none when unpaired. Reading
    /// the file is enough — the bridge is only asked when a command goes out.
    public IReadOnlyList<ILightingDevice> Devices()
    {
        lock (_lock)
        {
            if (_file is not { } file)
                return [];
            return file.Groups.Where(g => file.Chosen.Contains(g.Id))
                .Select(g => (ILightingDevice)new HueRoomDevice(g, file.Bridge, Send))
                .ToList();
        }
    }

    public Task<IReadOnlyList<HueBridgeInfo>> DiscoverAsync(string? ip, CancellationToken ct) =>
        HueBridge.DiscoverAsync(ip, ct);

    /// Pairs with the bridge at `ip`, or returns false while its link button
    /// hasn't been pressed. Nothing follows the rig until rooms are chosen.
    public async Task<bool> PairAsync(string ip, CancellationToken ct)
    {
        var info = await HueBridge.ConfigAsync(ip, ct);
        if (await HueBridge.PairAsync(info, ct) is not { } key)
            return false;
        using var bridge = new HueBridge(info.Ip, info.Id, key);
        var groups = await bridge.GroupsAsync(ct);
        Save(new HueFile(info, Protect(key), groups.ToList(), []));
        SidecarLog.Log($"[rigstats-control] Hue: paired with {info.Name} ({info.Model}, {info.Ip}), {groups.Count} room(s)/zone(s).");
        return true;
    }

    /// Reads the rooms and zones again (renamed, added in the Hue app);
    /// a chosen one that is gone drops out.
    public async Task RefreshGroupsAsync(CancellationToken ct)
    {
        HueBridge bridge;
        lock (_lock)
            bridge = _bridge ?? throw new InvalidOperationException("No Hue Bridge is paired.");
        var groups = await bridge.GroupsAsync(ct);
        lock (_lock)
        {
            if (_file is { } file)
                Save(file with { Groups = groups.ToList(), Chosen = file.Chosen.Where(id => groups.Any(g => g.Id == id)).ToList() });
        }
    }

    /// The rooms and zones that follow the rig from now on.
    public void Choose(IReadOnlyCollection<string> ids)
    {
        lock (_lock)
        {
            var file = _file ?? throw new InvalidOperationException("No Hue Bridge is paired.");
            Save(file with { Chosen = file.Groups.Where(g => ids.Contains(g.Id)).Select(g => g.Id).ToList() });
        }
    }

    /// Forgets the bridge; its lights are left as they are. (The key stays
    /// listed in the Hue app's settings until removed there.)
    public void Unpair()
    {
        lock (_lock)
        {
            _bridge?.Dispose();
            _bridge = null;
            _file = null;
            File.Delete(_path);
        }
        SidecarLog.Log("[rigstats-control] Hue: bridge unpaired.");
    }

    public void Dispose()
    {
        lock (_lock)
            _bridge?.Dispose();
    }

    /// One group command. A bridge that stopped answering may have a new
    /// address (DHCP): it is looked for by its id — at most once a minute —
    /// and the command sent once more there.
    private void Send(string groupedLightId, JsonObject body)
    {
        HueBridge bridge;
        lock (_lock)
            bridge = _bridge ?? throw new InvalidOperationException("No Hue Bridge is paired.");
        try
        {
            bridge.SetGroup(groupedLightId, body);
        }
        catch (HueUnreachableException)
        {
            if (Relocate(bridge) is not { } moved)
                throw;
            moved.SetGroup(groupedLightId, body);
        }
        _missingLogged = false;
    }

    private HueBridge? Relocate(HueBridge unreachable)
    {
        lock (_lock)
        {
            if (Environment.TickCount64 - _lastRelocate < (long)RelocateInterval.TotalMilliseconds)
                return null;
            _lastRelocate = Environment.TickCount64;
        }
        // Outside the lock: the search takes a couple of seconds.
        var found = HueBridge.DiscoverAsync(null, CancellationToken.None).GetAwaiter().GetResult()
            .FirstOrDefault(b => b.Id == unreachable.BridgeId && b.Ip != unreachable.Ip);
        if (found is null)
        {
            if (!_missingLogged)
                SidecarLog.Log($"[rigstats-control] Hue: bridge {unreachable.BridgeId} not found on the network (last at {unreachable.Ip}); trying again later.");
            _missingLogged = true;
            return null;
        }
        lock (_lock)
        {
            if (_file is not { } file || file.Bridge.Id != found.Id)
                return null; // unpaired or paired with another bridge meanwhile
            SidecarLog.Log($"[rigstats-control] Hue: bridge moved from {unreachable.Ip} to {found.Ip}.");
            Save(file with { Bridge = file.Bridge with { Ip = found.Ip } });
            return _bridge;
        }
    }

    private void Save(HueFile file)
    {
        lock (_lock)
        {
            Directory.CreateDirectory(Path.GetDirectoryName(_path)!);
            var temp = _path + $".tmp-{Guid.NewGuid():N}";
            File.WriteAllText(temp, JsonSerializer.Serialize(file, ControlJson.Options));
            RestrictToAdmins(temp);
            File.Move(temp, _path, overwrite: true);
            Use(file);
        }
    }

    private void Use(HueFile? file)
    {
        _bridge?.Dispose();
        _bridge = null;
        _file = file;
        if (file is not null)
            _bridge = new HueBridge(file.Bridge.Ip, file.Bridge.Id, Unprotect(file.Key));
    }

    /// SYSTEM and Administrators only — unlike the profiles beside it, which
    /// users may read.
    private static void RestrictToAdmins(string path)
    {
        var security = new FileSecurity();
        security.SetAccessRuleProtection(isProtected: true, preserveInheritance: false);
        foreach (var sid in new[] { WellKnownSidType.LocalSystemSid, WellKnownSidType.BuiltinAdministratorsSid })
            security.AddAccessRule(new FileSystemAccessRule(new SecurityIdentifier(sid, null), FileSystemRights.FullControl, AccessControlType.Allow));
        new FileInfo(path).SetAccessControl(security);
    }

    // ── DPAPI, machine scope ───────────────────────────────────────────

    private static string Protect(string key) =>
        Convert.ToBase64String(Crypt(Encoding.UTF8.GetBytes(key), protect: true));

    private static string Unprotect(string stored) =>
        Encoding.UTF8.GetString(Crypt(Convert.FromBase64String(stored), protect: false));

    private static byte[] Crypt(byte[] data, bool protect)
    {
        var handle = GCHandle.Alloc(data, GCHandleType.Pinned);
        try
        {
            var input = new DataBlob { Size = data.Length, Data = handle.AddrOfPinnedObject() };
            const int flags = CryptProtectUiForbidden | CryptProtectLocalMachine;
            var ok = protect
                ? CryptProtectData(ref input, null, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, flags, out var output)
                : CryptUnprotectData(ref input, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, flags, out output);
            if (!ok)
                throw new Win32Exception(Marshal.GetLastWin32Error());
            try
            {
                var result = new byte[output.Size];
                Marshal.Copy(output.Data, result, 0, output.Size);
                return result;
            }
            finally
            {
                LocalFree(output.Data);
            }
        }
        finally
        {
            handle.Free();
        }
    }

    private const int CryptProtectUiForbidden = 0x1;
    private const int CryptProtectLocalMachine = 0x4;

    [StructLayout(LayoutKind.Sequential)]
    private struct DataBlob
    {
        public int Size;
        public IntPtr Data;
    }

    [DllImport("crypt32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    private static extern bool CryptProtectData(ref DataBlob dataIn, string? description, IntPtr entropy, IntPtr reserved, IntPtr prompt, int flags, out DataBlob dataOut);

    [DllImport("crypt32.dll", SetLastError = true)]
    private static extern bool CryptUnprotectData(ref DataBlob dataIn, IntPtr description, IntPtr entropy, IntPtr reserved, IntPtr prompt, int flags, out DataBlob dataOut);

    [DllImport("kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr memory);
}

/// `hue.json`. `Key` is the DPAPI-encrypted application key.
public sealed record HueFile(HueBridgeInfo Bridge, string Key, List<HueGroup> Groups, List<string> Chosen);
