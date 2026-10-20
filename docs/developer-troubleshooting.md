# Developer Troubleshooting

The technical side of [troubleshooting](troubleshooting.md): what the log and
the diagnostics ZIP say, why things fail, and what to check when running from
source. The user-facing steps are in `troubleshooting.md` — the app's and the
service's **Read more** buttons link there, so keep its anchors working (old
ones are kept as `<a id="…">` above the new headings).

## PawnIO (CPU Temperature, Package Power)

Since v1.21.0, `LibreHardwareMonitorLib` uses **PawnIO** — a properly signed
kernel driver — instead of WinRing0 to read CPU temperature, package power and
other SMU/MSR sensors. The installer runs the official PawnIO setup
(`build\pawnio\PawnIO_setup.exe -install -silent`) before starting the service.
An existing PawnIO install from another tool (FanControl, LHM, …) is kept or
upgraded, and uninstalling RIGStats leaves PawnIO in place.

**Symptom of a missing PawnIO:** CPU temperature reads `0` / `--` and package
power `0 W` (in `sensor-tree.txt`: `Core (Tctl/Tdie) val=0`, `Package val=0`).
Check the service from any prompt:

```powershell
sc.exe query PawnIO
```

`STATE: 4 RUNNING` is correct. Error `1060` (service does not exist) means the
PawnIO device was never created — the install log (`rigstats-install.log` in
the diagnostics ZIP) records `pawnio_exit=` and `pawnio_service_query=`.
Installers before #205 only staged the driver with `pnputil`, which never
creates the device on a machine without another PawnIO consumer.

`pawnio_exit=50` (ERROR_NOT_SUPPORTED) with `windows_build=` below `17763`:
PawnIO's INF installs only on `NTamd64.10.0...17763` and its catalog is
signed for `10_RS5` and newer (`PawnIO/PawnIO.inf.in`, `PawnIO/CMakeLists.txt`
in PawnIO 2.2.0) — Windows 10 1809. Nothing to fix but Windows; the service
reports `pawnio_windows_too_old` in Status (`PawnIoProblems`, #507), and
`pawnio_missing` when the driver can't be opened on a newer build. Seen on a
Razer Blade on Windows 10 1709 (build 16299).

**Fix (and the dev-environment setup when running from source):** install
PawnIO once from an elevated prompt, then restart the sensor service:

```powershell
winget install namazso.PawnIO
sc.exe stop rigstats-sensor; sc.exe start rigstats-sensor
```

## The Sensor Service

```powershell
sc.exe query rigstats-sensor
```

`STATE: 4 RUNNING` means the service is active. If it is stopped or missing,
the installer may not have completed — check `install.log` in the diagnostics
ZIP.

## GPU Sensors Missing Or The Service Crashing

**Symptom:** the GPU panel shows core load and D3D 3D usage, but `TEMP`, `HOT`,
`FREQ`, `POWER`, `FAN` and `VRAM` stay at `--` / `0`. The Windows Event Log may
show `rigstats-sensor.exe` crashing with:

```
System.AccessViolationException: Attempted to read or write protected memory.
   at LibreHardwareMonitor.Hardware.Gpu.AmdGpu.Update()
```

(or a crash in `atiadlxx.dll`).

**Cause:** an outdated or mismatched GPU driver. The core GPU sensors come from
AMD's ADL library (`atiadlxx.dll`) via LibreHardwareMonitor. Some AMD driver
builds return invalid data — or corrupt memory — on these calls. An
`AccessViolationException` is a native fault that **cannot be caught** from
managed .NET. The D3D values (3D load, VRAM used) keep working because they
come from Windows' D3D counters, not ADL. Known LibreHardwareMonitor issue:
[LibreHardwareMonitor#736](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/issues/736).

**Since 1.49:** LibreHardwareMonitor runs in a worker process of the service. A
crash there restarts the worker within seconds; sensors, fans and lighting keep
working. If the same graphics card crashes it three times, RIGStats stops
reading that card until Windows restarts and says so in **Status → Problems**
("… isn't read any more"). A laptop that switches its NVIDIA GPU off to save
power (Eco mode, #338) can do this.

**AMD, the service crashing on connect (October 2026):** Windows Update
replaced the Adrenalin driver with an older one (*"AMD Display Driver Update
(32.0.21042.62)"*), so the driver's parts no longer matched. AMD's ADLX then
crashed the service when asked about the card for the GPU power limit, each
time RIGStats connected; Windows restarted the service a few times and gave up,
and meanwhile the installer couldn't replace `rigstats-sensor.exe`. In the
service log: `An earlier run crashed … at
SensorSidecar.Control.Providers.AdlxGpuPower.System()`; in Windows'
Application log, *Application Error* 1000 for `rigstats-sensor.exe` with
exception code `0xc0000005`. From 1.48.1 the graphics-driver calls run in a
separate process (#370), so a crashing driver can't stop the service — the
Control Center's **Graphics card** page says "The graphics driver crashed". The
diagnostics ZIP's `driver-updates.txt` lists the drivers Windows Update
installed, `gpu-worker.txt` why the library was turned off; after a crash the
library is tried again once Windows restarts or the driver changes, after a
hang 10 minutes later.

## Control Center Details

- **Which features this PC has, and why one is off:** `control-capabilities.json`
  in the diagnostics ZIP.
- **Armoury Crate's fan service:** RIGStats checks for `AsusFanControlService`.
  Stopping it in `services.msc` works until the next restart. The service log's
  `Other hardware software running:` line shows what RIGStats found.
- **ASUS devices in "Device Lighting" mode** accept every command and ignore
  it; RIGStats can't tell, because the device doesn't report its colour back.
  Older firmware needs Gear Link's Companion app to change the mode.
- **Lighting devices found:** `lighting-devices.json` in the diagnostics ZIP.
- **Refused clients:** the service log
  (`%ProgramData%\se.codeby.rigstats\rigstats-sensor.log`) shows
  `Client rejected: …`.

## Wireless Battery Details

The service reads batteries every 20 s. Its log has a line starting with
`Battery:` for every device it found; for a Logitech receiver,
`Battery: Logitech … receiver` lists what is paired in each slot. A device
listed without a `Battery:` line with a level didn't answer (asleep or off) or
doesn't report a percentage. Charging comes from Logitech's HID++; Windows
doesn't expose charging for Bluetooth devices. Seen on the owner's rig: the
Bluetooth mice dropped constantly until the board's Wi-Fi/Bluetooth antenna
was connected.

## Desktop Wallpaper Mode Details

The wallpaper is drawn by a separate process (`rigstats-wallpaper.exe`), a
child of a WorkerW window, that reads settings from disk about once a second —
so changes appear on Save. `WS_EX_LAYERED` can't be applied to a WorkerW child,
so opacity has no effect. The position is saved as `wallpaperPosition`. If
Explorer restarts, the host process exits and the main app starts it again
within a second.

## Wallpaper Engine Details

RIGStats finds Wallpaper Engine through `HKCU\Software\WallpaperEngine\installPath`
and drives it with its [command line](https://help.wallpaperengine.io/en/functionality/cli.html).
Every change of state is in the app log (**Status → Log**, lines starting
`wallpaper engine:`):

- `not installed` / `found at …`
- `starting …`, then `running`; `couldn't start … — <reason>`;
  `started, but not running 30 s later`
- `not running (closed or crashed)`
- `'<command>' failed — <reason>` (for example `'openWallpaper' failed: no
  answer within 5 s`)
- `screen … isn't in its monitor map — skipped` (its `config.json`
  `monitormap`), or `couldn't read its config.json`
- `skipped screen N — … is gone`

Its command line has no clean way to quit Wallpaper Engine. The diagnostics
ZIP's `wallpaper-engine.txt` has where it's installed, whether it runs, its
screen map and how many wallpapers the library has (no titles).

## Display Selection

The dashboard targets the connected monitor whose resolution and orientation
best match the profile (a dedicated 1920×450 strip wins for `landscape-xl`, a
portrait side-strip for the `*-side` profiles), within ~10 %, otherwise the
primary monitor. The logic is `pick_window_rect_for_profile` /
`select_profile_monitor` in `src-egui/src/geometry.rs`.

## The Files In The Diagnostics ZIP

What the ZIP contains: [Diagnostics export](../README.md#diagnostics-export).

### `displays.json`

Every monitor Windows reports to the app — the same data used by
`geometry::pick_window_rect_for_profile()` — plus which one was picked for the
current dashboard profile.

- `dashboard_profile` — the profile active when diagnostics were collected
- `monitors[]` — one entry per connected display:
  - `left` / `top` / `right` / `bottom` — position in the virtual desktop, in
    logical (DPI-normalized) pixels
  - `width` / `height` — derived resolution
  - `is_primary` — `true` for the monitor at the virtual origin `(0, 0)`
  - `is_selected` — `true` on the monitor chosen for `dashboard_profile`

If the dashboard appears on the wrong monitor, compare the `is_selected`
entry's `width`/`height` with the profile's size — a profile only targets a
monitor within ~10 %; otherwise it falls back to the primary monitor.

### `sensor-tree.txt`

The full LHM hardware and sensor tree as seen by the service at its last start:
each hardware node's type, identifier and name; each sensor's type,
identifier, name and value. Use it to find the identifier of a sensor that
isn't picked up — for example, a DIMM temperature that reads in standalone LHM
but not in RIGStats — and compare it with the filter in
`sensor-sidecar/SensorReader.cs`. Overwritten on every service start.

### `sidecar-log.txt`

The `rigstats-sensor` service's own log: each start (`Listening on
\\.\pipe\rigstats-sensors`, then `The hardware worker has opened the
hardware.`), clients (`Client connected.` / `Client disconnected.`), lighting
devices found, profiles applied, wireless batteries, and `Stopped.`. It does
**not** contain sensor values.

Use it to tell whether the service runs and accepts a pipe connection at all.
If it never logs "Client connected", the Rust backend isn't reaching the pipe —
check `sidecar-service.txt` for the service state first.

**The hardware worker.** The service reads the hardware through a worker
process it restarts when a driver crashes or hangs it. `The hardware worker
ended during 'sample' (exit code 0xC0000005) after 35 s while reading
<hardware> (<id>) — starting it again …` is such a crash, with the hardware it
was reading; `did not answer 'sample' within 8 s` is a hang; `The hardware
worker is running again.` follows. After three crashes in the same hardware,
`<hardware> (<id>) crashed the hardware worker 3 times — it is no longer read
until Windows restarts`: only that hardware's values are missing, and Status
lists it under Problems. Lines starting `[hardware-worker]` or `[gpu-worker]`
are a worker's own output — after a crash, its stack.

**Crashes.** A crash of the service itself (a native access violation outside
the workers) ends it before it can log anything. At its next start it copies
what Windows recorded into the log: `An earlier run crashed at …` with the
stack, and `Crash dump written at …:
C:\ProgramData\se.codeby.rigstats\dumps\rigstats-sensor.exe.<pid>.dmp (… MB,
administrators only)`. The dump (a minidump; Windows keeps the newest 3) is
**not** in the ZIP: it holds the service's stack memory, so the folder is
readable by administrators only. If we ask for it, open the folder in Explorer,
confirm the administrator prompt, and attach the `.dmp` file named in the log.

When a specific sensor always shows `--` while the service runs and is
connected, the mismatch is in `sensor-sidecar/SensorReader.cs` (the C#
extraction) or `rigstats-backend/src/lhm.rs` (the Rust selection) — find the
sensor's identifier in `sensor-tree.txt` and compare it with both.

## Inspecting WMI Strings

The system brand is classified from six WMI fields, with product-line names
like `Alienware`, `Legion`, `OMEN`, `Predator` and `AORUS` taking priority over
the generic OEM name. To capture them on a machine:

```powershell
$cs = Get-CimInstance Win32_ComputerSystem
$csp = Get-CimInstance Win32_ComputerSystemProduct
$bb = Get-CimInstance Win32_BaseBoard

[pscustomobject]@{
  ComputerSystemManufacturer = $cs.Manufacturer
  ComputerSystemModel = $cs.Model
  ProductName = $csp.Name
  ProductVersion = $csp.Version
  BaseBoardManufacturer = $bb.Manufacturer
  BaseBoardProduct = $bb.Product
} | Format-List
```

## Intel And NVIDIA

CPU load and frequency come from `sysinfo` whatever the vendor. The service
reads NVIDIA cards too; if GPU sensor labels differ on a machine, adjust the GPU
sensor matching in `sensor-sidecar/SensorReader.cs`.

## Changing The UI

The dashboard is a native egui app — all UI is Rust code in `src-egui/src/`.
Edit and rebuild:

```powershell
cargo build --manifest-path src-egui/Cargo.toml
.\target\debug\rigstats.exe
```

Build a new installer with `cargo xtask build`.

## Ghost Entries In "Other System Tray Icons"

Windows stores one tray icon entry per exe path. Entries from dev builds,
renamed binaries or old Tauri installs linger after those exes are gone. Run
the cleanup script as the user that runs RIGStats (it asks for administrator
rights itself — it also cleans Installed Apps entries in HKLM whose
uninstaller is missing):

```powershell
pwsh -ExecutionPolicy Bypass -File tools\clean-tray-ghosts.ps1
```

[`tools/clean-tray-ghosts.ps1`](../tools/clean-tray-ghosts.ps1) finds every
RIGStats tray entry, shows what it keeps and deletes, asks for confirmation,
then restarts Explorer. Afterwards **Settings → Personalization → Taskbar →
Other system tray icons** should list one RIGStats entry.
