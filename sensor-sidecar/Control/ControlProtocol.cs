using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace SensorSidecar.Control;

/// Shared JSON convention for the control pipe — snake_case, matching the
/// existing telemetry pipe's `_jsonOptions` in `SensorWorker.cs` and the
/// design doc's wire examples (`app_version`, `power_plan`, ...).
public static class ControlJson
{
    public static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };
}

/// One domain a provider is responsible for — mirrors `ProfilePart`'s
/// nullable properties (`"fan"`, `"cpu_limit"`, ...) in `docs/control-architecture.md`.
public interface IControlProvider
{
    /// `"power_plan"`, `"fan"`, `"cpu_limit"`, `"curve_opt"`, `"gpu"`, `"aura"`.
    string Domain { get; }

    /// What this provider can do on this machine — probed on every
    /// `capabilities` request.
    CapabilitySet Probe();

    /// Reject `part`'s value for this provider's domain if it can't be
    /// applied. Never trust UI-supplied values as-is: `Apply` clamps what
    /// passes to the probed hard limits at write time.
    ValidationResult Validate(ProfilePart part);

    /// Current hardware state for this domain, so a failed transaction can
    /// be rolled back with <see cref="Restore"/>.
    Snapshot Capture();

    /// Apply `part`'s value for this provider's domain. Must have already
    /// passed <see cref="Validate"/>.
    void Apply(ProfilePart part);

    /// Read back hardware state and confirm `part` actually took effect
    /// (some values are silently ignored/overridden by firmware).
    bool Verify(ProfilePart part);

    /// Undo back to a previously captured snapshot (transaction rollback).
    void Restore(Snapshot snapshot);

    /// Hand control back to firmware/BIOS — the panic-button and
    /// stop/crash-safety path. Must be safe to call even if nothing was
    /// ever applied. Returns what was handed back, in a few words for the
    /// service log ("2 headers to the BIOS (Fan #1, Fan #2)"), or null when
    /// there was nothing to hand back (#385).
    string? ReleaseToFirmware();
}

/// Optional for an `IControlProvider`: whether applying `part` could change
/// anything in its domain. False skips the domain for that apply entirely —
/// no hardware read — so a backend that is unavailable (a GPU driver that
/// won't initialize) can't block a profile that asks nothing of it.
/// Providers without it are always applied.
public interface IAffectsPart
{
    bool Affects(ProfilePart part);
}

/// What a provider can do on this machine, reported once from `Probe()`.
public sealed class CapabilitySet
{
    public required string Domain { get; init; }
    public bool Supported { get; init; }
    /// Human-readable reason when `Supported` is false (e.g. "locked by BIOS").
    public string? Reason { get; init; }
    /// Set when the domain is unavailable because something went wrong
    /// (not because this PC simply lacks it): the app then shows the page
    /// with the problem instead of hiding it.
    public Problem? Problem { get; init; }
    /// Free-form per-domain detail (available power schemes, fan header ids, ...).
    public JsonNode? Details { get; init; }
}

/// Something the user should know about and may be able to fix (#370) — the
/// one shape every kind of problem takes on its way to the app, which shows
/// them all the same way (`ui_kit::notice`). Written for someone who has
/// never seen the code: what happened, what still works, what to do.
public sealed class Problem
{
    /// Stable, for tests and to tell one problem from another ("gpu_driver_crashed").
    public required string Id { get; init; }

    /// One of the <see cref="ProblemSeverity"/> values.
    public required string Severity { get; init; }

    /// Short, in sentence case ("The graphics driver crashed").
    public required string Title { get; init; }

    /// What happened, what still works and what to do — plain words.
    public required string Message { get; init; }

    /// What the user can do from the app, in order of usefulness.
    public IReadOnlyList<ProblemAction> Actions { get; init; } = [];

    /// The technical detail for a bug report (exit code, driver files).
    public string? Detail { get; init; }
}

public static class ProblemSeverity
{
    public const string Info = "info";
    public const string Attention = "attention";
    public const string Error = "error";
}

/// A button on a problem: open a web page, or collect a diagnostics ZIP.
public sealed class ProblemAction
{
    /// One of the <see cref="ProblemActionKind"/> values.
    public required string Kind { get; init; }
    public required string Label { get; init; }

    /// The URL for <see cref="ProblemActionKind.OpenUrl"/>.
    public string? Target { get; init; }
}

public static class ProblemActionKind
{
    public const string OpenUrl = "open_url";
    public const string CollectDiagnostics = "collect_diagnostics";
}

/// The service's problems for a domain that failed (#373), in the same plain
/// words as the app's own (`src-egui/src/problems.rs`).
public static class ControlProblems
{
    public const string TroubleshootingUrl = "https://github.com/dvalfrid/rigstats/blob/main/docs/troubleshooting.md";

    public static ProblemAction Diagnostics() =>
        new() { Kind = ProblemActionKind.CollectDiagnostics, Label = "Collect diagnostics…" };

    public static ProblemAction ReadMore(string anchor) =>
        new() { Kind = ProblemActionKind.OpenUrl, Label = "Read more", Target = TroubleshootingUrl + anchor };

    /// ASUS's FAQ with its Armoury Crate Uninstall Tool — the supported way
    /// to remove Armoury Crate and its services (#193); a reinstall brings
    /// it back, so the step is reversible.
    public const string ArmouryCrateUninstallUrl = "https://rog.asus.com/support/FAQ/1041654";

    public static ProblemAction ArmouryCrateUninstallTool() =>
        new() { Kind = ProblemActionKind.OpenUrl, Label = "ASUS uninstall tool", Target = ArmouryCrateUninstallUrl };

    /// The program's own name from "Armoury Crate (LightingService)".
    private static string ProgramOf(string other) => other.Split(" (")[0];

    /// What a domain is called on its page ("Fan control didn't answer").
    public static string Name(string domain) => domain switch
    {
        "fan" => "Fan control",
        "cpu_limit" => "CPU power limits",
        "curve_opt" => "Curve Optimizer",
        "gpu" => "The graphics card's power limit",
        "aura" => "Lighting",
        "power_plan" => "Power plans",
        _ => domain,
    };

    /// A provider threw while asked what it can do.
    public static Problem DidNotAnswer(string domain, string error) => new()
    {
        Id = $"{domain}_unavailable",
        Severity = ProblemSeverity.Error,
        Title = $"{Name(domain)} didn't answer",
        Message = "RIGStats asked the hardware behind this page and got no answer, so it is off for now. "
            + "Everything else keeps working. Restarting Windows usually brings it back; if it doesn't, send diagnostics.",
        Actions = [Diagnostics()],
        Detail = error,
    };

    /// The AMD SMU didn't answer a CPU power-limit read.
    public static Problem SmuDidNotAnswer(string error) => new()
    {
        Id = "cpu_limit_smu",
        Severity = ProblemSeverity.Attention,
        Title = "The CPU didn't answer",
        Message = "RIGStats couldn't read the CPU's power limits from its management unit (SMU), so CPU limits are off for now. "
            + "Everything else keeps working. It usually answers again after a restart; if not, send diagnostics.",
        Actions = [Diagnostics()],
        Detail = error,
    };

    /// Another program owns the lighting ("Armoury Crate (LightingService)",
    /// "OpenRGB"): the title names the program, the message the full name.
    public static Problem LightingTakenBy(string other)
    {
        var program = ProgramOf(other);
        return new()
        {
            Id = "aura_conflict",
            Severity = ProblemSeverity.Attention,
            Title = $"{program} controls the lighting",
            Message = $"Only one program can drive the lighting at a time, and {other} has it. "
                + $"Close {program} — or turn off its lighting — then reopen the Control Center to control the lighting here.",
            Actions = program == "Armoury Crate"
                ? [ArmouryCrateUninstallTool(), ReadMore("#control-center")]
                : [ReadMore("#control-center")],
        };
    }

    /// Another program sets the fan speeds ("Armoury Crate
    /// (AsusFanControlService)", #193): RIGStats leaves the fans to it. A
    /// service can't be closed like an app, so the way forward is ASUS's
    /// uninstall tool.
    public static Problem FansTakenBy(string other)
    {
        var program = ProgramOf(other);
        var fix = program == "Armoury Crate"
            ? "Armoury Crate runs as a Windows service, so closing it isn't enough: remove it with ASUS's uninstall tool (a reinstall brings it back), then reopen the Control Center."
            : $"Close {program}, then reopen the Control Center.";
        return new()
        {
            Id = "fan_conflict",
            Severity = ProblemSeverity.Attention,
            Title = $"{program} controls the fans",
            Message = $"Only one program can set the fan speeds at a time, and {other} has them — two would fight over every fan. "
                + "RIGStats leaves the fans to it, so fan curves are off here; everything else keeps working. "
                + fix,
            Actions = program == "Armoury Crate"
                ? [ArmouryCrateUninstallTool(), ReadMore("#control-center")]
                : [ReadMore("#control-center")],
        };
    }
}

public sealed class ValidationResult
{
    public bool Ok { get; init; }
    public string? Reason { get; init; }

    public static ValidationResult Success() => new() { Ok = true };

    public static ValidationResult Failure(string reason) =>
        new() { Ok = false, Reason = reason };
}

/// Opaque per-domain hardware state captured before `Apply`, handed back to
/// `Restore` unchanged. Providers decide their own snapshot shape.
public sealed class Snapshot
{
    public required string Domain { get; init; }
    public JsonNode? State { get; init; }
}

/// One fan header's curve, keyed by its LHM control identifier (e.g.
/// `"lpc/nct6799d/0/control/1"`) in <see cref="ProfilePart.Fan"/>'s
/// `Headers` map.
public sealed class FanHeaderConfig
{
    /// User-facing label (LHM can't tell which header is the CPU cooler —
    /// see `identify_fan` and the ROADMAP's "CPU fan speed" note).
    public string? Label { get; init; }

    /// Source temperature sensor: `"cpu_package"`, `"gpu"`, or a motherboard
    /// temp sensor identifier.
    public required string Source { get; init; }

    /// `[[tempC, dutyPct], ...]`, sorted ascending by temperature.
    /// `FanProvider.Validate` rejects an unsorted curve; duty values are
    /// clamped to the probed hardware min/max at write time.
    public required List<List<double>> Curve { get; init; }

    /// Minimum temperature swing (°C) before the evaluated duty is allowed
    /// to change again, so small fluctuations around a curve breakpoint
    /// don't chatter the fan. See `FanCurveEvaluator.EvaluateHeader`.
    public double HysteresisC { get; init; } = 3.0;
}

public sealed class FanPart
{
    public Dictionary<string, FanHeaderConfig>? Headers { get; init; }
}

/// AMD package limits (#189). A null value means "the BIOS value" — so an
/// empty object puts every limit back to what the firmware set at boot.
public sealed class AmdCpuLimit
{
    public double? PptW { get; init; }
    public double? TdcA { get; init; }
    public double? EdcA { get; init; }
}

public sealed class CpuLimitPart
{
    public AmdCpuLimit? Amd { get; init; }

    /// Intel PL1/PL2 — not implemented yet (needs a newer signed IntelMSR
    /// module); kept as passthrough so stored values survive a round trip.
    public JsonNode? Intel { get; init; }
}

/// Lighting (#192): one effect for every zone of the lighting controller.
public sealed class AuraPart
{
    /// "off", "static", "breathing", "spectrum_cycle".
    public string? Effect { get; init; }

    /// "#rrggbb" — ignored by "off" and "spectrum_cycle".
    public string? Color { get; init; }

    /// 0–1, scales the colour.
    public double? Brightness { get; init; }

    /// White lamps beside the RGB (the ROG light bar's desk lamp, #214);
    /// null leaves them alone. A part with a lamp and no effect leaves the
    /// RGB alone.
    public LampPart? Lamp { get; init; }
}

/// A profile's mouse part (#431): which of the mouse's own onboard profiles
/// it uses (1-based). The settings inside an onboard profile are kept in the
/// mouse and edited on the Mouse page.
public sealed class MousePart
{
    public int? Profile { get; init; }
}

/// A profile's headset part (#483): the equalizer preset (by name, e.g.
/// "FPS") the headsets switch to with the profile. The equalizer's own
/// sliders are kept in the headset and edited on the Headset page.
public sealed class HeadsetPart
{
    public string? EqPreset { get; init; }
}

/// A profile's keyboard part (#458): which of the keyboard's own onboard
/// profiles it uses (1-based, as the keyboard and its Fn + 1…6 keys count).
public sealed class KeyboardPart
{
    public int? Profile { get; init; }
}

/// A white lamp: on or off, how bright and how warm.
public sealed class LampPart
{
    public bool On { get; init; }

    /// 0–1 of the lamp's allowed maximum.
    public double? Brightness { get; init; }

    /// Colour temperature in kelvin, 2700 (warm) – 6500 (cool).
    public int? Temperature { get; init; }
}

/// Curve Optimizer offsets (#191), in CO counts (negative = less voltage).
/// A core's value is `PerCore[index]`, else `AllCore`, else the BIOS value.
public sealed class CurveOptPart
{
    public int? AllCore { get; init; }

    /// Keyed by core index as a string ("0", "1", ...) in CCD/core order.
    public Dictionary<string, int>? PerCore { get; init; }
}

/// One GPU's power limit (#190), keyed by adapter id in <see cref="GpuPart.Adapters"/>.
public sealed class GpuAdapterConfig
{
    /// Offset from the driver default in % (0 = default). Null = the value
    /// that was in force before RIGStats first changed it.
    public int? PowerLimitPct { get; init; }
}

/// A profile's GPU part. Adapters missing from `Adapters` go back to the
/// value they had before RIGStats changed them.
public sealed class GpuPart
{
    public Dictionary<string, GpuAdapterConfig>? Adapters { get; init; }
}

/// One profile's per-domain settings. Every property is optional — a missing
/// part leaves that domain untouched by `ControlBroker`. `PowerPlan` (#187),
/// `Fan` (#188), `CpuLimit` (#189), `Gpu` (#190), `CurveOpt` (#191) and
/// `Aura` (#192) all have typed shapes.
public sealed class ProfilePart
{
    /// Symbolic scheme name: `"power_saver"`, `"balanced"`, `"high_performance"`,
    /// `"ultimate_performance"` — see `PowerPlanProvider`'s mapping to the
    /// well-known Windows scheme GUIDs.
    public string? PowerPlan { get; init; }

    public FanPart? Fan { get; init; }
    public CpuLimitPart? CpuLimit { get; init; }
    public CurveOptPart? CurveOpt { get; init; }
    public GpuPart? Gpu { get; init; }
    public AuraPart? Aura { get; init; }
    public MousePart? Mouse { get; init; }
    public KeyboardPart? Keyboard { get; init; }
    public HeadsetPart? Headset { get; init; }
}

public sealed class Profile
{
    public required string Id { get; init; }
    public required string Name { get; init; }
    public string? Icon { get; init; }
    public bool Builtin { get; init; }
    public required ProfilePart Part { get; init; }
}

/// The whole on-disk `profiles.json` shape.
public sealed class ProfileFile
{
    public string? Active { get; init; }
    public List<Profile> Profiles { get; init; } = [];
}

/// One consolidated result for a whole `apply_profile`/`preview` transaction —
/// never per-provider, per `ControlBroker`'s doc contract.
public sealed class ApplyResult
{
    public bool Ok { get; init; }
    /// e.g. "Profile Gaming not applied: CPU power limit is locked by BIOS."
    public string? Message { get; init; }
    public string? ProfileId { get; init; }

    public static ApplyResult Success(string profileId) =>
        new() { Ok = true, ProfileId = profileId };

    /// Applied, with a note — e.g. a domain that was down and skipped.
    public static ApplyResult Success(string profileId, string message) =>
        new() { Ok = true, ProfileId = profileId, Message = message };

    public static ApplyResult Failure(string profileId, string message) =>
        new() { Ok = false, ProfileId = profileId, Message = message };
}

// ── Pipe envelope ───────────────────────────────────────────────────────

/// A domain is down for a known reason (its driver crashed, #370): a profile
/// applies the rest and skips it, instead of rolling everything back. Skipping
/// is the safe direction — the hardware keeps what it has. Any other
/// exception still rolls the whole profile back.
public class DomainUnavailableException(string message) : InvalidOperationException(message)
{
    /// Wireless devices that didn't answer a profile switch (#484) — see
    /// <see cref="DeviceAsleepException"/>.
    public static DomainUnavailableException Silent(IReadOnlyList<string> names) => new DeviceAsleepException(names);
}

/// A wireless device (keyboard, mouse, headset) didn't answer a profile
/// switch — asleep or out of range. The broker tries its part again until
/// it answers (#489), so the user reads that it will catch up by itself:
/// "ROG Azoth X didn't answer — probably asleep — and gets the profile as
/// soon as it wakes up." The technical reason goes to the log.
public sealed class DeviceAsleepException(IReadOnlyList<string> names) : DomainUnavailableException(names.Count == 1
    ? $"{names[0]} didn't answer — probably asleep — and gets the profile as soon as it wakes up."
    : $"{string.Join(" and ", names)} didn't answer — probably asleep — and get the profile as soon as they wake up.")
{
    public IReadOnlyList<string> Names { get; } = names;
}

/// One incoming NDJSON line: `{"id":1,"method":"hello","params":{...}}`.
public sealed class ControlRequest
{
    public long Id { get; init; }
    public string Method { get; init; } = "";
    public JsonElement? Params { get; init; }
}

/// One outgoing response line: `{"id":1,"result":{...}}` or `{"id":1,"error":{...}}`.
public sealed class ControlResponse
{
    public long Id { get; init; }
    public object? Result { get; init; }
    public ControlErrorBody? Error { get; init; }

    public static ControlResponse Ok(long id, object? result) => new() { Id = id, Result = result };

    public static ControlResponse Fail(long id, string code, string message) =>
        new() { Id = id, Error = new ControlErrorBody { Code = code, Message = message } };
}

public sealed class ControlErrorBody
{
    public string Code { get; init; } = "";
    public string Message { get; init; } = "";
}

/// One unsolicited pushed line (no `id`): `{"event":"profile_changed","data":{...}}`.
public sealed class ControlEventMessage
{
    public string Event { get; init; } = "";
    public object? Data { get; init; }
}

/// A control-pipe request that is wrong in itself — a parameter missing or of
/// the wrong kind: answered `bad_request`, logged without a stack (#378).
public sealed class BadRequestException(string message) : Exception(message);
