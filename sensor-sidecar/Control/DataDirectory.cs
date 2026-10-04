using System.Security.AccessControl;
using System.Security.Principal;

namespace SensorSidecar.Control;

/// `%ProgramData%\se.codeby.rigstats` — the service's own state: profiles,
/// baselines, the boot-crash marker, the log. Everything in it is trusted
/// when read back, so nobody but SYSTEM and Administrators may write there.
///
/// A folder under `%ProgramData%` inherits rules that let any user create
/// files in it, and a user can also create the folder itself before RIGStats
/// is ever installed and so own it. `EnsureSecure` therefore runs before the
/// service reads or writes anything: it replaces the rules with protected
/// ones, takes the folder over, and removes what an unprivileged account
/// left behind.
public static class DataDirectory
{
    public static string DefaultPath { get; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        "se.codeby.rigstats");

    private static readonly SecurityIdentifier LocalSystem = new(WellKnownSidType.LocalSystemSid, null);
    private static readonly SecurityIdentifier Administrators = new(WellKnownSidType.BuiltinAdministratorsSid, null);
    private static readonly SecurityIdentifier Users = new(WellKnownSidType.BuiltinUsersSid, null);

    /// Null when the folder is now safe to use, otherwise why it is not.
    public static string? EnsureSecure(string directory) => EnsureSecure(directory, Administrators);

    /// `owner` is who ends up owning the folder and whose files are kept —
    /// Administrators, which is also what SYSTEM and elevated processes
    /// create files as. Tests pass the account they run under.
    internal static string? EnsureSecure(string directory, SecurityIdentifier owner)
    {
        try
        {
            // A link in place of the folder would send every write
            // somewhere else. Removing it removes the link, not its target.
            // Whoever put it there may put it back, hence the retries.
            for (var attempt = 0; attempt < 3; attempt++)
            {
                if (IsLink(directory))
                    Directory.Delete(directory);
                if (!Directory.Exists(directory))
                    new DirectoryInfo(directory).Create(Rules(owner));
                TakeOver(directory, owner);
                if (!IsLink(directory))
                {
                    RemoveForeignEntries(directory, owner);
                    return null;
                }
            }
            return "the data folder keeps being replaced by a link.";
        }
        catch (Exception e)
        {
            return e.Message;
        }
    }

    private static bool IsLink(string path)
    {
        var info = new DirectoryInfo(path);
        return info.Exists && info.Attributes.HasFlag(FileAttributes.ReparsePoint);
    }

    /// Protected (nothing inherited from `%ProgramData%`): SYSTEM and
    /// Administrators full control, Users read — the app reads the log and
    /// the sensor tree for its diagnostics export.
    private static DirectorySecurity Rules(SecurityIdentifier owner)
    {
        const InheritanceFlags inherit = InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit;
        var security = new DirectorySecurity();
        security.SetAccessRuleProtection(isProtected: true, preserveInheritance: false);
        foreach (var sid in new[] { LocalSystem, Administrators, owner }.Distinct())
        {
            security.AddAccessRule(new FileSystemAccessRule(
                sid, FileSystemRights.FullControl, inherit, PropagationFlags.None, AccessControlType.Allow));
        }
        security.AddAccessRule(new FileSystemAccessRule(
            Users, FileSystemRights.ReadAndExecute, inherit, PropagationFlags.None, AccessControlType.Allow));
        return security;
    }

    /// Owner first, rules second: once the folder is ours, its rules are
    /// ours to change whatever they said before.
    private static void TakeOver(string directory, SecurityIdentifier owner)
    {
        var info = new DirectoryInfo(directory);
        if (!Equals(info.GetAccessControl(AccessControlSections.Owner).GetOwner(typeof(SecurityIdentifier)), owner))
        {
            var ownership = new DirectorySecurity();
            ownership.SetOwner(owner);
            info.SetAccessControl(ownership);
        }
        info.SetAccessControl(Rules(owner));
    }

    /// Anything not created by SYSTEM, Administrators or `owner` was put
    /// there while the folder was open to everyone. All of the service's
    /// state can be rebuilt, so such entries are removed, not inspected.
    /// A foreign folder is only removed when empty: the service keeps
    /// nothing in subfolders, and walking a tree someone else controls
    /// invites being led elsewhere.
    internal static void RemoveForeignEntries(string directory, SecurityIdentifier owner)
    {
        foreach (var entry in new DirectoryInfo(directory).EnumerateFileSystemInfos())
        {
            var entryOwner = OwnerOf(entry);
            if (entryOwner is not null
                && (entryOwner.Equals(LocalSystem) || entryOwner.Equals(Administrators) || entryOwner.Equals(owner)))
                continue;
            try
            {
                entry.Delete();
                SidecarLog.Log($"[rigstats-sensor] Removed '{entry.Name}' from the data folder: created by {entryOwner?.Value ?? "an unknown account"}, not by the service.");
            }
            catch (IOException) when (entry is DirectoryInfo)
            {
                SidecarLog.Log($"[rigstats-sensor] Left the foreign folder '{entry.Name}' in the data folder: it is not empty.");
            }
        }
    }

    /// Null when the owner can't be read — treated as foreign.
    private static SecurityIdentifier? OwnerOf(FileSystemInfo entry)
    {
        try
        {
            FileSystemSecurity security = entry is DirectoryInfo dir
                ? dir.GetAccessControl(AccessControlSections.Owner)
                : ((FileInfo)entry).GetAccessControl(AccessControlSections.Owner);
            return security.GetOwner(typeof(SecurityIdentifier)) as SecurityIdentifier;
        }
        catch (Exception e) when (e is UnauthorizedAccessException or IOException)
        {
            return null;
        }
    }
}
