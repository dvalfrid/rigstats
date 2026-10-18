using SensorSidecar.Control;
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
    public void SupportedDevices_website_table_matches_the_model_tables() =>
        WebsiteTableMatches(Start, End, LightingCatalog.HtmlRows());

    [Fact]
    public void Battery_website_table_matches_the_code_that_reads_them() =>
        WebsiteTableMatches("<!-- battery-devices:start -->\n", "<!-- battery-devices:end -->", BatteryCatalog.HtmlRows());

    [Fact]
    public void Device_settings_website_table_matches_the_model_tables() =>
        WebsiteTableMatches("<!-- settings-devices:start -->\n", "<!-- settings-devices:end -->", DeviceSettingsCatalog.HtmlRows());

    [Fact]
    public void Device_settings_list_marks_only_verified_models_as_verified()
    {
        var rows = DeviceSettingsCatalog.All();

        Assert.Contains(rows, r => r.Name == "ROG Delta II" && r.Type == "Headset" && r.Status == LightingCatalog.Verified);
        Assert.Contains(rows, r => r.Name == "ROG Pelta" && r.Notes.StartsWith("Sidetone on/off and 0–20; noise reduction on/off,", StringComparison.Ordinal));
        Assert.Contains(rows, r => r.Name == "ROG Harpe Ace Aim Lab Edition" && r.Status == LightingCatalog.Verified + " (ROG Omni receiver)");
        Assert.Contains(rows, r => r.Name == "ROG Chakram X" && r.Status == DeviceSettingsCatalog.FromGHelper && r.Notes.Contains("no button bindings yet"));
        // One row per mouse, under ASUS's name — G-Helper names this one
        // differently per link (#446).
        Assert.Single(rows, r => r.Name.Contains("Keris Wireless", StringComparison.Ordinal));
        Assert.Contains(rows, r => r.Name == "ROG Keris Wireless AimPoint" && r.UsbIds == "1A66, 1A68, 1A6A");
        Assert.DoesNotContain(rows, r => r.Name == "ROG HARPE II ACE" || r.Name == "Harpe Ace Mini");
        Assert.Equal(
            SensorSidecar.Control.Mice.MouseCatalog.Entries.Select(e => e.Spec.Name).Distinct().Count(),
            rows.Count(r => r.Type == "Mouse"));
    }

    private static void WebsiteTableMatches(string startMarker, string endMarker, string expected)
    {
        var path = Path.Combine(RepoRoot(), "website", "index.html");
        var html = Normalize(File.ReadAllText(path));
        var start = html.IndexOf(startMarker, StringComparison.Ordinal);
        var end = html.IndexOf(endMarker, StringComparison.Ordinal);
        Assert.True(start >= 0 && end > start, $"website/index.html lacks the {startMarker.Trim()} markers.");

        var current = html[(start + startMarker.Length)..end];
        if (Update && current != expected)
        {
            File.WriteAllText(path, html[..(start + startMarker.Length)] + expected + html[end..]);
            current = expected;
        }

        Assert.Equal(expected, current);
    }

    [Fact]
    public void Battery_list_marks_only_verified_receivers_as_verified()
    {
        var rows = BatteryCatalog.All();

        Assert.Contains(rows, r => r.Name == "Logitech devices on a Bolt receiver" && r.Status == LightingCatalog.Verified + " (Logi M650)");
        Assert.Contains(rows, r => r.Name == "Logitech devices on a Unifying receiver" && r.Status == BatteryCatalog.FromSolaar);
        Assert.Contains(rows, r => r.Name == "ROG Delta II" && r.Status == LightingCatalog.Verified);
    }

    [Fact]
    public void Every_verified_model_is_listed_as_verified()
    {
        var rows = LightingCatalog.All();

        Assert.Contains(rows, r => r.Name == "ROG Falchion Ace HFX" && r.Status == LightingCatalog.Verified);
        Assert.Contains(rows, r => r.Name == "ROG Strix Scope II" && r.Status == LightingCatalog.FromOpenRgb);
        Assert.Contains(rows, r => r.Name == "ROG Aura Monitor Light Bar" && r.Notes.Contains("Desk lamp"));
        Assert.Contains(rows, r => r.Type == "Motherboard" && r.Status.StartsWith(LightingCatalog.Verified + " (19AF)", StringComparison.Ordinal));
        Assert.Equal(
            AuraUsb.Families.Count,
            rows.Where(r => r.Type == "Motherboard").Sum(r => r.UsbIds.Split(", ").Length));
    }
}
