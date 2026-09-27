# Control Center — Hardware Control Architecture

> Status: **Design / planned** (Milestone 3.0). Tracked per phase in GitHub
> Issues — see [Delivery phases](#delivery-phases).

## Contents

- [Goal](#goal)
- [Design principles](#design-principles)
- [System overview](#system-overview)
- [Service: `rigstats-service`](#service-rigstats-service)
- [Control pipe protocol](#control-pipe-protocol)
- [Profile model](#profile-model)
- [Safety model](#safety-model)
- [App integration (Rust / egui)](#app-integration-rust--egui)
- [UX integration](#ux-integration)
- [Providers](#providers)
- [Security](#security)
- [Testing strategy](#testing-strategy)
- [Licensing](#licensing)
- [Delivery phases](#delivery-phases)
- [Open questions](#open-questions)

---

## Goal

Extend RIGStats from *monitoring* to *control* — a "desktop G-Helper" with a
focus on ASUS ROG desktop boards — so users can drop Armoury Crate:

- Fan control (curves per header)
- CPU power limits (Intel PL1/PL2, AMD PPT/TDC/EDC)
- AMD Curve Optimizer (undervolt)
- GPU power limit profiles
- RGB (ASUS Aura)
- Windows power plan
- One-click / hotkey switching between Silent, Balanced, Gaming and Eco

G-Helper itself is **not portable** to desktops: it drives laptop-only ASUS
ACPI/WMI methods (`ATKACPI`). Desktop boards expose control through the Super
I/O chip, CPU MSRs/SMU, GPU vendor SDKs and a USB HID Aura controller instead.
Almost everything below is therefore vendor-neutral; the ROG-specific parts are
Aura, Armoury Crate coexistence, and a set of validated board profiles.

The hard requirement: **it must feel like one application, and it must be
stable.** Not a bundle of tools glued together.

---

## Design principles

1. **One process owns the hardware.** The existing LocalSystem sensor service
   becomes `rigstats-service` and is the *only* process that writes hardware.
   Two processes touching Super I/O / SMU concurrently is a source of
   instability, so no external helper processes (no OpenRGB server, no
   ZenStates tool, no second driver).
2. **One concept for the user: the Profile.** Fans, CPU limits, GPU, RGB and
   power plan are *parts* of a profile. The user switches profiles, never
   "tools".
3. **Capabilities drive the UI.** The service probes the machine and reports
   what is actually controllable. Unsupported features are hidden, not greyed
   out.
4. **Safety lives in the service, not the UI.** Fan curves are evaluated,
   limits clamped, and hardware reverted inside the service. If the UI crashes,
   the machine is still safe.
5. **One kernel driver: PawnIO.** Already shipped via LibreHardwareMonitor. No
   WinRing0 (flagged by Defender), no additional drivers.
6. **Telemetry stays untouched.** `\\.\pipe\rigstats-sensors` keeps its
   read-only, one-way contract. Control is added beside it, never mixed in.

---

## System overview

```text
┌──────────────── rigstats.exe (egui, user mode) ───────────────────┐
│ Dashboard panels ── Control Center ── Tray / Hotkey ── Overlay    │
│        └────────────── ControlState (in DashboardRuntime) ────────┘│
│ rigstats-backend:  lhm.rs (telemetry)    control.rs (new client)  │
└────────────┬──────────────────────────────────┬───────────────────┘
  \\.\pipe\rigstats-sensors (unchanged)   \\.\pipe\rigstats-control (new, duplex)
┌────────────┴──────────────────────────────────┴───────────────────┐
│ rigstats-service.exe (LocalSystem, .NET 10)                        │
│  HardwareHost   — single LHM Computer + single hardware lock      │
│  ControlBroker  — validate → snapshot → apply → verify → commit   │
│  SafetyGuard    — critical temp, watchdog, boot-crash guard        │
│  ProfileStore   — %ProgramData%\se.codeby.rigstats\profiles.json  │
│  Providers (IControlProvider):                                    │
│    PowerPlanProvider   FanProvider         CpuLimitProvider       │
│    CurveOptimizerProvider  GpuProvider     AuraProvider           │
└────────────────────────────────────────────────────────────────────┘
```

The wallpaper host (`rigstats-wallpaper`) never talks to the control pipe; only
the main app does.

---

## Service: `rigstats-service`

The current `sensor-sidecar` project grows into the service. The executable and
Windows service name stay `rigstats-sensor` until a rename is worth the
installer migration cost; "rigstats-service" is used here as the concept name.

### HardwareHost

- Owns the one `LibreHardwareMonitor.Hardware.Computer` instance already opened
  by `SensorWorker`.
- Exposes a single `SemaphoreSlim` hardware lock. The telemetry tick and every
  provider write take the same lock, so a fan write never interleaves with a
  Super I/O read.
- Providers receive `IHardwareHost`, never a raw `Computer`, which keeps them
  testable.

### IControlProvider

```csharp
public interface IControlProvider
{
    string Domain { get; }                          // "fan", "cpu_limit", ...
    CapabilitySet Probe();                          // once at start-up
    ValidationResult Validate(ProfilePart part);    // clamp to hard limits
    Snapshot Capture();                             // current hardware state
    void Apply(ProfilePart part);
    bool Verify(ProfilePart part);                  // read back registers
    void Restore(Snapshot snapshot);
    void ReleaseToFirmware();                       // hand control back to BIOS
}
```

### ControlBroker

Every profile change is a **transaction**:

1. `Validate` every part (reject or clamp against probed hard limits).
2. `Capture` a snapshot from every affected provider.
3. `Apply` in a fixed order: power plan → CPU limits → Curve Optimizer → GPU →
   fans → Aura.
4. `Verify` by reading back.
5. On any failure, `Restore` **all** affected providers in reverse order and
   return **one** consolidated result, e.g. *"Profile Gaming not applied: CPU
   power limit is locked by BIOS."*

Transactions are serialised — there is never more than one in flight.

---

## Control pipe protocol

- Name: `\\.\pipe\rigstats-control`, duplex, one client at a time.
- Framing: newline-delimited JSON, like the telemetry pipe.
- Request/response correlated by `id`; server pushes unsolicited `event`
  messages.

```json
{"id":1,"method":"hello","params":{"protocol":1,"app_version":"3.0.0"}}
{"id":1,"result":{"protocol":1,"service_version":"3.0.0"}}
```

| Method | Purpose |
| --- | --- |
| `hello` | Protocol version handshake. On mismatch the UI disables control and says why. |
| `capabilities` | Per-domain capability sets (headers, ranges, locked flags). |
| `get_state` | Active profile, per-domain applied state, last result. |
| `list_profiles` / `save_profile` / `delete_profile` | Profile CRUD (the store is service-owned). |
| `apply_profile` | Transactional apply (see ControlBroker). |
| `preview` | Apply temporarily; auto-revert after N seconds unless `confirm` arrives (display-mode-change pattern). |
| `confirm` | Keep a previewed change. |
| `release_to_firmware` | Panic button: every provider → `ReleaseToFirmware()`. |
| `identify_fan` | Spin one header to 100 % for 3 s so the user can see/hear which fan it is. |
| `subscribe` | Start event stream: `profile_changed`, `apply_result`, `safety_tripped`, `fan_duty`. |

---

## Profile model

Stored by the service in `%ProgramData%\se.codeby.rigstats\profiles.json`
(ACL: SYSTEM + Administrators write, Users read). The UI edits profiles only
through the pipe.

```json
{
  "active": "gaming",
  "profiles": [{
    "id": "gaming", "name": "Gaming", "icon": "bolt", "builtin": true,
    "fan": {
      "headers": {
        "lpc/nct6799d/control/1": { "label": "CPU cooler", "source": "cpu_package",
          "curve": [[40,30],[70,65],[85,100]], "hysteresis_c": 3 }
      }
    },
    "cpu_limit": { "intel": { "pl1_w": 180, "pl2_w": 253 },
                   "amd":   { "ppt_w": 142, "tdc_a": 95, "edc_a": 140 } },
    "curve_opt": { "all_core": -15 },
    "gpu":       { "power_limit_pct": 100 },
    "aura":      { "effect": "static", "color": "#ff0033", "brightness": 0.8 },
    "power_plan": "high_performance"
  }]
}
```

- Every part is optional; a missing part leaves that domain untouched.
- Built-in profiles: **Silent, Balanced, Gaming, Eco** — editable, resettable.
- Fan headers are keyed by LHM control identifier. Users give them labels
  (LHM cannot tell which header is the CPU cooler — see *CPU fan speed* in
  ROADMAP); `identify_fan` makes labelling easy.
- The service re-applies the active profile at start-up (subject to the
  boot-crash guard), so profiles work before anyone logs in.
- The Rust side mirrors the schema in `rigstats-backend/src/control.rs` with
  serde.

---

## Safety model

| Guard | Behaviour |
| --- | --- |
| Service-side fan loop | Curves are evaluated in the service at ~1 Hz. The UI only edits them. |
| Critical temperature override | CPU or GPU above a hard threshold → all controlled fans to 100 % regardless of profile, event `safety_tripped`. |
| Sensor loss | If a curve's source sensor disappears or goes stale, that header goes to 100 %. |
| Release on stop | `StopAsync` and a top-level `finally` call `ReleaseToFirmware()` on all providers. |
| Boot-crash guard | Before applying Curve Optimizer / CPU limits a `pending-apply` marker is written; it is cleared after 3 min of stable uptime. If the marker exists at service start, the risky parts are **not** re-applied and the UI shows *"Last undervolt caused a crash — reverted."* |
| Hard limits | The service always clamps against probed limits; UI values are never trusted. |
| Preview | Risky changes default to `preview` with auto-revert. |
| Armoury Crate / ASUS services | Detected at start-up. Conflicting domains are disabled until the user chooses to stop them (guided, reversible). |
| Dry-run | `--dry-run` (or a ProgramData flag) logs every write instead of performing it. |

---

## App integration (Rust / egui)

- **`rigstats-backend/src/control.rs`** — pipe client, protocol types, profile
  structs. Runs on its own tokio task like `poll_loop`; the UI thread never
  blocks on the pipe.
- **`ControlState`** in `dashboard::DashboardRuntime` — capabilities, active
  profile, last result, live fan duty. Updated by draining an mpsc channel in
  the same place telemetry is drained.
- **Commands** from UI → `mpsc::Sender<ControlCmd>` → control task → pipe.
- **Settings split:** UI preferences (hotkey on/off, show profile chip) stay in
  `rigstats-settings.json`; hardware profiles live in the service store.
- **Diagnostics ZIP** includes `capabilities`, the profile store and the
  service control log.

---

## UX integration

The Control Center must read as part of RIGStats, not an add-on.

| Surface | Integration |
| --- | --- |
| Tray | Profiles listed directly in the menu with a checkmark; icons via `menu_icons.rs`. |
| Hotkey | `hotkey.rs` gets a profile-cycle hotkey with an overlay-style toast. |
| Header panel | Active-profile chip (`● GAMING`); click opens Control Center. |
| Existing panels | Control *next to* the value it affects: fan RPM in the Motherboard panel opens its curve; CPU panel shows `PL1 180 W`; GPU panel shows the power limit. |
| Control Center (`windows/control.rs`) | One window: profiles on the left, tabs Fans / CPU / GPU / Lighting / Power on the right. Follows the dialog contract in `src-egui/src/windows/CLAUDE.md` and `AppTheme`. Only tabs with capabilities are shown. |
| Status window | Service capabilities, conflicts and last apply result. |
| Session recording | Active profile is logged per row, so History can compare Silent vs Gaming in one chart. |
| Errors | One consolidated message per transaction, same notification style as temperature alerts. |

---

## Providers

| Provider | Mechanism | Notes / risk |
| --- | --- | --- |
| `PowerPlanProvider` | `PowerGetActiveScheme` / `PowerSetActiveScheme` (powrprof) | Trivial, no driver. Phase 0 reference provider. |
| `FanProvider` | LHM `ISensor.Control.SetSoftware()` / `SetDefault()` on the shared `Computer` | Depends on Super I/O support per board; some firmware overrides writes — detect via `Verify`. |
| `CpuLimitProvider` (Intel) | MSR `0x610` (`PKG_POWER_LIMIT`) via PawnIO IntelMSR module, units from `0x606` | Honour lock bit 63 → report "locked by BIOS". Many boards also enforce the MCHBAR MMIO mirror; the effective limit is the lower of the two. |
| `CpuLimitProvider` (AMD) | SMU mailbox (PPT/TDC/EDC) via PawnIO AMD SMU module | Command IDs are per CPU generation; unsupported generations are simply not advertised. |
| `CurveOptimizerProvider` | SMU mailbox (per-core / all-core offset) | Highest risk. Boot-crash guard + preview mandatory. |
| `GpuProvider` | NVIDIA: NVML `nvmlDeviceSetPowerManagementLimit` (ships with driver). AMD: ADLX tuning services. | Official SDKs only in v1 — no undocumented clock offsets. |
| `AuraProvider` | USB HID to the ASUS Aura controller on ROG boards | Implemented in-service; must detect and yield to Armoury Crate / LightingService. |

---

## Security

The telemetry pipe is read-only for `BUILTIN\Users`. The control pipe is a
write path into a LocalSystem service and must not become a privilege
escalation vector:

- Pipe ACL: SYSTEM full control; **interactive user** read/write only.
- Client verification: `GetNamedPipeClientProcessId` → image path must be the
  installed `rigstats.exe`, Authenticode signature must match the service's
  own signer (skipped in debug builds only).
- Strict method allowlist and schema validation; unknown fields rejected.
- All numeric values clamped against probed limits in the service.
- Profile store writable only by the service.

---

## Testing strategy

- **Providers**: xUnit + NSubstitute in `sensor-sidecar.Tests/`, driven by
  `IHardwareHost` fakes. Register/sensor dumps per board become fixtures (same
  idea as `/sensor-fixture`), giving `Probe()` regression tests.
- **Broker**: a provider that fails mid-transaction must trigger full rollback.
- **Validation**: clamping and edge-case unit tests per domain.
- **Protocol**: round-trip tests on both sides (C# and Rust serde) from shared
  JSON fixtures.
- **Dry-run**: the whole flow can be exercised on any machine without writes.
- All existing gates (`cargo xtask verify`) stay mandatory.

---

## Licensing

RIGStats is MIT. LHM (MPL-2.0 library, consumed as a NuGet package) and PawnIO
are already in use. **ZenStates-Core and OpenRGB are GPL** — they may be used
as protocol documentation, but no code is copied; providers are written from
scratch. NVML and ADLX are vendor SDKs shipped with their drivers.

---

## Delivery phases

Each phase ships on its own and is useful on its own. Phases 1–6 depend on
phase 0.

| Phase | Issue | Scope | Risk | roadmap-id |
| --- | --- | --- | --- | --- |
| 0 | [#187](https://github.com/dvalfrid/rigstats/issues/187) | Control foundation: pipe, security, broker, capabilities, profile store, tray/hotkey/chip, Control Center shell, `PowerPlanProvider` | Very low | `control-foundation` |
| 1 | [#188](https://github.com/dvalfrid/rigstats/issues/188) | Fan control | Low–medium | `control-fans` |
| 2 | [#189](https://github.com/dvalfrid/rigstats/issues/189) | CPU power limits (Intel + AMD) | Medium | `control-cpu-limits` |
| 3 | [#190](https://github.com/dvalfrid/rigstats/issues/190) | GPU power profiles | Low–medium | `control-gpu` |
| 4 | [#191](https://github.com/dvalfrid/rigstats/issues/191) | AMD Curve Optimizer | High | `control-curve-optimizer` |
| 5 | [#192](https://github.com/dvalfrid/rigstats/issues/192) | ASUS Aura RGB | Medium | `control-aura` |
| 6 | [#193](https://github.com/dvalfrid/rigstats/issues/193) | Armoury Crate replacement (coexistence, guided removal, validated boards) | Low | `control-armoury-crate` |

---

## Open questions

- Which PawnIO modules (IntelMSR, AMD SMU generations) are available and signed
  for our target CPUs? Needs verification before phase 2/4.
- Which ROG boards expose writable fan control through LHM, and which firmware
  overrides it? Collect via diagnostics exports.
- Aura controller USB IDs and protocol variants across ROG board generations.
- Service rename (`rigstats-sensor` → `rigstats-service`): worth the installer
  migration, or keep the name?
