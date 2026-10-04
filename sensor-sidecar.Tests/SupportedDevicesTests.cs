using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// The supported-devices list (docs/supported-devices.md and the website's
/// table) is generated from the drivers' model tables; these tests fail when
/// a device is added without regenerating it. To regenerate:
/// <c>$env:RIGSTATS_UPDATE_SUPPORTED_DEVICES=1; dotnet test sensor-sidecar.Tests --filter SupportedDevices</c>
/// </summary>
public class SupportedDevicesTests
{
    private const string Start = "<!-- supported-devices:start -->\n";
    private const string End = "<!-- supported-devices:end -->";

    private static bool Update => Environment.GetEnvironmentVariable("RIGSTATS_UPDATE_SUPPORTED_DEVICES") == "1";

    private static string RepoRoot()
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir is not null; dir = dir.Parent)
        {
            if (File.Exists(Path.Combine(dir.FullName, "CLAUDE.md")) && Directory.Exists(Path.Combine(dir.FullName, "website")))
                return dir.FullName;
        }
        throw new DirectoryNotFoundException("Repository root not found above the test assembly.");
    }

    private static string Normalize(string text) => text.Replace("\r\n", "\n");

    [Fact]
    public void SupportedDevices_markdown_matches_the_model_tables()
    {
        var path = Path.Combine(RepoRoot(), "docs", "supported-devices.md");
        var expected = LightingCatalog.Markdown();
        if (Update)
            File.WriteAllText(path, expected);

        Assert.True(File.Exists(path), "docs/supported-devices.md is missing — regenerate it (see this class).");
        Assert.Equal(expected, Normalize(File.ReadAllText(path)));
    }

    [Fact]
    public void SupportedDevices_website_table_matches_the_model_tables()
    {
        var path = Path.Combine(RepoRoot(), "website", "index.html");
        var html = Normalize(File.ReadAllText(path));
        var start = html.IndexOf(Start, StringComparison.Ordinal);
        var end = html.IndexOf(End, StringComparison.Ordinal);
        Assert.True(start >= 0 && end > start, "website/index.html lacks the supported-devices markers.");

        var current = html[(start + Start.Length)..end];
        var expected = LightingCatalog.HtmlRows();
        if (Update && current != expected)
        {
            File.WriteAllText(path, html[..(start + Start.Length)] + expected + html[end..]);
            current = expected;
        }

        Assert.Equal(expected, current);
    }

    [Fact]
    public void Every_verified_model_is_listed_as_verified()
    {
        var rows = LightingCatalog.All();

        Assert.Contains(rows, r => r.Name == "ROG Falchion Ace HFX" && r.Status == LightingCatalog.Verified);
        Assert.Contains(rows, r => r.Name == "ROG Strix Scope II" && r.Status == LightingCatalog.FromOpenRgb);
        Assert.Contains(rows, r => r.Name == "ROG Aura Monitor Light Bar" && r.Notes.Contains("Desk lamp"));
        Assert.Contains(rows, r => r.Type == "Motherboard" && r.Status.StartsWith(LightingCatalog.Verified + " (19AF)"));
        Assert.Equal(
            AuraUsb.Families.Count,
            rows.Where(r => r.Type == "Motherboard").Sum(r => r.UsbIds.Split(", ").Length));
    }
}
