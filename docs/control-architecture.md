# Control Center — Hardware Control Architecture

> Status: phases 0 (foundation, #187), 1 (fan control, #188), 2 (CPU power
> limits, AMD part, #189), 3 (GPU power limit, AMD #190, NVIDIA desktop
> #210), 4 (Curve
> Optimizer, #191), 5 (ASUS Aura lighting, #192) and 6 (Armoury Crate
> coexistence, #193) are implemented; the rest is design / planned. Tracked
> per phase in GitHub Issues — see [Delivery phases](#delivery-phases).

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
- [Testing strategy](#testing-strategy) — incl. live testing on real hardware
  and finding a lighting protocol
- [Licensing](#licensing)
- [Delivery phases](#delivery-phases)
- [Phase 1 — fan control as built](#phase-1--fan-control-as-built)
- [Phase 2 — CPU power limits as built](#phase-2--cpu-power-limits-as-built)
- [Phase 3 — GPU power limit as built](#phase-3--gpu-power-limit-as-built)
- [Phase 4 — Curve Optimizer as built](#phase-4--curve-optimizer-as-built)
- [Phase 5 — ASUS Aura lighting as built](#phase-5--asus-aura-lighting-as-built)
- [Phase 6 — Armoury Crate coexistence as built](#phase-6--armoury-crate-coexistence-as-built)
- [Peripherals: battery and settings (research, #290–#292)](#peripherals-battery-and-settings-research-290292)
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
│ rigstats-backend:  lhm.rs (telemetry)    control.rs (control)     │
└────────────┬──────────────────────────────────┬───────────────────┘
  \\.\pipe\rigstats-sensors (read-only)   \\.\pipe\rigstats-control (duplex)
┌────────────┴──────────────────────────────────┴───────────────────┐
│ rigstats-sensor.exe ("rigstats-service", LocalSystem, .NET 10)    │
│  HardwareHost   — shared ≤1 Hz sample + single hardware lock      │
│  ControlBroker  — validate → snapshot → apply → verify → commit   │
│  SafetyGuard    — release to firmware on stop (logged, #385)      │
│  BootCrashGuard — a marker before every risky CPU/CO write        │
│  FanCurveLoop   — curves at ~1 Hz, critical-temperature override  │
│  FanWatchdog    — restarts the hardware worker if the loop hangs; │
│                   the service only if that doesn't help           │
│  ProfileStore   — %ProgramData%\se.codeby.rigstats\profiles.json  │
│  Providers (IControlProvider):                                    │
│    PowerPlanProvider   FanProvider         CpuLimitProvider       │
│    CurveOptimizerProvider  GpuPowerProvider  LightingProvider     │
├───────────────────────────────────────────────────────────────────┤
│  --hardware-worker child: LibreHardwareMonitor (sensors, fan      │
│    writes); restarted after a crash or hang, #374                 │
│  --gpu-worker child: ADLX / NVML for the GPU power limit, #370    │
└───────────────────────────────────────────────────────────────────┘
```

The wallpaper host (`rigstats-wallpaper`) never talks to the control pipe; only
the main app does.

---

## Service: `rigstats-service`

The current `sensor-sidecar` project grows into the service. The executable and
Windows service name stay `rigstats-sensor` until a rename is worth the
installer migration cost; "rigstats-service" is used here as the concept name.

### HardwareHost

- **LibreHardwareMonitor runs in the hardware worker** (#374):
  `rigstats-sensor.exe --hardware-worker`, a child of the service on the
  shared worker plumbing (`Control/Workers/`, see the GPU worker). It owns
  the one LHM `Computer` and the vendor libraries LHM loads for the GPU
  sensors (NVAPI, NVML, ADL). A native fault there — #338, NVML when a
  laptop switches its NVIDIA GPU off — ends the worker, not the service.
  Fan writes go through LHM's `IControl`, so they run in the worker too.
- **The boundary is `ISensorHardware`**: `Sample`, `SensorTree`,
  `FanHeaders`, `SetFan`, `SetFanDefault`, `ReadFan` — one JSON line each
  way per call (`HardwareWorkerProtocol`). `LhmHardware` implements it on an
  `IComputer` (in the worker, and in tests over fake trees);
  `HardwareWorkerClient` implements it over the worker.
- **`HardwareWorkerClient` restarts the worker.** Every call has a time
  limit (8 s; the first call of a new worker, which opens the hardware,
  60 s). A crash, a hang or a garbled answer ends the worker and is logged
  with its exit code. The first restart is at once, then after 1, 2, 5, 15
  and 60 s while it keeps failing; a worker that ran for a minute starts
  the count over. During a pause, calls fail at once
  (`HardwareWorkerDownException`).
- **`HardwareHost`** keeps one `SemaphoreSlim` for every call, so a fan
  write never interleaves with a Super I/O read. It caches one sample per
  900 ms for every telemetry client and `FanCurveLoop` (#196, #208). When
  no fresh sample can be had, the telemetry line is the last one with
  `sample_age_ms`, and the app takes one older than 5 s as no data. The
  fan loop gets no sample at all and runs every curve header at full speed
  (sensor loss).
- Providers receive `IHardwareHost`, never LHM types, which keeps them
  testable.
- Debug builds take `--hardware-worker-crash-after <s>` /
  `--hardware-worker-hang-after <s>`: the worker crashes or hangs on its
  first sample after that many seconds, to prove the service survives and
  restarts it; `--hardware-worker-crash-reading <id prefix>` crashes it
  while it reads that hardware, to see it left out. Each takes a value, so
  run the script from a PowerShell prompt, not through `pwsh -File` (which
  passes `'a','b'` as one string): `.\tools\dev-sidecar.ps1 -Live
  -SidecarArgs '--hardware-worker-crash-reading','/gpu-amd/0'`.
- **Hardware that keeps crashing the worker is left out** (#374, #338).
  Before reading each top-level hardware item the worker writes a progress
  line (`{"updating":"/gpu-nvidia/0","name":"…"}`, ahead of its answer), so
  when it dies the client knows which hardware it was reading. After
  `SkipAfter` (3) crashes or hangs in the same hardware, every new worker is
  started with `--skip <id>,…`: that hardware is neither read nor reported
  (`LhmHardware`), the sensor tree notes it, and the rest is read as
  before. It stays out until Windows restarts (or the service restarts —
  then it takes 3 crashes again; nothing is stored). The telemetry line
  carries a `Problem` per hardware left out (`hardware_problems`,
  `HardwareProblems.Off`: the vendor's driver download, "Collect
  diagnostics…", the troubleshooting page), shown in the app's Status under
  Problems. Pinned by `contract/telemetry-hardware-problem.json` on both
  sides. A crash while nothing was being read (opening the hardware) is
  not put on any hardware: the restarts just get slower.

### Start-up

The service manager gives a starting service 30 s to connect, and it connects
inside `host.Run()` (`WindowsServiceLifetime.WaitForStartAsync`) — before the
host builds any hosted service or provider (verified with a scratch host on
Microsoft.Extensions.Hosting 10.0.12, #383). So `Program.cs` only registers:
nothing is resolved before `Run()`, and slow work runs after it — the
hardware opens and the lighting devices are discovered as the hosted
services start, and the event log is read for earlier crashes in the
background (`BackgroundStartupTask`). Only the data folder's ACL
(`DataDirectory.EnsureSecure`, which decides dry-run) comes first. The
top-level `finally` reaches `SafetyGuard` through the instance the host
built, not by resolving it early.

### IControlProvider

```csharp
public interface IControlProvider
{
    string Domain { get; }                          // "fan", "cpu_limit", ...
    CapabilitySet Probe();                          // on every `capabilities` request
    ValidationResult Validate(ProfilePart part);    // reject what can't be applied
    Snapshot Capture();                             // current hardware state
    void Apply(ProfilePart part);
    bool Verify(ProfilePart part);                  // read back registers
    void Restore(Snapshot snapshot);
    void ReleaseToFirmware();                       // hand control back to BIOS
}
```

### ControlBroker

Every profile change is a **transaction**:

1. `Validate` every part (reject what can't be applied; values are clamped to
   the probed hard limits when they are written).
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
| `list_profiles` / `save_profile` / `delete_profile` | Profile CRUD (the store is service-owned). Saving keeps a profile's place in the list; deleting the active profile makes Balanced (else the first) active and applies it. |
| `reset_profile` | A built-in profile back to its defaults; re-applied when it is the active profile. |
| `aura_preview` | `{"aura": {...}}` — sets the lights at once without saving (the Lighting tab's live preview). Harmless, so outside the broker transaction. |
| `hue_discover` / `hue_pair` / `hue_refresh` / `hue_choose` / `hue_unpair` | Philips Hue (#215): find bridges (mDNS, or `{"ip": ...}`), pair `{"ip": ...}` after the link button (`link_button` error until pressed), re-read rooms/zones, choose `{"groups": [...]}` which follow the rig, forget the bridge. Not part of any profile; each change rediscovers the lighting devices. |
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

The app's own part of a profile (accent colour, panels, overlay content, alert
thresholds — #305) is not part of this model: it is per Windows user, in the
app's settings file (`Settings.profile_looks`), keyed by profile id, and
switched by the app when the active profile changes. See `settings.rs` in
`docs/architecture.md`; the Control Center shows both halves side by side.

Each save first copies the file it replaces to `profiles.json.bak`. A file
that can't be read (a downgrade, a disk error, a manual edit) is moved to
`profiles.json.corrupt`, and the profiles come from the `.bak`, else the
built-ins — saved right away, logged, and announced by `get_state`'s
`profiles_notice` as a banner in the Control Center until the next save
(#221).

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
- `power_plan` is an intent, not a requirement: which plans exist is up to the
  PC (Modern Standby laptops often have Balanced only). A missing plan falls
  back to the closest existing one (`high_performance` ↔ `ultimate_performance`,
  then `balanced`; anything else → `balanced`), and is left untouched when not
  even Balanced exists — it never fails a profile.
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
| Release on stop | `StopAsync` and a top-level `finally` call `ReleaseToFirmware()` on all providers. What was handed back is logged in one line with why (service stop, the Control Center's release, the fan watchdog), e.g. `Handed back to the firmware because the service stops: fan: 2 headers to the BIOS (Fan #1, Fan #2); cpu_limit: the BIOS limits (…)` — nothing when there was nothing to hand back; a provider that fails is logged and the rest still get their turn (#385). |
| Service hung or killed | `FanWatchdog`, on its own thread: while a curve is active and the fan loop hasn't completed a tick for 10 s (a hung LHM/PawnIO call, a deadlock, a starved thread pool), it first ends the hardware worker (#374) — a hung LHM or driver call lives there; the next call starts a new worker and the curves carry on, nothing to re-apply. Only after 2 such restarts within 2 minutes (the hang is in the service itself) does it hand the fans back to firmware (given 3 s — the release may hang on the same lock) and fail fast. The service manager restarts the service (the installer sets 5 s, 10 s and 30 s for the first three failures within a minute) and the active profile is re-applied. A gap in the watchdog's own checks (the PC slept) restarts its 10 s window instead. A service that dies hard is restarted the same way. Known limit: if the service can't start again, the fans keep their last duty until a reboot hands them to the BIOS. Off in dry-run. |
| Boot-crash guard | Before applying Curve Optimizer / CPU limits a `pending-apply` marker is written; if it can't be written, the risky write isn't made and the profile is rolled back (#390); it is cleared after 3 min of stable uptime. If the marker exists at service start, the risky parts are **not** re-applied: they are applied as BIOS values (a no-op after a reboot; when only the service went down, it puts the SMU back) and the Control Center shows a red "Reverted after a restart" notice until a profile is applied again. A service stop inside the window keeps the marker. |
| Hard limits | The service always clamps against probed limits; UI values are never trusted. |
| Preview | Risky changes default to `preview` with auto-revert. |
| Armoury Crate / other hardware software | Detected by process name (`OtherSoftware`) when capabilities are read and before a profile applies: a domain another program writes — Armoury Crate's LightingService or OpenRGB → lighting, Armoury Crate's AsusFanControlService → fans — is reported as taken with a `Problem` and skipped by applies. RIGStats yields, never fights, and never stops another vendor's service; the problem links to ASUS's own uninstall tool (reversible: reinstall). Which such programs run is logged once at start-up, for the diagnostics. See [Phase 6](#phase-6--armoury-crate-coexistence-as-built). |
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
| Status window | Whether the Control Center pipe is connected (and its protocol version). A failed profile apply is written to the debug log, which Status shows. |
| Session recording | Active profile is logged per row, so History can compare Silent vs Gaming in one chart. |
| Errors | One consolidated message per transaction, same notification style as temperature alerts. |

---

## Providers

| Provider | Mechanism | Notes / risk |
| --- | --- | --- |
| `PowerPlanProvider` | `PowerGetActiveScheme` / `PowerSetActiveScheme` (powrprof) | Trivial, no driver. Phase 0 reference provider. |
| `FanProvider` | LHM `ISensor.Control.SetSoftware()` / `SetDefault()` on the shared `Computer` | Depends on Super I/O support per board; some firmware overrides writes — detect via `Verify`. Yields to Armoury Crate's fan service (`OtherSoftware`, [Phase 6](#phase-6--armoury-crate-coexistence-as-built)). |
| `CpuLimitProvider` (Intel) | MSR `0x610` (`PKG_POWER_LIMIT`) via PawnIO IntelMSR module, units from `0x606` | Honour lock bit 63 → report "locked by BIOS". Many boards also enforce the MCHBAR MMIO mirror; the effective limit is the lower of the two. Not built yet (#209): LHM's bundled IntelMSR module is read-only. |
| `CpuLimitProvider` (AMD) | RSMU mailbox (PPT/TDC/EDC) via LHM's signed `RyzenSMU` PawnIO module; readback from the PM table | Command IDs are per CPU generation and PM table layouts per table version; only combinations verified on hardware are advertised. See [Phase 2](#phase-2--cpu-power-limits-as-built). |
| `CurveOptimizerProvider` | RSMU mailbox (per-core / all-core offset, readback per core) via the same `RyzenSMU` module | Highest risk. Boot-crash guard + preview mandatory. See [Phase 4](#phase-4--curve-optimizer-as-built). |
| `GpuPowerProvider` | AMD: ADLX manual power tuning (`amdadlx64.dll`, ships with Adrenalin). NVIDIA desktop GPUs: NVML `nvmlDeviceSetPowerManagementLimit` (ships with the driver, #210); laptop GPUs are firmware-owned (#234). | Official SDKs only in v1 — no undocumented clock offsets. See [Phase 3](#phase-3--gpu-power-limit-as-built). |
| `LightingProvider` | Aura Sync over `ILightingDevice`s: ASUS Aura USB motherboard controllers (`AuraController`), ASUS Aura monitors + light bar (`AsusMonitorDevice`), ASUS TUF-protocol keyboards incl. via the ROG Omni receiver (`AsusKeyboardDevice`), ASUS GearLink-protocol headsets (`AsusHeadsetDevice`), any HID LampArray / Dynamic Lighting device (`LampArrayDevice`), chosen Philips Hue rooms and zones (`HueRoomDevice`, over the bridge's local API) — Windows HID APIs | Implemented in-service; yields to Armoury Crate, OpenRGB and (per device) Windows Dynamic Lighting. See [Phase 5](#phase-5--asus-aura-lighting-as-built). |

---

## Security

The telemetry pipe is read-only for `BUILTIN\Users`. The control pipe is a
write path into a LocalSystem service and must not become a privilege
escalation vector:

- Pipe ACL: SYSTEM full control; **interactive user** read/write only.
- Client verification: `GetNamedPipeClientProcessId` → image path must be the
  installed `rigstats.exe`, Authenticode signature must match the service's
  own signer (skipped in debug builds only). The path is the real gate — only
  an administrator can put a file there; the signer comparison catches a
  mismatched install, it does not validate the signature.
  A refused client is just closed. The service logs a refusal when its reason
  changes or every 10 minutes, with the count in between (`RepeatLog`); the
  app treats a close before `hello` is answered as a refusal, backs off
  2 s → 60 s, and the Control Center says "Refused by the service" (#231).
- Pipe names can't be taken over: the service creates each name with
  `FirstPipeInstance` and always keeps one instance listening (the control
  pipe creates the next before serving the connected client). If another
  process got the name first, the service logs it and retries instead of
  joining that pipe. The app, in turn, only accepts a pipe served from
  session 0 (`pipe_server.rs`; release builds).
- Known limit: the gate is in practice "the logged-in user", not "only
  RIGStats" — a process running as that user can inject code into a running
  `rigstats.exe` and drive the pipe through it. What it can then do is bounded
  by the service-side clamps (CPU limits only lower, Curve Optimizer within its
  range behind the boot-crash guard, fan curves with the critical-temperature
  override intact); no request takes a file path.
- Strict method allowlist; requests are deserialized into typed parts. A
  parameter that is missing or of the wrong kind is answered `bad_request`
  (#378). A request line is at most 256 KB (`BoundedLineReader`, the same cap
  the app puts on a frame); a longer one ends the connection. An IP address
  from the client (`hue_discover`, `hue_pair`) must parse as one, and client
  text goes into the log without control characters (`Printable`).
  Unknown JSON fields are **ignored**, not rejected (System.Text.Json's
  default in `ControlJson.Options`) — the app and service ship together.
- Debug builds of the service accept any client (`PipeClientVerifier`), so
  while `dev-sidecar.ps1 -Live` runs, any process of the interactive user can
  drive the hardware through the pipe. Only on a developer's own machine.
- The Hue Bridge's certificate check returns each TLS handshake's own verdict
  (`HueBridge.Accept`); one client can run two handshakes at once, and a
  verdict read back from shared state could be the other one's (#378).
- All numeric values clamped against probed limits in the service.
- State writable only by the service: `DataDirectory.EnsureSecure` runs first
  thing at start-up and gives `%ProgramData%\se.codeby.rigstats` protected
  rules (SYSTEM and Administrators write, Users read — nothing inherited from
  `%ProgramData%`, which would let any user create files there), replaces a
  link put in the folder's place, and removes files an unprivileged account
  left in it. If the folder can't be secured the service runs in dry-run and
  writes no log file.

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

### Live testing on real hardware

Control features are checked on real hardware before they ship.

**On a new machine**, once: install a released RIGStats first — its installer
brings the PawnIO driver the sidecar needs and registers the
`rigstats-sensor` service the script swaps out — then clone, follow
[setup.md](setup.md) (Rust, .NET 10 SDK, `cargo xtask setup`) and build
the sidecar and the app. The first run of the dev sidecar writes
`%ProgramData%\se.codeby.rigstats\lighting-devices.json`, whose HID scan
shows every lighting-capable collection on the machine — the starting
point for a new device.

The loop:

1. **Elevated window (the developer):** `pwsh -File tools\dev-sidecar.ps1 -Live`.
   It stops the installed `rigstats-sensor` service, runs the debug sidecar in
   its place (it logs every discovery and write to the console), and on
   Ctrl+C hands everything back to the firmware and restarts the service.
   Without `-Live` it is a dry run.
2. **Rebuild the sidecar** only while the dev sidecar is stopped (Ctrl+C):
   it holds `rigstats-sensor.exe`, so `dotnet build` fails with MSB3027
   otherwise (the code still compiled — the copy of the exe failed).
3. **Rebuild and restart the app** (`target\debug\rigstats.exe`, see
   `CLAUDE.md`). Debug sidecar builds skip `PipeClientVerifier`, so the debug
   app can connect; quit an installed RIGStats first (single-instance guard).
4. Start the dev sidecar again, try the change, read its console output.

Notes:

- An agent's shell is usually not elevated: it builds, the developer runs
  the script and pastes the console output back.
- The control pipe is single-client while the app is open; live sensor
  values (fan RPM, temperatures) can be read from the telemetry pipe
  `\\.\pipe\rigstats-sensors`, which takes several clients.
- Dialogs are found by enumerating window titles containing e.g.
  "Control Center" (`FindWindow` with the em-dash title fails from
  PowerShell) — for screenshots of a change.
- Hardware writes during protocol work are done one at a time with the
  developer watching the device, reversible first (off → back on), and every
  value read back where the device allows.
- **Never send a command number that isn't documented for that device —
  not even under a "read"/"get" prefix, and never as a sweep.** A sweep of
  ASUS keyboard "get" sub-commands `12 01`–`12 3F` (GearLink only uses
  `00`–`08` and `12`–`16`) left a ROG Falchion Ace HFX triggering keys by
  itself and with scrambled per-key lighting, persistently, on any PC — no
  reset, GearLink, Armoury Crate Gear or firmware tool undid it; it went back
  under warranty. Undocumented sub-commands can be factory, test or
  calibration functions, and devices keep settings in flash. Only send
  commands taken from documentation for that device family (OpenRGB, a
  GearLink schema or bundle, a vendor DLL's disassembly), and ask the
  developer before anything else.

### Finding a lighting protocol

Native protocols only — other software is read as documentation, never
shipped or copied. What has worked, in order of cost:

1. **OpenRGB's source** for devices it supports.
   For ASUS mice, keyboards and headsets also **G-Helper**
   (`app/Peripherals/`, GPL — read only): per-model product ids, report ids,
   battery and settings commands; a second source to check GearLink against
   (see "Peripherals: battery and settings").
2. **Read-only probes** of the HID collections (`get` commands, config
   tables) — the diagnostics export's `lighting-devices.json` lists every
   HID collection with VID/PID and usage page, so a user's export shows where
   to look.
3. **ASUS GearLink** (gearlink.asus.com, a WebHID app): each device page
   `/view/<pid in decimal>` loads a bundle with the device's command schema,
   and its console logs every report sent (the ROG Delta II).
4. **The vendor's Windows app, unpacked, not installed** (the light bar's desk
   lamp): extract the MSI with `msiexec /a <msi> /qn TARGETDIR=<dir>`;
   decompile its .NET parts with `ilspycmd` (`dotnet tool install ilspycmd
   --tool-path <dir>`) to find the native DLL calls and their meaning; read
   the native DLL's exports and command bytes with a small disassembler (a
   scratch console project using the `Iced` NuGet package). DisplayWidget
   Center's `ScreenLightBarHid.dll` gave the lamp command this way.

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
| 4 | [#191](https://github.com/dvalfrid/rigstats/issues/191) | AMD Curve Optimizer (Granite Ridge) | High | `control-curve-optimizer` |
| 5 | [#192](https://github.com/dvalfrid/rigstats/issues/192) | ASUS Aura RGB (USB motherboard controllers) | Medium | `control-aura` |
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

What phase 3 (#190) shipped for AMD, and #210 for NVIDIA desktop GPUs:

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
- **When ADLX won't answer** (#241). An AMD iGPU installs ADLX without
  having power tuning (a Ryzen laptop with an NVIDIA dGPU), and ADLX can fail
  to initialize (driver not ready, a stale session). None of that may cost
  more than the GPU domain itself:
  - `GpuPowerProvider` implements `IAffectsPart`: a GPU part with no
    power-limit value only counts when RIGStats changed an adapter (the
    persisted originals), so the built-in profiles' empty GPU part never
    opens the driver — the broker skips the domain, no capture, no apply.
    A profile asking for an explicit value on a broken driver still fails,
    as a requested setting that can't be applied should.
  - AMD's ADLX server is restarted (#230) at most once per service process;
    after a failed `ADLXInitialize` it is not tried again for a minute.
  - **A profile skips a domain that is down** (#370): a provider throwing
    `DomainUnavailableException` (the GPU worker's `GpuDriverCrashedException`)
    in `Capture` or `Apply` is skipped by `ControlBroker`; the rest of the
    profile applies (`ApplyResult` ok, with "gpu skipped: …" as its message,
    logged). Before, a crashed driver rolled back the whole profile — fans,
    CPU and lighting too. Skipping is the safe direction: the card keeps its
    current limit. Any other failure still rolls everything back; a profile
    whose only affected domain is down is not applied.
  - **What the user sees** (#370): `Probe` turns a worker crash or hang
    into an error `Problem` and a driver error into an attention one
    (`GpuProblems`): what happened, that sensors, fans and lighting keep
    working, to install the latest driver and restart — with buttons for the
    AMD/NVIDIA driver page (from the installed driver files), "Collect
    diagnostics…" and the help page, a note when a power limit RIGStats set
    can't be put back, and the exit code and driver files as the detail. The
    Control Center keeps the Graphics card page and its Overview tile for a
    failed GPU domain (the tile shows the problem's title in its colour) and
    shows the problem there; a PC without a GPU driver keeps it hidden. The
    diagnostics ZIP adds `driver-updates.txt` (drivers Windows Update
    installed) and `gpu-worker.txt` (the crash marker).
  - **GPU worker process** (#370). The driver libraries (ADLX, NVML) run in
    a child of the service, `rigstats-sensor.exe --gpu-worker`
    (`GpuWorker.cs`): `GpuWorkerClient` is the provider's `IGpuPowerApi`
    and forwards each call as one JSON line on the child's stdin; the child
    (`GpuWorkerHost` → `GpuWorkerLoop`) answers on stdout with the real
    `GpuPowerApis`. A driver failure ("ADLX_FAIL") is an error answer. A
    native fault ends only the child: its output closes, the client logs the
    exit code (`0xC0000005`) and throws `GpuDriverCrashedException`, whose
    text the GPU tab shows ("GPU power limits are off: the graphics driver
    crashed …"). A call with no answer within 8 s (the app gives up on a request after 5 s; ADLX's start takes 2–3 s; a new worker's first call gets 20 s, since right after boot the driver and AMD's ADLX server can be slow) kills the child (tree)
    the same way; a garbled answer restarts it without blaming the driver.
    A crash is remembered in `gpu-worker-crash.marker`
    (`GpuCrashGuard`: boot time + the driver files' versions, read without
    loading them), so the worker isn't started again — not even by a
    restarted service — until Windows restarts or a driver changes. A hang
    is only paused for 10 minutes, in memory: a slow driver right after boot
    must not cost the power limits for the whole day. The
    child inherits the service's account and session, is put in a
    kill-on-close job object (`KillOnCloseJob`) so Windows ends it the
    moment the service ends — even stuck inside a driver call — and also
    exits when its stdin closes, logs on stderr (the service writes those
    lines to its log; two processes appending one file would collide) and
    writes `gpu-power.json`. Each worker has its own output-reader thread,
    so calls don't depend on the thread pool. Debug builds take
    `--gpu-worker-crash-test` / `--gpu-worker-hang-test` to prove the
    service survives both. Tests: `GpuWorkerTests` (scripted workers and
    real `cmd.exe` children for pipes, exit codes, killing, UTF-8).
    The process plumbing is shared with the coming sensor worker (#374) in
    `Control/Workers/`: `WorkerProcess` (the child, its stdio and log
    forwarding), `KillOnCloseJob`, and `WorkerChannel` (the reader thread;
    a call's answer, a hang or the end of output). What to do after a crash
    or a hang stays with each client — `GpuWorkerClient` turns the GPU limits
    off; the sensor worker will be restarted. Tests: `WorkerChannelTests`.

    Why (2026-10-15, owner's rig): Windows Update installed "AMD Display
    Driver Update (32.0.21042.62)" over the newer Adrenalin driver
    (32.0.32015.2008). ADLX (`amdadlx64.dll` 1.5.0.124, from Adrenalin) no
    longer matched it: in the service (LocalSystem, session 0)
    `ADLXInitialize` ended the process with an access violation (Event 1000
    named the service's own single-file image, offset `0x19c4c8`); as a
    normal user it listed no GPU with power tuning; AMD Software said
    "Update Required". The service crashed on every capabilities request and
    Windows' recovery restarted it into the next one — which also locked
    `rigstats-sensor.exe` against the 1.48.0 installer. Reinstalling
    Adrenalin fixed the machine; the worker keeps the next such driver from
    taking sensors, fans and lighting with it. LibreHardwareMonitor's own
    vendor libraries (AMD ADL, NVIDIA NVAPI, for the GPU sensors) still run
    in the service — the next step.
  - `Probe` reports any driver failure as "unavailable", and the pipe's
    `capabilities` reports a provider that throws as unavailable instead of
    failing the whole list (every tab of the Control Center).
- **NVIDIA through NVML** (#210). `NvmlGpuPower` loads `nvml.dll` (System32
  on current drivers, `NVSMI\` on old ones) and P/Invokes the documented C
  API: `nvmlDeviceGetPowerManagementLimitConstraints` / `…DefaultLimit` /
  `…Limit` to read, `nvmlDeviceSetPowerManagementLimit` to write (needs
  admin — the service is SYSTEM). NVML speaks milliwatts; the GPU tab's knob
  stays a % offset from the driver default: the range is the whole percents
  inside the driver's min/max, limit = default × (1 + pct/100), clamped.
  "At factory" is the limit equal to the default, and a factory reset writes
  the default, so the original/Default handling above is shared. Adapters
  are keyed by NVML UUID.
  - **Laptop GPUs are left out** ("… Laptop GPU", "… Max-Q"). Their budget
    belongs to the firmware: on the G14's RTX 5070 Ti Laptop GPU NVML reads
    5–120 W, default 80 W and an enforced limit of 118 W set by Dynamic
    Boost / the ASUS performance mode, but answers `NOT_SUPPORTED` for the
    power-management limit itself. A GPU is only offered when that limit
    reads, whatever its name. On ASUS
    laptops that budget is controlled through ATKACPI (#234). A desktop
    board that answers a write with `NOT_SUPPORTED` / `NO_PERMISSION` is
    not offered again for the rest of the service run.
  - `GpuPowerApis` combines the drivers: each adapter goes to the driver
    that listed it, and a failing driver hides only its own adapters — an
    AMD iGPU whose ADLX won't initialize (#241) doesn't hide an NVIDIA card.
  - A failed `nvmlInit` is not retried for a minute, like ADLX.
  - **Diagnostics:** at start the service writes `gpu-power.json` (in the
    diagnostics ZIP): per NVIDIA GPU the raw NVML readings in mW — min, max,
    default, limit, enforced limit, or NVML's error for each — whether it is
    a laptop GPU, refused a write, and is offered. Read-only, no UUIDs. ADLX
    isn't opened for it (#241); AMD adapters are in
    `control-capabilities.json`. A user's export answers whether NVML works
    from the service on their card without them testing anything.

## Phase 4 — Curve Optimizer as built

What phase 4 (#191) shipped:

- **Commands** (Zen 4/5 RSMU, ZenStates-Core as protocol documentation
  only): SetDldoPsmMargin `0x6` (per core), SetAllDldoPsmMargin `0x7`,
  GetDldoPsmMargin `0xD5` (readback). Argument: core mask in bits 31:20
  (`ccd << 28 | core << 20`) with the offset as 16-bit two's complement in
  the low bits. Offered only where verified: Granite Ridge (9800X3D),
  through the same `RyzenSmu` handle as CPU limits.
- **Which cores exist** is asked from the SMU itself: GetDldoPsmMargin for
  every possible core (2 CCDs × 8); absent cores and CCDs are rejected
  (on the 9800X3D CCD 0 cores 0–7 answer, CCD 1 doesn't). Per-core values
  are offered only when that count equals Windows' physical core count
  (`GetLogicalProcessorInformation`), so "core 3" means the same core to
  user and SMU; otherwise only all-core.
- **Bounds:** −30 … 0 — the classic PBO2 range, undervolt only. A core's
  value is its own, else the all-core value, else the BIOS value; the BIOS
  values (read once per boot, `curve-opt-baseline.json`) are kept as they
  are even outside the bounds. All-equal targets are one SetAll, otherwise
  only the cores that differ are written; verify re-reads every core.
- **Safety:** any write that leaves the BIOS values arms the boot-crash
  guard; the UI always goes through preview; release and service stop put
  the BIOS values back if anything was changed. Acceptance was checked by
  killing the dev service within 3 minutes of a kept −10 on core 3: the
  next start logged the trip, applied CO as BIOS values (the SMU still held
  −10, since nothing rebooted) and showed the red notice; the profile keeps
  its −10 for when the user applies it again.

## Phase 5 — ASUS Aura lighting as built

What phase 5 (#192) shipped — built for every ASUS board with a USB Aura
controller, not one board:

- **Aura Sync over a device list.** `LightingProvider` drives a list of
  `ILightingDevice`s; a profile's one effect goes to every device, each
  translating it into its own protocol. The motherboard controller is the
  first device; monitors, peripherals and other vendors are added as more
  `ILightingDevice`s (#212) without touching profiles, the pipe or the UI.
  One device failing (unplugged) doesn't keep the others dark; only all of
  them failing fails the profile.
- **Native protocols only, current devices.** Decided for #192: RIGStats
  implements the protocols itself (OpenRGB, GPL, is documentation only — no
  code, no OpenRGB process or SDK server), aiming over time at what OpenRGB
  covers for *current* hardware; legacy devices (e.g. SMBus Aura on boards
  before ~2019) are out of scope.
- **The controller describes itself.** `AuraController` finds the first
  known Aura USB controller by its USB ids (OpenRGB's list, as protocol
  documentation only): the motherboard family `18F3`, `1939`, `19AF`,
  `1AA6`, `1BED` and the older addressable family `1867`, `1872`, `18A3`,
  `18A5`. It picks the HID collection on vendor usage page `0xFF72` and
  asks for the firmware string (`0xEC 0x82`) and the 60-byte config table
  (`0xEC 0xB0`). Zones come from that table: the onboard LEDs (count at
  `0x1B`) as one zone, then one zone per ARGB header (count at `0x02`) —
  so each board gets its own zones without being listed anywhere.
- **Broad on purpose.** Lighting can only get colours wrong, unlike CPU
  limits or Curve Optimizer, so every known controller is offered rather
  than only verified ones. The firmware and config table are logged at
  start (`Aura: 0x19AF Motherboard firmware '…', config …, zones …`), so a
  user's diagnostics export becomes a fixture in
  `sensor-sidecar.Tests/fixtures/aura/` without the hardware.
- **HID without hidapi:** `HidDevice.cs` uses SetupAPI + `hid.dll` and
  overlapped reads with a timeout; works from the service as SYSTEM.
- **Effects v1:** off, static, breathing, spectrum cycle, applied to every
  zone; brightness scales the colour (Aura effects have no brightness of
  their own). Motherboard family: an effect report (`0x35`) and a colour
  report with an LED bit mask (`0x36`) per zone; addressable family: one
  `0x3B` report with the colour. Nothing is committed to the controller's
  flash (no wear) — the service re-applies the active profile at start.
- **A profile without a lighting part leaves the lights alone**, so the
  built-ins change nobody's lighting. The controller can't report its
  effect: `Verify` trusts a successful write, `Capture` returns what the
  service last set, release leaves the lights as they are.
- **Armoury Crate / OpenRGB:** if LightingService, Armoury Crate or OpenRGB
  (app or service) is running, the domain is unsupported with
  `conflict: true` — the Lighting tab shows the explanation instead of
  disappearing (a machine with no lighting device gets no tab at all).
  Writes are skipped while the conflict lasts. Opening the Control Center
  re-reads the capabilities, so closing the other app takes effect at once.
- **UI:** effect, colour and brightness with a live preview (`aura_preview`,
  throttled to ~10 Hz); Save & apply stores it in the profile; Revert or
  closing the window puts the saved lighting back.

### More lighting devices (#212)

Each is one more `ILightingDevice`; all verified on the dev rig:

- **ASUS Aura monitors and the ROG Aura Monitor Light Bar**
  (`AsusMonitorDevice`) — the current family (XG27AQDMG `1BA3`, XG27ACDNG,
  XG27UCG, PG32UCDM/UCDMR/UCDP, light bar `1AC8`), same `0xEC` framing as
  the motherboard: LED count at byte 32 of the config reply, direct mode
  (`0x35 … 0xFF`), frames `0x40 0x84 0x00 <n> RGB…`. Two identical monitors
  are two devices. The older feature-report family (XG27AQ, XG279Q, ...)
  is out of scope (legacy).
- **Any HID LampArray device** (`LampArrayDevice`) — the vendor-neutral
  standard behind Windows Dynamic Lighting (usage page `0x59`), so one
  implementation covers every current keyboard, mouse or accessory that
  supports Dynamic Lighting, any brand (first: ROG Harpe Ace via the ROG
  Omni receiver, `1ACE`). Report ids and field layout come from the
  device's own descriptor (`HidP_*`); colours go out as a range update over
  all lamps; control is taken by clearing AutonomousMode and handed back on
  release / service stop.
  **ASUS "WDL state".** ASUS mice keep a setting in the device — Gear Link's
  *Cross-device Lighting Toggle* — that decides whether the LampArray is
  obeyed. In "Device Lighting" mode every LampArray report is accepted and
  ignored (no error, no colour change; the device can't report its colour,
  so the service can't detect it). Found on the ROG Harpe Ace Aim Lab
  Edition (`1A94` via dongle) behind the Omni receiver; captured from Gear
  Link's own HID log (it logs every report in the browser console). On the
  receiver's mouse channel (report id 3 — usage page `0xFF01` here; the
  receiver's vendor collections are found by report id, as their usage
  pages don't follow it: `0xFF02` = id 1, `0xFF00` = id 2, `0xFF01` = id 3;
  64-byte output):
  `51 42 00 00 01` → WDL on ("Aura Sync & Windows Dynamic Lighting"),
  `51 42 00 00 00` → off ("Device Lighting"); the reply echoes `51 42 00 00`.
  Read-only, verified against the device (replies after the echo):
  `12 00 09 00` → WDL state (`01` on, `00` off); `12 00 02 00` → paired
  mouse present (`01`); `12 00 00 00` → device info / versions
  (`04 00 07 00 05 07 00 03 FF FF 10 00 08`). On the receiver's own channel
  (report id 1): `a0 00` → paired devices
  (`01 00 94 1A 03 05`: one device, product id `0x1A94` little-endian, on
  report id 3 — so the mouse channel is found, not assumed), `a1 01` →
  receiver firmware (`04 00 07 00`, format unconfirmed).
  The service uses this (`OmniMouse`, #236): the receiver's paired product id
  names the mouse's LampArray (model table, else "Mouse via ROG Omni
  receiver"); the WDL state goes out as `wdl_on` in the capabilities, and
  `aura_enable_wdl` switches it on — only from the Lighting tab's button,
  only for verified models, confirmed by the echo, then the current
  lighting is applied. Which mice have the toggle: Gear Link's per-device
  manifests (`gearlink.asus.com/view/<pid>/manifest.json`, then the
  device's `main-<version>-<bundleId>.js`, which shows the WDL switch as
  `WDL:toggle`) and its Companion's model configs (`WDL=1`) agree on four —
  Harpe Ace Aim Lab Edition, Keris II Ace, Harpe Ace Mini, Harpe Ace
  Extreme. Newer mice (Harpe II, Gladius IV, Keris II Origin, Spatha X 65K)
  have no toggle and follow LampArray's autonomous mode; they are only
  named. Gear Link covered 99 ASUS product ids when scanned (0x1800–0x1F40,
  36 mouse ids, 45 keyboard, 8 headset, 5 receiver); its device modules
  name every command (`setWDLState` → `sendCommandWithResponse(0x51,
  0x42, 0, …)`), so they are the source for further ASUS devices — read as
  documentation, like OpenRGB, and marked "From Gear Link, not yet
  verified" until tested. The channel is opened per question: Windows queues
  every input report for every open handle, so a long-lived handle first
  read replies to Gear Link's own questions (a stale WDL state).
- **ASUS keyboards, TUF protocol family** (`AsusKeyboardDevice`, #213) —
  OpenRGB's `AsusAuraTUFKeyboardController` models (ROG Azoth, Falchion,
  Strix Flare / Flare II, Strix Scope / RX / NX / II / II 96, TUF K1/K3/
  K5/K7) on their vendor collection (usage page `0xFF00`), plus any keyboard
  paired to the **ROG Omni receiver** (`1ACE`): the receiver has one vendor
  channel per paired device (`0xFF00`–`0xFF02`, own report id), and the
  keyboard's is the one that answers "get layout" (`0x12 0x12`) with a
  layout. Effect: `0x51 0x2C mode 0 speed brightness …` with the keyboard's
  own static / breathing / colour cycle — nothing animated over the radio;
  not saved to the keyboard (no `0x50 0x55`). Per model: brightness scale
  (0–4 per OpenRGB; **0–100 measured on the Azoth X, the original Azoth and
  the Falchion Ace HFX** — at 4 they look off or very dim, so OpenRGB's 0–4
  may be outdated for current firmware of the whole family),
  speed scale, and K1/K5 without the per-key marker. Verified: ROG Azoth X
  through the Omni receiver and by cable (`1C24`), and the **ROG Falchion
  Ace HFX** (`1B7E`, not in OpenRGB): same layout reply as the Azoth X,
  0–100 brightness. Newer ASUS keyboards also expose a HID LampArray
  collection; on that path their firmware takes its own lighting back at
  once (a flash, no change), so a keyboard driven here has its LampArray
  collection left out of discovery (`WithoutDirectKeyboards`) — one path
  per keyboard. A new model: probe "get layout" read-only; an Azoth-style
  reply (`12 12 00 00 02 0B`) means this protocol, then one static write and
  a brightness-4 write settle the scale. Not covered: ROG Claymore (other
  layout, 2018), Strix Scope TKL family (direct per-key only).
  **From Gear Link (#240):** every keyboard in Gear Link's device data (18
  models, 45 ids — Azoth / Azoth Extreme / Azoth 96 HE, Falchion Ace /
  Ace 75 HE, Falcata, Strix Scope II 96 / II RX, Strix Morph 96 / 96 X,
  ProArt KD300, TX75 Analog / Core, TUF K4 Magnetic) builds its effect the
  same way (`0x51 0x2C <effect>` with `speed, brightness, flags, FF, FF,
  colours`) and shows a 0–100 brightness slider (default 50) — so they are
  in the model table at the Azoth X's settings, "From Gear Link, not yet
  verified"; a model's dongle and Bluetooth ids share its name. The Strix
  Scope II 96 Wireless / II RX moved from OpenRGB's 0–4 to Gear Link's 0–100.
  Driving one natively drops its LampArray collection, as above.
- **ASUS headsets, GearLink protocol** (`AsusHeadsetDevice`) — not in
  OpenRGB; the protocol is GearLink's own declarative command schema (read
  as documentation): report `0xCC` on usage page `0xFF00`, frame
  `[command, key, index0, index1, data…]`, command `0x12` get / `0x51` set;
  lighting set key 40 / get key 3 = `effectId, brightness 0–100, R, G, B`
  (static 1, breathing 2, colour cycle 4). The only lighting device with
  readback, so every write is **verified by reading it back**. Verified:
  ROG Delta II through its 2.4 GHz dongle (`1AFA`). The schema lives in
  each headset's Gear Link module (`bundle-*.js` beside `main-*.js`, as
  `lightingConfig:{get:{key:3,…},set:{key:40,…}}`): ROG Pelta, Delta II
  (KJP) and Delta II (PBZ) carry the identical entry and are in the model
  table "From Gear Link, not yet verified" (#240); Pelta Core, Cetra Open
  Wireless and Gjallar have no lighting entry.
- **Software effects.** LampArray devices have no built-in effects, so
  breathing and spectrum cycle are drawn by the service (`SoftwareEffect` /
  `SoftwareEffectLoop`, 25 frames a second). So are Aura monitors not yet
  seen running their own: the light bar and the XG27AQDMG use theirs (the
  `0xEC 0x35` effect command with mode 1 static, 2 breathing, 4 colour cycle,
  from DisplayWidget Center's DLL, verified on hardware) — one write per
  change, and the effect keeps running when the service stops.
  `AsusMonitorDevice.HasBuiltInEffects` lists the verified models.
- **Finding protocols without OpenRGB.** The ASUS ones above came from:
  read-only "get" probes (`0x12 0x00` version, `0x12 0x12` layout) whose
  replies identify channels; one reversible write at a time with the user
  watching; and ASUS GearLink — a WebHID web app whose public bundles carry
  each device's command schema, and whose console can log every report.
- **Devices coming and going.** `LightingProvider.Rescan` looks again when
  the Control Center asks for capabilities and before a profile applies
  (at most every 2 s, never per live-preview step). Devices are probed again
  only when the set of HID collections changed (e.g. a keyboard switched
  from its receiver to the cable). Devices still there keep their instance;
  new ones get the current lighting at once; gone ones are closed. Only the
  first discovery is logged in full, later ones log just "+ X, - Y". A
  headset switched on behind an already-plugged dongle changes no HID
  collection, so the scan carries a `Retry` that asks only the dongles whose
  headset isn't connected, on each rescan, until it is. The dongle answers
  by itself, so "not connected" is a deviceInfo reply whose headset version
  is all zeros.
- **Supported devices list.** [`supported-devices.md`](supported-devices.md)
  and the website's "Supported lighting devices" table are generated by
  `LightingCatalog` from the drivers' model tables (`AuraUsb.Families` +
  `Verified`, `AsusMonitorDevice.Models`, `AsusKeyboardDevice.Models`,
  `AsusHeadsetDevice.Models`), each with a `Verified` flag — hardware
  seen working vs. taken from OpenRGB. `SupportedDevicesTests` fails when
  either file has drifted; regenerate with
  `$env:RIGSTATS_UPDATE_SUPPORTED_DEVICES=1; dotnet test sensor-sidecar.Tests --filter SupportedDevices`.
  Verifying a model on hardware = setting its `Verified` and regenerating.
- **Desk lamp (#214).** The ROG light bar's white lamp is channel 1 of the
  same `0xEC 0x35` effect command (static, the 6500 K and 2700 K channel
  values instead of R, G, B); `0xEC 0x31` / `0xB1` on channel 1 reads them
  back, so every lamp write is verified. Found in ASUS DisplayWidget
  Center's `ScreenLightBarHid.dll` (read as documentation). Brightness sets
  the total, at most 178 — DisplayWidget Center's 70 % power budget —
  and temperature (2700–6500 K) splits it. A profile's `aura.lamp`
  (`on`, `brightness`, `temperature`) is independent of the RGB effect (a
  part with only a lamp leaves the RGB alone). The tray's "Toggle Desk
  Lamp" (`lamp_toggle`, shown only while a lamp is connected) reads the
  lamp first — its own button switches it too — and turns it off, or back
  on as it last was; not saved in a profile. Probe reports each lamp's real
  state (`lamp_on`), and the Lighting tab's live preview sends only the part
  that changed (RGB or lamp), so trying a colour never resets a lamp
  switched from the tray.
- **Diagnostics.** After every discovery the service writes `lighting-devices.json`
  (in every diagnostics ZIP): each device's raw data (Aura firmware + config
  table, monitor config reply, LampArray kind/lamps/update interval/report
  layout), the conflict state, and a scan of every HID collection on the
  machine (no paths or serials) — entries RIGStats doesn't know
  (`known_as: null`) are the candidates for new support. `asus_probes`
  (`OmniMouse.Probe`): each ASUS receiver in Gear Link's data (Omni `1ACE`,
  SpeedNova 8K `1AD0`, Asus Dongle `1D54`) is asked its firmware and paired
  list, and each paired device its device info, presence and WDL state —
  only read commands Gear Link itself sends while idle, no serial numbers —
  so a report names a new mouse and shows whether it has the WDL toggle.
  Reports come in through the "Device" issue form
  (`.github/ISSUE_TEMPLATE/device.yml`), which also asks for Gear
  Link's console log (its `HID OUT`/`HID IN` lines name every command).
- **Per-device blockers.** While Windows Dynamic Lighting is on for a
  signed-in user (`AmbientLightingEnabled` under `HKEY_USERS\<sid>\Software\
  Microsoft\Lighting` — the service runs as SYSTEM), Windows drives the
  LampArray devices itself: they report `blocked` and Aura Sync skips them,
  the Lighting tab explains it per device, and the rest keep syncing.

### Battery status of wireless devices (#290)

Read-only, from the lighting devices already open. `IBatteryDevice`
(`Lighting/Battery.cs`) beside `ILightingDevice`: `AsusHeadsetDevice` asks
`12 07` + `12 08` (every headset in its table), `AsusKeyboardDevice` `12 01`
(only ids in `BatteryModels` — `hasPowerInfo` in the model's Gear Link
manifest; behind the Omni receiver, the paired keyboard's id decides), the
Omni receiver's mouse `12 07` on its channel (`OmniMouse.ReadBattery`, via its
`LampArrayDevice`; every mouse in `OmniMouse.Models` has power info). The
reply layouts are in `BatteryReplies`; a standby answer (0 %, not charging) and
an `FF AA` error read as no answer. `PeripheralBatteryMonitor` reads every
20 s (first after 10 s) and calls `LightingProvider.Rescan()` every 5 s (a HID
list comparison; full discovery only on a change — nothing else may trigger
discovery), reading at once when the device list changed, so a device
plugged in or switched shows within seconds; charging over a cable on a
2.4 GHz device appears only in the battery reply, so within one read plus the
device's own delay in reporting it (measured on the owner's rig: the Harpe Ace on the Omni receiver ~15 s after plugging or unplugging, the Azoth X up to ~45 s — longer than two reads, so the keyboard itself updates its charging flag late). A silent
device keeps its last reading for 10 minutes, sent with `connected: false`
and `charging: false` (#351 — the app dims it, says "Not connected" and
doesn't alert on it; then it drops off until it answers), logs the first reading and charging changes (`Battery: ROG Azoth X
82 %.`), and `HardwareHost` adds the list to every telemetry line as
`peripherals: [{id, name, kind, battery, charging, connection, connected}]` and, since #302, `active_profile: {id, name}` (from `ProfileStore.ActiveProfile`, omitted when none — the wallpaper host has no control pipe and shows the header's profile chip from it) (`connection`: `usb`, `bluetooth` or `2.4ghz`, from `Connection.Of` — Bluetooth from the HID path (`BTHENUM` or the HID-over-GATT service `{00001812-…}`), 2.4 GHz for a receiver or a dongle whose product/model name says "2.4", else a cable; no device command). The icon is the data path, not the power: a cable plugged into the ROG Azoth X while it is in 2.4 GHz mode only charges it — no new HID device appears and the keyboard keeps answering through the Omni receiver, so the panel keeps the 2.4 GHz icon and shows it charging (owner's rig, 2026-10-09). It shows up as a USB device (`1C24`) only in its wired mode; switched there, it stays in the receiver's paired list, so keyboard discovery leaves a receiver entry out when the same model is connected directly (`AsusKeyboardDevice.KeepIndices`) — omitted until the first
round, so the golden fixtures don't carry it. The field's shape is pinned by
`sensor-sidecar.Tests/contract/telemetry-peripherals.json`, which the Rust
reader checks too. Not covered yet: ROG mice on their own 2.4 GHz dongle or
cable (driven as LampArray, no ASUS protocol path), devices while another
program owns the lighting.

### Battery status of Bluetooth devices (#291)

No device protocol: Windows already reads the standard battery level of a
paired device (GATT Battery Service on Bluetooth LE, the HFP battery report on
headsets) and stores it as a device property — what Settings › Bluetooth &
devices shows. `BluetoothBatteries.Read` lists the present `BTHLE` and
`BTHENUM` device nodes (`CM_Get_Device_ID_ListW`, filter enumerator +
present) and reads, with `CM_Get_DevNode_PropertyW`:

| Property | Key | Note |
|---|---|---|
| Battery level | `{104EA319-6EE2-4701-BD47-8DDBF425BBE5}` 2 | `DEVPROP_TYPE_BYTE`, percent; nodes without it are skipped |
| Address | `{2BD67D8B-8BEB-48D5-87E0-6CDA3428040A}` 1 | string; the id is `bt:<address>`, one entry per address (a classic device has a node per service) |
| Name | `DEVPKEY_NAME` `{A45C254E-…}` 14 | |
| Container category | `{78C34FC8-104A-4ACA-9EA4-524D52996E57}` 90 | string list, `Input.Mouse` → mouse, `Input.Keyboard` → keyboard, `Audio.*` → headset, else device |

Connected is `CM_Get_DevNode_Status` without `DN_DEVICE_DISCONNECTED`
(`0x02000000`). A paired device stays *present* when off, with its last
level, so `BluetoothBatteryTracker` shows a device while connected and for
10 minutes after with `connected: false` (like a silent ROG device); one already disconnected when
the service starts isn't shown. Merged after the ROG readings; a Bluetooth
entry whose name matches a ROG one is dropped (the ROG protocol also knows
charging). Read in the same 20 s round, logged when first shown and on a
10-point change (`Battery: MX Anywhere 3 Mac 80 % (Bluetooth).`).

Measured on the owner's rig with a Logitech MX Anywhere 3 for Mac
(2026-10-13), comparing the device's properties connected, off and charging:

- **No charging flag.** The level rose while charging (75 → 80 %), nothing
  else changed; `{104EA319-…}` 3 is a bool that stayed False either way, so it
  isn't charging. Bluetooth entries send `charging: false` — except Logitech
  devices, which say it themselves (below).
- **Connected flag:** `{83DA6326-97A6-4088-9453-A1923F573B29}` 15 also flips
  with the connection; `DN_DEVICE_DISCONNECTED` was used since it's documented.
- **`LastConnectedTime`** (`{2BD67D8B-…}` 11) is a FILETIME holding *local*
  time, not UTC — so it isn't used; the tracker keeps its own timestamps.
- `{104EA319-…}` 7 is a FILETIME (UTC) of the last level update.

Live-verified on the same rig: the MX Anywhere 3 for Mac and a Satechi M1
Mouse, both in the PERIPHERALS panel and the tray hover card with the
Bluetooth icon.

### Charging of Logitech devices on Bluetooth (#349)

Logitech devices answer in their own protocol, HID++ 2.0, on a vendor HID
collection — on Bluetooth LE usage page `0xFF43` (usage `0x0202`), long
reports only (20 bytes, report id `0x11`), device index `0xFF` (direct;
a reply may carry `0x00`). `LogitechBattery` sends only documented reads
(Logitech's feature specs as implemented by Solaar's `hidpp20.py`):

| Request | Bytes | Reply parameters |
|---|---|---|
| Root `getFeature(0x1004)` | `11 FF 00 0B 10 04` | feature index (0 = absent) |
| Root `getFeature(0x1000)` (only if no `0x1004`) | `11 FF 00 0B 10 00` | feature index |
| Unified Battery `0x1004` get_status (function 1) | `11 FF <idx> 1B` | state of charge %, level flags (8 full, 4 good, 2 low, 1 critical), charging status (0 discharging, 1 charging, 2 slow, 3 full, 4 error), external power |
| Battery Status `0x1000` (function 0) | `11 FF <idx> 0B` | level %, next level, status (0 discharging, 1 recharging, 2 almost full, 3 full, 4 slow recharge) |

The low nibble `B` is our software id: replies echo it, the device's own
notifications carry 0 and are skipped. An error reply (`FF`, or HID++ 1.0
`8F`) counts as no answer. The feature index is remembered per device, so a
round is one request. `LogitechBatteryReader` matches the HID collection to
the Windows Bluetooth node by the address in the HID-over-GATT path
(`…_pid&b025_rev&0015_de6768c18cf5&col02`) and `LogitechBattery.Apply`
puts its percentage (when it has one — a 0 % state of charge means level
flags only, then Windows' percentage stays) and charging on that entry.
Only connected devices are asked; a failure keeps the Windows reading and is
logged once.

Measured on the owner's MX Anywhere 3 for Mac (PID `B025`, 2026-10-13):
Unified Battery at index 8, 90 % — the same as Windows; plugging the cable in
showed charging in the next read (≤ 20 s), and unplugging it cleared it in the
same way. The MX Mechanical Mini for Mac keyboard (PID `B36D`, Bluetooth name
"MX MCHNCL M", category `Input.Keyboard`) answers the same way: Unified Battery,
100 %, and charging with the cable in — at 100 % it reports full on power,
which counts as charging (status 3, like Solaar). The MX Keys Mini (PID
`B369`, 2026-10-16) as well: Unified Battery, 95 %, the same as Windows
(`Battery: Logitech B369 (d8a82ea667ef) 95 % (HID++ 0x1004).`); with the cable
in, it showed charging (`… 95 %, charging (HID++ 0x1004).`) and the panel
turned it blue. Devices behind a receiver: next section.

### Logitech devices behind a Bolt or Unifying receiver (#407)

A device on a Logitech USB receiver isn't a Windows Bluetooth device, so it
has no Windows battery. `LogitechReceivers` finds the receivers among the HID
collections: Bolt `046D:C548`, and Unifying `C52B`/`C532`, from Solaar's
`base_usb` table. Nano and Lightspeed receivers aren't read. The battery
monitor reads each one every round (`LogitechReceiverDevice`, Connection
`2.4ghz`).

**Two collections.** Windows splits the receiver's HID++ interface (interface 2,
usage page `FF00`) into a short-report collection (usage `0001`, 7 bytes) and a
long one (usage `0002`, 20 bytes). A request goes to the collection of its
length, and the answer can come on either: a long register's answer comes on
the long one, an error on the short one. `ReceiverHid` writes to the right one
and reads from both. The two are paired by their Windows instance
(`HID\VID_046D&PID_C548&MI_02&COL01\B&6FD059D&0&0000` and `…COL02\…&0001`).

**Requests.** All are documented reads (Solaar's `receiver.py`, `base.py`):

| Request | Bytes | Answer |
|---|---|---|
| Connection count, register `0x02` | `10 FF 81 02 00 00 00` | 2nd parameter = connected devices |
| Pairing information, receiver info `0xB5` | Bolt `10 FF 83 B5 5N 00 00`, Unifying `… 2(N-1) …` | kind in the low nibble: Bolt byte 1, Unifying byte 7 (1 keyboard, 2 mouse, 8 trackball, 9 touchpad); an error (`10 FF 8F 83 B5 …`) = empty slot |
| Name | Bolt `10 FF 83 B5 6N 01 00` (`sub 01 length name`), Unifying `… 4(N-1) 00 00` (`sub length name`) | ASCII name |
| Battery | the HID++ 2.0 reads above, device index N instead of `FF` | as above |

The paired list (slots 1–6) is read again only when the connection count
changes or a receiver appears. Each round costs one register read plus each
device's battery read. A device that is asleep or off gets an error from the
receiver (`10 0N 8F …`). It is logged once and keeps its last reading for 10
minutes, like any device that doesn't answer.

**Measured on the owner's rig (2026-10-16).** A Logitech M650 on a Bolt
receiver:
- Log: `Battery: Logitech Bolt receiver C548: slot 2 Logi M650 (mouse).`, then
  `Battery: Logi M650 60 %.`
- It shows in Peripherals with the 2.4 GHz icon.
- The M650 runs on an AA battery, so charging wasn't seen.

A device that reports only a coarse level (Unified Battery with a 0 % state
of charge) shows no reading: no percentage is guessed. Which receivers are
verified: [supported-devices.md](supported-devices.md#battery).

### Philips Hue (#215)

Room lights through a Hue Bridge, on its official local API — no cloud
account, no OpenRGB.

- **Pairing** (`HueLink`, `HueBridge`): the Lighting tab's Philips Hue card
  finds bridges with one legacy-unicast mDNS question for `_hue._tcp.local`
  (asked from an ephemeral port, so bridges answer straight back — no
  multicast group, nothing for the firewall to open), or takes an IP
  address. Pairing is `POST /api` after the link button. The key, the
  bridge and its rooms/zones live in `%ProgramData%se.codeby.rigstatshue.json`:
  key DPAPI-encrypted (machine scope), file ACL SYSTEM + Administrators only.
  Never on the pipe, in the capabilities or in diagnostics.
- **TLS:** HTTPS only. The certificate must chain to Philips Hue's
  `root-bridge` CA or Signify's `Hue Root CA 01` (embedded) and its CN must
  be the bridge id — validation is never turned off. Bridges are reached by
  IP, so the host name isn't checked.
- **Devices:** one `HueRoomDevice` per chosen room or zone, driving its
  `grouped_light` over CLIP v2 (`on`, `dimming`, `color.xy`, `dynamics`).
  RGB → CIE xy (sRGB → XYZ), clamped to gamut C; brightness is the
  brightest channel. Lights not chosen are never touched; a profile without
  a lighting part leaves the room as it is (same rule as every device).
- **Rate limit → slow animations:** the bridge takes about one group
  command a second, so breathing and spectrum cycle are one command every
  2 s telling the bridge to fade to the next point — smooth, but not in
  step with the rig. (The Entertainment API would stream 25+ frames/s, but
  needs DTLS, which .NET lacks, and an entertainment area set up in the
  Hue app.) Commands are spaced 1 s apart per bridge and sent from each
  room's own thread, newest wins — a slow bridge never holds up the pipe.
- **Errors and reachability:** a failed send is logged once per failure
  streak (and "works again" when it recovers) and shown under the room in
  the Lighting tab (`problem` in the capability, as of when it was read) —
  it can't fail the profile, since sends run in the background. A bridge
  that stops answering is looked for by id again (at most once a minute) in
  case DHCP moved it; "not found" is logged once until it works again. A
  refused certificate says why (not signed by Hue / another bridge id),
  not just "SSL failed". Every failed pipe request is logged in the service
  log too, and the app gives `hue_*` requests 15 s instead of 5 s (pairing is
  up to four HTTPS calls). `Rescan` doesn't see network devices, so
  pairing and choosing rooms call `LightingProvider.Rediscover()`.

## Phase 6 — Armoury Crate coexistence as built

What phase 6 (#193) shipped, and what it deliberately didn't:

- **The goal was mostly met before it.** Fans, CPU limits, GPU power limit,
  lighting and profile switching all work without Armoury Crate. What was
  left is living *beside* it: a user who still has Armoury Crate (or OpenRGB)
  installed must not get two programs fighting over the same fan header or
  the same LEDs.
- **One table of other software** (`Control/OtherSoftware.cs`): process name
  → the name the user sees → the domain it writes. `LightingService` and the
  `ArmouryCrate` app → lighting (verified on the dev rig, since phase 5),
  `OpenRGB` → lighting, `AsusFanControlService` → fans (**hypothesis**, see
  below). Detection is by process name only — no service is queried, started
  or stopped. `LightingProvider` and `FanProvider` get a `Func<string?>`
  from it (null in tests, or any name) and check it in `Probe()` and before
  writing.
- **Taken, not hidden.** A domain another program owns is reported
  `supported: false` with `details.conflict = true` and a `Problem`
  (`aura_conflict` / `fan_conflict`, attention): the page stays, says who
  has it and what to do, Status lists it. A profile that asks for fans while
  they are taken throws `DomainUnavailableException` from `FanProvider.Apply`,
  so the rest of the profile applies and the result carries the note
  (`fan skipped: … controls the fans`). Lighting skips its writes the same
  way as before. Closing the other program takes effect the next time the
  Control Center reads capabilities — no restart.
- **Not stopping other vendors' services.** The issue asked for a guided,
  reversible stop of ASUS services from RIGStats. Dropped: a service RIGStats
  stops comes back at the next boot (or when Armoury Crate repairs itself),
  and stopping another vendor's software is the kind of thing that ends up in
  a support thread. Instead the problem's first action opens ASUS's own
  [Armoury Crate FAQ](https://rog.asus.com/support/FAQ/1041654) with the
  Armoury Crate Uninstall Tool — the supported way to remove it, and a
  reinstall brings it back. For Armoury Crate's fan service the message says
  that closing the app isn't enough.
- **Which ASUS component writes the fans — hypothesis.** Armoury Crate's fan
  control on desktop boards is Fan Xpert 4
  ([ASUS FAQ](https://www.asus.com/support/faq/1050426/)), and Armoury Crate
  installs a Windows service named `AsusFanControlService`; the table assumes
  that service is the writer. The owner's rig has no Armoury Crate (checked:
  only `ArmouryCrateDownloadTool` and `AsusUpdateCheck` exist, no ASUS
  process runs), so this is not verified on hardware. The evidence comes
  from users: the service logs one line at start-up —
  `Other hardware software running: Armoury Crate (AsusFanControlService) →
  fan control; also seen: ArmouryCrateService, AsusCertService` — listing
  every known program that runs plus any other process named ASUS, Armoury
  or Aura. A diagnostics ZIP from a user whose fans still fight (RIGStats'
  `Fan verify failed … firmware override?` lines while the table says no
  conflict) names the component to add; one where fans were skipped while
  the user had no Fan Xpert at all says the row is too broad.
- **Validated boards list — deferred.** It needs capabilities and fixtures
  from boards we don't have; the sensor-tree and Aura fixtures users send
  already become regression tests (`Probe_holds_for_every_sensor_tree_fixture`),
  which is the list in practice.
- **Live test without hardware risk:** any process with the right name
  triggers the detection, so `Copy-Item C:\Windows\notepad.exe
  $env:TEMP\AsusFanControlService.exe; Start-Process
  $env:TEMP\AsusFanControlService.exe` with the dev sidecar shows the Fans
  page's problem and the skipped note on an apply, and closing that Notepad
  gives the fans back.

## Peripherals: battery and settings (research, #290–#292)

Research for battery status (#290) and later device configuration (#292),
2026-10-09. Nothing here is implemented yet; commands are documented, cross-
checked between **two independent sources** where noted, and none were sent
to a device to find them out.

### Sources and how to read them

- **ASUS GearLink** (gearlink.asus.com, ASUS's WebHID app). Per device:
  `https://gearlink.asus.com/view/<pid in decimal>/manifest.json` gives
  `version` and `bundleId`; the module is
  `/view/<pid>/main-<version>-<bundleId>.js`, and its imports
  (`./bundle-*.js` or `./chunk-<version>-<bundleId>-*.js`) sit in the same
  folder. Headsets (`hidPattern` 2) carry a **declarative schema**
  (`{reportId, usagePage, command:{get,set,notify,index}, <name>:{get:{key,
  response:[…]}, set:{command?, key, data:[…]}}}`); mice and keyboards
  (`hidPattern` 1) have device classes calling
  `sendCommandWithResponse(cmd, key, index, …)`, whose replies are indexed
  **without** the report id (GearLink `t[4]` = byte 5 of the HID report).
  Download into a scratch folder outside the repo and read as text; never run
  it. The Delta II's schema is in `bundle-IDU94j3-.js` (v1.00.35); the Azoth X
  (`7205`, v1.00.33) ships its device classes in shared chunks, one per family.
- **G-Helper** (github.com/seerge/g-helper, GPL-3.0, actively maintained) —
  `app/Peripherals/{Headset,Mouse,Keyboard}/` with a base class per type
  (`AsusHeadset.cs`, `AsusMouse.cs`, `AsusKeyboard.cs`) and one file per model
  family with product ids, endpoint (`mi_00`, `mi_02&col03` …) and report id.
  Read as documentation only — no code copied (GPL).
- OpenRGB (lighting only, no battery).

### Transport facts both sources agree on

- Frame: `[reportId, command, key, index0, index1, data…]`; a reply repeats
  `command key`. Get `0x12`, set `0x51`; headsets also use set families
  `0x41`, `0x61` and `0x50` (reset).
- **Unsolicited reports arrive between replies.** G-Helper drains the input
  before each question and reads up to three more reports until
  `command key` match, logging the others as `EVT`. Our drivers match replies
  the same way (#281, #283).
- **`FF AA` is an error reply**: at bytes 1–2 (instead of the echo) or at
  bytes 5–6 after the echo. The Delta II answered `CC 51 28 00 00 FF AA` to a
  lighting set while it was off or out of range — that was a refusal, not a
  late acknowledgement. (`AsusHeadsetDevice` skips it and the read-back then
  fails; reporting it as "the headset refused" would be clearer.)
- Report ids: headsets `0xCC` (usage page `0xFF00`); mice and keyboards
  `0x00` by cable / own dongle; on the **ROG Omni receiver** (`1ACE`) the
  keyboard is on report `0x02` (`mi_02&col02`) and the mouse on `0x03`
  (`mi_02&col03`) — G-Helper hard-codes these, RIGStats takes them from the
  receiver's paired list. Keyboards send their events on report `0x70`
  (G-Helper `EventReportId`).

### Battery

Verified live 2026-10-09 (#294): ROG Azoth X on the Omni receiver 82 %, ROG
Harpe Ace Aim Lab Edition on the Omni receiver 60 %, ROG Delta II 23 % — one
device of each type, so all three layouts below are confirmed on hardware.

| Device type | Question | Reply (byte positions incl. report id) | Sources |
| --- | --- | --- | --- |
| Headset (GearLink pattern 2: Delta II, Pelta, …) | `12 07` | 5 sleep timer, **6 battery %**, 7 low-battery warning %, 8 low-battery voice prompt | GearLink `powerSaving`/`batteryLevel` (get key 7) + G-Helper `AsusHeadset.ParseBattery` ✓ |
| Headset charging | `12 08` | 5 = 1 charging | GearLink `chargingStatus` (key 8) + G-Helper ✓ |
| Mouse (Harpe Ace, Keris, Gladius …) | `12 07` | **5 battery %**, 6 auto power-off, 7 low-battery warning, 8–9 battery voltage (GearLink), 10 charging (> 0), 11 battery type, 12 full-charge effect | GearLink mouse power class (`18, 7`) + G-Helper `AsusMouse` ✓ |
| Keyboard (Azoth, Azoth X, Falchion Ace …) | `12 01` | **6 battery %**, 7 idle/sleep timeout, 8 power saving, 9 charging (== 1), 10 low-power level | GearLink keyboard power class (`18, 1`) + G-Helper `AsusKeyboard` ✓ |

- **Same command number, different meaning per type** — `12 07` is battery on
  mice and headsets but not on keyboards (`12 01`). Send each type only its own
  question; `12 01`–`12 3F` swept on a keyboard is what broke the Falchion Ace
  HFX (see "Live testing on real hardware").
- A mouse that went to standby answers battery 0 and not charging (G-Helper
  treats that as "not ready").
- The Delta II also sends an undocumented event `CC 12 09 00 00 <n>` with `n`
  falling slowly (1E → 18 over an hour, 2026-10-09). Neither source names key
  9, but it matches the battery: the last event read 0x18 (24 %) and `12 07`
  answered 23 % three hours later. Likely a battery notification — still ask
  `12 07`, which is documented.
- GearLink has more families with their own power commands (a gamepad class
  `18, 4`; a keyboard family reading `getDeviceInfo(CurrentPower)` with battery
  at its `n[5]`; a JSON `class_id:"10020000"` protocol). Check a device's own
  module before assuming one of the three above.

### Settings (for #292 — documented, not used)

**Headset (Delta II schema, confirmed by G-Helper):** lighting status get
`12 13` / set `51 10 00 00 <on>`; lighting get `12 03` / set `51 28 00 00
<effect brightness R G B>`; equalizer status get `12 21` (status + 10 bands)
/ set `41 03`; all gains `41 04 <10 bytes>`; one band `41 06 <index gain>`;
sidetone status get `12 24` / set `41 11`; sidetone level get `12 19` / set
`61 11 <0–20>`; noise reduction (ECNR) get `41 20` / set `41 02`, level `41
10`; voice prompt get `12 28` / set `41 0A`; power settings set `51 37 00 00
<sleep lowBattery% prompt>` (sleep values 2, 3, 5, 10, 15 min, 0 = never);
latency key 82 (`0x52`); aura sync key 51 (`0x33`); reset to defaults `50 40`;
reset device `50 60`. G-Helper also uses `41 08`, `41 0C`/`41 0D`, `41 05`
on other headset models (not in the Delta II schema).

**Headset settings as built (#428).** Sidetone and noise reduction are
offered on the Control Center's **Headset** page, for every profile (they
live in the headset, like GearLink's; nothing is stored by RIGStats).
`HeadsetCommands` sends only: `12 24` / `12 19` / `41 20` to read (sidetone
on, sidetone level at byte 5; noise reduction on at 5, level at 6), and
`41 11 <on>`, `61 11 <level>`, `41 02 <on>`, `41 10 <level>` to write —
only what differs, then all three are read again and must equal what was
sent, or the request fails. Limits per model (`AsusHeadsetDevice.SettingsModels`),
each from that model's own GearLink schema: Delta II `1AFA` / KJP `1D41` /
PBZ `1E6B` sidetone 0–20, noise reduction three levels (`ecnrConfig`; Low /
Medium / High in GearLink's UI, seen by the owner; G-Helper reads the level
as 0–2); Pelta `1B82` / `1B84` sidetone 0–20, noise reduction on/off (its
`ecnrStatus` answers only the status and there is no `ecnrConfig`).
G-Helper's `Pelta` says sidetone 0–100 — GearLink's 0–20 is used: ASUS's
own scale, and valid under both. Only the Delta II `1AFA` is verified; the
others show "settings not yet verified on this model". ROG Cetra, Clavis and
Strix Go have no sidetone or noise reduction (G-Helper overrides both off)
and aren't driven at all. GearLink's `supportFunctions` capability query is marked
unavailable on the Delta II and has no key, so models are listed rather
than asked. The page appears when a lighting capability device carries
`headset_settings: true` (no device I/O in the capabilities); the settings
themselves are read when the page opens (`headset_settings`) and written by
`headset_set`. **Verified 2026-10-17 on the Delta II (2.4 GHz):** 18 changes
covering both switches, sidetone 0–20 and all three noise levels, each read
back as written; sidetone is heard **only while a program uses the
microphone** (a call, a recording, the Settings level meter) — with nothing
recording it is silent, which is the headset's behaviour, not a failed
write. Noise reduction filters what the microphone sends, not what plays
in the headset.

**Mouse (G-Helper `AsusMouse`):** profile get `12 00` / set `50 02 <profile>`;
config get `12 04 00` (polling rate, angle snapping, debounce …), DPI get `12 04
02` on models with separate X/Y DPI (else `12 04 00`), DPI colours `12 04 03`, acceleration `12 04
01`, motion sync `12 04 04`; set `51 31 <sub> 00 <value…>` with sub `00`–`03`
DPI per stage, `04` polling rate, `05` debounce, `06` angle snapping, `07`
acceleration, `08` deceleration, `09`/`0A` DPI stage / stage count, `0B` angle
tuning, `12` motion sync; lift-off get `12 06` / set `51 35 FF 00 FF <dist>`;
energy `51 37 00 00 <powerOff> 00 <lowBattery%>`; lighting get `12 03 <zone>` /
set `51 28 <zone> …`; save profile `50 03`. Per-model limits (DPI range,
polling rates, which settings exist) are in each model file.

**Mouse settings as built (#431).** The Control Center's **Mouse** page and
the `mouse` profile domain (`Control/Mice/`):

- **Which mice.** `MouseCatalog` lists every ROG mouse on the common protocol
  — 45 rows (model × link: cable, own dongle, ROG Omni receiver) transcribed
  as facts from G-Helper's 72 mouse model classes by a resolver that follows
  each class's constructor chain and merges its overrides with
  `AsusMouse`'s defaults; the Omni ids are G-Helper's `MouseFromOmniPid`.
  Families whose packets or replies differ (Gladius II, Strix Impact I/II,
  Pugio I/II, Chakram / Core, Keris Wireless, Strix Carry, TUF M5) are left
  for #432. Found by id only: the mouse's own collection (interface, report
  id and a vendor usage page as the row names) or the Omni receiver's paired
  list (`OmniMouse.PairedDevices` / `Channel`) — nothing is sent to an
  unknown device.
- **Cross-checked with GearLink** (its module for the Harpe Ace Aim Lab
  Edition, `1A94`): the same frames for DPI (`setDPI` 81·49·stage−1), DPI
  stage count (81·49·10), active stage (`switchDPI` 81·49·9, 1-based), polling
  (`setPollingRate` 81·49·4), button response (81·49·5), angle snapping
  (81·49·6), angle tuning (81·49·11), motion sync (81·49·18), profile
  (`setProfile` 80·2·profile−1), save (80·3) and every read; the booster
  question `7D 20 02` too. Acceleration / deceleration (`51 31 07` / `08`)
  are written as G-Helper documents them — GearLink's module reads them
  (`12 04 01`) but has no writer.
- **Where GearLink is safer, it is followed.** Energy `51 37 00 00 <sleep> 00
  <low> <battery type> <full-charge effect>` (`setPowerConfig`) — the last
  two written back as `12 07` read them, where G-Helper sends 0. Lift-off
  `51 35 <type> 00 <material> <height> 00 <k0 k1 k2>` (`setCalibration`) —
  type, material and k-values written back as `12 06` read them, where
  G-Helper sends `FF`.
- **Every change:** read → write only what differs (`MouseProtocol.Writes`)
  → `50 03` → read back → equal, or the request fails. A profile's mouse
  part switches the onboard profile (`50 02`), verified by `12 00 00` byte 11;
  a missing mouse skips the domain (`DomainUnavailableException`).
- **Booster.** Mice whose G-Helper class offers 2000–8000 Hz only "if
  Booster" get them only when the Omni receiver answers `7D 20 02` with 1;
  on a cable or own dongle they get 125–1000 Hz, as in G-Helper. With
  booster the polling code is the high nibble of `12 04 00` byte 13.
- **Verified 2026-10-17 on the ROG Harpe Ace Aim Lab Edition on the Omni
  receiver:** every setting read as G-Helper showed it (400/800/1600/3200,
  stage colours, 1000 Hz, 12 ms, lift-off low, 3 min, 20 %); DPI, stage
  colours, active stage, polling 500 ↔ 1000 Hz, lift-off, angle snapping and
  tuning, acceleration, deceleration each read back as written. The first
  `12 04 00` of a change sometimes goes unanswered over the Omni link
  (3 of ~15 changes); asked once more it always answered, so every question
  is repeated once before a request fails. The onboard-profile switch per
  RIGStats profile was checked too: Balanced → profile 1 at 500 Hz, Gaming
  → profile 2 at 1000 Hz, each kept and shown again on switching back. The
  page reads the mouse again whenever the Control Center profile changes
  (`mice_read_for`) — before that fix it showed the old profile's values.
- **No stage colours on the page.** A stage colour is stored in the mouse
  (written and read back as `#ffbc0c` etc.), but its light kept the Lighting
  page's colour: RIGStats drives it through Dynamic Lighting, so the colour
  never shows — hypothesis: it shows only while the mouse runs its own
  lighting. The owner chose to leave colours out; the protocol still writes
  each stage's colour back as it was read.

**Keyboard (G-Helper `AsusKeyboard` / `Azoth`):** low-battery alert `51 37 00 00
<%>`, idle/sleep timeout `51 38 00 00 <value>`; the Azoth family's OLED/screen
commands use `0x61`, `0x63`, `0x68`, `0x69`, `0x6A`, and `0x21` to read.

**Microphone mute and the mute lamp (ROG Delta II, verified 2026-10-17,
#425):** there is no microphone command in the headset's HID schema
(GearLink's keys are battery, power saving, lighting, equalizer, sidetone,
voice prompt, demo mode; G-Helper has none either, and GearLink's own UI offers no mute for the Delta II — checked by the owner) — GearLink mutes through
`SetSystemSettings` on a Windows audio endpoint, which is what RIGStats does
too (`rigstats-backend/src/microphone.rs`, Core Audio, any headset). The
headset's own mute button is **local**: pressed while every one of its four
HID collections was read (consumer `0x0C`, vendor `FF13`, telephony
`0x0B/0x05`, vendor `FF00`), it sent nothing, and Windows' endpoint mute
didn't change — so neither Windows nor RIGStats can see it. The telephony
collection declares the standard LED outputs (page `0x08`: Mute `0x09`,
Off-Hook `0x17`, Ring `0x18`, Hold `0x20`, Microphone `0x21`, Online `0x2A`,
report `0x05`), the way Teams-certified headsets light their mute lamp;
every one of them was written — alone, Off-Hook + Mute, Online + Off-Hook +
Mute, all six, through `HidD_SetOutputReport` and through `WriteFile` — and
accepted, and the lamp never lit. The lamp is therefore not controllable by
any documented means; a vendor command for it is not guessed at (see "Never
probe with unknown commands"). Descriptors were read with
`tools/hid-caps.ps1` (read-only).

Model coverage when read: G-Helper 31 mouse model files (several classes each), 7 keyboard families (Azoth
incl. Extreme / X / Omni, Claymore II, Falchion, Strix Flare II, Strix Scope II
/ RX, TUF K3), 6 headset families (Delta II, Pelta, Cetra RGB / SpeedNova,
Clavis, Strix Go 2.4); GearLink 99 ids (see "More lighting devices").

---

## Open questions

- PM table layouts for Ryzen generations other than Granite Ridge `0x620105`
  (Matisse, Vermeer, Raphael have command ids but no verified layout).
  Unverified CPUs log their table version and head to `rigstats-sensor.log` —
  collect diagnostics exports and add them to `AmdSmuMap.Layout`.
- Curve Optimizer on other generations: Raphael (Zen 4) documents the same
  ids as Granite Ridge but is untested; Zen 3's are marked "not sure" even
  in ZenStates-Core. Per-core on CPUs with fused-off cores needs the
  firmware's core map (ZenStates reads it from the APOB) — until then they
  get all-core only.
- Does runtime CO need PBO enabled in the BIOS to take effect? The SMU
  accepts and reads back the offsets either way (verified with PBO state
  unknown); whether the voltage actually follows is not measured yet.
- ADLX power tuning is verified on one card (RX 9070 XT, ADLX 1.5). Which
  other Radeon generations, Pro drivers and AMD laptops report it, and do
  any misbehave? A native crash inside ADLX costs only the GPU worker
  (#370), which the GPU tab then explains.
- NVML power limits are built from NVIDIA's documentation and unit-tested
  but not yet checked on a desktop GeForce: that it works from session 0 as
  SYSTEM, the readback after a write, and whether the limit survives a
  driver reset (the original is persisted either way).
- Does an ADLX power limit persist across a reboot (Adrenalin re-applying
  "Custom")? Harmless either way — the original is persisted and the active
  profile is re-applied at start — but worth knowing.
- Which ROG boards expose writable fan control through LHM, and which firmware
  overrides it? Diagnostics exports now record it (verify failures and the
  measured channel→fan mapping, #207) — collect and turn them into fixtures.
- Aura on other boards: every known USB controller id is offered, but only
  the PRIME B650M-A (`19AF`, ARGB headers only) is checked on hardware — in
  particular the onboard-LED zone (`0x1B` > 0) and the addressable family
  are untested. Each user's diagnostics export carries the firmware and
  config table; turn them into `fixtures/aura/` files.
- More lighting devices (#212, umbrella) — still open: the light bar's desk
  lamp (#214 — GearLink may hold its schema too), more ASUS headsets on the
  GearLink protocol (only the Delta II is listed), the TUF keyboard models
  OpenRGB documents but nobody has tested here, other ASUS peripherals, then
  other vendors' current devices. Any Dynamic Lighting device already works
  through `LampArrayDevice`.
- The colour picker only updates the lights when the pointer stops (the
  egui colour picker reports the change late); check with the egui
  upgrade (#200).
- Service rename (`rigstats-sensor` → `rigstats-service`): worth the installer
  migration, or keep the name?
- Is `AsusFanControlService` the process that writes the fans for Armoury
  Crate on desktop boards, and does it write them even when the user never
  opened Fan Xpert? Unverified (no Armoury Crate on the dev rig); users'
  service logs carry the `Other hardware software running:` line — see
  [Phase 6](#phase-6--armoury-crate-coexistence-as-built). Other fan tools
  (FanControl, Argus Monitor) are not in the table yet; each is one row.
