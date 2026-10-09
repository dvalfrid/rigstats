---
name: add-control-provider
description: Add a new hardware-control domain to the Control Center service (an IControlProvider in sensor-sidecar/Control/Providers/) — e.g. Intel PL1/PL2, ASUS ATKACPI, a new GPU vendor. Use when a task adds a new thing the service writes to hardware, or extends a profile with a new part. Not for lighting devices (use add-lighting-device).
---

A provider writes to real hardware as LocalSystem. Read `sensor-sidecar/Control/CLAUDE.md` (safety rules) first. Never run it against real hardware yourself — the owner tests live.

## Steps

1. **Issue + branch** per `/commit-workflow`.
2. **Profile part** — add a `<Name>Part` class and a property on `ProfilePart` in `ControlProtocol.cs` (snake_case on the wire via `ControlJson.Options`). A part left `null` means "don't touch this domain".
3. **Provider** — `Providers/<Name>Provider.cs`, `public sealed class`, implementing `IControlProvider`:
   - `Domain` — new snake_case string.
   - `Probe()` — what this machine supports; unsupported → `Supported = false` + a user-readable `Reason`. Must not throw for "no such hardware".
   - `Validate(part)` — reject nonsense; real clamping happens at write time.
   - `Capture()` / `Restore()` — the broker's rollback when a later domain fails.
   - `Apply(part)` — **clamp to probed hardware limits here**, honour `dryRun` (log only).
   - `Verify(part)` — read back; firmware may silently ignore a write.
   - `ReleaseToFirmware()` — back to the BIOS / pre-RIGStats value; safe when nothing was applied; never throws.
   - Optional `IAffectsPart` when an unavailable backend must not block profiles that don't use it.
4. **Risky?** (can crash or destabilise the machine — voltage, power limits): take `BootCrashGuard` and call `crashGuard.Arm()` *before* the write (as `CurveOptimizerProvider` does), and add the part to `ActiveProfileApplier`'s tripped-at-start reset.
5. **Wire up**
   - `Program.cs`: `builder.Services.AddSingleton<IControlProvider>(…)`.
   - `ControlBroker.HasPart`: map the new domain to its part.
   - Rust: mirror the part and capability in `rigstats-backend/src/control.rs` (serde, snake_case), UI in `src-egui/src/windows/control.rs`. User-tried changes go through `preview` (auto-revert).
6. **Tests** — `sensor-sidecar.Tests/<Name>ProviderTests.cs` with the hardware behind an interface/fake (see `CpuLimitProviderTests`, `FakeFanHeader`): clamping, dry-run, verify-failure → rollback, release when nothing applied. Add a wire-shape case to `ControlProtocolSerializationTests` and a serde test in `control.rs` for the same JSON.
7. **Docs** — "Providers" table + an "as built" section in `docs/control-architecture.md`; add the provider to "What lives here" in `sensor-sidecar/Control/CLAUDE.md`.
8. **Verify** — `cargo xtask verify` (stop `rigstats-sensor` first; ask the owner if it runs). Then hand over for live testing: tell the owner exactly what to try with `tools\dev-sidecar.ps1 -Live` and what to watch in the log.

## Don'ts

- No command ids or registers that aren't documented or observed — no sweeps, not even reads.
- Don't gate on a device *name* when a measurement or a version check is possible (SMU: `AmdSmuMap.Layout(tableVersion)`).
- No new NuGet package without the owner's approval.
