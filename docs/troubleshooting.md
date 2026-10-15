# Troubleshooting

## CPU Temperature / Package Power Missing, Or Defender Alert On Service Start

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

**Fix (and the dev-environment setup when running from source):** install
PawnIO once from an elevated prompt, then restart the sensor service:

```powershell
winget install namazso.PawnIO
sc.exe stop rigstats-sensor; sc.exe start rigstats-sensor
```

## GPU Data Always Shows `--`

Make sure the `rigstats-sensor` Windows Service is running:

```powershell
sc.exe query rigstats-sensor
```

`STATE: 4 RUNNING` means the sidecar is active. If it is stopped or missing,
the installer may not have completed successfully — check `install.log` in the
diagnostics ZIP (Status → Collect Diagnostics).

## GPU Sensors Missing (Temp / Clock / Power / Fan) Or Sidecar Crashes

**Symptom:** The GPU panel shows core load and D3D 3D usage, but `TEMP`, `HOT`,
`FREQ`, `POWER`, `FAN` and `VRAM` stay at `--` / `0`. The Windows Event Log may
also show `rigstats-sensor.exe` crashing with:

```
System.AccessViolationException: Attempted to read or write protected memory.
   at LibreHardwareMonitor.Hardware.Gpu.AmdGpu.Update()
```

(or a crash in `atiadlxx.dll`).

**Cause:** This is an **outdated GPU driver**, not a RIGStats bug. The core GPU
sensors (temp, clock, power, fan) come from AMD's ADL library (`atiadlxx.dll`)
via LibreHardwareMonitor. Some AMD driver builds return invalid data — or
corrupt memory — on these ADL calls, which both hides the sensors and can crash
the sidecar. The `AccessViolationException` is a native-side fault that **cannot
be caught** from managed .NET, so the whole service dies until it auto-restarts.
The D3D values (3D load, VRAM-used) keep working because they come from Windows
D3D counters, not ADL.

This is a known LibreHardwareMonitor issue (see
[LibreHardwareMonitor#736](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/issues/736)).
The same RIGStats build on identical hardware works perfectly with a newer
driver — the GPU read path (`sensor-sidecar/` + `LibreHardwareMonitorLib`) has
not changed across recent app versions.

**Fix:** Update the GPU **and** chipset drivers to the latest versions:

- AMD: install the latest [AMD Software: Adrenalin Edition](https://www.amd.com/en/support).
- Note that Windows Update can silently install an older driver build; always
  prefer the vendor's latest package.

After updating, restart the `rigstats-sensor` service (or reboot) and the GPU
sensors should populate.

## Desktop Wallpaper Mode

Set **Settings → Display → Window Layer → Desktop Wallpaper** to render the
dashboard as a live wallpaper, between the desktop background and the icons. It
survives `Win+D`, is never covered by other windows, and coexists with Wallpaper
Engine / Lively.

A few behaviours are specific to this mode and are **expected**, not bugs:

- **Settings apply on Save, not as a live preview.** The wallpaper is drawn by a
  separate background process (`rigstats-wallpaper.exe`) that reads settings from
  disk about once a second, so theme/panel/threshold changes only appear after you
  press **Save**. Settings shows an amber banner reminding you of this.
- **Controls with no live effect are greyed out** while the applied layer is
  Desktop Wallpaper, and the **Display Profile selector is locked**. To change the
  profile, switch the Window Layer back to a non-wallpaper option (Normal / Always
  on Top / Always Behind), pick the profile, then return to Desktop Wallpaper and
  Save.
- **Window opacity has no effect** in wallpaper mode. `WS_EX_LAYERED` cannot be
  applied to a WorkerW child window, so the dashboard is opaque over the wallpaper.
- **The dashboard position** is the screen position the window had when you
  switched into wallpaper mode (saved as `wallpaperPosition`). Place the window
  where you want it *before* switching to Desktop Wallpaper.

If the dashboard disappears after Explorer restarts, the host process exits and is
respawned automatically by the main app within a second.

## Can I Change Which Display Is Used?

Yes — open Settings and pick a **Display Profile** that matches the target screen.
The dashboard auto-targets the connected monitor whose resolution and orientation
best match the profile (a dedicated 1920×450 strip wins for `landscape-xl`, a
portrait side-strip wins for the `*-side` profiles, etc.), and otherwise falls
back to the primary monitor — it no longer lands on an arbitrary small screen.

When you switch profiles the window keeps its current screen position if that spot
is still on a connected monitor; only when the saved position is off-screen does it
re-target the matching monitor. **Fill Screen** fills the height of the monitor the
window currently sits on, so enabling it never moves the dashboard to another
screen.

The low-level targeting logic lives in `pick_window_rect_for_profile` /
`select_profile_monitor` in `src-egui/src/geometry.rs`.

## Can I Switch Dashboard Size Manually?

Yes. Open Settings and change Display Profile. Save to apply immediately and persist the choice.

## Control Center

- **A tab is missing** (Fans, CPU, GPU, Lighting): each tab appears only when this PC's hardware supports it — writable fan control, an AMD Ryzen 9000 on a verified SMU table, an AMD Radeon, a supported lighting device. The diagnostics ZIP's `control-capabilities.json` says what was found and why a feature is unavailable.
- **"… controls the lighting"**: Armoury Crate (its LightingService) or OpenRGB is running and owns the lighting; close it, or turn its lighting off, then reopen the Control Center. A device marked as controlled by **Windows Dynamic Lighting** is skipped until you turn that off in Settings → Personalization → Dynamic Lighting.
- **An ASUS mouse or keyboard is listed but doesn't change colour**: the device itself has a lighting mode setting, saved in the device, that decides whether it follows lighting commands from outside. For the mice that have it on the ROG Omni receiver (ROG Harpe Ace Aim Lab Edition, Keris II Ace, Harpe Ace Mini, Harpe Ace Extreme), the Lighting tab reads the setting and offers **Let RIGStats control it**. Newer ROG mice have no such setting. For other devices, open [Gear Link](https://gearlink.asus.com) (older firmware needs its Gear Link Companion app) or Armoury Crate and set the device's **Cross-device Lighting Toggle** to **Aura Sync & Windows Dynamic Lighting**. In "Device Lighting" mode it accepts every command and ignores them — RIGStats can't tell, because the device doesn't report its colour back. The setting stays in the device, so Gear Link / Armoury Crate can be closed or uninstalled afterwards. Leave Windows' own "Use Dynamic Lighting on my devices" off, or RIGStats leaves the device to Windows.
- **A lighting device isn't listed**: check [supported-devices.md](supported-devices.md). Devices plugged in or switched on later are found when the Control Center opens. If it still doesn't show, open a [lighting device report](https://github.com/dvalfrid/rigstats/issues/new?template=lighting_device.yml) with the diagnostics ZIP (`lighting-devices.json`) — the same form confirms a device marked "not yet verified" that works for you.
- **"Not connected" / changes don't apply**: the `rigstats-sensor` service must be running (`sc query rigstats-sensor`). Only the installed, signed `rigstats.exe` may control hardware — a copied or self-built `rigstats.exe` is refused by design; `%ProgramData%\se.codeby.rigstats\rigstats-sensor.log` then shows `Client rejected: …`.
- **A CPU limit or Curve Optimizer change was undone after a restart**: if Windows goes down shortly after such a change, it isn't re-applied at the next start, and the CPU tab says so — choose a milder value.

## Peripherals (Wireless Batteries)

The PERIPHERALS panel (switch it on under **Control Center → Dashboard → Panels**) and the hover card over the tray icon show the battery of wireless devices: ROG headsets, keyboards and mice the service drives for lighting, and any Bluetooth device whose battery Windows knows (the percentage in **Settings → Bluetooth & devices**). Logitech devices on Bluetooth also show charging. The service reads every 20 s; its log (`%ProgramData%\se.codeby.rigstats\rigstats-sensor.log`) has a line starting with `Battery:` for every device it found.

- **A device shows greyed, "Not connected"**: it disconnected or stopped answering (switched off, asleep, out of range, or a 2.4 GHz device moved to its cable). The panel keeps its last level in grey for 10 minutes so a device that sleeps and wakes doesn't flicker in and out, then drops it; it comes back at the next read once it answers. A greyed device raises no low-battery alert, since its level is old.
- **A Bluetooth device doesn't show**: RIGStats shows what Windows has. If Settings → Bluetooth & devices shows no percentage for it, the device doesn't report its battery over Bluetooth (common for older "classic" Bluetooth mice and keyboards). A device that was already off when the service started appears once it connects.
- **No charging shown for a Bluetooth device**: Windows doesn't expose charging for Bluetooth devices. RIGStats asks Logitech devices themselves (HID++); other brands show only the percentage.
- **A Logitech device on a Bolt, Unifying or Lightspeed receiver doesn't show**: receivers aren't supported yet ([#349](https://github.com/dvalfrid/rigstats/issues/349)). Over Bluetooth the same device works.
- **Bluetooth devices keep disconnecting**: usually the signal, not RIGStats. A desktop board's Wi-Fi/Bluetooth module (for example ASUS boards with "AX" or "WIFI" in the name) needs its antenna screwed onto **both** connectors on the back panel; without it, Bluetooth reaches only a metre or two and drops in and out. Stand the antenna on the desk, not behind the case, and away from USB 3 hubs. Seen on the owner's rig: the mice dropped constantly until the antenna was connected.
- **A ROG device doesn't show or keeps its old level**: see "A lighting device isn't listed" under [Control Center](#control-center); its battery is read through the same connection.

## AMD Graphics: The Sensor Service Keeps Stopping

**Symptoms:** the dashboard loses its sensors and the Control Center says "Connecting" a few seconds after RIGStats starts; a RIGStats update stops with *"Error opening file for writing: C:\Program Files\RIGStats\rigstats-sensor.exe"*; AMD Software says **"Update Required"** when opened.

**Cause:** Windows Update replaced the driver AMD Software (Adrenalin) installed with an older one, so the driver's parts no longer match. AMD's driver library (ADLX) then crashes the service when it is asked about the graphics card (for the GPU power limit), each time RIGStats connects. Windows restarts the service a few times and gives up; while it is restarting, the installer can't replace its file. In the log (`%ProgramData%\se.codeby.rigstats\rigstats-sensor.log`): `An earlier run crashed … at SensorSidecar.Control.Providers.AdlxGpuPower.System()`; in Windows' Application log, *Application Error* 1000 for `rigstats-sensor.exe` with exception code `0xc0000005`. Seen in October 2026 after Windows Update installed *"AMD Display Driver Update (32.0.21042.62)"* over a newer Adrenalin driver.

**Fix:**
1. Open AMD Software and choose **Download & Install**, or install the latest driver from [amd.com](https://www.amd.com/en/support/download/drivers.html). Restart if it asks.
2. Restart Windows, or start the service from an administrator terminal: `sc start rigstats-sensor`.
3. Run a failed RIGStats update again.

**Prevent it:** *Settings → System → About → Advanced system settings → Hardware → Device Installation Settings → No* stops Windows from replacing drivers on its own; security updates still install.

A coming version runs the graphics-driver calls in a separate process, so a crashing driver can't stop the service ([#370](https://github.com/dvalfrid/rigstats/issues/370)).

## Wallpaper Engine

The Control Center's **Desktop** page can switch [Wallpaper Engine](https://www.wallpaperengine.io/) per profile: leave it alone, show a wallpaper on each screen, pause it (the frame freezes), or turn it **Off** for that profile (the Windows background shows, and the profiles that use it start it again). RIGStats doesn't draw live wallpapers itself; it sends Wallpaper Engine the commands of its [command line](https://help.wallpaperengine.io/en/functionality/cli.html). Every change of state is written to the log (**Status → Log**, lines starting with `wallpaper engine:`), and **Status → Health** has a Wallpaper Engine row.

- **The page says "Not installed"**: RIGStats finds Wallpaper Engine through the path it registers for itself (`HKCU\Software\WallpaperEngine\installPath`). **Get it on Steam** opens its store page (in the Steam client when Steam is installed). After installing, start it once so it registers. The log says `wallpaper engine: not installed`, or `found at …` once it's there.
- **"Not running" / it doesn't start**: click **Start** on the Desktop page, or start it from the Start menu. The profile's choice applies as soon as it runs; until then nothing is sent. The log says `wallpaper engine: starting …`, then `running` within a few seconds. If it can't be started, the page shows the reason in red under **Last problem**, and the log has `wallpaper engine: couldn't start … — <reason>`. If it hasn't come up 30 seconds after **Start**, the page says so under **Last problem** (log: `started, but not running 30 s later`); start it once by itself: Wallpaper Engine may be waiting for an answer (an update, or a question after a crash).
- **It crashed or was closed**: within about 5 seconds the log says `wallpaper engine: not running (closed or crashed)` and the page shows **Not running**. Start it again and the active profile's wallpapers come back. If it keeps crashing, the cause is in Wallpaper Engine or the wallpaper itself: try another wallpaper, or Wallpaper Engine's own help (help.wallpaperengine.io).
- **Back to the Windows background without closing it**: **Stop** on the Desktop page stops its wallpapers. The Windows background shows, and Wallpaper Engine stays in the tray with its wallpapers kept; **Start** brings them back. (Quitting Wallpaper Engine itself is done from its tray icon; its command line has no clean way to quit it.)
- **A command failed**: the page shows **Last problem** (for example `'openWallpaper' failed: no answer within 5 s`), and the log has `wallpaper engine: '<command>' failed — <reason>`. It clears when the next command succeeds.
- **A screen can't get a wallpaper ("doesn't know this screen yet")**: Wallpaper Engine numbers the screens itself and keeps the list in its `config.json` (`monitormap`). A screen it hasn't seen yet (new, or never shown a wallpaper) isn't in it. Open Wallpaper Engine once with that screen connected. The log says `screen … isn't in its monitor map — skipped`, or `couldn't read its config.json` if the file can't be read at all.
- **"No longer in your library"**: the wallpaper was unsubscribed or deleted. Choose another one; until then nothing is sent for that screen (log: `skipped screen N — … is gone`).
- **Reporting a problem**: the diagnostics ZIP (**Status → Collect Diagnostics**) has `wallpaper-engine.txt`: where it's installed, whether it runs, its screen map and how many wallpapers the library has (no titles).

## Intel And NVIDIA Support

CPU data comes from `sysinfo` regardless of vendor.

For NVIDIA GPUs, the sidecar works as well. If labels differ on your machine, adjust the GPU sensor matching in `sensor-sidecar/SensorReader.cs`.

Control Center: fan curves, power plans and lighting work regardless of CPU/GPU vendor; CPU power limits and Curve Optimizer are AMD Ryzen only for now (Intel: [#209](https://github.com/dvalfrid/rigstats/issues/209)), and the GPU power limit covers AMD Radeon and desktop NVIDIA GeForce cards. Laptop GPUs don't get a GPU tab: their power budget follows the laptop's own performance modes (ASUS laptops: [#234](https://github.com/dvalfrid/rigstats/issues/234)).

## How Do I Inspect Real WMI Strings?

Use PowerShell and capture these values from the actual machine:

```powershell
Get-CimInstance Win32_ComputerSystem |
  Select-Object Manufacturer, Model |
  Format-List

Get-CimInstance Win32_ComputerSystemProduct |
  Select-Object Name, Version |
  Format-List

Get-CimInstance Win32_BaseBoard |
  Select-Object Manufacturer, Product |
  Format-List
```

If you want one copy-paste friendly block for support/debugging, run:

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

Those six fields are the ones RIGStats now uses to classify the system brand, with product-line names like `Alienware`, `Legion`, `OMEN`, `Predator`, and `AORUS` taking priority over the generic OEM name.

## How Do I Report A Bug Or Missing Sensor Support?

Use the **Status dialog → Collect Diagnostics…** button.

It opens a native Windows save dialog. Pick a location and a ZIP file is written immediately.
No data is sent automatically — the file is written only to the path you choose.
Share it by email or attach it to a GitHub issue.

See [Diagnostics Export](../README.md#diagnostics-export) in the README for a full description of what the ZIP contains.

### What To Look For In `displays.json`

The file lists every monitor Windows reports to the app — the same data used by
`geometry::pick_window_rect_for_profile()` — plus which one was picked for the
current dashboard profile.

Top-level fields:

- `dashboard_profile` — the profile active when diagnostics were collected
- `monitors[]` — one entry per connected display:
  - `left` / `top` / `right` / `bottom` — position in the virtual desktop
    coordinate space, in logical (DPI-normalized) pixels
  - `width` / `height` — derived resolution
  - `is_primary` — `true` for the monitor at the virtual origin `(0, 0)`
  - `is_selected` — `true` on the monitor that was actually chosen for
    `dashboard_profile`

If the dashboard appears on the wrong monitor, check which entry has
`is_selected: true` and compare its `width`/`height` against the profile's
expected size — a profile only targets a monitor whose resolution matches
within ~10%; otherwise it falls back to the primary monitor.

### What To Look For In `sensor-tree.txt`

The file contains the full LHM hardware and sensor tree as seen by the sidecar at the last service start.
Each hardware node shows its type, identifier, and name; each sensor shows its type, identifier, name, and current value.

Use this file to identify the exact identifier path for a sensor that isn't being picked up.
For example, if a DIMM temperature reads correctly in standalone LHM but not in RIGStats, find the sensor here and compare its identifier against the filter in `sensor-sidecar/SensorReader.cs`.

The file is overwritten on every service start, so it always reflects the current hardware configuration.

### What To Look For In `sidecar-log.txt`

The file is the raw log written by the `rigstats-sensor` Windows Service
itself — lifecycle events only: `Hardware opened. Listening on
\\.\pipe\rigstats-sensors`, `Client connected.` / `Client disconnected.`, and
`Stopped.`. It does **not** contain parsed sensor values.

Use it to tell whether the sidecar is running and accepting a pipe
connection at all. If the service never logs "Client connected", the Rust
backend isn't reaching the pipe — check `sidecar-service.txt` for the service
state first.

**Crashes.** A crash inside a driver call (a native access violation) ends
the service before it can log anything itself. At its next start it copies
what Windows recorded into the log: `An earlier run crashed at …` with the
stack, and `Crash dump written at …: C:\ProgramData\se.codeby.rigstats\dumps\rigstats-sensor.exe.<pid>.dmp (… MB, administrators only)`.
The dump (a minidump; Windows keeps the newest 3) is **not** in the ZIP: it
holds the service's stack memory, so the folder is readable by
administrators only. If we ask for it, open the folder in Explorer, confirm
the administrator prompt, and attach the `.dmp` file named in the log.

When a specific sensor always shows `--` in the dashboard (the sidecar itself
is clearly running and connected), the mismatch is in
`sensor-sidecar/SensorReader.cs` (the C# extraction logic) or
`rigstats-backend/src/lhm.rs` (the Rust selection logic) — use
`sensor-tree.txt` (below) to find the exact sensor identifier and compare it
against both.

## How Do I Change The UI?

The dashboard is a native egui app — all UI is Rust code in `src-egui/src/`. Edit the relevant file and rebuild:

```powershell
cargo build --manifest-path src-egui/Cargo.toml
.\target\debug\rigstats.exe
```

Build a new installer after:

```powershell
cargo xtask build
```

## Ghost Entries in "Other System Tray Icons"

Windows stores one tray icon entry per unique exe path. Old entries from dev builds, renamed binaries, or previous Tauri installs linger even after those exes are gone.

Run the cleanup script (no elevation needed — run as the same user that runs RIGStats):

```powershell
pwsh -ExecutionPolicy Bypass -File tools\clean-tray-ghosts.ps1
```

The script ([`tools/clean-tray-ghosts.ps1`](../tools/clean-tray-ghosts.ps1)) finds all RIGStats-related tray entries, shows what it will keep vs delete, asks for confirmation, then restarts Explorer.

After it runs, check **Settings → Personalization → Taskbar → Other system tray icons** — only one RIGStats entry should remain.

## Display Still Goes To Sleep

Display sleep blocking is not currently implemented in the app.

Use Windows power settings or the monitor OSD if you need the display to stay awake.
