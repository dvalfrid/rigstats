# Control Center — Hardware Control Architecture

> Status: phases 0 (foundation, #187), 1 (fan control, #188), 2 (CPU power
> limits, AMD part, #189) and 3 (GPU power limit, AMD part, #190) are
> implemented;
> the rest is design / planned (Milestone 3.0). Tracked per phase in GitHub
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
- [Phase 1 — fan control as built](#phase-1--fan-control-as-built)
- [Phase 2 — CPU power limits as built](#phase-2--cpu-power-limits-as-built)
- [Phase 3 — GPU power limit as built](#phase-3--gpu-power-limit-as-built)
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
- Exposes a single `SemaphoreSlim` hardware lock. The telemetry sample and every
  provider write take the same lock, so a fan write never interleaves with a
  Super I/O read.
- **Current telemetry model (since #196), which HardwareHost must absorb:**
  - There is no fixed telemetry tick. The telemetry pipe accepts several clients
    at once (`MaxClients = 4`, one task per client). Each client asks
    `SensorWorker.GetFreshLine()` once per second.
  - That method re-samples LHM (`Computer.Accept` + `SensorReader.Extract`) only
    when the cached JSON line is older than 900 ms, so LHM is still read at
    most ~1 Hz no matter how many clients are connected. It is idle when no
    client is connected.
  - The sample is guarded by a plain C# `lock (_sampleLock)`. That lock
    becomes the HardwareHost lock. It has to change to the `SemaphoreSlim`
    above, because a `lock`/`Monitor` can't be held across `await`, and
    provider writes are async.
  - The telemetry wire format must stay byte-for-byte unchanged. Only the
    locking moves.
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
| `get_state` | Active profile, dry-run flag, `crash_guard_notice` (set when the boot-crash guard skipped risky parts this boot). |
| `list_profiles` / `save_profile` / `delete_profile` | Profile CRUD (the store is service-owned). |
| `apply_profile` | Transactional apply (see ControlBroker). |
| `preview` | `{"profile": {...}, "seconds": 15}` — apply an edited, unsaved profile; the service reverts it after N seconds (default 15, max 60) unless `confirm` arrives (display-mode-change pattern). A new `apply_profile`/`preview` reverts a pending one first. |
| `confirm` | `{"keep": true}` stores the previewed profile and makes it active; `{"keep": false}` reverts now. |
| `release_to_firmware` | Panic button: every provider → `ReleaseToFirmware()`. |
| `identify_fan` | Spin one header to 100 % for 5 s so the user can see/hear which fan it is; the service measures which RPM sensors respond and reports them (`fan_identified`). |
| `subscribe` | Start event stream: `profile_changed`, `apply_result`, `safety_tripped`, `fan_duty`, `fan_identified`, `preview_reverted`. |

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
| Boot-crash guard | Before applying Curve Optimizer / CPU limits a `pending-apply` marker is written; it is cleared after 3 min of stable uptime. If the marker exists at service start, the risky parts are **not** re-applied (they stay at BIOS values) and the Control Center explains why until a profile is applied again. A service stop inside the window keeps the marker. |
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
| `CpuLimitProvider` (Intel) | MSR `0x610` (`PKG_POWER_LIMIT`) via PawnIO IntelMSR module, units from `0x606` | Honour lock bit 63 → report "locked by BIOS". Many boards also enforce the MCHBAR MMIO mirror; the effective limit is the lower of the two. Not built yet (#209): LHM's bundled IntelMSR module is read-only. |
| `CpuLimitProvider` (AMD) | RSMU mailbox (PPT/TDC/EDC) via LHM's signed `RyzenSMU` PawnIO module; readback from the PM table | Command IDs are per CPU generation and PM table layouts per table version; only combinations verified on hardware are advertised. See [Phase 2](#phase-2--cpu-power-limits-as-built). |
| `CurveOptimizerProvider` | SMU mailbox (per-core / all-core offset) | Highest risk. Boot-crash guard + preview mandatory. |
| `GpuPowerProvider` | AMD: ADLX manual power tuning (`amdadlx64.dll`, ships with Adrenalin). NVIDIA: NVML `nvmlDeviceSetPowerManagementLimit` (ships with driver) — not built yet (#210). | Official SDKs only in v1 — no undocumented clock offsets. See [Phase 3](#phase-3--gpu-power-limit-as-built). |
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
| 2 | [#189](https://github.com/dvalfrid/rigstats/issues/189) | CPU power limits (AMD PPT/TDC/EDC) | Medium | `control-cpu-limits` |
| 2b | [#209](https://github.com/dvalfrid/rigstats/issues/209) | CPU power limits (Intel PL1/PL2) | Medium | `control-cpu-limits-intel` |
| 3 | [#190](https://github.com/dvalfrid/rigstats/issues/190) | GPU power limit (AMD, ADLX) | Low–medium | `control-gpu` |
| 3b | [#210](https://github.com/dvalfrid/rigstats/issues/210) | GPU power limit (NVIDIA, NVML) | Low–medium | `control-gpu-nvidia` |
| 4 | [#191](https://github.com/dvalfrid/rigstats/issues/191) | AMD Curve Optimizer | High | `control-curve-optimizer` |
| 5 | [#192](https://github.com/dvalfrid/rigstats/issues/192) | ASUS Aura RGB | Medium | `control-aura` |
| 6 | [#193](https://github.com/dvalfrid/rigstats/issues/193) | Armoury Crate replacement (coexistence, guided removal, validated boards) | Low | `control-armoury-crate` |

---

## Phase 1 — fan control as built

What phase 1 (#188) shipped, and what testing on real hardware changed in the
design above:

- **Headers** are LHM `Control` sensors under `/lpc/` (Super I/O); GPU fan
  controls are excluded. A board with none reports the `fan` domain as
  unsupported, so the Fans tab is hidden.
- **Curve loop** (`FanCurveLoop`) reads the same cached sample as the
  telemetry pipe (`IHardwareHost.GetSampleAsync`), so it keeps sampling with
  no client connected. Sources: `cpu_package`, `gpu` (hottest GPU, so an idle
  iGPU can't mask a hot dGPU) and `mb:<label>`. Hysteresis only damps falling
  temperatures. Critical thresholds are fixed: CPU 95 °C, GPU 90 °C.
- **Every write** goes through one clamped, locked `FanProvider.ApplyDuty`
  that never touches a released header; `Verify` re-reads the PWM register
  (±5 %), so firmware that overrides writes fails the transaction.
- **Start-up**: the service re-applies the active profile
  (`ActiveProfileApplier`). Built-in profiles carry an explicit empty fan
  part (BIOS control) so switching to them releases earlier curves.
- **Channels ≠ fans.** On an ASUS PRIME B650M-A (NCT6799D) one control
  channel drives two RPM sensors and channel numbers don't match RPM sensor
  numbers. So `identify_fan` measures which RPM sensors rise during the spin;
  the result is persisted in `%ProgramData%\se.codeby.rigstats\fan-channels.json`,
  reported as `drives` per header in the capability set, and used for the
  Motherboard panel's "click a fan → its curve". Nothing is ever matched by
  name.
- **Shared curves** are a UI concept only: the profile still stores one curve
  per header, and headers with an identical curve + source + hysteresis are
  shown and edited as one group ("Same curve on").
- **Diagnostics** (#207): profile applies/rollbacks, verify failures and
  identify results are logged to `rigstats-sensor.log`; with
  `control-capabilities.json` and `sensor-tree.txt` they are in every
  diagnostics ZIP. Every sensor-tree fixture also runs `FanProvider.Probe()`.

## Phase 2 — CPU power limits as built

What phase 2 (#189) shipped — AMD only; Intel is #209:

- **Module.** No new driver code or binary: `PawnIoModule` opens
  `\\.\PawnIO` directly and loads LHM's own signed `RyzenSMU.bin`, which
  exports `ioctl_send_smu_command` (RSMU mailbox) and the PM table reads.
  Every SMU access holds the `Global\Access_PCI` mutex, shared with LHM.
  LHM's bundled `IntelMSR.bin` only exports `ioctl_read_msr`, hence the
  Intel split.
- **Two keys must both be known** (`AmdSmuMap`): the CPU generation
  (CPUID family/model/package) for the command ids — Zen 2/3 use
  `0x53/0x54/0x55`, Zen 4/5 `0x56/0x57/0x58`, and Zen 4's SetPPT id is Zen
  2's SetTctlMax — and the PM table version for where the limits read back.
  Layouts move between versions (LHM has TDC at index 3 on Vermeer, 48 on
  Raphael), so only hardware-verified versions are listed: today
  `0x620105` (Ryzen 7 9800X3D, SMU 98.78.0): PPT limit [2], TDC limit [8],
  EDC limit [63]. Desktop Ryzen only; mobile, Threadripper and server parts
  are not offered.
- **Limits only go down.** The ceiling is the BIOS value at boot (SMU
  limits reset every reboot), read before the first write and persisted in
  `%ProgramData%\se.codeby.rigstats\cpu-limit-baseline.json` keyed by boot
  time, so a service restart in the same boot doesn't take an applied limit
  for the BIOS one. Floor: 45 W / 30 A / 45 A. A `null` value — and the
  built-ins' empty `cpu_limit: {}` — means the BIOS value. Raising limits
  above the BIOS (PBO territory) is out of scope.
- **Verify** re-reads the PM table (±1 W/A). On the 9800X3D,
  TransferTableToDram is sometimes rejected with 0xFD (prerequisite) after
  LHM opens its own module instance; `RyzenSmu` resolves the table again and
  retries.
- **Unsupported is not an error.** On a CPU without a verified layout the
  capability is unsupported (CPU tab hidden) and a stored `cpu_limit` is
  skipped with a log line, so a BIOS update that changes the table version
  never blocks the rest of a profile. Release-to-firmware only writes if this
  service changed a limit.
- **Preview.** The CPU tab always goes through `preview` (15 s); `confirm`
  saves and activates the profile. The broker keeps the transaction's
  snapshots and restores them on timeout, on `confirm {keep:false}`, or
  before any other apply.
- **Boot-crash guard** (`BootCrashGuard`): armed by `CpuLimitProvider` only
  when it writes a non-BIOS value; the marker is consumed at start and
  `ActiveProfileApplier` then drops `cpu_limit` (and later `curve_opt`).

## Phase 3 — GPU power limit as built

What phase 3 (#190) shipped — AMD only; NVIDIA (NVML) is #210:

- **ADLX from C#, no wrapper.** `AdlxGpuPower` loads `amdadlx64.dll` (part
  of the Adrenalin driver) and calls the SDK's C interface — plain vtables —
  through delegates with the slot numbers from the ADLX headers; no native
  helper DLL, no `unsafe`. It initializes with the runtime's own version
  (`ADLXQueryFullVersion`). Works from session 0 as SYSTEM, checked on an RX
  9070 XT with ADLX runtime 1.5. Every call takes one pass over the GPU list
  and releases what it acquired — **before** `ADLXTerminate` on failure: a
  release after terminate is an access violation that .NET cannot catch and
  would take the whole service down.
- **The knob** is Adrenalin's Power Limit: a % offset from the driver default
  within the driver's own range (−30 … +10 % on the 9070 XT; other cards
  differ, the range is always read from the driver). The full range is
  offered — it is an official, driver-enforced setting. Adapters are keyed by
  PNP device instance path; only adapters where `IsSupportedManualPowerTuning`
  says yes are listed (an iGPU says no), and a profile entry for a GPU that is
  gone is skipped, not rejected.
- **Original, not default.** A profile without a value means the value in
  force before RIGStats first changed that adapter (the user's own Adrenalin
  setting), recorded at the first write together with whether all tuning was
  at factory settings, in
  `%ProgramData%\se.codeby.rigstats\gpu-power-original.json`. Release and
  service stop restore it and clear the record; it survives a crash.
- **Adrenalin's Default/Custom.** Any power-limit write switches Adrenalin's
  tuning to "Custom", and setting the value back doesn't switch it back. So
  returning to an original that was at factory settings is an ADLX
  `ResetToFactory` (Adrenalin shows "Default" again). A user's own Custom
  tuning is never factory-reset — only its value is restored.
- **Same flow as CPU limits:** GPU tab edits go through `preview` (15 s,
  Keep/Undo); verify reads the limit back; the GPU panel shows the active
  profile's limit under POWER (`PL -15 %`), matched to the displayed GPU by
  name. Not a boot-crash-guarded domain: the value is inside the driver's
  own range.

## Open questions

- PM table layouts for Ryzen generations other than Granite Ridge `0x620105`
  (Matisse, Vermeer, Raphael have command ids but no verified layout).
  Unverified CPUs log their table version and head to `rigstats-sensor.log` —
  collect diagnostics exports and add them to `AmdSmuMap.Layout`.
- Curve Optimizer (phase 4) uses the same `RyzenSMU` module's
  `send_smu_command`; its command ids need the same per-generation care.
- ADLX power tuning is verified on one card (RX 9070 XT, ADLX 1.5). Which
  other Radeon generations, Pro drivers and AMD laptops report it, and do
  any misbehave? A native crash inside ADLX would take the service down —
  if that ever shows up in the field, move ADLX into a small helper process.
- Does an ADLX power limit persist across a reboot (Adrenalin re-applying
  "Custom")? Harmless either way — the original is persisted and the active
  profile is re-applied at start — but worth knowing.
- Which ROG boards expose writable fan control through LHM, and which firmware
  overrides it? Diagnostics exports now record it (verify failures and the
  measured channel→fan mapping, #207) — collect and turn them into fixtures.
- Aura controller USB IDs and protocol variants across ROG board generations.
- Service rename (`rigstats-sensor` → `rigstats-service`): worth the installer
  migration, or keep the name?
