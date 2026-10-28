# ASUS laptops: performance mode, fan curves and power limits through ATKACPI

Research notes for #234 (Control Center: ASUS laptops), 2026-10-27.

**Built so far:** step 1 of the live-test plan only. `AsusAcpi`
(`sensor-sidecar/Control/Providers/AsusAcpi.cs`) opens `\\.\ATKACPI` once at
start and logs `DSTS` (read) on the ids listed there. It never sends `DEVS`
or `INIT`, and the list is pinned by `AsusAcpiTests`. Nothing has been sent
to a real laptop yet. Everything below was read from primary sources — the
Linux `asus-wmi` and `asus-armoury` drivers and G-Helper's source and wiki —
or from files already in this repository. G-Helper (GPL-3.0) and the Linux
drivers (GPL-2.0) are read as documentation of an interface: device ids,
buffer layout and semantics. No code is copied. Every claim is marked:

- **verified** — with the source (URL, or file and line) it was read from;
- **unconfirmed** — the sources don't settle it;
- **hypothesis** — a reasoned guess, with what would confirm or rule it out.

Scope: ROG/TUF/Vivobook laptops that have the ASUS System Control Interface
driver. Three phases, as in #234: A = performance mode, B = fan curves,
C = CPU power limits (SPL/sPPT/fPPT) and NVIDIA Dynamic Boost / temperature
target. Out of scope: GPU Eco/MUX, APU memory, battery charge limit, keyboard
lighting, CPU core counts, undervolting (the same ATKACPI channel carries
them, and some of them are dangerous; see "Risks").

## Summary

1. **The interface is small and well documented.** One device
   (`\\.\ATKACPI`), one IOCTL (`0x0022240C`), two methods: `DSTS` (get,
   `0x53545344`) and `DEVS` (set, `0x53564544`), each taking a 32-bit device
   id and a 32-bit argument (or a byte payload for fan curves). A device id
   is supported when its `DSTS` result has the presence bit `0x00010000`.
   G-Helper and Linux agree on all of it. **Verified.**
2. **Support is detected with a documented read of the same id, never a
   sweep.** Linux probes every id it uses with `DSTS` at boot, on every ASUS
   laptop that runs it; G-Helper does the same (`IsSupported`). RIGStats can
   do exactly that for the ids in this document and nothing else. G-Helper
   also has a `DSTS` sweep over `0x00000000`–`0x001600FF` behind a button
   (`ScanRange`); RIGStats must never do that. **Verified.**
3. **Phase A needs one id: `0x00120075`** (ROG/TUF "throttle thermal
   policy": 0 Balanced, 1 Turbo, 2 Silent), with `0x00110019` (Vivobook: 0
   Balanced, 1 Silent, 2 Turbo) as the alternative when the first is absent.
   **Verified** (Linux and G-Helper). Whether `DSTS` on it returns the
   *current* mode is **unconfirmed** — neither Linux nor G-Helper ever reads
   it back; both keep the mode in software and Linux writes Balanced at
   probe. So "restore the mode that was active before RIGStats" may not be
   possible; the way back is Balanced until the read-only test shows
   otherwise. **On the GA403WR it doesn't (verified, step 1):** the word is
   `0x00010003` in Balanced, Silent and Turbo alike, so the way back there is
   Balanced.
4. **Writing the mode again is the way back for fans and limits.** Linux:
   "For machines with throttle this is the only way to reset fans to default
   mode of operation"; G-Helper's *Factory defaults* is the same mode write.
   A mode change also drops any custom fan curve, so curves must be written
   after the mode, every time. **Verified** (both sources). One known
   exception: G14 2024 (GA403UI/UU/UV) needs a switch to *another* mode first
   to reset its power limits (G-Helper workaround). **Verified** for those
   models; for the owner's GA403WR **unconfirmed**.
5. **Fan curves are 8 temperature + 8 duty bytes, evaluated by the EC.**
   `DSTS` on the curve id with the mode as argument returns the firmware
   (factory) curve of that mode; `DEVS` writes a curve to the *active* mode.
   The custom curve can't be read back through `DSTS` — **verified** on the
   GA403WR (step 1): the reads gave the same curves in every run although
   G-Helper had written its own curves before each.
6. **Power-limit ranges are not reported by the firmware.** Neither source
   reads a min, max or default from ACPI; Linux keeps a per-model table
   (`asus-armoury.h`, by DMI board name) and G-Helper a few per-model maxima.
   So phase C has to carry a model table, unlike #234's "capabilities from
   what the firmware reports": presence from the firmware, range from the
   table, not offered on a model not in the table. **Verified** (absence in
   both sources).
7. **The GA403WR is in Linux's table** (AC: SPL 15–80 W, sPPT 25–80 W,
   fPPT 35–80 W, Dynamic Boost 0–25 W, temperature target 75–87 °C; on
   battery SPL/sPPT ≤ 35 W, fPPT ≤ 65 W) and is marked
   `requires_fan_curve = true`: its power limits are only accepted while a
   custom fan curve is active. So phase C depends on phase B. **Verified**
   (`asus-armoury.h` lines 1284–1314, `asus-armoury.c` lines 222–235).
8. **No brick report for modes, fan curves or power limits was found.** The
   dangerous ids on this channel are others: APU memory (`0x000600C1`, Linux
   refuses two values as "difficult or impossible to recover from") and GPU
   Eco/MUX. What fan curves do go wrong with is firmware bugs — fans stuck at
   0 RPM until a mode switch or an EC reset, including one GA403WR report.
   **Verified** (issues cited in "Risks"); "no brick" is by absence in a
   limited issue search.

**Blocker:** none for phase A. Phase C needs a decision on carrying a
per-model range table (transcribed from Linux, GPL-2.0 source; see open
questions).

## Sources

| Source | Version read | Used for |
|---|---|---|
| Linux `include/linux/platform_data/x86/asus-wmi.h` | [`d135ab453a74`](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/include/linux/platform_data/x86/asus-wmi.h?id=d135ab453a7400a34930bdda1ef4bdc8acfac80e) (2026-10-26) | method ids, device ids, `DSTS` masks |
| Linux `drivers/platform/x86/asus-wmi.c` | [same commit](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/drivers/platform/x86/asus-wmi.c?id=d135ab453a7400a34930bdda1ef4bdc8acfac80e) | presence, thermal policy, fan RPM, fan curves |
| Linux `drivers/platform/x86/asus-armoury.{c,h}` | [same commit](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/drivers/platform/x86/asus-armoury.h?id=d135ab453a7400a34930bdda1ef4bdc8acfac80e) | power-limit attributes, per-model ranges, write guards |
| G-Helper [seerge/g-helper](https://github.com/seerge/g-helper) | `master` at [`0a1538ac`](https://github.com/seerge/g-helper/tree/0a1538ac4fb0337f6cf3d64a907acde10c91a3d9) (2026-10-26) | `app/AsusACPI.cs`, `app/Mode/ModeControl.cs`, `app/Mode/Modes.cs`, `app/AppConfig.cs`, `app/Fans.cs`, `app/Fan/FanSensorControl.cs`, `app/Program.cs`, `app/Helpers/AsusService.cs` |
| G-Helper wiki | [`638a92b4`](https://github.com/seerge/g-helper/wiki) (2026-09-08) | `FAQ.md`, `Requirements.md`, `Troubleshooting.md`, `_ACPI-Monitoring-and-Debugging.md` |
| G-Helper issues | read 2026-10-27 | user reports — **secondary** (#5192, #5529, #5935, #6013, #6016, #6241, #4539) |
| This repository | branch `feat/intel-rapl-readonly` | `Control/OtherSoftware.cs`, `Control/ControlProtocol.cs`, the providers, `docs/control-architecture.md` |

A local copy of G-Helper's mouse code exists in the owner's scratch folder;
the rest of G-Helper was cloned fresh for this research. The Linux files are
cited at the commit above; line numbers refer to it.

## The interface

### Device and IOCTL

| What | Value | Source |
|---|---|---|
| Device path | `\\.\ATKACPI` | G-Helper `AsusACPI.cs` line 32 |
| Opened with | `GENERIC_READ \| GENERIC_WRITE`, `FILE_SHARE_READ \| FILE_SHARE_WRITE`, `OPEN_EXISTING` | `AsusACPI.cs` lines 290–304 |
| IOCTL | `0x0022240C` = `CTL_CODE(0x22 FILE_DEVICE_UNKNOWN, 0x903, METHOD_BUFFERED, FILE_ANY_ACCESS)` | `AsusACPI.cs` line 33; decode by arithmetic |
| `DSTS` (device status, get) | `0x53545344` ("DSTS" little-endian) | `AsusACPI.cs` line 35; `asus-wmi.h` line 25 |
| `DEVS` (device set) | `0x53564544` ("DEVS") | `AsusACPI.cs` line 36; `asus-wmi.h` line 27 |
| Driver | "ASUS System Control Interface v3" (System devices); missing → G-Helper refuses to start and links [ASUS FAQ 1047338](https://www.asus.com/support/FAQ/1047338/) | G-Helper `Program.cs` lines 99–106, wiki `Troubleshooting.md` |

**Verified.** `INIT` (`0x54494E49`) and `WDOG` (`0x474F4457`) exist too;
G-Helper sends `INIT` only from its hotkey setup (`InputDispatcher.cs`
lines 51, 99), Linux once at probe (`asus-wmi.c` line 4923). Mode, fans and
limits work in G-Helper without it being tied to them, so RIGStats doesn't
send it — **hypothesis** that it isn't needed; sending it could also change
who receives the hotkey events.

### Buffer layout

Input buffer of `DeviceIoControl`, all little-endian (`AsusACPI.cs`
`CallMethod`, lines 406–421):

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | method id (`DSTS` / `DEVS`) |
| 4 | 4 | length of the arguments, in bytes |
| 8 | n | arguments |

Arguments per call (**verified**, `AsusACPI.cs` lines 437–503):

| Call | Arguments (n) | Used for |
|---|---|---|
| `DSTS` | device id (u32), argument (u32, normally 0) — 8 bytes | presence, value reads; fan curves use the argument for the mode |
| `DEVS`, integer | device id (u32), value (u32) — 8 bytes | mode, power limits |
| `DEVS`, payload | device id (u32), payload bytes — 4 + 16 bytes for a fan curve | fan curves |

Output buffer: G-Helper passes 16 bytes. The first u32 is the result; for a
fan-curve `DSTS` the 16 bytes are the curve itself. Linux allows a 32-byte
curve buffer (`FAN_CURVE_BUF_LEN`, `asus-wmi.c` line 112) and uses the first
16. **Verified.** RIGStats should pass 32 bytes and use 16 (**hypothesis**,
harmless: `METHOD_BUFFERED` copies at most what the driver returns).

Linux's `DEVS` for a fan curve passes the 16 bytes as four u32 arguments
(`fan_curve_write`, `asus-wmi.c` lines 3692–3715): temps 0–3, temps 4–7,
duties 0–3, duties 4–7, each byte-packed little-endian. That is the same
byte order as G-Helper's 16-byte payload. **Verified.**

### Return values

| Result | Meaning | Source |
|---|---|---|
| `DSTS` bit 16 `0x00010000` set | the id is present; the value is in the other bits | `asus-wmi.h` line 170; `asus-wmi.c` lines 574–590 |
| `DSTS` bit 16 clear | not present | the same |
| `0xFFFFFFFE` | method unsupported (`ASUS_WMI_UNSUPPORTED_METHOD`) | `asus-wmi.h` line 34; `asus-wmi.c` lines 391–395 |
| `0xFFFFFFFF` | treated as "no device" | `asus-wmi.c` `asus_wmi_get_devstate`, lines 553–565 |
| bit 0 `0x1` / bit 1 `0x2` | status / unknown bit, for on-off devices | `asus-wmi.h` lines 168–169 |
| bits 17 / 18 `0x20000` / `0x40000` | "user" / "BIOS" bits; G-Helper reads bit 18 of `0x00110019` as "Full Speed mode available" | `asus-wmi.h` lines 171–172; `Modes.cs` lines 110–127 |
| `DEVS` 1 | success | G-Helper logs `OK` only for 1 (`AsusACPI.cs` line 447); `asus-armoury.c` treats 0 as failure (lines 262–268) |
| `DEVS` on the mode id | **result ignored** by Linux: "Some machines do not return an error code as a result" | `asus-wmi.c` line 4124 |

G-Helper's `DeviceGet` returns `raw − 0x10000`, so a negative number means
"not present" (`AsusACPI.cs` lines 486–494, `IsSupported` lines 783–791).
RIGStats should test the bit itself and keep the raw value for the log.

On TUF laptops "unknown methods return nothing", so the method set can't be
probed — per-id presence still works (`asus-wmi.c` lines 4943–4950).
Machines whose WMI `_UID` is `ASUSWMI` use `DCTS` instead of `DSTS` in Linux
(lines 4955–4966); G-Helper always uses `DSTS` through ATKACPI. Which `_UID`
the ATKACPI machines have is **unconfirmed**; the read-only test shows
whether `DSTS` answers on the GA403WR.

### Who can open it, and at the same time

- **Any user process can.** G-Helper runs `asInvoker` (`app.manifest`
  line 7) and opens the device read/write without elevation. So LocalSystem
  can too. **Verified** (for G-Helper's users; on the GA403WR the read-only
  test confirms it).
- **Several handles at once.** G-Helper opens with both share flags, and its
  wiki says only that running it beside Armoury Crate is "not recommended …
  because they adjust the same settings" (`Requirements.md` line 15) — not
  that it fails. **Hypothesis:** the driver allows several handles; every
  call is one synchronous ACPI method evaluation, serialised in the kernel.
  The read-only test opens it while whatever ASUS software the owner has
  runs.
- **No vendor DLL.** The calls are `CreateFile` / `DeviceIoControl` from
  kernel32, so the rule "third-party native code runs out of process" doesn't
  apply; the provider may run in the service like the HID devices.
  **Verified** (no DLL in the path). A hung ACPI method would block the
  calling thread — **hypothesis**, no report of it found.

## Device ids

Only the ids below are ever sent. Everything else on this channel stays
unsent, documented or not.

### Phase A — performance mode

| Id | Name (Linux / G-Helper) | Values | Read safe? |
|---|---|---|---|
| `0x00120075` | `THROTTLE_THERMAL_POLICY` / `PerformanceMode` | 0 Balanced, 1 Turbo, 2 Silent (G-Helper also 3 Full Speed, 4 Manual) | yes: Linux's presence probe (`asus-wmi.c` lines 5171–5172) |
| `0x00110019` | `THROTTLE_THERMAL_POLICY_VIVO` / `VivoBookMode` | 0 Balanced, 1 Silent, 2 Turbo | yes: same probe (lines 5173–5174), G-Helper reads it (`Modes.cs` line 112) |

**Verified:** values in `asus-wmi.c` lines 96–102 and `throttle_thermal_policy_write`
(lines 4101–4144); G-Helper `AsusACPI.cs` lines 72–73, 164–168,
`SetVivoMode` lines 522–527. Linux accepts only 0–2
(`PLATFORM_PROFILE_MAX`); RIGStats should too — Full Speed and Manual are
G-Helper extras (Manual is forced only on G733, `AppConfig.cs` lines 641–645).

Detection: `DSTS 0x00120075`; present → ROG/TUF encoding. Else
`DSTS 0x00110019`; present → Vivobook encoding. Neither → no phase A.
Linux does it in this order (lines 5171–5174); G-Helper's `SetPerformanceMode`
the same (`AsusACPI.cs` lines 529–537, except that it also tries a blind
`DEVS 0x00120075` when neither reports presence — RIGStats must not).

Reading the current mode: **unconfirmed.** Linux's `profile_get` returns its
own variable (lines 4202–4226) and G-Helper its config (`Modes.GetCurrent`);
neither decodes `DSTS 0x00120075` as a value. The read-only test logs the
raw value under each mode.

### Phase B — fan RPM and fan curves

| Id | Name | Use | Read safe? |
|---|---|---|---|
| `0x00110013` | `CPU_FAN_CTRL` / `CPU_Fan` | RPM: low 16 bits × 100 | yes: Linux reads it every hwmon read (lines 3077–3085), G-Helper every second (`HardwareControl.cs` lines 840–860) |
| `0x00110014` | `GPU_FAN_CTRL` / `GPU_Fan` | the same, GPU fan | yes (lines 3212–3217) |
| `0x00110031` | `MID_FAN_CTRL` / `Mid_Fan` | the same, middle fan | yes (lines 3230–3235) |
| `0x00110024` | `CPU_FAN_CURVE` / `DevsCPUFanCurve` | `DSTS(arg = mode)` → firmware curve; `DEVS` → custom curve | yes: Linux's presence probe *is* this read (lines 3595–3650) |
| `0x00110025` | `GPU_FAN_CURVE` | the same, GPU fan | yes |
| `0x00110032` | `MID_FAN_CURVE` | the same, middle fan | yes |

**Verified:** `asus-wmi.h` lines 97–104; G-Helper `AsusACPI.cs` lines 63–65,
100–102.

**RPM.** `value & 0xFFFF` × 100 RPM (Linux lines 3084, 3103; G-Helper
`HardwareControl.cs` line 859). G-Helper treats a raw value above 120
(12 000 RPM) as invalid (`AsusACPI.cs` lines 555–568). A fan is present when
its id has the presence bit (Linux lines 3439–3450). **Verified.**

**Curve layout** (16 bytes, **verified**: Linux `fan_curve_copy_from_buf`
lines 3581–3593, G-Helper `SetFanCurve` lines 600–628):

| Bytes | Content |
|---|---|
| 0–7 | 8 temperatures, °C, ascending |
| 8–15 | 8 duties, 0–100 % (Linux's hwmon shows them as 0–255 and converts) |

**Mode argument for the read.** `DSTS` on a curve id takes the mode with
Turbo and Silent swapped: argument 0 = Balanced, 1 = Silent, 2 = Turbo
(Linux lines 3603–3608, comment "switched for OVERBOOST vs SILENT"; G-Helper
`GetFanCurve` lines 630–661, "because it's asus, and modes are swapped
here"). **Verified.** An empty answer means no curve: Linux treats an
integer 0 with no buffer as absent (lines 499–506), G-Helper an all-zero
buffer (`IsEmptyCurve`). Linux also requires the CPU fan to be present
before it probes curves (line 3640).

**What the read returns.** Linux names it `fan_curve_get_factory_default`
and uses it to seed the editor and for "reset to defaults" (enable = 3,
lines 3787–3793); G-Helper uses it when it has no saved curve
(`Fans.cs` lines 1263–1273). So it returns the firmware curve of that mode,
not one written by software — **hypothesis** (both sources use it that way;
neither reads after a write).

**The write.** `DEVS` with the 16 bytes; it has no mode field, so it acts on
the *active* mode. Result 1 = accepted. **Verified** (G-Helper treats ≠ 1 as
"BIOS rejected fan curve", wiki FAQ; Linux returns the method's status).
Rules both sources follow:

- A mode write cancels the custom curve. Linux marks every curve disabled
  after a mode write (lines 4135–4141); G-Helper writes its custom curves
  again 100 ms after every mode write, when the mode has them
  (`ModeControl.cs` lines 158–164, `AutoFans` lines 231–250).
  **Verified.**
- The way back to the firmware curve is to write the mode again (Linux
  lines 3803–3812; G-Helper `ResetPerformanceMode`, `ModeControl.cs`
  lines 104–113, wiki FAQ "Factory defaults"). **Verified.**
- Temperatures must be strictly ascending: G-Helper moves a duplicate point
  up by 6 °C ("preventing 2 points in same spot from default asus profiles",
  `FixFanCurve`, lines 708–758) — so the firmware's own default curves can
  contain duplicates. **Verified.** G-Helper also offers "Clamp to grid"
  (point *i* within 30 + 10·*i* … +10 °C, on by default, `AppConfig.cs`
  lines 819–822) and clamps duties to 0–100 (`SetFanCurve` line 612). Its
  editor allows 20–110 °C (`Fans.cs` lines 31–32).
- An all-zero curve is never sent (`SetFanCurve` line 604).
- Older models without curves: G-Helper falls back to a min/max range on
  `0x00110022` / `0x00110023` and, if that fails too, writes the mode back
  (`ModeControl.cs` lines 252–264). RIGStats leaves those models out.

### Phase C — power limits

| Id | Linux name (sysfs) | G-Helper name | Meaning |
|---|---|---|---|
| `0x001200A3` | `PPT_PL1_SPL` (`ppt_pl1_spl`) | `PPT_APUA3` | sustained limit (SPL / STAPM, Intel PL1), W |
| `0x001200A0` | `PPT_PL2_SPPT` (`ppt_pl2_sppt`) | `PPT_APUA0` | slow boost limit (sPPT, Intel PL2), W |
| `0x001200C1` | `PPT_PL3_FPPT` (`ppt_pl3_fppt`) | `PPT_APUC1` | fast boost limit (fPPT), W |
| `0x001200B0` | `PPT_APU_SPPT` (`ppt_apu_sppt`) | `PPT_CPUB0` | APU sPPT — all-AMD models |
| `0x001200B1` | `PPT_PLAT_SPPT` (`ppt_platform_sppt`) | `PPT_CPUB1` | platform sPPT — all-AMD models |
| `0x001200C0` | `NV_DYN_BOOST` (`nv_dynamic_boost`) | `PPT_GPUC0` | NVIDIA Dynamic Boost, W |
| `0x001200C2` | `NV_THERM_TARGET` (`nv_temp_target`) | `PPT_GPUC2` | NVIDIA temperature target, °C |
| `0x00120099` | `DGPU_BASE_TGP` (`nv_base_tgp`, read-only) | `GPU_BASE` | base TGP, W — a value read |
| `0x00120098` | `DGPU_SET_TGP` (`nv_tgp`) | `GPU_POWER` | added TGP, W — not proposed (see below) |

**Verified:** `asus-wmi.h` lines 107–113, 143–144; `AsusACPI.cs` lines
108–129; `asus-armoury.c` lines 789–822.

**Presence.** Both read `DSTS` on the same id: Linux creates an attribute
only when `armoury_has_devstate(id)` and the model's table has a non-zero
maximum for it (`asus-armoury.c` lines 144–153, 862–890, 956–965); G-Helper
shows a slider when `IsSupported(id)` (`Fans.cs` lines 1028–1034,
687–688). **Verified**; reading them is safe on the same grounds as above.

**The current value is not read.** Linux's `current_value` for these shows
the last value *it* wrote, starting from the table's default (= the maximum
when no default is given, lines 1036–1068 and the `power_limits` comment
"Default is always the maximum value unless *_def is specified");
G-Helper never reads them. Only the base TGP (`0x00120099`) and the added TGP
(`0x00120098`) are read as values (`Fans.cs` lines 302, 619; Linux
`nv_base_tgp` "INT_VALUE_ONLY_RO"). So whether `DSTS` on a limit id returns
its value is **unconfirmed**; the read-only test logs the raw words.

**Ranges come from tables, not the firmware.** Linux: per DMI board name,
AC and DC tables (`asus-armoury.h` lines 309–345 and the table from 346);
the AC or DC one is chosen by `power_supply_is_system_supplied()` at the
time of the write (`asus-armoury.c` lines 765–771); a value outside
`[min, max]` is refused with `-EINVAL` (lines 296–308); an attribute whose
min equals its max is refused (`asus-armoury.h` lines 264–266). G-Helper:
fixed bounds (5 W minimum, `MaxTotal` 150 W by default, 75–87 °C for the
temperature target, 5–25 W Dynamic Boost, `AsusACPI.cs` lines 174–206) with
per-model overrides in the constructor (lines 312–381). The older Linux
`asus-wmi` attributes used one global 5–250 W range (lines 120–127).
**Verified.**

**Order and timing in G-Helper.** Mode → 100 ms → fan curves → 1000 ms →
power limits (`ModeControl.cs` lines 158–167): SPL `0x001200A3` then sPPT
`0x001200A0` when `0x001200A0` is present; on AMD with an NVIDIA GPU also
fPPT `0x001200C1`; on all-AMD models `0x001200B0` instead (lines 349–406);
then Dynamic Boost and temperature target (lines 458–480). **Verified.**

**Models that need a curve.** Linux's `requires_fan_curve` makes power-limit
writes return `-EBUSY` until a custom fan curve is active ("On some models,
PPT changes require an active fan curve", `asus-armoury.c` lines 222–235).
G-Helper's own list for the same idea (`IsFanRequired`, `AppConfig.cs`
lines 652–655: writes the curves first when only power is applied) does not
contain GA403. The two sources disagree for the GA403 family — see open
questions.

**Not proposed:** `0x0012009C` (GPU→CPU dynamic boost), `0x0012009E` (CPU
temperature limit), `0x0012009F` (cross-loading power), `0x00120098`
(added TGP: Linux's range for the GA403WR, 80–95 W, reads like a total while
the attribute is described as "the additional TGP on top of the base TGP";
G-Helper computes its maximum from `nvidia-smi` minus the base, `Fans.cs`
lines 606–628 — the encoding is **unconfirmed**). G-Helper-only ids with no
Linux counterpart are left for later.

## The owner's machine: GA403WR

ROG Zephyrus G14 GA403WR: Ryzen AI 9 HX 370 (Strix Point), RTX 5070 Ti
Laptop. Users call it the 2025 G14 (G-Helper issues #6241 "2025 zephyrus G14
GA403WR", #5487); the 2024 G14 is GA403UI/UU/UV (Ryzen 8945HS). Linux keeps
the two families apart (`GA403U*` at `asus-armoury.h` lines 1156–1251, `GA403W*`
at lines 1252–1347). **Verified** (Linux tables; the year from user reports,
secondary).

### Linux (`asus-armoury.h` lines 1284–1314, DMI board name `GA403WR`)

| Limit | AC min–max | DC min–max |
|---|---|---|
| SPL `0x001200A3` | 15–80 W | 15–35 W |
| sPPT `0x001200A0` | 25–80 W | 25–35 W |
| fPPT `0x001200C1` | 35–80 W | 35–65 W |
| APU / platform sPPT `0x001200B0` / `0x001200B1` | not in the table → not offered | the same |
| Dynamic Boost `0x001200C0` | 0–25 W | not in the table |
| Temperature target `0x001200C2` | 75–87 °C | 75–87 °C |
| Added TGP `0x00120098` | 80–95 W | not in the table |

`requires_fan_curve = true`. No `_def` values, so Linux's defaults are the
maxima. **Verified.** The GA403WW has the same numbers; the GA403WM differs
only in Dynamic Boost (0–15 W) and TGP (55–85 W).

### G-Helper (`AppConfig.cs`)

| Check | GA403WR? | Effect |
|---|---|---|
| `IsCPULight` (line 657, contains `GA403`) | yes | `MaxTotal` = 90 W (power sliders 5–90 W) |
| `IsResetRequired` (line 647: GA403UI/UU/UV, FA507XV) | no | the toggle-to-another-mode reset workaround is not used |
| `IsFanRequired` (line 652) | no | power limits may be written without curves |
| `DynamicBoost15` (line 615: GA403UM, GA403GM, …) | no | Dynamic Boost 5–25 W |
| `IsSlash`, `IsChargeLimit6080` | yes | lighting / battery — out of scope |

**Verified.** G-Helper's 5–90 W is wider than Linux's 15–80 W; RIGStats
should use the intersection (Linux's table) — the narrower range is the one
two independent sources both accept.

### Step 1 results (read-only, 2026-10-27/28)

Seven runs of the start-up log with BIOS 310 and G-Helper 0.288 running (no
Armoury Crate); the mode set in G-Helper before each run. The distinct
readings are the fixture `sensor-sidecar.Tests/fixtures/asus-acpi/asus-ga403wr-ryzen-ai9-hx370-rtx5070ti.log`.

| Question | Answer |
|---|---|
| Device opens while G-Helper runs | yes, every call answered — **verified** (Armoury Crate not tested) |
| Present (presence bit) | `0x00120075`, `0x00110013`, `0x00110014`, `0x001200A3`, `0x001200A0`, `0x001200C1`, `0x001200C0`, `0x001200C2`, `0x00120099`, `0x00120098`, curves `0x00110024` / `0x00110025` — **verified** |
| Absent | `0x00110019`, `0x00110031`, `0x00110032` (no middle fan), `0x001200B0`, `0x001200B1`: the IOCTL itself fails with Win32 error 1 (`ERROR_INVALID_FUNCTION`), not a `0xFFFFFFFE` word — **verified** |
| Mode readable (`0x00120075`) | no: `0x00010003` in Balanced, Silent and Turbo — **verified**. What the 3 is: unknown |
| Limits readable (`A3`/`A0`/`C1`/`C0`/`C2`) | no: `0x00010000` (value 0) after G-Helper wrote 9–13 W (Silent) and 80 W (Turbo) — **verified** |
| Base TGP `0x00120099` | `0x50` = 80 — **hypothesis**: watts |
| Added TGP `0x00120098` | 0 in the first run, 15 in every later one; G-Helper writes 15 in Turbo, but it already read 15 before that write once — not settled |
| RPM | low 16 bits × 100 as documented: Balanced idle 2200–2300 / 2600, Silent idle 0 / 0, Silent at ~100 % CPU 2200 / 2500 (owner: faint), Turbo idle 3100–3600 / 3400–3800 — **verified** that it follows mode and load |
| Firmware curves | 16 bytes per mode, the same in all seven runs — see below |

Firmware curves (temperatures °C → duty):

| Curve | Temperatures | Duty |
|---|---|---|
| CPU Balanced | 45 62 65 68 70 72 74 76 | 5 9 12 16 20 28 37 56 |
| CPU Silent | 47 69 70 72 74 76 78 **255** | 1 5 9 12 16 20 28 28 |
| CPU Turbo | 20 64 66 68 70 72 74 76 | 9 12 16 28 33 37 56 66 |
| GPU Balanced | 42 59 62 65 67 69 71 73 | 5 7 12 16 20 28 39 60 |
| GPU Silent | 47 67 68 **70 70 70 70** **255** | 1 5 7 12 16 20 28 28 |
| GPU Turbo | 20 57 60 63 66 69 72 75 | 7 12 16 28 35 39 60 71 |

The Silent curves break the phase B validation below: repeated
temperatures, a last point at 255 °C, and equal duties at the end. Whether
the duty is a percentage is a **hypothesis** (G-Helper treats it as one); if
it is, the firmware tops out at 56–71 %.

G-Helper also writes a CPU undervolt per mode (Silent −20, Turbo 0) — through
the SMU, not ATKACPI, so it is a second writer for RIGStats' Curve Optimizer
domain, not only for this one.

### Not known for the GA403WR

- whether a mode write resets the power limits, or the G14 2024 toggle is
  needed too;
- whether power limits are really refused without a custom curve (Linux
  says yes, G-Helper doesn't act on it).

One GA403WR report relates to fans: the CPU fan stayed at 0 RPM with a
custom curve and G-Helper showed no CPU temperature; an EC reset fixed it
(G-Helper #5529). Another: with an uneven custom Silent curve the firmware
started the fans at ~57 °C despite 0 %; spreading the points evenly fixed it
(#5192). Both **secondary**.

## Coexistence with Armoury Crate and G-Helper

### What they do

- **G-Helper keeps its own mode and writes it on its own triggers:** at
  start, on every AC/DC change (after a settle timer), on resume, on Fn+F5 or
  its hotkeys, and on GPU mode changes when `mode_reapply` is set
  (`Program.cs` `SetAutoModes`, lines 298–321, 380–410, and `Settings.cs` line 814 on wake-up; `ModeControl.cs`
  `AutoPerformance`, lines 92–101). It never reads the firmware's mode, so a
  change by another program stays until G-Helper's next trigger and is then
  overwritten. With "shutdown reset" (on by default only for FX507Z) it
  writes Balanced at suspend and shutdown (lines 640–650). **Verified.**
- **Fn+F5** reaches software as an ATK WMI event (code 174, older models
  153); G-Helper cycles the mode on it (`InputDispatcher.cs` lines 965–968).
  When the ASUS Optimization service runs, G-Helper leaves the Fn keys to it
  (wiki FAQ; `AsusService.IsAsusOptimizationRunning`). Whether Fn+F5 does
  anything with neither running is **unconfirmed** (**hypothesis**: no — the
  mode is changed by software).
- **Armoury Crate:** G-Helper's wiki says not to run both "because they
  adjust the same settings" and that ASUS services may overwrite a setting
  after G-Helper wrote it (charge limit, FAQ). What Armoury Crate does when
  another program changes the mode is **unconfirmed** (**hypothesis**: like
  G-Helper, it writes its own mode on its own triggers).

### Process names for `OtherSoftware`

| Process (no `.exe`) | Program | Source | Proposed |
|---|---|---|---|
| `GHelper` | G-Helper | `GHelper.csproj` line 15 (`AssemblyName`) | conflict row, `Verified: false` |
| `ArmouryCrate.Service` | Armoury Crate's service | G-Helper `AsusService.cs` line 46 (`IsArmouryRunning`) | conflict row, `Verified: false` |
| `ArmouryCrate` | Armoury Crate app | already in `OtherSoftware` (aura, verified) | conflict row for the new domain too |
| `AsusOptimization` | ASUS Optimization (MyASUS / ASUS System Control Interface) | `AsusService.cs` line 42 | **not** a conflict row (see below) |

Service names G-Helper stops (from `AsusService.cs` lines 10–33, for the
start-up log, not detection): `ArmouryCrateControlInterface`,
`ASUSOptimization`, `AsusAppService`, `ArmouryCrateService`,
`ArmouryCrateSEService`, `LightingService` and others. Their process names
are not in the source, so they are **unconfirmed**.

`AsusOptimization` runs on most ASUS laptops with MyASUS, and what it is
known to do is the Fn keys. Making it a conflict would hide phase A on most
machines for a reason not shown. Proposal: no conflict row; the start-up
line already lists it ("also seen", vendor word `asus`), and a ZIP from a
user whose mode keeps changing back settles it. Add `ghelper` to
`OtherSoftware.VendorWords` so G-Helper shows in that line even before its
row is verified.

## The way back

- **`ReleaseToFirmware`:** only if RIGStats wrote anything this session:
  `DEVS 0x00120075` (or the Vivobook id) = Balanced. That resets the fan
  curves (verified, above) and normally the power limits (verified for most
  models through G-Helper's *Factory defaults*; the G14 2024 exception needs
  Turbo first — G-Helper writes Turbo, or Balanced if the old mode was
  Turbo, waits 1.5 s, then the target, `ModeControl.cs` lines 141–153).
  Balanced rather than "the mode before RIGStats", because that can't be
  read (**unconfirmed**, above); Linux does the same at every probe
  (`throttle_thermal_policy_set_default`, lines 4146–4153).
- **What survives a reboot:**
  - The **mode** probably persists: Linux writes Balanced at probe "or
    otherwise the system will often remain in silent mode" (lines
    4282–4285). **Verified** for the machines Linux saw; GA403WR
    **unconfirmed**. This is why release writes Balanced — a RIGStats that
    is uninstalled must not leave a laptop in Silent.
  - **Custom curves and limits** reset at reboot on most models: G-Helper's
    FAQ "Most devices reset settings to defaults on reboot anyway" and
    "After restart, BIOS resets all settings to defaults, including charge
    limit". **Verified** as G-Helper's statement; GA403WR **unconfirmed**.
- **Sleep and hibernate:** the firmware runs its own fan policy in sleep
  (G-Helper #6013, the maintainer). On a Flow X16 the performance after
  hibernate matched Silent while Balanced was set, and writing Balanced
  again fixed it (#5935). **Secondary.** **Hypothesis:** RIGStats needs to
  write the mode (and curves, limits) again after a resume, like G-Helper
  does; the service has no resume hook today (the same open question as the
  AMD/Intel limits).
- **Last resort for the user:** an EC reset (power off, hold the power
  button 30–40 s; G-Helper wiki `Troubleshooting.md`, ASUS FAQ 1050239).

## Risks

| Risk | Evidence | What RIGStats does |
|---|---|---|
| Sending an id or method nobody documented | the Falchion Ace HFX sweep (`docs/control-architecture.md`); G-Helper ships a `DSTS` sweep (`AsusACPI.cs` lines 865–902) | a fixed allow-list of the ids in this document, enforced inside `IAsusAcpi`'s real implementation and pinned by a test; no sweep, no blind `DEVS` |
| APU memory `0x000600C1` | Linux refuses 0x100/0x101: "difficult or impossible to recover from" (`asus-armoury.c` lines 238–252) | never sent |
| GPU Eco / MUX (`0x00090020`, `0x00090016`) | black screens, a dGPU Windows deletes after 15 days in Eco (G-Helper wiki FAQ) | never sent |
| Fans stuck at 0 RPM with a custom curve | #6016 (FX507VV, after a dGPU toggle), #5529 (GA403WR, fixed by EC reset), #4539 (FX507ZE) — firmware bugs per the maintainer | duty floor at the hot end (below); temperature watch that writes the mode again (restores the firmware curve) |
| Firmware follows an uneven curve oddly | #5192 (GA403WR) | strictly ascending temperatures; editor snaps to a 10 °C grid by default |
| Power limits out of range | Linux refuses (`-EINVAL`); no report of what the firmware does with them | clamp to the model table at write time, in the service |
| SPL > sPPT > fPPT inverted | not reported; AMD's semantics are sustained ≤ slow ≤ fast | resolve SPL ≤ sPPT ≤ fPPT |
| Freezes with a CPU temperature limit on the GA403WR | #6241 — set through the SMU (PawnIO), not ATKACPI | not offered (that is the SMU, out of scope) |
| Two programs writing the mode | G-Helper's triggers (above) | yield (`OtherSoftware`) |

No report of a laptop bricked through modes, fan curves or power limits was
found (issue searches "brick", "fans stopped", "GA403WR", "G14 2025",
"Armoury Crate"). By absence, in a limited search — **unconfirmed**.

## Proposed design

One provider for all three phases, because they are coupled: a mode write
resets the curves and limits, the GA403WR wants a curve before limits, and
the order (mode → curves → limits) matters. It fits the safety rules in
`sensor-sidecar/Control/CLAUDE.md`.

### Seam and allow-list

- `IAsusAcpi` (new): `Dsts(uint id, uint arg = 0) → uint`,
  `DstsBuffer(uint id, uint arg) → byte[16]`, `Devs(uint id, uint value) →
  uint`, `Devs(uint id, ReadOnlySpan<byte> payload) → uint`. The real
  `AtkAcpi` opens `\\.\ATKACPI` with both share flags and **throws for any id
  not in `AsusAcpiIds`** — the table in this document, nothing else. A test
  pins the table (like `BatteryQueryTests`). `FakeAsusAcpi` for tests.
- `AsusAcpiCodec` (pure, unit-tested): presence bit, RPM, mode encodings
  (ROG ↔ Vivobook, the curve-read swap), curve bytes ↔ `[[t, d]] × 8`,
  clamps.
- `AsusLaptopProvider(IAsusAcpi? acpi, bool dryRun, Func<string?> conflict,
  BootCrashGuard guard, AsusModelTable models)`, domain `asus_laptop`.
  Built in the start-up task like the other hardware providers (opening a
  device is slow work, `Program.cs` rule). Broker order: right after
  `power_plan` (on these laptops `cpu_limit`, `fan` and `gpu` are
  unsupported anyway).
- `CapabilitySet`: no driver → `Supported = false`, `Reason` ("No ASUS
  System Control Interface", no `Problem`: the PC simply lacks it). Taken by
  G-Helper / Armoury Crate → `Problem` with `details.conflict`, the same as
  the fans. The probe result (every raw `DSTS` word, board name, BIOS
  version) goes into the log once at start for the diagnostics ZIP and
  becomes a fixture (`sensor-sidecar.Tests/fixtures/asus-acpi/<board>.json`).

### Profile part

```json
"asus_laptop": {
  "mode": "turbo",
  "fans": {
    "cpu": [[30,0],[40,10],[50,20],[60,35],[70,55],[80,70],[90,85],[100,100]],
    "gpu": null
  },
  "limits": { "spl_w": 60, "sppt_w": 70, "fppt_w": 80,
              "nv_dynamic_boost_w": null, "nv_temp_target_c": 85 }
}
```

`null` (or missing) = the firmware's value for that mode. Built-ins map
Silent → `silent`, Balanced → `balanced`, Gaming → `turbo`, Eco → `silent`;
the user can change it.

### Capability (`details`)

```json
{
  "board": "GA403WR",
  "bios": "GA403WR.312",
  "mode": { "id": "0x00120075", "modes": ["silent", "balanced", "turbo"], "readable": false },
  "fans": {
    "cpu": { "rpm": true, "curve": true, "points": 8, "temp_c": [20, 110],
             "defaults": { "silent": [[30,0], "…"], "balanced": ["…"], "turbo": ["…"] } },
    "gpu": { "rpm": true, "curve": true, "points": 8, "temp_c": [20, 110], "defaults": { "…": "…" } }
  },
  "limits": {
    "power_source": "ac",
    "requires_fan_curve": true,
    "spl_w":  { "min": 15, "max": 80 },
    "sppt_w": { "min": 25, "max": 80 },
    "fppt_w": { "min": 35, "max": 80 },
    "nv_dynamic_boost_w": { "min": 0, "max": 25 },
    "nv_temp_target_c":   { "min": 75, "max": 87 }
  }
}
```

(Values illustrative; `limits` is absent when the board isn't in the model
table, `defaults` are the three `DSTS` curve reads.)

### Phase A — performance mode

- **Probe:** open; `DSTS 0x00120075`, else `0x00110019`. Present → offer the
  three modes.
- **Validate:** `mode` ∈ {silent, balanced, turbo}.
- **Apply:** skip if it equals what RIGStats last wrote (unless after a
  resume); `DEVS` the encoded value; dry-run logs. No `BootCrashGuard`: the
  mode is a vendor setting the user can set with Fn+F5.
- **Verify:** read back only if the read-only test shows `DSTS` returns the
  mode; otherwise accept and log the `DEVS` result (Linux ignores it).
- **Capture / Restore:** RIGStats' last written mode (the firmware's can't
  be read).
- **Release:** Balanced, only if written.
- **UI:** a mode picker on the profile page (three segments), where the
  power plan sits.

### Phase B — fan curves and RPM

- **RPM:** `0x00110013` / `0x00110014` / `0x00110031` once a second into the
  telemetry (a "Laptop fans" group on the Motherboard panel), from the
  service like the peripheral batteries. Read-only, can come before phase B
  ships.
- **Probe:** the three `DSTS` curve reads per present fan, mode arguments
  0/1/2 → `defaults`.
- **Validate:** exactly 8 points; temperatures strictly ascending within
  20–110 °C; duties 0–100 and non-decreasing. This is for curves RIGStats
  writes; the GA403WR's own Silent curves fail it (repeated temperatures, a
  last point at 255 °C — "Step 1 results"), so the firmware defaults must be
  shown and used for the clamp without passing through it.
- **Clamp (service, at write time):** the duty of the last two points is at
  least the firmware default's duty at those points for the target mode —
  a curve may be quieter in the middle but never below what ASUS ships at
  the hot end. (A proposal: the firmware's own Silent curve sits at ~41 % at
  100 °C, `AppConfig.GetDefaultCurve`, so "≥ 100 % at the end" would be
  wrong.)
- **Apply:** after the mode write, wait 100 ms, `DEVS` each curve in the
  part; result ≠ 1 → `DomainUnavailableException` ("The laptop's firmware
  rejected the fan curve"), the mode stays, no retry. A fan with `null` →
  nothing written (the mode write already restored the firmware curve).
- **Safety net:** the firmware evaluates the curve, so `FanCurveLoop`'s
  critical override can't reach it. Instead: if CPU or GPU temperature (LHM)
  passes the critical threshold while a custom curve is active, write the
  mode again (firmware curve) and raise `safety_tripped`. No
  `FanWatchdog` dependency.
- **UI:** the Fans tab's curve editor (`curve_editor`,
  `src-egui/src/windows/control.rs` line 4288) in a fixed-8-points mode (no
  add/remove), the source shown as "the laptop's firmware", the mode's
  default curve as a faint line and a "Firmware curve" reset.

### Phase C — power limits

- **Gate:** board in `AsusModelTable` (transcribed from `asus-armoury.h`,
  regenerated, never edited by hand — like `MouseCatalog`) **and** the id
  present. Start with the GA403WR only.
- **Validate:** finite, > 0.
- **Resolve (pure):** clamp each to the table's range for the power source
  at write time; then SPL ≤ sPPT ≤ fPPT (raise the later ones).
- **Apply:** `BootCrashGuard.Arm()` before the first write; mode → 100 ms →
  curves (with `requires_fan_curve` and no curve in the part: write the
  mode's own default curve read at probe) → 1000 ms → SPL, sPPT, fPPT,
  Dynamic Boost, temperature target. Any result ≠ 1 → fail; the broker
  restores.
- **Preview:** the CPU tab's "Try new limits" with auto-revert; revert =
  write the mode again (+ the G14-2024 toggle if the test shows the GA403WR
  needs it).
- **AC/DC change:** **hypothesis** that the firmware drops custom limits on
  a power-source change (G-Helper writes everything again then); the
  service re-applies the active profile on the change, clamped to the new
  table.
- **UI:** the CPU tab with SPL / sPPT / fPPT sliders instead of AMD's
  PPT/TDC/EDC; the GPU tab with Dynamic Boost and temperature target for the
  NVIDIA laptop GPU (today "laptop GPUs are firmware-owned").

## Live-test plan (GA403WR)

The owner runs `pwsh -File tools\dev-sidecar.ps1 -Live` in an elevated
window; Ctrl+C hands everything back. Each step is a separate build and a
separate go; stop at anything unexpected — a reboot, and at worst an EC
reset, brings everything back.

**Step 0 — nothing sent by RIGStats.**

1. Note which ASUS software is installed and running (Armoury Crate,
   G-Helper, MyASUS) and the BIOS version (BIOS setup or `msinfo32`).
2. If G-Helper is installed: send `%AppData%\GHelper\log.txt`. Its lines
   (`Mode = 1 : OK`, `Default Curve: CPU - …`, `PowerLimit A3 = … : OK`)
   already show which ids answer on this laptop and its firmware curves.

**Step 1 — read-only (no `DEVS` anywhere).** Built (`AsusAcpi.LogSnapshot`,
called from the service's start-up checks). The build logs once at start
(lines `ASUS ACPI (board …, BIOS …), read-only; ASUS software running: …`,
`ASUS ACPI DSTS: …` and one `ASUS ACPI curve 0x…` line per fan curve):

1. Start the dev sidecar; let it run a minute; Ctrl+C. Collect diagnostics.
2. The log holds: whether the device opened (and with which ASUS software
   running); the raw `DSTS` word for `0x00120075`, `0x00110019`,
   `0x00110013`, `0x00110014`, `0x00110031`, `0x001200A3`, `0x001200A0`,
   `0x001200C1`, `0x001200B0`, `0x001200B1`, `0x001200C0`, `0x001200C2`,
   `0x00120099`, `0x00120098`; the 16 bytes of `0x00110024`, `0x00110025`,
   `0x00110032` with arguments 0, 1 and 2.
3. Only if G-Helper or Armoury Crate is installed: set Silent there, start
   and stop the dev sidecar, set Turbo, start and stop it again. Comparing
   the `0x00120075` words answers whether the mode can be read back.
4. The fan RPM line should match what the fans sound like (idle vs a short
   load).

What it answers: presence of every id, middle fan or not, whether mode and
limits are readable, the factory curves. The raw words become the fixture.

**Step 2 — mode (phase A).** G-Helper and Armoury Crate closed (the domain
is taken otherwise).

1. Start a steady CPU load (Cinebench R23 multi-core, 10 min) and watch
   "CPU Package" power and the fan RPM in RIGStats.
2. Apply Silent, then Balanced, then Gaming (Turbo), a minute each. Power
   and fan noise should step with them; the log shows each `DEVS` result.
3. Ctrl+C: the log shows "Balanced" handed back. Reboot once and note which
   mode the laptop comes up in (answers persistence).

**Step 3 — one fan curve (phase B), the loud direction first.**

1. Balanced, light load. In the Fans tab set the CPU curve to the firmware
   curve with every duty ≥ 60 %, and apply. CPU fan RPM should rise within
   seconds; the GPU fan unchanged.
2. Press "Firmware curve" (mode written again): RPM falls back.
3. Then a curve quieter than the default in the middle (the hot end held by
   the clamp), and back. Note any 0 RPM under load — stop and write the mode
   again if it happens.

**Step 4 — power limits (phase C), only after steps 1–3.**

1. Turbo, the same Cinebench load. In the CPU tab set SPL 45 W, sPPT 50 W,
   fPPT 60 W and press "Try new limits" (15 s preview).
2. "CPU Package" should settle near 45–60 W, and go back up when the preview
   reverts. If it doesn't move: try again with the CPU curve set (answers
   `requires_fan_curve`).
3. After the revert, compare package power with plain Turbo; if it stays
   low, the GA403WR needs the G14-2024 toggle.
4. Unplug the charger with limits applied; note whether power changes
   (answers the AC/DC question). Ctrl+C at the end.

## What RIGStats will and won't send

**Will (only the ids in this document):**

| Call | Ids | When |
|---|---|---|
| `DSTS`, argument 0 | `0x00120075`, `0x00110019`, `0x00110013`, `0x00110014`, `0x00110031`, `0x001200A3`, `0x001200A0`, `0x001200C1`, `0x001200B0`, `0x001200B1`, `0x001200C0`, `0x001200C2`, `0x00120099`, `0x00120098` | probe; RPM ids every second |
| `DSTS`, argument 0/1/2 | `0x00110024`, `0x00110025`, `0x00110032` | probe |
| `DEVS`, value 0–2 | `0x00120075` or `0x00110019` (whichever is present) | apply, restore, release |
| `DEVS`, 16 bytes | `0x00110024`, `0x00110025`, `0x00110032` (present ones) | apply (phase B) |
| `DEVS`, value in range | `0x001200A3`, `0x001200A0`, `0x001200C1`, `0x001200C0`, `0x001200C2` | apply (phase C, table models) |

**Won't:**

- Any id not in the table, a sweep, or `DEVS` on an id whose `DSTS` lacks
  the presence bit.
- `INIT`, `WDOG`, any other method; the WMI route (`AsusAtkWmi_WMNB`).
- Modes 3 (Full Speed) and 4 (Manual); fan ranges `0x00110022`/`0x00110023`.
- GPU Eco/MUX, APU memory, CPU cores, TGP, battery, keyboard and display ids.
- Power limits on a board not in the model table, or outside its range.

## Open questions

1. Does `DSTS 0x00120075` return the current mode on the GA403WR (and in
   which bits)? If yes, release can restore the mode from before RIGStats.
   **Answered (step 1): no** — `0x00010003` in every mode.
2. Does `DSTS` on a power-limit id return its value? Does `DSTS` on a curve
   id return the custom curve after a write, or always the factory one?
   **Answered (step 1), for writes by G-Helper: the limits read 0, the
   curves always the factory ones.**
3. Is a custom fan curve really required for power limits on the GA403WR
   (Linux `requires_fan_curve`), when G-Helper doesn't act on it?
4. Does writing the mode reset the power limits on the GA403WR, or does it
   need the G14-2024 toggle (another mode first)?
5. Does the mode persist across a reboot, and do curves and limits reset?
   What do sleep, hibernate and an AC/DC change do to each?
6. Is a middle fan (`0x00110031` / `0x00110032`) present? **Answered
   (step 1): no.**
7. Is `INIT` ever needed before `DEVS` for the mode to take?
8. Does the device open, and do calls work, while Armoury Crate's services
   or G-Helper have it open? **Answered for G-Helper (step 1): yes.**
   Armoury Crate is untested.
9. Which process names does Armoury Crate run on a laptop (is
   `ArmouryCrate.Service` the one that writes the mode)? Should
   `AsusOptimization` count as a conflict?
10. Phase C needs a per-model range table. Is transcribing the numbers from
    `asus-armoury.h` (GPL-2.0) acceptable for an MIT project the way
    `MouseCatalog` transcribes G-Helper's (the owner decides; not legal
    advice)? And is "not offered on boards outside the table" the right
    rule?
11. What does the added-TGP id `0x00120098` encode on the GA403WR (Linux's
    80–95 W vs "additional on top of base")? Left out until known. Step 1
    read 0 once and 15 afterwards; not settled.
12. Is a hung ACPI call possible, and does the provider need a timeout
    thread?
13. Does Fn+F5 change the mode with no ASUS software running? If it does,
    the service should notice (a mode it didn't write) rather than fight.
