# Intel CPU power limits (PL1/PL2): MSR 0x606 / 0x610, PawnIO and the MCHBAR mirror

Research notes for #209 (Control Center phase 2b), 2026-10-27.

**Nothing is built and nothing has been sent to any hardware.** Everything
below was read from primary sources (the Intel SDM, an Intel datasheet, the
PawnIO and PawnIO.Modules source, the Linux `intel_rapl` driver, coreboot and
LibreHardwareMonitor) or from files already in this repository. Every claim
is marked:

- **verified** — with the source (URL, or file and line) it was read from;
- **unconfirmed** — the sources don't settle it;
- **hypothesis** — a reasoned guess, with what would confirm or rule it out.

Scope: package PL1 and PL2 on Intel desktop CPUs, lowered only (like AMD in
phase 2). Out of scope: raising limits, PL3/PL4, the current limit (0x601),
voltage/undervolting, laptops (see "Laptops and OEM tools").

## Summary

1. **The write-capable module is already in the build.** RIGStats uses
   `LibreHardwareMonitorLib` 0.9.7-pre749 (`sensor-sidecar/sensor-sidecar.csproj`
   line 24, since #256), not 0.9.6. Its embedded `IntelMSR.bin` is
   byte-identical to the signed PawnIO.Modules 0.2.11 release (SHA-256
   `d6ed85d65ab17a22f813ef98207d6d537155ee2ded5976a21cb48413c9b92e5f`) and
   exports `ioctl_write_msr` with 0x610 on its write allow-list. The first
   item in #209 ("embed the signed upstream `IntelMSR.bin`") is not needed:
   `PawnIoModule.LoadFromLhm("IntelMSR.bin")` is enough. **Verified** (see
   "Which binary").
2. **The MCHBAR mirror can be read.** PawnIO.Modules 0.2.11 has an
   `IntelMCHBAR` module with `ioctl_read_qword`, read-only. So MCHBAR+0x59A0 can
   be read back, which #189's finding said was impossible ("can only be
   detected by behaviour, not read back" is wrong). It still can't be written.
   **Verified** (`IntelMCHBAR.p`). It is not in LHM, so reading it means
   embedding one more signed blob.
3. **For a design that only lowers limits, the mirror matters much less.**
   If the effective limit is the lower of MSR and MMIO, a write *below* both
   always takes effect. The only no-op values are those between the MMIO value
   and the MSR value. **Hypothesis** — the "lower of the two" rule comes from
   user reports and the issue, not from an Intel document (see "The MCHBAR
   mirror").
4. **The register layout is architectural and fully documented.** SDM vol. 3B
   §17.10 gives the 0x606 units, the 0x610 fields, the Y/Z time window and the
   lock bit 63 ("all write attempts to this MSR are ignored until next
   RESET"). Linux uses the same masks. **Verified.**
5. **PERF_LIMIT_REASONS (0x64F) is not on IntelMSR's read allow-list**, so it
   can't be used to detect throttling. `IA32_PACKAGE_THERM_STATUS` (0x1B1) bit
   10 "Package Power Limitation Status" and `MSR_PKG_PERF_STATUS` (0x613) are
   on the list, and so are the energy counter (LHM's "CPU Package" power) and
   `MSR_PKG_POWER_INFO` (0x614). **Verified** (`IntelMSR.p` lines 70–87).
6. **Plundervolt doesn't touch 0x610.** Intel's advisory (SA-00289) is about
   "voltage settings"; nothing in it or in the module's allow-list ties it to
   RAPL. **Hypothesis** — no source says so explicitly.
7. **Arrow Lake (Core Ultra 200S, the tester's 285K) still has MSR RAPL.**
   Linux matches `INTEL_ARROWLAKE` to its plain MSR defaults and also has an
   MMIO RAPL driver for the Arrow Lake-S processor thermal device.
   LHM already reads the RAPL energy counter on the tester's 285K (fixture
   `gigabyte-z890-ultra9-285k-rtx5070ti`, "CPU Package" 79.8 W).
   **Verified.**
8. **No blocker found.** The open risks are test coverage (one Intel tester),
   laptops (left out), and whether a sleep/resume brings back the BIOS value
   (**unconfirmed**).

## Sources

| Source | Version read | Used for |
|---|---|---|
| Intel SDM vol. 3B §17.10 "Platform Specific Power Management Support" | Order no. 325384-093US (combined vol. 3, [cdrdv2 671447](https://cdrdv2.intel.com/v1/dl/getContent/671447)), pages 17-49 – 17-52 | 0x606, 0x610, 0x611, 0x613, 0x614 layouts, lock |
| Intel SDM vol. 4 (MSRs) | 335592-093US ([cdrdv2 671098](https://cdrdv2.intel.com/v1/dl/getContent/671098)) | 0x1B1 bits, 0x64F bits, "Package" scope |
| 13th Gen Intel Core Datasheet vol. 2, "Package Power Limit (PACKAGE_RAPL_LIMIT_0_0_0_MCHBAR_PCU) – Offset 59a0" | doc 767624 rev 1.3 ([edc.intel.com](https://edc.intel.com/content/www/us/en/design/publications/13th-generation-core-processors-datasheet-volume-2-of-2/package-power-limit-package-rapl-limit-0-0-0-mchbar-pcu-offset-59a0)) | MMIO mirror fields |
| [namazso/PawnIO.Modules](https://github.com/namazso/PawnIO.Modules) | tag `0.2.11` (`7345a80e`), master `85786f30` | `IntelMSR.p`, `IntelMCHBAR.p`, release zip |
| [namazso/PawnIO](https://github.com/namazso/PawnIO) | `9d529658` | `PawnIO/src/natives_impl_windows.cpp` (`msr_write`) |
| Linux `drivers/powercap/intel_rapl_{common,msr}.c`, `drivers/thermal/intel/int340x_thermal/processor_thermal_rapl.c`, `arch/x86/include/asm/msr-index.h` | `d135ab453a74` (2026-10-26) | masks, time-window code, lock handling, MMIO offsets, CPU list |
| coreboot `src/soc/intel/common/block/power_limit/power_limit.c` | `206078d6d990` | how firmware writes MSR + MCHBAR, time-window table |
| LibreHardwareMonitor | master `3f118427`; NuGet 0.9.6 and 0.9.7-pre749 from the local package cache | bundled modules, `IntelMsr.cs`, notices |
| TechPowerUp ThrottleStop forum, WinRAID ([search results](https://www.techpowerup.com/forums/goto/post?id=5361370)) | user posts | how ThrottleStop handles the MMIO limit — **secondary, user reports only** |

## MSR_RAPL_POWER_UNIT (0x606)

Read-only, package scope. **Verified**: SDM vol. 3B §17.10.1, Figure 17-35;
Linux `intel_rapl_common.c` lines 38–45 and `rapl_default_check_unit()`.

| Bits | Field | Meaning |
|---|---|---|
| 3:0 | Power Units (PU) | 1 W / 2^PU |
| 12:8 | Energy Status Units (ESU) | 1 J / 2^ESU |
| 19:16 | Time Units (TU) | 1 s / 2^TU |
| others | reserved | |

The SDM text gives "default" values that differ between chapters (§17.10.1:
PU 3, ESU 16, TU 10; the Goldmont table in vol. 4: PU 8, ESU 14, TU 10), so
**never hard-code the units**: read 0x606 every time the provider opens.
LHM already reads it on every Intel CPU it gives power for
(`IntelCpu.cs`, `EnergyUnitsMultiplier`); on Silvermont/Airmont Atoms the
energy unit is different (µJ-based) — irrelevant here, since those are not
offered.

Typical Core desktop value `0x000A0E03` (**unconfirmed on the tester's
machine** — it is what the read-only test below will show): PU = 3 → 0.125 W,
ESU = 14 → 61.04 µJ, TU = 10 → 976.5625 µs.

## MSR_PKG_POWER_LIMIT (0x610)

Read/write, package scope. **Verified**: SDM vol. 3B §17.10.3, Figure 17-36
(page 17-50/17-51); Linux `intel_rapl_msr.c` lines 50–63 (masks) and
lines 285–310 (`rpi_msr`).

| Bits | Field | Notes |
|---|---|---|
| 14:0 | PL1 | in power units |
| 15 | PL1 enable | 0 = disabled |
| 16 | PL1 clamp | allow going below the OS-requested P/T state |
| 23:17 | PL1 time window (Tau) | Y = bits 21:17, Z = bits 23:22 |
| 31:24 | reserved | Linux's `POWER_LOW_LOCK` (bit 31) is for one-limit registers only, not 0x610 (`get_pl_lock_prim()`, `intel_rapl_common.c` lines 81–103) |
| 46:32 | PL2 | in power units |
| 47 | PL2 enable | |
| 48 | PL2 clamp | |
| 55:49 | PL2 time window | SDM: "may have a hard-coded value in hardware and ignores values written by software"; coreboot: "Power limit 2 time is only programmable on server SKU" (`power_limit.c` line 178) |
| 62:56 | reserved | |
| 63 | Lock | "If set, all write attempts to this MSR are ignored until next RESET" (SDM). Linux reports the limit "locked by BIOS" and keeps it read-only (`rapl_detect_powerlimit()`, `intel_rapl_common.c` lines 1032–1050) |

A write to a locked 0x610 is ignored, not refused — so after any write the
register has to be read back to know whether it took. **Verified** (SDM
wording); that `wrmsr` returns without a fault in that case is
**unconfirmed** on hardware (a WinRAID user reports exactly that on a Gemini
Lake, secondary).

### Time window

SDM: `time = 2^Y × (1 + Z/4) × time_unit`, Y = 5 bits, Z = 2 bits.
Linux `rapl_default_compute_time_window()` (`intel_rapl_common.c`
lines 836–867) does the same in both directions; encode = `y = floor(log2(t /
unit))`, `z = floor(4 × (t/unit − 2^y) / 2^y)`, and returns `0x7F` when y
would exceed 31. coreboot's `power_limit_time_sec_to_msr[]` table
(`power_limit.c`) gives the same values for TU = 10 (e.g. 28 s → `0x6E`,
56 s → `0x6F`), which cross-checks the formula. **Verified.**

`MSR_PKG_POWER_INFO` (0x614) bits 53:48 hold the largest window allowed
(SDM §17.10.3). The proposal below never writes either time window.

### Worked examples (unit-test vectors)

All with units `0x000A0E03` (PU 3, TU 10). Computed with a small script from
the formulas above; each one round-trips.

| # | Input | Raw | Decoded |
|---|---|---|---|
| U1 | 0x606 = `0x000A0E03` | | 0.125 W, 61.035 µJ (1/16384 J), 976.5625 µs (1/1024 s) |
| T1 | time window raw `0x0A` | Y 10, Z 0 | 1024 units = 1 s |
| T2 | raw `0x0B` | Y 11, Z 0 | 2 s |
| T3 | raw `0x4B` | Y 11, Z 2 | 2048 × 1.5 = 3072 units = 3 s |
| T4 | raw `0x6E` | Y 14, Z 3 | 28 s (coreboot table) |
| T5 | raw `0x6F` | Y 15, Z 3 | 56 s (coreboot table) |
| T6 | encode 0.0025 s | `0x01` | 2 units = 1.953 ms (rounds down) |
| T7 | encode a huge time | `0x7F` | the cap, 2^31 × 1.75 units |
| P1 | PL1 125 W on, clamp, Tau 56 s; PL2 253 W on, clamp; TW2 0; unlocked | `0x000187E8_00DF83E8` | PL1 raw `0x3E8` (1000), PL2 raw `0x7E8` (2024) |
| P2 | "unlimited" board: PL1 = PL2 = field max, both on and clamped, Tau 56 s | `0x0001FFFF_00DFFFFF` | 4095.875 W each |
| P3 | P1 with the lock bit | `0x800187E8_00DF83E8` | `locked = true` |
| P4 | P1 with PL1 → 95 W and PL2 → 200 W, every other bit kept | `0x00018640_00DF82F8` | PL1 raw `0x2F8` (760), PL2 raw `0x640` (1600); Tau, enables and clamps unchanged |
| P5 | encode 87.3 W | raw `0x2BA` (698) | 87.25 W — round **down**, so a request never ends above what was asked |

P2 is what boards that advertise "unlimited" power limits are expected to
read (4095.875 W is the largest 15-bit value at PU 3) — **hypothesis**, to
be checked against the tester's read.

## Other registers worth reading

All on IntelMSR's read allow-list (0.2.11, `IntelMSR.p` lines 70–87),
**verified**:

| MSR | Name | Use |
|---|---|---|
| 0x611 | `MSR_PKG_ENERGY_STATUS` | package energy; LHM already turns it into "CPU Package" W |
| 0x613 | `MSR_PKG_PERF_STATUS` | bits 31:0: total time the package was throttled by RAPL limits ("availability … is platform specific", SDM) |
| 0x614 | `MSR_PKG_POWER_INFO` | 14:0 thermal spec power, 30:16 min, 46:32 max (power units), 53:48 max time window |
| 0x1B1 | `IA32_PACKAGE_THERM_STATUS` | bit 10 "Package Power Limitation Status": a package power limit is forcing the cores below the OS-requested P-state; bit 11 the sticky log (SDM vol. 3B §17.9, vol. 4) |

**Not allowed:** 0x64F `MSR_CORE_PERF_LIMIT_REASONS` (bit 10 PL1, bit 11
PL2/PL3 limiting, SDM vol. 4 Table 2-39) and 0x690. Calling `ioctl_read_msr`
with them returns `STATUS_ACCESS_DENIED` (`IntelMSR.p` line 110) — RIGStats
must not try. Getting 0x64F would take an upstream PR to PawnIO.Modules (there
is precedent: #109 added a read of 0x650) and a new signed release.

## PawnIO modules

### IntelMSR

**Verified** from `IntelMSR.p` at tag 0.2.11:

| ioctl | Input | Output | Behaviour |
|---|---|---|---|
| `ioctl_read_msr` | `[0]` = MSR (low 32 bits used) | `[0]` = 64-bit value | `STATUS_ACCESS_DENIED` unless on the read list; otherwise the `msr_read` NTSTATUS |
| `ioctl_write_msr` | `[0]` = MSR, `[1]` = value | none | `STATUS_ACCESS_DENIED` unless on the write list; otherwise the `msr_write` NTSTATUS |

`main()` refuses to load on anything but x64 Intel (`STATUS_NOT_SUPPORTED`,
lines 137–145). Both sizes are fixed by `DEFINE_IOCTL_SIZED` (1/1 and 2/0), so
the existing `PawnIoModule.Execute(fn, input, outCount)` fits as it is.

- **Read list:** 0x8B is master-only; at 0.2.11: 0x1B1, 0x198, 0x1A2, 0x19C,
  0xE7, 0xE8, 0x601, 0x610, 0x611, 0x613, 0x614, 0x618, 0x619, 0x61B, 0x61C,
  0x620, 0x621, 0x638, 0x639, 0x63A, 0x63B, 0x640, 0x641, 0x642, 0x64D, 0x606,
  0xCE, 0x2A, 0x1AD, 0x150, 0x607, 0x608, 0x1A4 (0x650 is master-only too).
- **Write list:** 0x601 (`VR_CURRENT_CONFIG`), **0x610**, 0x150 (OC mailbox),
  0x607/0x608 (VR mailbox), 0x1A4 (prefetchers). RIGStats would write 0x610
  only; the others stay unused.

`msr_write` in the driver is `__writemsr` inside `__try/__except`, returning
the exception code as the NTSTATUS (`natives_impl_windows.cpp` lines
457–467) — **verified**. Whether a #GP from a bad value is really caught there
in kernel mode is **unconfirmed**; RIGStats only writes values it read from
the same register with two fields changed, so it should never send a value
the CPU rejects.

The MSR is executed on whatever core the calling thread is on. 0x606, 0x610,
0x611, 0x614 are package-scoped (SDM vol. 4 "Scope: Package"; Linux:
"Package-scoped MSRs are readable from any CPU in the package",
`intel_rapl_msr.c` `rapl_msr_read_raw()`), so no affinity is needed on a
single-socket desktop. **Verified.**

LHM's own `IntelMsr` wrapper takes no mutex around MSR reads (`IntelMsr.cs`);
RIGStats' write needs none either — one `wrmsr` is atomic and LHM never
writes 0x610. **Hypothesis**, low risk.

### Which binary

**Verified** by extracting the manifest resources from the NuGet packages in
the local cache and hashing them against the release zip:

| Blob | Size | SHA-256 | Exports |
|---|---|---|---|
| LHM 0.9.6 `IntelMSR.bin` | 4068 | `ed41a0d0…54b77a9` | `ioctl_read_msr` only |
| LHM 0.9.7-pre749 `IntelMSR.bin` | 5324 | `d6ed85d6…c9b92e5f` | `ioctl_read_msr`, `ioctl_write_msr` |
| PawnIO.Modules 0.2.11 `release_0_2_11.zip` → `IntelMSR.bin` | 5324 | `d6ed85d6…c9b92e5f` | same file |
| PawnIO.Modules 0.2.11 → `IntelMCHBAR.bin` | 13332 | `3f82b832…0e8a6eae` | not in LHM |

LHM's `Resources/PawnIo/README` says the same: "Signed PawnIO modules from the
official repository, belonging to the 0.2.11 release". 0.2.11 (2026-08-30) is
the latest PawnIO.Modules release.

So the Intel provider loads LHM's resource, like `RyzenSmu` does. Because
the write path depends on which LHM is referenced (0.9.6 would silently
drop it), the provider should **pin the blob's SHA-256** and offer Intel
limits only when it matches — the same idea as `STDUTILS_DLL_SHA256` in the
installer. That also avoids ever calling an ioctl the loaded module may not
have.

### Signing

PawnIO only loads modules signed by its author (`PawnIoModule.cs` doc comment;
PawnIO.Modules README: "Signed builds can be found in Releases"; the CI only
builds unsigned `.amx`). The 0.2.11 blobs carry that signature — LHM ships
them and PawnIO loads them on users' PCs today. **Verified** for IntelMSR via
the hash match; that `IntelMCHBAR.bin` loads is **unconfirmed** until it runs
on a machine.

### Licence

PawnIO.Modules is LGPL-2.1-or-later (file headers, `COPYING`). LHM ships the
modules as embedded resources and the LGPL text in its
`THIRD-PARTY-NOTICES.txt` ("License notice for PawnIO.Modules"). The
PawnIO.Modules wiki ("Using PawnIO Modules") just says to take the release
binary and "include its contents in your software"; it says nothing about
notices. **Verified.**

RIGStats already ships LGPL modules today (inside LHM's DLL, and loads
`RyzenSMU.bin` directly), but the installer ships no third-party notice file
(`build/installer.nsi` lines 167–190: the exes, `CHANGELOG.md`, PawnIO's
setup). Using `IntelMSR.bin` from LHM changes nothing here. Embedding
`IntelMCHBAR.bin` directly would make RIGStats a distributor of that file on
its own; the conservative way is the StdUtils one (`build/nsis-plugins/StdUtils/README.md`):
unmodified, hash-pinned, `COPYING` beside it, and a notice. That a notice for
LHM and PawnIO.Modules is owed already today is a **hypothesis** (not legal
advice) — see the open questions.

### IntelMCHBAR

**Verified** from `IntelMCHBAR.p` (added 2026-05, in 0.2.11):

- `main()` checks Intel x64, maps CPU family/model to a code name (Sandy
  Bridge … Nova Lake, incl. `0x06C6` Arrow Lake) and refuses unknown ones;
  reads the MCHBAR base from PCI 0:0.0 offsets 0x48/0x4C (enable bit 0) and
  maps 32/64/128 KB of it.
- `ioctl_read_dword` (`[0]` = offset, 4-aligned), `ioctl_read_qword`
  (`[0]` = offset, 8-aligned), `ioctl_get_mchbar_addr` (no input). Offsets
  outside the mapping return `STATUS_ACCESS_DENIED`.
- **No write ioctl.**

0x59A0 is 8-aligned, so `ioctl_read_qword(0x59A0)` reads the whole mirror.

## The MCHBAR mirror

**Register.** MCHBAR+0x59A0 `PACKAGE_RAPL_LIMIT_0_0_0_MCHBAR_PCU` (13th gen
datasheet vol. 2, **verified**): PL1 14:0, PL1 enable 15, PL1 clamp 16, PL1
time window 23:17 (`1.x × 2^y`, x = bits 23:22, y = bits 21:17), PL2 46:32,
PL2 enable 47, lock 63 ("typically set by BIOS during boot time or resume
from Sx"). Unlike 0x610 it has **no PL2 clamp bit and no second time window**
(bits 62:48 reserved). Units: the same `PWR_UNIT` (the datasheet points to the
package power SKU unit MSR); Linux reads units from MCHBAR+0x5938
(`processor_thermal_rapl.c` line 86) — the read-only test should compare that
with 0x606.

**Who writes it.** coreboot writes the same 64-bit value to 0x610 and to
MCHBAR 0x59A0/0x59A4 at every boot (`power_limit.c` lines 179–183), and has a
config to disable RAPL through MCHBAR instead of the MSR. Linux exposes the
mirror as a separate powercap zone (`intel-rapl-mmio`) through the processor
thermal device (B0:D4:F0) on Alder Lake, Raptor Lake, Meteor Lake, Lunar Lake,
Arrow Lake-S (`0xAD03`), Panther Lake and others
(`processor_thermal_device_pci.c` lines 481–509). **Verified.** Which
consumer boards set the MMIO value differently from the MSR is
**unconfirmed**: no list exists; the forum reports are mostly laptops.

**Which limit wins.** ThrottleStop forum posts say "the lowest active limit
wins" (a 45 W MMIO PL1 overriding a higher MSR setting) and ThrottleStop
offers "Sync MMIO" (copy the MSR values to MMIO) and "MMIO Lock" (since 9.4).
**Unconfirmed** — user reports, no Intel document read here says it.

**What it means for RIGStats.** RIGStats only lowers. Let `msr` and `mmio`
be the BIOS values at boot and `eff = min(msr, mmio enabled ? mmio : ∞)`:

- a target ≤ `eff` should take effect;
- a target between `eff` and `msr` is accepted by the MSR and does nothing.

So the fix is to make the ceiling `eff`, not `msr`, when the mirror can be
read (IntelMCHBAR), or to say in the UI that values above the board's
second limit may have no effect when it can't. **Hypothesis**, resting on
the "lower wins" rule; the write test below checks it.

**Detecting a write with no effect by behaviour** (without MCHBAR): under a
sustained all-core load, after Tau, package power (0x611 → LHM "CPU
Package") should settle at or below PL1, 0x1B1 bit 10 should be set, and
0x613 should count up. Power well above the new PL1 with bit 10 clear means
the limit is not in force. This needs a load, so it suits a live test, not
a runtime check. **Hypothesis** (the bit and counter semantics are verified
from the SDM; that they show this cleanly on a desktop part is not).

## Other blockers

- **Plundervolt (CVE-2019-11157, INTEL-SA-00289)** is about "voltage
  settings" ([advisory](https://www.intel.com/content/www/us/en/security-center/advisory/intel-sa-00289.html));
  the published attack and the BIOS "undervolt protection" options act on
  the OC mailbox (0x150), not RAPL. No source ties it to 0x610.
  **Hypothesis** (by absence).
- **Hybrid (P/E-core) CPUs.** 0x610 is one package register; P- and E-cores
  share the budget. No per-core handling, no affinity. **Verified** (scope
  "Package", SDM vol. 4). LHM already reads the 285K's RAPL counters through
  IntelMSR (fixture). **Verified.**
- **Laptops and OEM tools.** On laptops the limits are owned by the firmware
  and its helpers: Intel Dynamic Tuning (DTT/DPTF) drives the processor
  thermal device (datasheet: "D4:F0 Dynamic Tuning Technology Registers"),
  ASUS sets PL1/PL2 through ATKACPI (G-Helper's `AsusACPI.cs`: `0x001200A3`
  SPL/PL1, `0x001200A0` sPPT/PL2 — GPL, read as documentation only; #234),
  and MSI Center / Dragon Center and Lenovo Vantage have their own modes.
  Any of them may rewrite 0x610 or MMIO at runtime and undo RIGStats' value.
  **Hypothesis** (well-known behaviour, not checked here). Leave laptops out,
  as phase 3 does for laptop GPUs. Mobile code names (`_L`, `_P`, `_H`, `_U`,
  Lunar Lake) are easy to exclude; HX laptop chips share the desktop model
  numbers (`0x97`, `0xB7`, `0xBF`), so the code name alone is not enough —
  a battery or chassis check is also needed (open question).
- **Core Ultra.** Meteor Lake is mobile only. Arrow Lake-S desktop (`0xC6`)
  uses Linux's plain `rapl_defaults_core` (MSR 0x610, no MSR PL4;
  `intel_rapl_msr.c` line 464), with MMIO RAPL via its thermal device
  `0xAD03`; Arrow Lake-H/U get PL4 through 0x601. TPMI RAPL is server-only.
  **Verified** (Linux tables). Intel's published 285K limits (125 W base /
  250 W maximum turbo) are **unconfirmed** here; the read-only test shows
  what the tester's board sets.
- **Other software writing the same register.** Intel XTU, ThrottleStop and
  board tools (Gigabyte Control Center, MSI Center) can write 0x610. Following
  "Yield, never fight", they belong in `OtherSoftware` as `Verified: false`
  rows until their process names are seen on a machine. **Unconfirmed** names.
- **Sleep.** The datasheet says the BIOS typically sets the MMIO lock "during
  boot time or resume from Sx", so after S3/S4 resume the BIOS has probably
  rewritten 0x610 too and RIGStats' limit is gone. **Hypothesis.** The service
  has no resume hook today (only `FanWatchdog` tracks resumes); the AMD SMU
  limits have the same question. Modern Standby (S0ix) is not a firmware
  resume and should keep the value — also **hypothesis**.
- **PL4 / 0x601 (`VR_CURRENT_CONFIG`).** On IntelMSR's write list; Linux
  uses it as PL4 on the parts in `rapl_defaults_core_pl4`. Mentioned only —
  out of scope.

## Proposed design

Fits the existing AMD provider (`CpuLimitProvider` + `RyzenSmu`), the safety
rules in `sensor-sidecar/Control/CLAUDE.md`, and #209's checklist. Nothing
here is built.

### Module and gating

- `IntelRapl` (new, beside `RyzenSmu`): `PawnIoModule.LoadFromLhm("IntelMSR.bin")`
  after checking the resource's SHA-256 against the pinned 0.2.11 hash. The
  pure part (`IntelRaplMath`: units, decode, encode, time window, RMW) is
  static and unit-tested with the vectors above.
- Offer it only when all of these hold: CPUID vendor GenuineIntel; family 6
  with a **verified** model (start empty; Arrow Lake-S `0x06C6` after the
  tester's run); a desktop (no battery / desktop chassis); the blob hash
  matches. Anything else: `Supported = false` with a `Reason` (the PC simply
  doesn't offer it — no `Problem`), and the 0x606/0x610/0x614 raw values
  logged once so a diagnostics ZIP becomes a fixture (like the PM table head
  for AMD).
- `IntelMCHBAR.bin` (optional, read-only): embed the signed 0.2.11 blob
  hash-pinned with its `COPYING`; read 0x59A0 (and 0x5938 for units) once at
  probe. If it is left out, the UI uses the hedged text below instead.

### Capability (`cpu_limit` domain, `details`)

```json
{
  "vendor": "intel",
  "generation": "Arrow Lake-S",
  "units": { "power_w": 0.125, "time_s": 0.0009765625 },
  "locked": false,
  "stock":   { "pl1_w": 125, "pl2_w": 250, "tau_s": 56, "pl1_enabled": true, "pl2_enabled": true },
  "mmio":    { "pl1_w": 125, "pl2_w": 250, "pl1_enabled": true, "pl2_enabled": true },
  "ceiling": { "pl1_w": 125, "pl2_w": 250 },
  "min":     { "pl1_w": 35,  "pl2_w": 35 },
  "current": { "pl1_w": 125, "pl2_w": 250 }
}
```

(`mmio` is absent when the mirror isn't read; the numbers are illustrative.)
`CpuLimitPart.Intel` (today a `JsonNode` passthrough, `ControlProtocol.cs`
line 295) becomes `IntelCpuLimit { Pl1W, Pl2W }`, both nullable, null = BIOS
value — the same contract as `AmdCpuLimit`, pinned by a shared JSON fixture.

### Values

- **Baseline** = the raw 64-bit 0x610 read before the first write of this
  boot, persisted like `cpu-limit-baseline.json` (keyed by boot time,
  `CpuLimitProvider.LoadBaseline`), plus the MMIO raw value when read. MSRs
  reset on every reboot, so per-boot is right. (The sleep question above may
  need the baseline re-read after a resume — open.)
- **Ceiling** = the BIOS value: per limit, `min(msr, mmio)` when the mirror
  was read and enabled, otherwise `msr`. A limit the BIOS left **disabled**
  (enable bit 0) has no ceiling of its own: use `MSR_PKG_POWER_INFO` max
  power if non-zero, else the field max (4095.875 W at PU 3), and show it as
  "no limit".
- **Floor** = `min(35 W, ceiling)` for both (Intel's own lowest desktop
  TDP class is 35 W "T" parts — a usability floor, not a safety one: lower
  power can't harm the CPU). Open: whether to use 0x614's minimum power when
  it is non-zero.
- **Resolve** (pure, like `CpuLimitProvider.Resolve`): null → ceiling;
  otherwise clamp to `[floor, ceiling]`, then round **down** to power units;
  then `PL2 = max(PL2, PL1)` (coreboot enforces the same, `power_limit.c`
  line 166).
- **Validate:** reject non-finite or ≤ 0 values (as for AMD); clamping is in
  Resolve, at write time.

### Write

1. Read 0x610. If bit 63 is set → no write; `DomainUnavailableException`
   ("locked by the BIOS") so the rest of the profile still applies.
2. If the target equals what is in force → no write (no crash-guard window).
3. `BootCrashGuard.Arm()` when the target is not the baseline.
4. New value = the value just read with **only** bits 14:0 and 46:32 replaced,
   bit 15 / 47 set for a limit RIGStats now sets, and bit 63 **forced to 0**
   — RIGStats never locks the register (a lock would make the way back
   impossible until a reboot). Clamp bits and both time windows are kept as
   the BIOS set them. Dry-run logs instead.
5. `ioctl_write_msr(0x610, value)`.

### Verify

Read 0x610 back and compare the two fields exactly (they are integers — no
tolerance needed). A mismatch with the lock bit clear means something else
rewrote it (OEM tool, firmware) → log both raw values and fail the apply, as
for AMD. If MMIO was read and the target sits above it, report that the value
"may have no effect" rather than failing.

### Restore and crash

- `Restore(snapshot)` writes the snapshot's raw value back (fields only, same
  RMW rule).
- `ReleaseToFirmware()`: only if this service wrote; writes the **baseline
  raw fields** back (not re-encoded watts), never throws.
- Crash/kill: the register keeps RIGStats' (lower) value until the next
  reboot or until the restarted service releases it — the same known limit
  as AMD, in the safe direction. `BootCrashGuard`'s marker makes the next
  start apply the BIOS values.

### UI text (proposals)

- Locked: "Your BIOS has locked the CPU power limits, so RIGStats can show
  them but not change them. The setting is usually called *Package Power
  Limit Lock* in the BIOS." (The BIOS option name is **unconfirmed** — it
  varies by vendor.)
- MCHBAR read and lower than the MSR: "Your motherboard enforces its own
  limit of *X* W. Values above that have no effect." The slider's top is
  then *X*.
- MCHBAR not read: a short line under the sliders: "Some motherboards enforce
  a second limit that RIGStats can't change; if a value has no effect, the
  board's own limit is lower."
- Rewritten by another program: "Another program changed the CPU power
  limits." with the `OtherSoftware` conflict when one is known.
- CPU panel: `PL1 … W` (and `PL2` in the tooltip) when the capability is
  supported — mirrors the AMD `PPT` line.

### Mapping to #209

| #209 item | Finding / proposal |
|---|---|
| Embed the signed upstream `IntelMSR.bin` | **Not needed** — LHM 0.9.7-pre749 already contains the identical 0.2.11 blob. Load it from LHM and pin its hash. Optional new embed: `IntelMCHBAR.bin` (read-only) |
| Units from 0x606, read/write 0x610, pure encode/decode with tests | Layout and vectors above (U1, T1–T7, P1–P5) |
| Lock bit 63 → "locked by BIOS" | `locked: true`, read-only tab, writes skipped with `DomainUnavailableException`; never set bit 63 |
| Ceiling = BIOS at boot, floor, clamp in Validate | Raw baseline per boot; ceiling `min(MSR, MMIO)`; floor 35 W; clamp in Resolve at write time; PL2 ≥ PL1 |
| Intel UI in the CPU tab; panel `PL1 … W` | PL1 + PL2 sliders, Tau shown read-only |
| Document the MCHBAR limitation in the UI | Exact text when MMIO is read; hedged text otherwise |
| Test on real Intel hardware | Plan below |

## Live-test plan

The owner's machines are AMD. The only Intel tester is SirBlacky21: Gigabyte
Z890 EAGLE WIFI7, Core Ultra 9 285K (family 6 model `0xC6`), RTX 5070 Ti
(fixture `sensor-sidecar.Tests/fixtures/gigabyte-z890-ultra9-285k-rtx5070ti/meta.json`).
He once declined a power-limit test, so step 1 writes **nothing** and step 2
is a separate request he can decline. Each step is shipped as a release
candidate (`/pre-release`), since he runs the installed app, not the dev
sidecar.

**Step 1 — read-only (no writes anywhere).** The RC's service, at start,
logs once (only allow-listed reads, all documented above):

- CPUID vendor/family/model, the `IntelMSR.bin` SHA-256;
- raw 0x606, 0x610, 0x614, 0x1B1, 0x613;
- if `IntelMCHBAR.bin` is embedded: MCHBAR+0x59A0 (qword) and +0x5938.

He then:

1. Installs the RC, waits a minute, uses **Collect diagnostics…** and sends the
   ZIP.
2. Writes down what his BIOS shows for the power limits (PL1/PL2 or "Intel
   Default Settings" profile, any "lock" option), with a photo if easier.
3. Optional: if he already has HWiNFO or ThrottleStop, a screenshot of the
   power-limit readout (read-only views only — no changes in those tools).

What it answers: the units, the BIOS values and lock state, whether MMIO
equals MSR, whether `IntelMCHBAR.bin` loads, the P2-style "unlimited"
question. The raw values become a fixture
(`sensor-sidecar.Tests/fixtures/intel-rapl/…`) that the decode tests read.

**Step 2 — one reversible write, only if he agrees.** Only after step 1 shows
`locked = false`. The RC offers the CPU tab for model `0xC6`:

1. Start a sustained all-core load (Cinebench R23 multi-core, 10 min mode)
   and note "CPU Package" power in RIGStats (expected near his PL2/PL1).
2. In the CPU tab, set PL1 and PL2 to 150 W and press **Try new limits**
   (15 s preview, auto-revert).
3. Within the 15 s, "CPU Package" should drop to about 150 W or below
   (PL2 acts within milliseconds-to-seconds; **hypothesis**). Then let it
   revert, and check that power goes back up.
4. Collect diagnostics again: the log shows the write, the read-back, the
   release, and 0x610 read back equal to the baseline raw value.

Stop and report if anything looks wrong — the PC needs only a reboot to be
back at BIOS values in any case (MSRs reset; RIGStats never sets the lock).

**Step 3 — profile and restart behaviour** (later, same consent): apply a
profile with PL1 150 W, restart the RIGStats service (Services or reboot
prompt), check the limit is restored to the BIOS value at stop and
re-applied at start; sleep and wake the PC and note whether the limit is
still 150 W (answers the sleep question).

## What RIGStats will and won't send

**Will (only on an offered CPU, only on an apply):**

| Access | Why |
|---|---|
| Read 0x606, 0x610, 0x614 | Units, current limits, power range — at probe |
| Read 0x611, 0x613, 0x1B1 | Diagnostics and the live test only |
| Write 0x610: only bits 14:0, 15, 46:32, 47 changed, bit 63 always 0 | Apply, restore, release |
| Read MCHBAR+0x59A0, +0x5938 (if IntelMCHBAR is embedded) | The board's mirror, once at probe |

**Won't:**

- Any MSR not in the table — in particular not 0x64F/0x690 (denied by the
  module anyway), not 0x601, 0x150, 0x607/0x608, 0x1A4 even though the
  module would allow writing them.
- Set the lock bit, change the clamp bits or either time window.
- Raise a limit above the BIOS value.
- Any MCHBAR write (impossible with the module, and not wanted).
- Anything on a laptop, an unverified model, or with an unknown module hash.

## Open questions

1. Does the effective limit equal `min(MSR, MMIO)` on the tester's Z890, and
   does an MSR write change the MMIO copy? (Step 1 reads both; step 2 shows
   the effect.)
2. What do 0x606 and 0x610 read on the 285K — `0x000A0E03`? Locked? The
   "unlimited" 4095.875 W pattern?
3. Does `IntelMCHBAR.bin` load and read on that board, and are its units
   (0x5938) the same as 0x606? Is it worth embedding (new binary + licence
   notice) given that limits only go down?
4. After S3/S4 resume, is 0x610 back at the BIOS value? If so, the service
   needs a resume hook to re-apply — and the same question applies to the AMD
   SMU limits.
5. Is a third-party notice for LHM (MPL-2.0) and PawnIO.Modules (LGPL-2.1)
   owed already today, and where should it go (installer folder, About
   dialog)? The owner decides; this is not legal advice.
6. How to tell an HX laptop from a desktop with the same model number in the
   service (battery presence, SMBIOS chassis type, or both)?
7. Floor: a fixed 35 W, or `MSR_PKG_POWER_INFO` minimum power when non-zero?
8. Process names of Intel XTU, ThrottleStop and the board tools for
   `OtherSoftware`.
9. Is `wrmsr` with the lock bit set silent on these parts (no #GP), as the
   SDM's "ignored" suggests?
10. Should PawnIO.Modules be asked to add 0x64F to IntelMSR's read list, for a
    clean "limited by PL1/PL2" indicator?
