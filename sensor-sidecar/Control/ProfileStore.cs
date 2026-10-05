using System.Text.Json;

namespace SensorSidecar.Control;

/// Service-owned profile CRUD, `%ProgramData%\se.codeby.rigstats\profiles.json`
/// — the UI only ever edits profiles through the control pipe (see
/// `docs/control-architecture.md`, "Profile model"). Writable only by this
/// service; `Users` get read access the same way the telemetry pipe does
/// (the folder's rules are set by `DataDirectory`).
public sealed class ProfileStore
{
    private readonly string _path;
    private readonly SemaphoreSlim _fileLock = new(1, 1);
    private ProfileFile? _cached;

    /// Set when profiles.json could not be read and was recovered (#221);
    /// shown in the Control Center until profiles are saved again.
    public string? Notice { get; private set; }

    private string BackupPath => _path + ".bak";
    private string CorruptPath => _path + ".corrupt";

    public ProfileStore() : this(DefaultPath()) { }

    // Seam for tests: a fresh temp path per test, never the real ProgramData one.
    public ProfileStore(string path) => _path = path;

    private static string DefaultPath() =>
        Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
            "se.codeby.rigstats",
            "profiles.json");

    // Built-ins carry explicit, empty fan, CPU-limit, Curve Optimizer and
    // GPU parts ("BIOS / driver control") rather than none: a missing part
    // leaves the domain untouched, so switching from a profile with curves
    // or limits to one without would otherwise keep them running while the
    // Control Center shows none.
    private static List<Profile> BuiltinProfiles() =>
    [
        new Profile { Id = "silent", Name = "Silent", Icon = "moon", Builtin = true, Part = new ProfilePart { PowerPlan = "power_saver", Fan = BiosFans(), CpuLimit = BiosCpuLimits(), CurveOpt = BiosCurveOpt(), Gpu = DriverGpu() } },
        new Profile { Id = "balanced", Name = "Balanced", Icon = "scale", Builtin = true, Part = new ProfilePart { PowerPlan = "balanced", Fan = BiosFans(), CpuLimit = BiosCpuLimits(), CurveOpt = BiosCurveOpt(), Gpu = DriverGpu() } },
        new Profile { Id = "gaming", Name = "Gaming", Icon = "bolt", Builtin = true, Part = new ProfilePart { PowerPlan = "high_performance", Fan = BiosFans(), CpuLimit = BiosCpuLimits(), CurveOpt = BiosCurveOpt(), Gpu = DriverGpu() } },
        new Profile { Id = "eco", Name = "Eco", Icon = "leaf", Builtin = true, Part = new ProfilePart { PowerPlan = "power_saver", Fan = BiosFans(), CpuLimit = BiosCpuLimits(), CurveOpt = BiosCurveOpt(), Gpu = DriverGpu() } },
    ];

    private static FanPart BiosFans() => new() { Headers = [] };

    private static CpuLimitPart BiosCpuLimits() => new();

    private static GpuPart DriverGpu() => new();

    private static CurveOptPart BiosCurveOpt() => new();

    /// Built-ins saved before fan (#188), CPU-limit (#189), GPU (#190) or Curve
    /// Optimizer (#191) support existed
    /// lack those parts — give them the empty ones `BuiltinProfiles` now seeds.
    private static ProfileFile WithBiosPartsOnBuiltins(ProfileFile file) => new()
    {
        Active = file.Active,
        Profiles = file.Profiles
            .Select(p => p.Builtin && (p.Part.Fan is null || p.Part.CpuLimit is null || p.Part.Gpu is null || p.Part.CurveOpt is null)
                ? new Profile
                {
                    Id = p.Id,
                    Name = p.Name,
                    Icon = p.Icon,
                    Builtin = true,
                    Part = new ProfilePart
                    {
                        PowerPlan = p.Part.PowerPlan,
                        Fan = p.Part.Fan ?? BiosFans(),
                        CpuLimit = p.Part.CpuLimit ?? BiosCpuLimits(),
                        CurveOpt = p.Part.CurveOpt ?? BiosCurveOpt(),
                        Gpu = p.Part.Gpu ?? DriverGpu(),
                        Aura = p.Part.Aura,
                    },
                }
                : p)
            .ToList(),
    };

    public async Task<ProfileFile> LoadAsync(CancellationToken ct)
    {
        await _fileLock.WaitAsync(ct);
        try
        {
            return await LoadUnlockedAsync(ct);
        }
        finally
        {
            _fileLock.Release();
        }
    }

    private async Task<ProfileFile> LoadUnlockedAsync(CancellationToken ct)
    {
        if (_cached is not null)
            return _cached;

        if (!File.Exists(_path))
        {
            _cached = new ProfileFile { Active = "balanced", Profiles = BuiltinProfiles() };
            await SaveUnlockedAsync(_cached, ct);
            return _cached;
        }

        string error;
        try
        {
            _cached = await ReadAsync(_path, ct);
            return _cached;
        }
        catch (Exception e) when (e is JsonException or IOException or NotSupportedException)
        {
            error = e.Message;
        }

        // Saved right away: profiles.json is moved aside, so the next start
        // would otherwise begin again from the built-ins.
        // No backup of what it replaces: if moving it aside failed, that is
        // the unreadable file, and it must not overwrite a good backup.
        var (recovered, notice) = await RecoverAsync(error, ct);
        await SaveUnlockedAsync(recovered, ct, backup: false);
        Notice = notice;
        return recovered;
    }

    /// An unreadable profiles.json (#221) — a downgrade that doesn't know a
    /// newer value, a disk error, a manual edit — must never lock the Control
    /// Center: it is kept as `profiles.json.corrupt` (not deleted), and the
    /// profiles come from `profiles.json.bak`, else the built-ins.
    private async Task<(ProfileFile File, string Notice)> RecoverAsync(string error, CancellationToken ct)
    {
        try
        {
            File.Move(_path, CorruptPath, overwrite: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            SidecarLog.Log($"[rigstats-control] Could not keep the unreadable profiles.json as {Path.GetFileName(CorruptPath)}: {e.Message}");
        }

        try
        {
            var restored = await ReadAsync(BackupPath, ct);
            SidecarLog.Log($"[rigstats-control] profiles.json could not be read ({error}) — restored from {Path.GetFileName(BackupPath)}.");
            return (restored, "Profiles could not be read and were restored from a backup.");
        }
        catch (Exception e) when (e is JsonException or IOException or NotSupportedException)
        {
            SidecarLog.Log($"[rigstats-control] profiles.json could not be read ({error}), nor its backup ({e.Message}) — reset to the built-in profiles.");
            return (new ProfileFile { Active = "balanced", Profiles = BuiltinProfiles() },
                "Profiles could not be read and were reset to the built-in profiles.");
        }
    }

    private static async Task<ProfileFile> ReadAsync(string path, CancellationToken ct)
    {
        await using var stream = File.OpenRead(path);
        var loaded = await JsonSerializer.DeserializeAsync<ProfileFile>(stream, ControlJson.Options, ct);
        return loaded is null
            ? new ProfileFile { Active = "balanced", Profiles = BuiltinProfiles() }
            : WithBiosPartsOnBuiltins(loaded);
    }

    public async Task<IReadOnlyList<Profile>> ListAsync(CancellationToken ct) => (await LoadAsync(ct)).Profiles;

    public async Task<Profile?> GetAsync(string id, CancellationToken ct) =>
        (await LoadAsync(ct)).Profiles.FirstOrDefault(p => p.Id == id);

    public async Task<string?> GetActiveIdAsync(CancellationToken ct) => (await LoadAsync(ct)).Active;

    public async Task SetActiveAsync(string id, CancellationToken ct)
    {
        await _fileLock.WaitAsync(ct);
        try
        {
            var file = await LoadUnlockedAsync(ct);
            var updated = new ProfileFile { Active = id, Profiles = file.Profiles };
            await SaveUnlockedAsync(updated, ct);
        }
        finally
        {
            _fileLock.Release();
        }
    }

    public async Task SaveProfileAsync(Profile profile, CancellationToken ct)
    {
        await _fileLock.WaitAsync(ct);
        try
        {
            var file = await LoadUnlockedAsync(ct);
            // Replace in place: editing, renaming or resetting a profile must
            // not move it to the end of the list. New profiles are appended.
            var profiles = file.Profiles.ToList();
            var index = profiles.FindIndex(p => p.Id == profile.Id);
            if (index >= 0)
                profiles[index] = profile;
            else
                profiles.Add(profile);
            await SaveUnlockedAsync(new ProfileFile { Active = file.Active, Profiles = profiles }, ct);
        }
        finally
        {
            _fileLock.Release();
        }
    }

    /// Puts a built-in profile back to its defaults. Null when `id` isn't a
    /// built-in (a custom profile has no defaults to go back to).
    public async Task<Profile?> ResetBuiltinAsync(string id, CancellationToken ct)
    {
        var defaults = BuiltinProfiles().FirstOrDefault(p => p.Id == id);
        if (defaults is null)
            return null;
        await SaveProfileAsync(defaults, ct);
        return defaults;
    }

    public async Task<bool> DeleteProfileAsync(string id, CancellationToken ct)
    {
        await _fileLock.WaitAsync(ct);
        try
        {
            var file = await LoadUnlockedAsync(ct);
            var existing = file.Profiles.FirstOrDefault(p => p.Id == id);
            if (existing is null)
                return false;
            if (existing.Builtin)
                throw new InvalidOperationException($"Built-in profile '{id}' cannot be deleted.");

            var profiles = file.Profiles.Where(p => p.Id != id).ToList();
            // Balanced is the neutral fallback; a deleted profile must not
            // silently switch the machine to e.g. Silent just because it is first.
            var active = file.Active == id
                ? (profiles.FirstOrDefault(p => p.Id == "balanced") ?? profiles.FirstOrDefault())?.Id ?? "balanced"
                : file.Active;
            await SaveUnlockedAsync(new ProfileFile { Active = active, Profiles = profiles }, ct);
            return true;
        }
        finally
        {
            _fileLock.Release();
        }
    }

    private async Task SaveUnlockedAsync(ProfileFile file, CancellationToken ct, bool backup = true)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(_path)!);

        // Atomic write: serialize to a temp file, then rename over the real
        // one, so a crash mid-write never leaves a truncated/corrupt file.
        var tempPath = _path + $".tmp-{Guid.NewGuid():N}";
        await using (var stream = File.Create(tempPath))
        {
            await JsonSerializer.SerializeAsync(stream, file, ControlJson.Options, ct);
        }
        // The file being replaced was read or written by this store, so it is
        // a good backup.
        if (backup && File.Exists(_path))
            File.Copy(_path, BackupPath, overwrite: true);
        File.Move(tempPath, _path, overwrite: true);
        _cached = file;
        Notice = null;
    }
}
