using System.Text.Json;

namespace SensorSidecar.Control.Lighting;

/// The energy settings chosen for each keyboard (#497), kept in
/// `keyboard-energy.json` in the service's data folder. A keyboard drops a
/// written sleep time, power saving or low-battery level when it switches
/// its own onboard profile — it loads the one saved in it (seen live on the
/// ROG Azoth X, 2026-10-19) — so RIGStats writes the chosen ones again
/// after each switch rather than saving them into the keyboard's flash
/// (Gear Link's `50 55` save may also store the lighting RIGStats set —
/// not tried).
public sealed class KeyboardEnergyStore(string path)
{
    private readonly object _lock = new();
    private Dictionary<string, KeyboardSettings>? _chosen;

    /// The settings chosen for keyboard `id`, or null when none were.
    public KeyboardSettings? Get(string id)
    {
        lock (_lock)
            return Load().GetValueOrDefault(id);
    }

    public void Set(string id, KeyboardSettings settings)
    {
        lock (_lock)
        {
            var chosen = Load();
            chosen[id] = settings;
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            var temp = path + $".tmp-{Guid.NewGuid():N}";
            File.WriteAllText(temp, JsonSerializer.Serialize(chosen, ControlJson.Options));
            File.Move(temp, path, overwrite: true);
        }
    }

    // Caller holds _lock.
    private Dictionary<string, KeyboardSettings> Load()
    {
        if (_chosen is not null)
            return _chosen;
        try
        {
            _chosen = File.Exists(path)
                ? JsonSerializer.Deserialize<Dictionary<string, KeyboardSettings>>(File.ReadAllText(path), ControlJson.Options) ?? []
                : [];
        }
        catch (Exception e) when (e is IOException or JsonException or UnauthorizedAccessException)
        {
            SidecarLog.Log($"[rigstats-control] Keyboard energy settings not read ({e.Message}); starting without them.");
            _chosen = [];
        }
        return _chosen;
    }
}
