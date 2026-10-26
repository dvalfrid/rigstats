# Corsair V2 peripheral protocol: M65 RGB Ultra (wired) and MM700

Protocol notes for Corsair lighting (#620, step 1), 2026-10-25.

**Implemented in** `sensor-sidecar/Control/Lighting/CorsairV2Device.cs`
(#620, step 2), frames pinned byte for byte by
`sensor-sidecar.Tests/CorsairV2Tests.cs`; both models `Verified = false`.
Choices on the open points: the endpoint is checked the way OpenRGB does it
(START `22`, `01` only on a non-zero status; no reply at all stays on `22`,
as OpenRGB's zeroed buffer would), GET `5F` is sent, GET `12` / `11` / `41`
are not, there is no keep-alive, and SET mode = hardware is sent on release
only when RIGStats switched the device to software mode. The first use logs
the replies (endpoint, mode, `5F`) to answer questions 1, 3 and 5 below.
Still **nothing has been sent to a real device**. Everything below
was read from OpenRGB's `Controllers/CorsairPeripheralV2Controller/`
(GPL-2.0-or-later, master at `f7e5cae9`, 2026-10-25) as documentation; no
code is copied. Facts are cited as *file, function*; anything the source
does not settle is marked **unconfirmed**.

Scope: static colour and off on two wired devices only.

| Device | VID:PID | OpenRGB name | OpenRGB controller |
|---|---|---|---|
| M65 RGB Ultra (wired) | 1B1C:1B9E | `CORSAIR_M65_RGB_ULTRA_WIRED_PID` | `DetectCorsairV2SoftwareControllers` → `CorsairPeripheralV2SWController` + `RGBController_CorsairV2SW` |
| MM700 RGB mousepad | 1B1C:1B9B | `CORSAIR_MM700_PID` | the same |

Both are registered with `REGISTER_HID_DETECTOR_IP(…, 1, 0xFF42)`
(`CorsairPeripheralV2ControllerDetect.cpp`): interface 1, usage page
`0xFF42`. The tester's HID scan
(`sensor-sidecar.Tests/fixtures/hid-scan/gigabyte-z890-ultra9-285k-rtx5070ti.json`)
has exactly one such collection per device: interface 1, usage page `FF42`,
usage `0001`, input 65 / output 65 bytes, output report id `0`. The other
`FF42` collections (usage `0002`, input only: MM700 interface 0, M65
interface 2) are not used.

Out of scope here: the wireless path (the SLIPSTREAM receiver, M65 RGB Ultra
Wireless `1BB5`), the hardware-mode controller (`CorsairPeripheralV2HWController`,
used by other models), keyboards, and onboard effects.

## Framing

Every message is one 65-byte output report: report id `00` and 64 data
bytes, zero-filled (`memset` in every sender in
`CorsairPeripheralV2Controller.cpp`). Byte numbers below are **report
bytes** (byte 0 = report id), which is the same as OpenRGB's `buffer[n]`
index for writes (hidapi's `hid_write` takes the report id as byte 0).

| Byte | Meaning | Source |
|---|---|---|
| 0 | Report id `00` | hidapi convention; every buffer is zeroed first |
| 1 | `08` — the "write" id for a wired device (`CORSAIR_V2_WRITE_WIRED_ID`). `09` is the wireless one (via a receiver), `80` is the K57 wired; neither applies here | `CorsairPeripheralV2Controller.h`; constructor `CorsairPeripheralV2Controller()` picks it |
| 2 | Command | `enum corsair_v2_cmd`, `CorsairPeripheralV2Controller.h` |
| 3… | Arguments, per command (below) | |

Commands OpenRGB sends to these two devices (`enum corsair_v2_cmd`):

| Byte 2 | OpenRGB name | Comment in the source |
|---|---|---|
| `01` | `CORSAIR_V2_CMD_SET` | "Command for setting values" |
| `02` | `CORSAIR_V2_CMD_GET` | "Command for getting values" |
| `05` | `CORSAIR_V2_CMD_STOP_TX` | "Finish Transaction" |
| `06` | `CORSAIR_V2_CMD_BLK_W1` | "Block write packet 1" |
| `07` | `CORSAIR_V2_CMD_BLK_WN` | "Block write remaining packets" — never needed for these two devices (see "Colour write") |
| `0D` | `CORSAIR_V2_CMD_START_TX` | "Start Transaction" |

### Replies

After most writes OpenRGB reads one input report with
`hid_read_timeout(…, 65, 50 ms)` (`CORSAIR_V2_TIMEOUT` = 50). On Windows,
hidapi drops the leading report id `00` from an input report, so OpenRGB's
`read[n]` is report byte `n + 1` as `ReadFile` returns it (hidapi
`windows/hid.c` behaviour, not OpenRGB; **unconfirmed on these devices**).
In report bytes:

| Report byte | Meaning | Source |
|---|---|---|
| 0 | Report id `00` | |
| 1, 2 | Not interpreted by OpenRGB (only logged). Probably an echo of `08` and the command — **unconfirmed** | `GetAddress()` logs `read[0]`, `read[1]` |
| 3 | Status: `00` = OK; `01` "Invalid Value", `03` "Failed", `05` "Unsupported", anything else "Protocol Error (Unknown)" | `GetAddress()` (`read[2]`), `StartTransaction()` returns `buffer[2]`; texts from `GetErrorString()` |
| 4–7 | For a GET: the value, 32-bit little-endian | `GetAddress()`: `read[3]` is the low byte, `read[6]` the high byte |

`GetAddress()` returns `NA` (`0xFFFFFFFF`) when the status is non-zero. No
write path checks the reply beyond the status of `StartTransaction()`; the
other reads only drain the reply.

## Frames

All 65 bytes; bytes not shown are `00`.

| Frame | Report bytes 0– | Read reply? | Source |
|---|---|---|---|
| GET property `p` | `00 08 02 p` | yes, 50 ms | `GetAddress()`; `LightingControl()` is the same frame with `buffer[5] = 00` |
| SET render mode | `00 08 01 03 00 m` — property `03` (`CORSAIR_V2_VALUE_MODE`), value `m` at byte 5: `01` hardware (`CORSAIR_V2_MODE_HW`), `02` software (`CORSAIR_V2_MODE_SW`) | yes, 50 ms | `SetRenderMode()`; `enum corsair_v2_device_mode` in `CorsairPeripheralV2Devices.h` |
| Start transaction | `00 08 0D 00 e` — byte 3 `opt1` = `00` in every call, byte 4 the lighting endpoint `e`: `22` (`CORSAIR_V2_LIGHT_CTRL2`, the default) or `01` (`CORSAIR_V2_LIGHT_CTRL1`) | yes, 50 ms; status at report byte 3 | `StartTransaction()` |
| Stop transaction | `00 08 05 01 00` — byte 3 is always `01`, byte 4 `opt1` = `00` | yes, 50 ms | `StopTransaction()` |
| Block write, first packet | `00 08 06 00 Ll Lh 00 00 d0 d1 …` — total data length as 16-bit little-endian at bytes 4–5, data from byte 8, at most 57 bytes in this packet | one read with a **3 ms** timeout; if nothing came, sleep 2 ms | `SetLEDs()` (`offset1` = 8, `CORSAIR_V2_TIMEOUT_SHORT` = 3, `CORSAIR_V2_WRITE_PACE` = 2) |
| Block write, next packets | `00 08 07 00 d…` — data from byte 4, at most 61 bytes; the tail of the last one zeroed | none; sleep 2 ms after each | `SetLEDs()` (`offset2` = 4) |

Bytes 6–7 of the first block packet are left `00`; whether the length is
really 32-bit (bytes 4–7) is **unconfirmed** — OpenRGB only writes 4–5.
OpenRGB names byte 3 of START/STOP only `opt1`; reading `0D 00 e` as "open
handle 0 on resource `e`" and `05 01 00` as "close handle 0" is an
interpretation, **unconfirmed** by the source.

## Colour data

The software controller lays the colours out **planar**: all reds, then all
greens, then all blues, one byte each, `count × 3` bytes, no header
(`CorsairPeripheralV2SoftwareController.cpp`, `SetLedsDirect()`). It does
this whatever endpoint (`22` or `01`) was chosen. (The hardware controller's
`SetLedsDirectTriplets()` — a `12 00` prefix and interleaved RGB on endpoint
`22` — is not used for these two devices.)

LED count and order: each device has three single-LED zones
(`CorsairPeripheralV2Devices.cpp`). `RGBController_CorsairV2SW::SetupZones()`
gives each LED the index `leds.size()` in zone order, and `buffer_map`
sends them in that order:

| Index | M65 RGB Ultra wired (`m65_rgb_ultra_wired_device`) | MM700 (`mm700_device`) |
|---|---|---|
| 0 | Logo | Left |
| 1 | Scroll Wheel | Right |
| 2 | Indicator | Logo |

So a colour write is 9 bytes, `R0 R1 R2 G0 G1 G2 B0 B1 B2`, and always fits in
the first block packet:

```
00 08 06 00 09 00 00 00 R0 R1 R2 G0 G1 G2 B0 B1 B2 00 … 00   (65 bytes)
```

Which physical LED each index is has not been seen on a device —
**unconfirmed**. The MM700's comment block in `CorsairPeripheralV2Devices.cpp`
describes a different layout ("Logo" single, "Edge" linear 1 × 2) than the
zones it defines (Left, Right, Logo); the count is three either way. The
M65's "Indicator" is presumably the DPI indicator — **unconfirmed**.

Brightness: the software path has none; colours are sent as they are.

**Off** has no command of its own in OpenRGB's software path: it is the same
colour write with all nine bytes `00`. **Static colour** is the colour write
with the same R, G, B for all three LEDs — OpenRGB's only mode for these
devices is "Direct" (`RGBController_CorsairV2SW` constructor;
`DeviceUpdateMode()` is empty, so `UpdateHWMode()` is never reached).

## Order of operations in OpenRGB

### Opening (detection)

`DetectCorsairV2SoftwareControllers()` opens the collection with
`hid_open_path` and builds a `CorsairPeripheralV2SWController`. For a wired
M65 RGB Ultra or MM700 that sends, in order:

1. **GET `12`** (the device's PID) — `CorsairPeripheralV2Controller()` via
   `GetAddressRetry(0x12)`: up to 8 tries (`CORSAIR_V2_PID_RETRIES`), 120 ms
   apart (`CORSAIR_V2_PID_RETRY_DELAY`), until the value is neither `NA` nor
   0 ("A device that has just enumerated does not answer the PID query
   straight away"). Neither PID is a receiver or the K57, so the write id
   stays `08`.
2. **GET `11`** (VID) — constructor; reply read with a 1024-byte buffer,
   50 ms, and otherwise ignored. (In this one call OpenRGB's buffer is not
   zeroed first; RIGStats would send zeros.)
3. The PID from step 1 is looked up in `corsair_v2_device_list`; an unknown
   PID ends here and the detector drops the device
   (`IsDeviceSupported()`).
4. **Endpoint check** — constructor: START `0D 00 22`; if the status byte is
   non-zero, switch to endpoint `01` and START `0D 00 01`; then STOP
   `05 01 00`. Which endpoint the M65 RGB Ultra and MM700 end up on is
   **unconfirmed** (a device log would show it: OpenRGB logs "Lighting
   Endpoint set to %02X").
5. **SET mode = software** `01 03 00 02` — `CorsairPeripheralV2SWController()`
   via `SetRenderMode(CORSAIR_V2_MODE_SW)`.
6. **GET `5F`** — `CorsairPeripheralV2SWController()` via
   `LightingControl(0x5F)`. The source calls it an initialisation packet
   ("the registers and options differ for each peripheral"); what `5F` is,
   and whether software lighting works without it, is **unconfirmed**.
7. **GET `41`** (keyboard layout) — `RGBController_CorsairV2SW::SetupZones()`
   via `GetKeyboardLayout()`. Sent to mice and mousepads too, but the value
   only matters for a matrix (keyboard) zone; neither device has one.
8. A keepalive thread starts (below).

### Writing colours

`RGBController_CorsairV2SW::DeviceUpdateLEDs()` →
`CorsairPeripheralV2SWController::SetLedsDirect()` →
`CorsairPeripheralV2Controller::SetLEDs()`:

1. Drain pending input reports: read with a 3 ms timeout until none comes
   (`ClearPacketBuffer()`).
2. START `00 08 0D 00 e` (endpoint from opening step 4), read 50 ms.
3. Block write first packet with the 9 colour bytes, read 3 ms (or sleep
   2 ms).
4. STOP `00 08 05 01 00`, read 50 ms.

### Keepalive

`RGBController_CorsairV2SW::KeepaliveThread()` checks every 12.5 s
(`CORSAIR_V2_SLEEP_PERIOD`) and sends the current colours again when the last
write is more than 30 s old (`CORSAIR_V2_UPDATE_PERIOD` = 30000 ms). The
reason given is the K55 RGB PRO, which "requires a packet within 1 minutes
… in order to not revert back into rainbow mode"; the thread runs for every
software-controller device. Whether the M65 RGB Ultra or MM700 revert
without it is **unconfirmed**.

### Closing (handing back to the firmware)

`~RGBController_CorsairV2SW()` stops OpenRGB's update thread (`Shutdown()`)
and the keepalive thread, then deletes the controller.
`~CorsairPeripheralV2Controller()` sends **SET mode = hardware**
`00 08 01 03 00 01` (`SetRenderMode(CORSAIR_V2_MODE_HW)`), reads 50 ms, and
closes the handle. Its comment: some devices (K70 Core RGB) stop sending
onboard input events while left in software mode, "and the mode persists
until the next USB enumeration if it is never reverted". No STOP or other
frame is sent on close. What the device shows after going back to hardware
mode (presumably its stored onboard lighting) is **unconfirmed**.

## What RIGStats will and won't send

The safety rules in `sensor-sidecar/Control/CLAUDE.md` apply: only
documented commands, no probing, always a way back.

**Will send** (all from OpenRGB's code path for exactly these two PIDs, and
only after a profile asks for lighting — discovery opens the collection and
sends nothing, as for the other families):

| Frame | Why |
|---|---|
| SET mode `01 03 00 02` (software) | Take control, once per session |
| GET `5F` | OpenRGB's initialisation step after the mode switch; kept until a test shows it isn't needed |
| START `0D 00 22`, falling back to `0D 00 01` only on a non-zero status | Endpoint check, the way OpenRGB does it |
| START / block write first packet (9 bytes) / STOP | Every colour or off |
| SET mode `01 03 00 01` (hardware) | `ReleaseToFirmware()` and service stop — best-effort, whenever RIGStats switched the device to software mode (a device it never wrote, e.g. one iCUE owns, is left alone) |

**Optional, OpenRGB sends them but RIGStats has no use for them:** GET `12`
(PID; RIGStats already knows it from USB) and GET `11` (VID), GET `41`
(keyboard layout). They are documented and harmless reads, but leaving them
out means fewer frames to a device nobody has tested with RIGStats yet; the
decision belongs to the implementer.

**Won't send:**

- Any command number not in the table above — not even a GET with another
  property. The Falchion Ace HFX broke from a command sweep.
- The wireless write id `09` or the K57's `80`.
- `BLK_WN` (`07`) — nine bytes never need it.
- Onboard / hardware effects: OpenRGB's `UpdateHWMode()` frame is commented
  out in the source, so OpenRGB never sends it and its bytes are not
  documented use.
- Anything that saves to the device's memory: OpenRGB has no such frame
  here.

**Other software:** iCUE takes these devices over while it runs (#620), so
the family is reported as taken and skipped while iCUE runs, like Armoury
Crate and SteelSeries GG. The implementation checks for the process `iCUE`
(the app) only — not yet seen on a machine; Corsair's background services
are left out because they run while the app is closed. OpenRGB itself does
not check for iCUE.

## Open questions for the first live test

All of these are **unconfirmed** until a device answers:

1. Endpoint: does START `0D 00 22` return status `00`, or does the device
   need `01`?
2. Which physical LED is index 0, 1 and 2 on each device.
3. Is GET `5F` needed before colours show?
4. Does either device fall back to its own lighting without a write every
   30 s?
5. Do the reply bytes 1–2 echo `08` and the command?
6. What the device shows after SET mode = hardware.
7. The iCUE process name(s) on a real machine.
