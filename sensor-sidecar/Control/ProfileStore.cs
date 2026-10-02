using System.Security.AccessControl;
using System.Security.Principal;
using System.Text.Json;

namespace SensorSidecar.Control;

/// Service-owned profile CRUD, `%ProgramData%\se.codeby.rigstats\profiles.json`
/// — the UI only ever edits profiles through the control pipe (see
/// `docs/control-architecture.md`, "Profile model"). Writable only by this
/// service; `Users` get read access the same way the telemetry pipe does.
public sealed class ProfileStore
{
    private readonly string _path;
    private readonly SemaphoreSlim _fileLock = new(1, 1);
    private ProfileFile? _cached;

    public ProfileStore() : this(DefaultPath()) { }

    // Seam for tests: a fresh temp path per test, never the real ProgramData one.
    public ProfileStore(string path) => _path = path;

    private static string DefaultPath() =>
        Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
            "se.codeby.rigstats",
            "profiles.json");

    // Built-ins carry an explicit, empty fan part ("every header on BIOS
    // control") rather than none: a missing part leaves the domain untouched,
    // so switching from a profile with curves to one without would otherwise
    // keep the old curves running while the Fans tab shows none.
    private static List<Profile> BuiltinProfiles() =>
    [
        new Profile { Id = "silent", Name = "Silent", Icon = "moon", Builtin = true, Part = new ProfilePart { PowerPlan = "power_saver", Fan = BiosFans() } },
        new Profile { Id = "balanced", Name = "Balanced", Icon = "scale", Builtin = true, Part = new ProfilePart { PowerPlan = "balanced", Fan = BiosFans() } },
        new Profile { Id = "gaming", Name = "Gaming", Icon = "bolt", Builtin = true, Part = new ProfilePart { PowerPlan = "high_performance", Fan = BiosFans() } },
        new Profile { Id = "eco", Name = "Eco", Icon = "leaf", Builtin = true, Part = new ProfilePart { PowerPlan = "power_saver", Fan = BiosFans() } },
    ];

    private static FanPart BiosFans() => new() { Headers = [] };

    /// Built-ins saved before fan support existed (#187) have no fan part —
    /// give them the empty one `BuiltinProfiles` now seeds.
    private static ProfileFile WithBiosFansOnBuiltins(ProfileFile file) => new()
    {
        Active = file.Active,
        Profiles = file.Profiles
            .Select(p => p.Builtin && p.Part.Fan is null
                ? new Profile
                {
                    Id = p.Id,
                    Name = p.Name,
                    Icon = p.Icon,
                    Builtin = true,
                    Part = new ProfilePart
                    {
                        PowerPlan = p.Part.PowerPlan,
                        Fan = BiosFans(),
                        CpuLimit = p.Part.CpuLimit,
                        CurveOpt = p.Part.CurveOpt,
                        Gpu = p.Part.Gpu,
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

        ProfileFile? loaded;
        await using (var stream = File.OpenRead(_path))
            loaded = await JsonSerializer.DeserializeAsync<ProfileFile>(stream, ControlJson.Options, ct);
        _cached = loaded is null
            ? new ProfileFile { Active = "balanced", Profiles = BuiltinProfiles() }
            : WithBiosFansOnBuiltins(loaded);
        return _cached;
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
            var profiles = file.Profiles.Where(p => p.Id != profile.Id).Append(profile).ToList();
            await SaveUnlockedAsync(new ProfileFile { Active = file.Active, Profiles = profiles }, ct);
        }
        finally
        {
            _fileLock.Release();
        }
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
            var active = file.Active == id ? (profiles.FirstOrDefault()?.Id ?? "balanced") : file.Active;
            await SaveUnlockedAsync(new ProfileFile { Active = active, Profiles = profiles }, ct);
            return true;
        }
        finally
        {
            _fileLock.Release();
        }
    }

    private async Task SaveUnlockedAsync(ProfileFile file, CancellationToken ct)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(_path)!);
        ApplyAcl(Path.GetDirectoryName(_path)!);

        // Atomic write: serialize to a temp file, then rename over the real
        // one, so a crash mid-write never leaves a truncated/corrupt file.
        var tempPath = _path + $".tmp-{Guid.NewGuid():N}";
        await using (var stream = File.Create(tempPath))
        {
            await JsonSerializer.SerializeAsync(stream, file, ControlJson.Options, ct);
        }
        File.Move(tempPath, _path, overwrite: true);
        _cached = file;
    }

    // SYSTEM + Administrators write, Users read — mirrors the PipeAccessRule
    // construction in `SensorWorker.cs`'s pipe security, applied to the
    // ProgramData directory instead of a pipe.
    private static void ApplyAcl(string directory)
    {
        try
        {
            var info = new DirectoryInfo(directory);
            var security = info.GetAccessControl();
            security.AddAccessRule(new FileSystemAccessRule(
                new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null),
                FileSystemRights.FullControl,
                InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
                PropagationFlags.None,
                AccessControlType.Allow));
            security.AddAccessRule(new FileSystemAccessRule(
                new SecurityIdentifier(WellKnownSidType.BuiltinAdministratorsSid, null),
                FileSystemRights.FullControl,
                InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
                PropagationFlags.None,
                AccessControlType.Allow));
            security.AddAccessRule(new FileSystemAccessRule(
                new SecurityIdentifier(WellKnownSidType.BuiltinUsersSid, null),
                FileSystemRights.ReadAndExecute,
                InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
                PropagationFlags.None,
                AccessControlType.Allow));
            info.SetAccessControl(security);
        }
        catch
        {
            // Best-effort — an ACL failure shouldn't block the service from
            // running; it just means the directory keeps its inherited ACL.
        }
    }
}
