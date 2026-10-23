# Troubleshooting

Something not working? Find what you see below. Each section says what to do,
step by step.

## Start here

1. Open **Status** from the tray menu. Under **Problems** RIGStats lists
   everything it knows is wrong right now, what it means, and a button for the
   next step.
2. If that doesn't help, find your problem on this page.
3. Still stuck? [Report it](#report-a-problem) with a diagnostics file — it
   tells us what your PC looks like without you having to describe it.

**Jump to:**
[Values show `--`](#the-rigstats-service-isnt-answering-or-values-show---) ·
[CPU temperature missing](#cpu-temperature-or-package-power-missing) ·
[Graphics card values missing](#graphics-card-values-missing-or-the-service-keeps-stopping) ·
[Control Center](#control-center) ·
[Wireless batteries](#peripherals-wireless-batteries) ·
[A shortcut doesn't work](#a-shortcut-doesnt-work) ·
[Notifications](#notifications-dont-show) ·
[Wrong screen or size](#the-dashboard-is-on-the-wrong-screen-or-the-wrong-size) ·
[Desktop wallpaper mode](#desktop-wallpaper-mode) ·
[Wallpaper Engine](#wallpaper-engine) ·
[Intel and NVIDIA](#intel-and-nvidia) ·
[Display goes to sleep](#the-display-still-goes-to-sleep)

<a id="gpu-data-always-shows---"></a>

## The RIGStats Service Isn't Answering, Or Values Show `--`

**What you see:** the dashboard shows `--` instead of values, Status says
"The RIGStats service isn't answering", or the Control Center can't change
anything.

RIGStats reads your hardware through its own Windows service,
`rigstats-sensor`, which starts with Windows.

**What to do:**

1. Restart Windows. The service starts with it.
2. Still `--`? Open **Status**: the **Sensor service** row should say
   **Running**.
3. If it isn't running, install RIGStats again from
   [rigstats.app](https://rigstats.app/#download) — the installer sets the
   service up.
4. Only CPU temperature or power missing? See the next section.
5. Graphics card values missing, or the service keeps stopping? See
   [Graphics card values missing](#graphics-card-values-missing-or-the-service-keeps-stopping).

<a id="cpu-temperature--package-power-missing-or-defender-alert-on-service-start"></a>

## CPU Temperature Or Package Power Missing

**What you see:** CPU temperature shows `0` or `--`, and CPU power `0 W`.

These values are read through **PawnIO**, a signed driver the RIGStats
installer installs (since version 1.21; older versions used a driver that
Windows Defender could flag — update RIGStats if you see that). A PawnIO you
already had from another tool (FanControl, LibreHardwareMonitor, …) is kept,
and uninstalling RIGStats leaves PawnIO in place.

PawnIO installs only on **Windows 10 version 1809 (build 17763) or newer**.
On an older Windows the CPU values stay empty, and **Status** says so
("CPU temperature and power need a newer Windows"). Everything else works.
Update Windows to read the CPU too. To see your version, press **Win + R** and
run `winver` (#507).

**What to do:**

1. Install RIGStats again from [rigstats.app](https://rigstats.app/#download);
   the installer sets PawnIO up.
2. If it still shows `0`, install PawnIO yourself. Open **Terminal (Admin)** and
   run:

   ```powershell
   winget install namazso.PawnIO
   sc.exe stop rigstats-sensor; sc.exe start rigstats-sensor
   ```

3. Still missing? [Report it](#report-a-problem) with a diagnostics file.

<a id="gpu-sensors-missing-temp--clock--power--fan-or-sidecar-crashes"></a>
<a id="amd-graphics-the-sensor-service-keeps-stopping"></a>

## Graphics Card Values Missing, Or The Service Keeps Stopping

**What you see** — one or more of:

- The graphics card's load shows, but its temperature, clock, power, fan or
  memory stay at `--` or `0`.
- Status says a graphics card "isn't read any more", or the Control Center's
  **Graphics card** page says "The graphics driver crashed" / "stopped
  answering".
- On an AMD graphics card: the dashboard loses its values a few seconds after
  RIGStats starts, an update stops with *"Error opening file for writing:
  C:\Program Files\RIGStats\rigstats-sensor.exe"*, or AMD Software says
  **"Update Required"**.

- A small **amber dot** next to the card dots in the GPU panel: the card you
  picked in Settings isn't read right now, so another card is shown. Hover the
  dot to see which. On a laptop this is normal while the graphics card sleeps
  (Eco / iGPU-only mode). Once the card is read again, RIGStats shows it again
  by itself.

**Why:** almost always the graphics driver, not RIGStats. Some driver versions
give wrong answers or crash when asked about the card. A common cause on AMD:
Windows Update replaced the driver AMD Software installed with an older one
(seen in October 2026 with *"AMD Display Driver Update (32.0.21042.62)"*).

RIGStats keeps the rest working: since 1.49 a crash restarts only the part that
reads the hardware, and after three crashes on the same card it stops reading
that card until Windows or the RIGStats service restarts. The graphics card's power limit has run apart
from the service since 1.48.1.

**What to do:**

1. Install the latest driver from the maker — not from Windows Update:
   - AMD: open AMD Software and choose **Download & Install**, or get it from
     [amd.com](https://www.amd.com/en/support/download/drivers.html).
   - NVIDIA: [nvidia.com](https://www.nvidia.com/Download/index.aspx).
   - Install the latest chipset driver too.
2. Restart Windows.
3. If a RIGStats update had failed, run it again.
4. Still the same? [Report it](#report-a-problem) with a diagnostics file — it
   shows which drivers Windows Update installed.

**Stop Windows from replacing drivers:** Settings → System → About → Advanced
system settings → Hardware → Device Installation Settings → **No**. Security
updates still install.

## Control Center

- **A page is missing (Fans, CPU, Graphics card, Lighting…)** — a page only
  appears when this PC's hardware supports it: fan control the board allows, an
  AMD Ryzen 9000 processor, an AMD Radeon or desktop NVIDIA graphics card, a
  supported lighting device. Laptop graphics cards have no Graphics card page;
  the laptop's own performance modes decide their power.
- **"Armoury Crate controls the fans"** — Armoury Crate's fan service is
  running, so RIGStats leaves the fans to it instead of two programs fighting
  over every fan. Everything else in a profile still applies.
  1. Remove Armoury Crate with ASUS's
     [Armoury Crate Uninstall Tool](https://rog.asus.com/support/FAQ/1041654)
     (the **ASUS uninstall tool** button in RIGStats). Reinstalling Armoury
     Crate brings it back.
  2. Open the Control Center again.

  Never used Armoury Crate's fan control, or the fans still fight afterwards?
  [Report it](#report-a-problem).
- **"… controls the lighting"** — Armoury Crate or OpenRGB is running and owns
  the lighting. Close it, or turn its lighting off, then open the Control
  Center again. A device "controlled by Windows Dynamic Lighting" is left alone
  until you turn that off in Windows: Settings → Personalization → Dynamic
  Lighting.
- **An ASUS mouse or keyboard is listed but doesn't change colour** — the
  device has its own setting that decides whether it follows lighting from
  other programs.
  - ROG Harpe Ace Aim Lab Edition, Keris II Ace, Harpe Ace Mini and Harpe Ace
    Extreme on the ROG Omni receiver: the Lighting page offers **Let RIGStats
    control it**.
  - Other devices: open [Gear Link](https://gearlink.asus.com) or Armoury
    Crate and set the device's **Cross-device Lighting Toggle** to **Aura Sync
    & Windows Dynamic Lighting**. The setting is kept in the device, so you can
    close or uninstall Gear Link / Armoury Crate afterwards.
  - Leave Windows' own "Use Dynamic Lighting on my devices" off, or RIGStats
    leaves the device to Windows.
- **A lighting device isn't listed** — check the
  [supported devices](supported-devices.md). A device plugged in or switched on
  later is found when the Control Center opens. Still missing? Send a
  [device report](https://github.com/dvalfrid/rigstats/issues/new?template=device.yml)
  with a diagnostics file — the same form confirms a device marked "not yet
  verified" that works for you.
- **"The RIGStats service refused this app" / changes don't apply** — only the
  installed RIGStats may control fans, power and lighting; a copied or
  self-built `rigstats.exe` is refused on purpose. Start RIGStats from the Start
  menu, or install it again from [rigstats.app](https://rigstats.app/#download).
- **"The graphics driver crashed" / "stopped answering"** — see
  [Graphics card values missing](#graphics-card-values-missing-or-the-service-keeps-stopping).
  The rest of your profiles (fans, CPU, lighting) keeps applying meanwhile.
- **A CPU limit or Curve Optimizer change was undone after a restart** — if
  Windows goes down shortly after such a change, RIGStats doesn't apply it again
  at the next start, and the CPU page says so. Choose a milder value.

## Peripherals (Wireless Batteries)

The PERIPHERALS panel (turn it on under **Control Center → Dashboard →
Panels**) and the card over the tray icon show the battery of wireless devices
([which ones](supported-devices.md#battery)). They update every 20 seconds.

- **A device shows greyed, "Not connected"** — it is off, asleep, out of range,
  or a 2.4 GHz device moved to its cable. Its last level stays in grey for 10
  minutes, then it disappears until it answers again. A greyed device raises no
  low-battery alert, since its level is old.
- **A Bluetooth device doesn't show** — RIGStats shows what Windows has. If
  Windows' Settings → Bluetooth & devices shows no percentage for it, the device
  doesn't report its battery (common for older Bluetooth mice and keyboards). A
  device that was off when Windows started appears once it connects.
- **No charging shown for a Bluetooth device** — Windows doesn't tell programs
  whether a Bluetooth device is charging. RIGStats asks Logitech devices
  themselves; other brands show only the percentage.
- **A Logitech device on a receiver doesn't show** — check that its receiver is
  in [the list](supported-devices.md#battery). A device that is asleep or off
  doesn't answer until it wakes.
- **Bluetooth devices keep disconnecting** — usually the signal, not RIGStats.
  On a desktop PC, the Wi-Fi/Bluetooth antenna must be screwed onto **both**
  connectors on the back panel; without it Bluetooth reaches only a metre or
  two. Stand the antenna on the desk, not behind the case, and away from USB 3
  hubs.
- **A ROG device doesn't show or keeps its old level** — see "A lighting device
  isn't listed" under [Control Center](#control-center); its battery is read
  the same way.

## A Shortcut Doesn't Work

RIGStats' shortcuts are **Ctrl+Alt+O** (overlay), **Ctrl+Alt+P** (next
profile), **Ctrl+Alt+F1**…**F9** (a profile) and **Ctrl+Alt+M** (microphone).
Windows gives each shortcut to the program that asked first, so another program
can hold one. Since 1.51, **Status → Problems** says which one is taken
("A shortcut is used by another program"). The rest keep working.

**What to do:** change or turn off the shortcut in the other program, then
restart RIGStats. A common one: **NVIDIA App** uses Ctrl+Alt+M for its own
microphone switch. RIGStats' shortcuts can't be changed yet (#569); the tray
menu does the same things.

## Notifications Don't Show

Alerts and other notifications come from **RIGStats** (with its icon), and
RIGStats is listed under Windows **Settings → System → Notifications**.

- **Turned off in Windows** — RIGStats says so under **Settings →
  Notifications** and in **Status**. **Open Windows settings** takes you to the
  right page (not shown when your organisation's policy decides — ask whoever
  manages the PC).
- **Do Not Disturb is on** — Windows holds notifications back meanwhile; they
  wait in the notification centre (Win+N). RIGStats can't tell, so it won't
  mention it.
- **Not even the test shows** — **Settings → Notifications → Send Test** shows a
  sample. If it doesn't appear and RIGStats reports nothing turned off, check
  Do Not Disturb, then [report it](#report-a-problem).
- **An alert you expected didn't come** — check that the kind is on (**Settings
  → Notifications**: Critical alerts / Warnings) and its level in the profile
  (**Control Center → Alerts**). The same alert repeats at most once per
  "Repeat at most every".

<a id="can-i-change-which-display-is-used"></a>
<a id="can-i-switch-dashboard-size-manually"></a>

## The Dashboard Is On The Wrong Screen, Or The Wrong Size

Open **Settings → Display** and pick a **Display profile** that matches the
screen you want. RIGStats places the dashboard on the connected screen whose
resolution and orientation fit the profile best, otherwise on the main screen.
Save to apply.

- When you switch profiles, the dashboard stays where it is if that spot is
  still on a connected screen.
- **Fill screen** fills the height of the screen the dashboard is on; it never
  moves it to another screen.

## Desktop Wallpaper Mode

**Settings → Display → Window layer → Desktop wallpaper** draws the dashboard
as part of the desktop background, under your icons. It stays when you press
Win+D and is never covered by other windows.

These are expected in this mode, not faults:

- **Changes show when you click Save**, not while you try them — Settings
  reminds you with a banner.
- **The display profile can't be changed here.** Switch the window layer to
  another option, pick the profile, then go back to Desktop wallpaper and Save.
- **Opacity has no effect** — the dashboard is solid over the background.
- **Where it sits:** where the window was when you switched to Desktop
  wallpaper. Move it into place *before* you switch.

If Explorer restarts, the dashboard comes back by itself within a second.

## Wallpaper Engine

The Control Center's **Desktop** page can switch
[Wallpaper Engine](https://www.wallpaperengine.io/) per profile: leave it alone,
show a wallpaper on each screen, pause it, or turn it off. **Status** has a
Wallpaper Engine row.

- **"Not installed"** — install Wallpaper Engine (**Get it on Steam** on the
  page), then start it once so RIGStats can find it.
- **"Not running" / it doesn't start** — click **Start** on the Desktop page.
  If it hasn't started after 30 seconds, start it once by itself: it may be
  waiting for an answer (an update, or a question after a crash).
- **It crashed or was closed** — the page shows **Not running** within about 5
  seconds. Start it again and the profile's wallpapers come back. If it keeps
  crashing, try another wallpaper, or
  [Wallpaper Engine's help](https://help.wallpaperengine.io).
- **Back to the Windows background** — **Stop** on the Desktop page. Wallpaper
  Engine stays in the tray with your wallpapers kept; **Start** brings them
  back. To quit Wallpaper Engine itself, use its tray icon.
- **A red "Last problem" on the page** — the last command to Wallpaper Engine
  failed; it clears when the next one works.
- **"Doesn't know this screen yet"** — open Wallpaper Engine once with that
  screen connected, so it learns about it.
- **"No longer in your library"** — the wallpaper was unsubscribed or deleted.
  Choose another one.

<a id="intel-and-nvidia-support"></a>

## Intel And NVIDIA

- Sensors, fan curves, power plans and lighting work whatever your processor
  and graphics card brand.
- CPU power limits and Curve Optimizer are for AMD Ryzen only for now (Intel:
  [#209](https://github.com/dvalfrid/rigstats/issues/209)).
- The graphics card power limit covers AMD Radeon and desktop NVIDIA GeForce
  cards. Laptop graphics cards follow the laptop's own performance modes (ASUS
  laptops: [#234](https://github.com/dvalfrid/rigstats/issues/234)).

<a id="display-still-goes-to-sleep"></a>

## The Display Still Goes To Sleep

RIGStats doesn't keep the display awake. Use Windows' power settings (Settings
→ System → Power & battery) or the monitor's own menu.

<a id="how-do-i-report-a-bug-or-missing-sensor-support"></a>

## Report A Problem

1. Open **Status** from the tray menu and click **Collect diagnostics…**.
2. Choose where to save the ZIP file. Nothing is sent anywhere — the file is
   only written where you choose.
3. [Open an issue on GitHub](https://github.com/dvalfrid/rigstats/issues/new/choose)
   describing what you see, and attach the ZIP.

What the ZIP contains: [Diagnostics export](../README.md#diagnostics-export).

---

Developers: log lines, the files in the diagnostics ZIP, PawnIO from source and
more are in [developer troubleshooting](developer-troubleshooting.md).
