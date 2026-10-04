using System.Security.AccessControl;
using System.Security.Principal;
using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Against a temp directory, with the test's own account standing in for
/// Administrators as the trusted owner — the tests run unelevated and can't
/// hand a folder to a group they don't hold.
/// </summary>
public sealed class DataDirectoryTests : IDisposable
{
    private static readonly SecurityIdentifier Me = WindowsIdentity.GetCurrent().User!;
    private static readonly SecurityIdentifier Users = new(WellKnownSidType.BuiltinUsersSid, null);

    private readonly string _root = Path.Combine(Path.GetTempPath(), "rigstats-datadir-tests-" + Guid.NewGuid());
    private readonly string _dir;

    public DataDirectoryTests()
    {
        Directory.CreateDirectory(_root);
        _dir = Path.Combine(_root, "se.codeby.rigstats");
    }

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch { }
    }

    private static List<FileSystemAccessRule> RulesOf(string directory, bool inherited) =>
        new DirectoryInfo(directory).GetAccessControl()
            .GetAccessRules(includeExplicit: !inherited, includeInherited: inherited, typeof(SecurityIdentifier))
            .Cast<FileSystemAccessRule>()
            .ToList();

    [Fact]
    public void CreatesTheFolderWithProtectedRules()
    {
        Assert.Null(DataDirectory.EnsureSecure(_dir, Me));

        Assert.True(Directory.Exists(_dir));
        Assert.True(new DirectoryInfo(_dir).GetAccessControl().AreAccessRulesProtected);
        Assert.Empty(RulesOf(_dir, inherited: true));
    }

    [Fact]
    public void UsersCanReadButNotWrite()
    {
        Assert.Null(DataDirectory.EnsureSecure(_dir, Me));

        var users = RulesOf(_dir, inherited: false).Where(r => r.IdentityReference.Equals(Users)).ToList();
        var rule = Assert.Single(users);
        Assert.Equal(AccessControlType.Allow, rule.AccessControlType);
        Assert.Equal(FileSystemRights.ReadAndExecute | FileSystemRights.Synchronize, rule.FileSystemRights | FileSystemRights.Synchronize);
    }

    [Fact]
    public void AnExistingOpenFolderIsLockedDown()
    {
        // As under %ProgramData%: rules inherited from the parent.
        Directory.CreateDirectory(_dir);
        Assert.NotEmpty(RulesOf(_dir, inherited: true));

        Assert.Null(DataDirectory.EnsureSecure(_dir, Me));

        Assert.Empty(RulesOf(_dir, inherited: true));
    }

    [Fact]
    public void KeepsFilesOfTheTrustedOwner()
    {
        Directory.CreateDirectory(_dir);
        var profiles = Path.Combine(_dir, "profiles.json");
        File.WriteAllText(profiles, "{}");

        Assert.Null(DataDirectory.EnsureSecure(_dir, Me));

        Assert.True(File.Exists(profiles));
    }

    [Fact]
    public void RemovesFilesSomeoneElseCreated()
    {
        Directory.CreateDirectory(_dir);
        var planted = Path.Combine(_dir, "pending-apply");
        File.WriteAllText(planted, "x");
        var plantedFolder = Path.Combine(_dir, "stuff");
        Directory.CreateDirectory(plantedFolder);
        File.WriteAllText(Path.Combine(plantedFolder, "inside.txt"), "x");
        var emptyFolder = Path.Combine(_dir, "empty");
        Directory.CreateDirectory(emptyFolder);

        // Run elevated, new files belong to Administrators — always trusted.
        if (!Me.Equals(new FileInfo(planted).GetAccessControl().GetOwner(typeof(SecurityIdentifier))))
            return;

        // With Users as the trusted owner, the test account is a stranger.
        DataDirectory.RemoveForeignEntries(_dir, Users);

        Assert.False(File.Exists(planted));
        Assert.False(Directory.Exists(emptyFolder));
        // Not walked into: the service keeps nothing in subfolders.
        Assert.True(Directory.Exists(plantedFolder));
    }

    [Fact]
    public void ALinkInPlaceOfTheFolderIsReplacedAndItsTargetLeftAlone()
    {
        var target = Path.Combine(_root, "elsewhere");
        Directory.CreateDirectory(target);
        var victim = Path.Combine(target, "victim.txt");
        File.WriteAllText(victim, "keep me");
        try
        {
            Directory.CreateSymbolicLink(_dir, target);
        }
        catch (Exception e) when (e is UnauthorizedAccessException or IOException)
        {
            return; // creating links needs a privilege this account lacks.
        }

        Assert.Null(DataDirectory.EnsureSecure(_dir, Me));

        Assert.False(new DirectoryInfo(_dir).Attributes.HasFlag(FileAttributes.ReparsePoint));
        Assert.True(File.Exists(victim));
        Assert.Empty(Directory.EnumerateFileSystemEntries(_dir));
    }
}
