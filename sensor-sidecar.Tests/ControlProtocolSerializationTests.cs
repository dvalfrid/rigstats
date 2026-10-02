using System.Text.Json;
using SensorSidecar.Control;
using Xunit;

namespace SensorSidecar.Tests;

/// <summary>
/// Wire-shape tests for the control pipe's NDJSON envelope and profile
/// model — snake_case per `docs/control-architecture.md`'s examples. The
/// cross-language round-trip against `rigstats-backend/src/control.rs`'s
/// serde types happens once that side exists (Stage 2 of the #187 plan);
/// this locks down the C# side of the contract in the meantime.
/// </summary>
public class ControlProtocolSerializationTests
{
    [Fact]
    public void Request_deserializes_the_doc_hello_example()
    {
        const string json = """{"id":1,"method":"hello","params":{"protocol":1,"app_version":"3.0.0"}}""";

        var request = JsonSerializer.Deserialize<ControlRequest>(json, ControlJson.Options)!;

        Assert.Equal(1, request.Id);
        Assert.Equal("hello", request.Method);
        Assert.Equal(1, request.Params!.Value.GetProperty("protocol").GetInt32());
        Assert.Equal("3.0.0", request.Params!.Value.GetProperty("app_version").GetString());
    }

    [Fact]
    public void Response_serializes_result_as_snake_case()
    {
        var response = ControlResponse.Ok(1, new { protocol = 1, service_version = "3.0.0" });

        var json = JsonSerializer.Serialize(response, ControlJson.Options);

        Assert.Contains("\"id\":1", json);
        Assert.Contains("\"service_version\":\"3.0.0\"", json);
        Assert.DoesNotContain("error", json); // omitted when null, per ControlJson.Options.
    }

    [Fact]
    public void Response_serializes_error_and_omits_result()
    {
        var response = ControlResponse.Fail(2, "not_found", "profile 'x' not found.");

        var json = JsonSerializer.Serialize(response, ControlJson.Options);

        Assert.Contains("\"code\":\"not_found\"", json);
        Assert.DoesNotContain("\"result\"", json);
    }

    [Fact]
    public void Profile_round_trips_the_doc_gaming_example_shape()
    {
        const string json = """
            {
              "id": "gaming", "name": "Gaming", "icon": "bolt", "builtin": true,
              "part": { "power_plan": "high_performance" }
            }
            """;

        var profile = JsonSerializer.Deserialize<Profile>(json, ControlJson.Options)!;

        Assert.Equal("gaming", profile.Id);
        Assert.True(profile.Builtin);
        Assert.Equal("high_performance", profile.Part.PowerPlan);

        var roundTripped = JsonSerializer.Deserialize<Profile>(
            JsonSerializer.Serialize(profile, ControlJson.Options), ControlJson.Options)!;
        Assert.Equal(profile.Id, roundTripped.Id);
        Assert.Equal(profile.Part.PowerPlan, roundTripped.Part.PowerPlan);
    }

    [Fact]
    public void ProfileFile_round_trips_active_plus_profiles_list()
    {
        var file = new ProfileFile
        {
            Active = "gaming",
            Profiles =
            [
                new Profile { Id = "gaming", Name = "Gaming", Builtin = true, Part = new ProfilePart { PowerPlan = "high_performance" } },
            ],
        };

        var json = JsonSerializer.Serialize(file, ControlJson.Options);
        var loaded = JsonSerializer.Deserialize<ProfileFile>(json, ControlJson.Options)!;

        Assert.Equal("gaming", loaded.Active);
        Assert.Single(loaded.Profiles);
    }

    [Fact]
    public void ApplyResult_failure_includes_a_consolidated_message()
    {
        var result = ApplyResult.Failure("gaming", "Profile Gaming not applied: CPU power limit is locked by BIOS.");

        var json = JsonSerializer.Serialize(result, ControlJson.Options);

        Assert.Contains("locked by BIOS", json);
        Assert.Contains("\"ok\":false", json);
    }

    [Fact]
    public void Fan_duty_event_keeps_header_ids_as_keys()
    {
        var message = ControlPipeWorker.ToEventMessage(new SensorSidecar.Control.Providers.FanEvent.DutyUpdate(
            new Dictionary<string, double> { ["/lpc/nct6799d/0/control/1"] = 42.5 }));

        var json = JsonSerializer.Serialize(message, ControlJson.Options);

        Assert.Equal("""{"event":"fan_duty","data":{"duty":{"/lpc/nct6799d/0/control/1":42.5}}}""", json);
    }

    [Fact]
    public void Fan_identified_event_lists_the_responding_rpm_sensors()
    {
        var result = new SensorSidecar.Control.Providers.FanIdentifyResult(
            "/lpc/nct6799d/0/control/1",
            [new SensorSidecar.Control.Providers.FanResponder("Fan #2", 996, 2015)]);

        var json = JsonSerializer.Serialize(ControlPipeWorker.ToIdentifiedMessage(result), ControlJson.Options);

        Assert.Equal(
            """{"event":"fan_identified","data":{"header":"/lpc/nct6799d/0/control/1","responders":[{"label":"Fan #2","before_rpm":996,"peak_rpm":2015}]}}""",
            json);
        Assert.Equal(
            "/lpc/nct6799d/0/control/1 -> Fan #2 (996 -> 2015 rpm)",
            ControlPipeWorker.DescribeIdentify(result));
    }

    [Fact]
    public void Safety_tripped_event_carries_the_reason()
    {
        var message = ControlPipeWorker.ToEventMessage(
            new SensorSidecar.Control.Providers.FanEvent.SafetyTripped("CPU 97°C"));

        var json = JsonSerializer.Serialize(message, ControlJson.Options);

        using var doc = JsonDocument.Parse(json);
        Assert.Equal("safety_tripped", doc.RootElement.GetProperty("event").GetString());
        Assert.Equal("CPU 97°C", doc.RootElement.GetProperty("data").GetProperty("reason").GetString());
    }
}
