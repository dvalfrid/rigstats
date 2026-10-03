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

    /// Probed once at start-up: what this provider can do on this machine.
    CapabilitySet Probe();

    /// Clamp/reject `part`'s value for this provider's domain against the
    /// probed hard limits. Never trust UI-supplied values as-is.
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
    /// ever applied.
    void ReleaseToFirmware();
}

/// What a provider can do on this machine, reported once from `Probe()`.
public sealed class CapabilitySet
{
    public required string Domain { get; init; }
    public bool Supported { get; init; }
    /// Human-readable reason when `Supported` is false (e.g. "locked by BIOS").
    public string? Reason { get; init; }
    /// Free-form per-domain detail (available power schemes, fan header ids, ...).
    public JsonNode? Details { get; init; }
}

public sealed class ValidationResult
{
    public bool Ok { get; init; }
    public string? Reason { get; init; }
    /// The value actually clamped to hard limits — may differ from what was requested.
    public JsonNode? ClampedValue { get; init; }

    public static ValidationResult Success(JsonNode? clampedValue = null) =>
        new() { Ok = true, ClampedValue = clampedValue };

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
    /// `FanProvider.Validate` clamps duty values to the probed hardware
    /// min/max and rejects an unsorted curve.
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

/// One profile's per-domain settings. Every property is optional — a missing
/// part leaves that domain untouched by `ControlBroker`. `PowerPlan` (#187),
/// `Fan` (#188) and `CpuLimit` (#189) have typed shapes; the remaining
/// domains are raw JSON passthrough so `ProfileStore` round-trips them
/// untouched even before their providers (GPU #190, Curve Optimizer #191,
/// Aura #192) exist — each phase replaces its own placeholder with a typed shape.
public sealed class ProfilePart
{
    /// Symbolic scheme name: `"power_saver"`, `"balanced"`, `"high_performance"`,
    /// `"ultimate_performance"` — see `PowerPlanProvider`'s mapping to the
    /// well-known Windows scheme GUIDs.
    public string? PowerPlan { get; init; }

    public FanPart? Fan { get; init; }
    public CpuLimitPart? CpuLimit { get; init; }
    public JsonNode? CurveOpt { get; init; }
    public JsonNode? Gpu { get; init; }
    public JsonNode? Aura { get; init; }
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

    public static ApplyResult Failure(string profileId, string message) =>
        new() { Ok = false, ProfileId = profileId, Message = message };
}

// ── Pipe envelope ───────────────────────────────────────────────────────

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
