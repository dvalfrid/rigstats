using System.Runtime.InteropServices;
using System.Text.Json.Nodes;

namespace SensorSidecar.Control.Providers;

/// Thin seam over `powrprof.dll` so `PowerPlanProvider` is unit-testable
/// against a fake instead of calling real OS power APIs (see
/// `PowerPlanProviderTests`). `Win32PowerPlanApi` is the only implementation
/// that actually calls Windows.
public interface IPowerPlanApi
{
    Guid GetActiveScheme();
    void SetActiveScheme(Guid scheme);
    IReadOnlyList<(Guid Guid, string Name)> EnumerateSchemes();
}

/// P/Invoke wrapper around `powrprof.dll`. No logic beyond marshaling —
/// keep this file thin so `PowerPlanProvider`'s actual behavior (mapping,
/// validation, rollback) stays testable without it.
public sealed class Win32PowerPlanApi : IPowerPlanApi
{
    [DllImport("powrprof.dll")]
    private static extern uint PowerGetActiveScheme(IntPtr userRootPowerKey, out IntPtr activePolicyGuid);

    [DllImport("powrprof.dll")]
    private static extern uint PowerSetActiveScheme(IntPtr userRootPowerKey, ref Guid schemeGuid);

    [DllImport("powrprof.dll")]
    private static extern uint PowerEnumerate(
        IntPtr rootPowerKey,
        IntPtr schemeGuid,
        IntPtr subGroupOfPowerSettingsGuid,
        uint accessFlags,
        uint index,
        IntPtr buffer,
        ref uint bufferSize);

    [DllImport("powrprof.dll")]
    private static extern uint PowerReadFriendlyName(
        IntPtr rootPowerKey,
        ref Guid schemeGuid,
        IntPtr subGroupOfPowerSettingGuid,
        IntPtr powerSettingGuid,
        IntPtr buffer,
        ref uint bufferSize);

    [DllImport("kernel32.dll")]
    private static extern IntPtr LocalFree(IntPtr hMem);

    private const uint AccessScheme = 16; // ACCESS_SCHEME
    private const uint ErrorSuccess = 0;

    public Guid GetActiveScheme()
    {
        var status = PowerGetActiveScheme(IntPtr.Zero, out var guidPtr);
        if (status != ErrorSuccess || guidPtr == IntPtr.Zero)
            throw new InvalidOperationException($"PowerGetActiveScheme failed (0x{status:x8}).");
        try
        {
            return Marshal.PtrToStructure<Guid>(guidPtr);
        }
        finally
        {
            LocalFree(guidPtr);
        }
    }

    public void SetActiveScheme(Guid scheme)
    {
        var status = PowerSetActiveScheme(IntPtr.Zero, ref scheme);
        if (status != ErrorSuccess)
            throw new InvalidOperationException($"PowerSetActiveScheme failed (0x{status:x8}).");
    }

    public IReadOnlyList<(Guid Guid, string Name)> EnumerateSchemes()
    {
        var result = new List<(Guid, string)>();
        for (uint index = 0; ; index++)
        {
            uint bufferSize = (uint)Marshal.SizeOf<Guid>();
            var buffer = Marshal.AllocHGlobal((int)bufferSize);
            try
            {
                var status = PowerEnumerate(IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, AccessScheme, index, buffer, ref bufferSize);
                if (status != ErrorSuccess)
                    break; // ERROR_NO_MORE_ITEMS or similar — enumeration done.

                var guid = Marshal.PtrToStructure<Guid>(buffer);
                result.Add((guid, ReadFriendlyName(guid)));
            }
            finally
            {
                Marshal.FreeHGlobal(buffer);
            }
        }
        return result;
    }

    private static string ReadFriendlyName(Guid scheme)
    {
        uint size = 0;
        var status = PowerReadFriendlyName(IntPtr.Zero, ref scheme, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, ref size);
        if (status != ErrorSuccess || size == 0)
            return scheme.ToString();

        var buffer = Marshal.AllocHGlobal((int)size);
        try
        {
            status = PowerReadFriendlyName(IntPtr.Zero, ref scheme, IntPtr.Zero, IntPtr.Zero, buffer, ref size);
            return status == ErrorSuccess ? Marshal.PtrToStringUni(buffer) ?? scheme.ToString() : scheme.ToString();
        }
        finally
        {
            Marshal.FreeHGlobal(buffer);
        }
    }
}

/// Phase 0's reference `IControlProvider` — the only real hardware-control
/// write in Control Center phase 0, chosen because it's trivial and has no
/// driver risk (see `docs/control-architecture.md`, "Providers"). Maps
/// symbolic scheme names (as stored in `ProfilePart.PowerPlan`) to Windows'
/// well-known scheme GUIDs, falling back to whatever `EnumerateSchemes()`
/// reports for anything not in the well-known set (e.g. a vendor-added
/// scheme, or "Ultimate Performance" which isn't enabled on every machine).
public sealed class PowerPlanProvider(IPowerPlanApi api) : IControlProvider
{
    public string Domain => "power_plan";

    // Windows' well-known power scheme GUIDs — stable across all Windows
    // versions that expose them (Ultimate Performance is opt-in via
    // `powercfg -duplicatescheme`, not always present).
    private static readonly IReadOnlyDictionary<string, Guid> WellKnown = new Dictionary<string, Guid>
    {
        ["power_saver"] = new Guid("a1841308-3541-4fab-bc81-f71556f20b4a"),
        ["balanced"] = new Guid("381b4222-f694-41f0-9685-ff5bb260df2e"),
        ["high_performance"] = new Guid("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"),
        ["ultimate_performance"] = new Guid("e9a42b02-d5df-448d-aa00-03f14749eb61"),
    };

    private Guid? ResolveScheme(string name, IReadOnlyList<(Guid Guid, string Name)> available)
    {
        if (WellKnown.TryGetValue(name, out var guid) && available.Any(s => s.Guid == guid))
            return guid;
        // Not a well-known name — allow matching by the scheme's own friendly
        // name too, so a vendor/custom scheme can still be selected.
        var byFriendlyName = available.FirstOrDefault(s => string.Equals(s.Name, name, StringComparison.OrdinalIgnoreCase));
        return byFriendlyName.Guid != default ? byFriendlyName.Guid : null;
    }

    public CapabilitySet Probe()
    {
        var schemes = api.EnumerateSchemes();
        var details = new JsonObject
        {
            ["schemes"] = new JsonArray(schemes.Select(s => (JsonNode)new JsonObject
            {
                ["id"] = WellKnown.FirstOrDefault(w => w.Value == s.Guid).Key ?? s.Guid.ToString(),
                ["name"] = s.Name,
            }).ToArray()),
        };
        return new CapabilitySet { Domain = Domain, Supported = schemes.Count > 0, Details = details };
    }

    public ValidationResult Validate(ProfilePart part)
    {
        if (part.PowerPlan is null)
            return ValidationResult.Success(); // domain untouched — not this provider's concern.

        var available = api.EnumerateSchemes();
        var resolved = ResolveScheme(part.PowerPlan, available);
        return resolved is null
            ? ValidationResult.Failure($"Unknown power plan '{part.PowerPlan}'.")
            : ValidationResult.Success(JsonValue.Create(part.PowerPlan));
    }

    public Snapshot Capture() =>
        new() { Domain = Domain, State = JsonValue.Create(api.GetActiveScheme().ToString()) };

    public void Apply(ProfilePart part)
    {
        if (part.PowerPlan is null)
            return;
        var available = api.EnumerateSchemes();
        var resolved = ResolveScheme(part.PowerPlan, available)
            ?? throw new InvalidOperationException($"Unknown power plan '{part.PowerPlan}'.");
        api.SetActiveScheme(resolved);
    }

    public bool Verify(ProfilePart part)
    {
        if (part.PowerPlan is null)
            return true;
        var available = api.EnumerateSchemes();
        var resolved = ResolveScheme(part.PowerPlan, available);
        return resolved is not null && api.GetActiveScheme() == resolved;
    }

    public void Restore(Snapshot snapshot)
    {
        if (snapshot.Domain != Domain || snapshot.State is null)
            return;
        var guidString = snapshot.State.GetValue<string>();
        api.SetActiveScheme(Guid.Parse(guidString));
    }

    // Power plan is always OS-owned — there is nothing to "hand back to
    // firmware" the way a fan override or undervolt would need to be.
    public void ReleaseToFirmware() { }
}
