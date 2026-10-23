using System.Text.Json;
using System.Text.Json.Nodes;
using SensorSidecar.Control.Lighting;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Real HID scans (#572): `lighting-devices.json`'s `hid_scan` from users'
/// diagnostics ZIPs, in <c>fixtures/hid-scan/*.json</c>. Every collection goes
/// through <see cref="LightingDiagnostics.KnownAs"/> and must come out as its
/// `known_as` says — so a device that stops being recognised, or starts
/// (new support), shows as a reviewed diff. Regenerate with
/// RIGSTATS_WRITE_EXPECTED=1 and copy the files from the test output's
/// fixtures/hid-scan back. See fixtures/README.md, "HID scans".
/// </summary>
public class HidScanFixtureTests
{
    private static string Root => Path.Combine(AppContext.BaseDirectory, "fixtures", "hid-scan");

    public static IEnumerable<object[]> Scans() =>
        Directory.Exists(Root)
            ? Directory.EnumerateFiles(Root, "*.json").Select(f => new object[] { Path.GetFileName(f) })
            : [];

    [Theory]
    [MemberData(nameof(Scans))]
    public void Every_collection_is_known_as_recorded(string file)
    {
        var path = Path.Combine(Root, file);
        var doc = JsonNode.Parse(File.ReadAllText(path))!.AsObject();
        var wrong = new List<string>();
        foreach (var h in doc["hid_scan"]!.AsArray().Select(n => n!.AsObject()))
        {
            var actual = LightingDiagnostics.KnownAs(Info(h));
            var recorded = h["known_as"]?.GetValue<string>();
            if (actual != recorded)
                wrong.Add($"{h["vendor_id"]}:{h["product_id"]} mi {h["interface"]} page {h["usage_page"]} '{h["product"]}': recorded {recorded ?? "null"}, now {actual ?? "null"}");
            h["known_as"] = actual;
        }

        if (Environment.GetEnvironmentVariable("RIGSTATS_WRITE_EXPECTED") == "1")
        {
            File.WriteAllText(path, doc.ToJsonString(new JsonSerializerOptions { WriteIndented = true, Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping }) + "\n");
            return;
        }
        Assert.True(wrong.Count == 0, $"{file}:\n  " + string.Join("\n  ", wrong));
    }

    // The scan's fields as the enumeration gave them; the interface number
    // goes back into the path the way Windows writes it (`&mi_02`).
    private static HidDeviceInfo Info(JsonObject h)
    {
        ushort Hex(string key) => Convert.ToUInt16(h[key]!.GetValue<string>(), 16);
        var iface = h["interface"]?.GetValue<int?>();
        var path = $@"\\?\hid#vid_{h["vendor_id"]}&pid_{h["product_id"]}" + (iface is { } mi ? $"&mi_{mi:X2}" : "") + "#fixture";
        return new HidDeviceInfo(path, Hex("vendor_id"), Hex("product_id"), Hex("usage_page"), Hex("usage"),
            h["output_len"]!.GetValue<int>(), h["input_len"]!.GetValue<int>(), h["feature_len"]!.GetValue<int>(),
            h["product"]?.GetValue<string>(), (byte?)h["output_report_id"]?.GetValue<int>());
    }
}
