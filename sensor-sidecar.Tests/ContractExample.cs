using Xunit;

namespace SensorSidecar.Tests;

/// The shared examples in `sensor-sidecar.Tests/contract/`: the service's
/// side writes them, the app's reader (`rigstats-backend`) parses the same
/// files, so neither side can rename, add or drop a field alone (#679).
/// Regenerate with RIGSTATS_UPDATE_CONTRACT=1 after a deliberate change —
/// and change the app's side in the same commit.
internal static class ContractExample
{
    public static string PathOf(string name) => Path.Combine(RepoRoot(), "sensor-sidecar.Tests", "contract", name);

    /// `json` is exactly the example `name`.
    public static void Check(string name, string json)
    {
        var path = PathOf(name);
        if (Environment.GetEnvironmentVariable("RIGSTATS_UPDATE_CONTRACT") == "1")
            File.WriteAllText(path, json + "\n");
        Assert.True(File.Exists(path), $"no contract example {name}: run the tests once with RIGSTATS_UPDATE_CONTRACT=1");
        Assert.Equal(File.ReadAllText(path).Trim(), json);
    }

    /// One domain's `capabilities` entry, `details` included, as
    /// `control-caps-<domain>.json`. Build it from a fake that offers
    /// everything the domain can say, so every field is in the example.
    public static void CheckCapability(Control.CapabilitySet caps)
    {
        Assert.True(caps.Supported, $"{caps.Domain}: the example must be a supported domain with its details");
        Check($"control-caps-{caps.Domain}.json",
            System.Text.Json.JsonSerializer.Serialize(caps, Control.ControlJson.Options));
    }

    private static string RepoRoot()
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
            if (Directory.Exists(Path.Combine(dir.FullName, "sensor-sidecar.Tests", "contract")))
                return dir.FullName;
        throw new DirectoryNotFoundException("repo root with sensor-sidecar.Tests/contract");
    }
}
