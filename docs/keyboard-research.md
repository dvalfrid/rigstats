# ROG keyboards: what can be configured (research)

Research for keyboard settings in the Control Center (#292), 2026-10-18.
Nothing here is implemented. Every command below was **read** from ASUS
Gear Link's device modules and G-Helper's keyboard code (both as
documentation, no code copied); **none was sent to a keyboard**. A command
sweep once broke the Falchion Ace HFX — only commands listed here, and only
after a decision, may ever reach a keyboard.

Byte notation: frames are `[report id, command, key, index0, index1, data…]`;
hex below omits the report id. Gear Link writes commands in decimal
(`makeCommand(81,49,0,[s])` = `51 31 00 00 <s>`); its reply arrays start at
the command byte, so its `e[5]` is byte 6 of the HID report.

## Sources

| Source | What it gave | How it was read |
|---|---|---|
| **ASUS Gear Link** (gearlink.asus.com, ASUS's WebHID app) | 46 keyboard ids / 19 models: manifests (`/view/<pid>/manifest.json`, capability flags) and each model's module with every file it imports (286 MB of minified JavaScript). The keyboard protocol library names every command (`[鍵盤…]` log tags = keyboard; `[滑鼠…]` mouse, `[手把…]` gamepad), its event table and the firmware key table | Scanned all ids 0x1800–0x1FFF (100 manifests: 46 keyboards, 36 mice, 8 headsets, 5 receivers, 4 gamepads, 1 other); modules downloaded to a scratch folder outside the repo, read as text, never run. Tools: `scratch/tools/gl-scan.mjs`, `gl-modules.mjs`, `gl-methods.mjs` |
| **G-Helper** (github.com/seerge/g-helper, `app/Peripherals/Keyboard/`, `app/Input/InputDispatcher.cs`, `app/AsusKeyboardSettings.cs`) | Seven keyboard families incl. older ones not in Gear Link (Claymore II, Strix Flare / II, Strix Scope RX / TKL, TUF K1 / K3 / TX98, Falchion); the commands G-Helper uses, how it follows the keyboard's profile and how its "Launch App" keys work | Read on GitHub, GPL — documentation only |
| **OpenRGB** | Lighting only (already used for the lighting models) | — |

## Models

Gear Link's keyboards (deviceType `02`), with the capability flags of their
manifests. Every one is already in RIGStats' lighting table
(`AsusKeyboardDevice.Models`); Azoth X (cable and Omni) and Falchion Ace HFX
are verified on hardware.

| Model | Ids | Product no. | `isMagneticAxis` (HE) | `hasPowerInfo` (battery) | `isServiceShortcut` | `hasHwCopilot` |
|---|---|---|---|---|---|---|
| ROG Azoth | 1A83, 1A85 | m701 | — | yes | — | — |
| ROG Azoth Extreme | 1B3F, 1B40, 1B42 | m702 | — | yes | yes | yes |
| ROG Azoth Extreme Special Edition | 1CEF, 1CF0, 1CF1 | m706 | — | yes | yes | — |
| ROG Azoth Extreme Edition 20 | 1DA2, 1DA3, 1DA4 | m709 | — | yes | yes | — |
| ROG Azoth X | 1C24, 1C25, 1C26 | m704 | — | yes | yes | yes |
| ROG Azoth 96 HE / Lite | 1C10, 1C11, 1C12 | m901 | yes | yes | — | yes |
| ROG Falchion Ace HFX | 1B7E, 1B7F | m605 | yes | — | yes | yes |
| ROG Falchion Ace HFX ZywOo Edition | 1D1D | m606 | — | — | — | — |
| ROG Falchion Ace 75 HE | 1C7D | m705 | yes | — | — | — |
| ROG Falcata | 1C2F, 1C31 | m703 | yes | yes | — | — |
| ROG Strix Scope II 96 Wireless | 1AAE, 1AAF, 1AB0 | x901 | — | yes | — | — |
| ROG Strix Scope II RX | 1AB5, 1AB6 | xa12 | — | — | — | — |
| ROG Strix Morph 96 Wireless | 1CE8, 1CE9, 1CEA | x903 | — | yes | — | yes |
| ROG Strix Morph 96 X Wireless | 1E0A, 1E0B, 1E0C | x904 | — | yes | — | — |
| ROG Strix Morph 96 Wireless-PBZ | 1E5B, 1E5C, 1E5D | x906 | — | yes | — | — |
| ProArt Keyboard KD300 | 1D45, 1D46, 1D47 | kd300 | — | yes | — | — |
| ASUS TX75 Analog Gaming Keyboard | 1D68, 1D6A | r701 | yes | yes | — | — |
| ASUS TX75 Core Gaming Keyboard | 1DF3, 1DF4 | r702 | yes | — | — | — |
| TUF Gaming K4 Magnetic | 1DF5, 1DF6 | r704 | yes | — | — | — |

Ids per model are cable, own dongle and Bluetooth (one name). Every one of
the 46 ids ships the same keyboard setters (profile, key binding and its
read-back, idle time, power saving, low-battery level, polling rate, Fn
mode — checked per module, #461); the ZywOo Edition (`1D1D`) bundles them
in its own build. Not in Gear Link but in G-Helper: Claymore II (`196B`), Strix
Flare (`1875`) / Flare II (`19FE`), Strix Scope RX (`1951`) / RX TKL
(`1A07`), TUF K1 (`1945`), TUF K3 (`194B`) / K3 Gen II / TX98, older
Falchion (`193C`).

## Settings — both sources compared

| Setting | Gear Link (decimal → hex) | G-Helper | Read back? | Agree? |
|---|---|---|---|---|
| **Onboard profile** | `changeProfile`: `81,0,0,[p]` = `51 00 00 00 <p>` | `51 00 00 00 <p>` (0–5) | `12 00`: byte 11 current profile, byte 10 current effect (both sources), **byte 9 number of profiles** (Gear Link `getKeyboardDeviceInfo`; G-Helper assumes 6) | **yes** |
| Save to the current profile | `saveCurrentProfile`: `80,85` = `50 55` | `50 55` (after lighting) | — | **yes** |
| Profile changed on the keyboard (event) | `70 00 00 <bit>` after the report id: byte 4 = bit per profile (`PROFILE_n` = bit n; `0x0101` = hardware reset); also `03 81 06 00 <p>` | `70`, byte 4 = `1 << n` | — (it is the event) | **yes — verified on the Azoth X**, profile n = bit n (below) |
| Idle / sleep timeout | `setIdleTime`: `81,56` = `51 38 00 00 <code>` (0 = 1 min, 1 = 2, 2 = 3, 3 = 5, 4 = 10, 255 = never) | `51 38 00 00 <code>` | `12 01` byte 7 | **yes** |
| Power saving | `setSavingMode`: `81,57` = `51 39 00 00 <0 off / 1 turn off / 2 dim>` | — | `12 01` byte 8 | Gear Link only |
| Low-battery level | `setLowPowerLevel`: `81,58` = `51 3A 00 00 <0–100>` (UI marks Off / 25 / 50 / 75 / 100, default 20) | `51 37 00 00 <%>` "low-battery alert" | `12 01` byte 10 (Gear Link; 20 read live) | **no — settled for Gear Link (#461):** in Gear Link `51 37` is the *mouse's* power frame (`setPowerConfig`, `[滑鼠電源]`, 7 data bytes) and no keyboard class sends it; G-Helper's keyboard `51 37` looks carried over from its mouse code. `51 3A` is the one to use; `51 37` is never sent to a keyboard |
| Battery, charging | `12 01`: byte 6 %, 9 charging | same | — | yes (already used, #290) |
| Polling rate | `setPollingRate`: `81,49,0,[s]` = `51 31 00 00 <s>`; read `37,36` = `25 24` byte 5 | — | yes | Gear Link only |
| Fn-key mode (F-row media vs. F1–F12) | `setFnKeyModeEnable`: `81,98` = `51 62 00 00 <0/1>`; read `37,40` = `25 28` | — | yes | Gear Link only |
| **Key binding** | `setKeyBinding`: `81,32,<src lo>,<src hi>,[dst lo, dst hi]` = `51 20 <src> <dst>`; read `getKeyBinding`: `37,13,0,[src lo, src hi]` = `25 0D`, target at bytes 5–6 | `51 20 <src> 00 <dst lo hi>`; "the device can't report bindings back" | **Gear Link: yes** | write frame agrees; read only in Gear Link |
| Reset key bindings / lighting of the profile | `80,96` = `50 60` / `80,97` = `50 61` | per key, back to itself | — | — |
| Macro (key sends a sequence) | Two frames, by model (#461): **Azoth X only** (`1C24`–`1C26`) `setCombinationKey`: `81,38,<axis>` = `51 26 <axis> 00 <key lo hi> [<key> <action> <delay lo hi>]… FF 00 00 00` (action: keyboard press 2 / release 1, mouse 18 / 17; read back `25 31`); **magnetic models** (Falcata, Falchion Ace 75 HE, TX75 Analog) `setMacroMode` `81,24,<mode>` = `51 18 <mode>` + `writeMacroFlash` `84,<axis>` = `54 <axis> 00 00 <key lo hi> <58 bytes>`. No other model has a macro setter | `SetKeyCombo` for every family: `51 18 02 00 <src>` then `53 <src> <count> <cont> <4-byte records>`, `FF` ends | Azoth X: yes (`25 31`) | **no** — G-Helper's `53` record frame appears in no Gear Link keyboard class, and Gear Link's two frames are per model; not to be sent on G-Helper's word alone |
| Windows-key lock | key code `WindowLock` (209) | — | — | Gear Link only |
| OLED (Azoth family) | on/off `105` = `69`, brightness `104` = `68` (0–100), mode `106` = `6A`, KPS `107` = `6B`, clock `99` = `63`, laptop battery `100` = `64`; reads `35,x` = `23 xx`, `36,x` = `24 xx` | `69`, `68`, `6A` | Gear Link: yes | **yes** for on/off, brightness, mode |
| Magnetic keys (HE) | actuation `51 4F`/`51 50`, dead zone `51 58`/`51 59`, rapid trigger `51 54`/`51 5B`, speed tap, DKS, mod-tap; reads `25 xx` | — | yes | Gear Link only |
| Lighting | `51 2C …` (in use, #213) | same | — | yes |
| **Never** | `resetAllProfiles` `50 40 01`, `resetCurrentProfile` `50 40 00`, `deviceReset` `B0`, calibration `80 26`/`80 27` | — | — | not to be sent |

## Key codes (bindings)

Bindings use ASUS's own firmware key codes, not HID usages (Gear Link's
`NORMAL` table, 357 entries; a UK table for ISO boards): `Escape` 110,
`F1`–`F12` 112–123, `PrintScreen` 124, digits 2–11, letters …, plus
functions: media (`AV_PlayPause` 183, `AV_VolUp` 187 …), mouse
(`MS_LeftButton` 496 …), Windows actions (`WS_ShowDesktop` 768,
`WS_SwitchApp` 769, `WS_Copy` 772 …), keyboard functions
(`KB_Brightness_Up` 800, `Effect_Next` 804, `Mode_PowerSaving` 798),
`MacroKey` 4080, `CombinationKey` 4081, `WindowLock` 209, `Disabled` 0,
`DefaultKey` 255, and every `Fn_<key>` (40705…). **There is no F13–F24** —
a key can't be bound to an otherwise unused key code by a documented
command.

## Events the keyboard sends by itself

Read from Gear Link's keyboard event table (only reading, nothing is sent):

| Report / prefix | Meaning |
|---|---|
| `70 …` | the first non-zero byte and its position: `HW_RESET` (`0x0101`), `PROFILE_0`…`PROFILE_6` (`0x0201`…`0x0240`); bit 0x20 of byte 1 = a "fly macro" armed, `08` = lighting changed, bit 0x40 = brightness changed |
| `03 81 06 00 <p>` | profile changed |
| `03 81 07 00` | all profiles reset |
| `03 81 03 00 <s>` | connection state (wireless) |
| `03 81 05 00`, `72 00` | plugged / unplugged |
| `72 01`, `72 04` | lighting changed |
| `71`, `61` | "common keys" (key state) |
| `73`, `74`, `7E`, `7C`, `7D`, `76`, `78`, `7F` | magnetic-key events (DKS, mod-tap, travel, rapid trigger, speed tap, polling mode) |
| `92`, `93`, `94`, `95`, `96` | OLED brightness / mode / gestures |
| `91`, `97`, `A0`, `A1` | lever, touch panel, key type / plug-in key |
| `03 81 0D 00` | wireless signal |

G-Helper listens to report `0x70` for the profile only and follows the
keyboard's profile when it changes: its byte 4 = `1 << n` for n = 1–6, 6
meaning profile 0 — Gear Link names bits 0–6 `PROFILE_0`…`PROFILE_6`. The
same bitmask, read slightly differently; which bit the Azoth X sends for
which profile is a hardware check.

## Macro keys and profiles

Three ways, from no device writes to some:

1. **Follow the keyboard's own profile (read-only).** When the keyboard's
   onboard profile is switched on the keyboard (Fn + a profile key on
   models that have one), it sends `70` / `03 81 06 00` by itself. RIGStats
   could listen and switch the Control Center profile mapped to it —
   nothing sent to the keyboard. Both sources decode it the same way, and it
   is **verified on the Azoth X** (Fn + 1…6 through the Omni receiver, below).
2. **RIGStats hotkeys, the key bound by the user.** A global hotkey per
   Control Center profile (today only Ctrl+Alt+P cycles them), and the
   user assigns a macro key in Gear Link or Armoury Crate to that
   combination. No keyboard commands at all, works for any brand. The
   simplest option.
3. **RIGStats writes the binding.** As G-Helper's "Launch App 1–4": the key
   gets a macro that types Ctrl+Shift+Alt+F1–F4, and the app registers
   that combination. Writes to the keyboard with a macro frame the sources
   don't share (G-Helper's `53` records are in no Gear Link class; Gear
   Link's `51 26` is Azoth X-only, `54` magnetic-only) — ruled out until a
   frame is confirmed per model on hardware. F13–F24 (G-Helper's route for
   mice) isn't available on keyboards: no such key code.

Options 1 and 2 shipped (#459, #456). None of the 19 Gear Link models has
dedicated M1…M5 macro keys (ASUS's pages and reviews describe on-the-fly
recording onto ordinary keys, and the manifests carry no such key); the
keys that select a profile are Fn + 1…6, so "a macro key for a profile" is
either Fn + n (option 1) or any key bound to Ctrl+Alt+F1…F9 in Gear Link
(option 2).

## Verified on the ROG Azoth X (2026-10-18, #457)

Through the ROG Omni receiver (`1ACE`), with `tools/keyboard-listen-guided.ps1`
(guided by the owner; only `12 00` and `12 01` sent, everything else
listened to):

- **The keyboard reports its profile switch.** Fn + 1 / 2 / 3 (number row)
  switched the onboard profile — the keyboard changed colour and its OLED
  showed the profile — and each sent `04 70 00 00 <bit>` on `mi_03&col04`
  (usage page `0xFFC0`, report `0x04`, input only): Fn + 1 → `02`,
  Fn + 2 → `04`, Fn + 3 → `08`. Profile *n* (1–6) is bit *n* of byte 4 —
  G-Helper's `1 << n`, Gear Link's `PROFILE_n`. `03 81 06 00` was not seen.
- **`12 00`** (on `mi_02&col02`, report `0x02`): `02 12 00 00 00 08 00 07 00 06
  00 01 24 00 01` — byte 9 = 6 profiles, byte 10 = effect 0, byte 11 =
  profile 1 (after Fn + 1, at the start and at the end).
- **`12 01`**: `02 12 01 00 00 00 43 02 01 00 14 43 6F 0F` — byte 6 battery
  67 %, 7 idle 2 (= 3 min), 8 power saving 1 (turn off), 9 not charging,
  10 low-power level 0x14 = 20; bytes 12–13 look like the battery voltage
  (`0F6F` = 3951, moving between reads) — not documented, not used.
- **Report `71` on the same collection is a key-state bitmap**: every key
  press, Fn included (Gear Link's `onCommonKeys`). It would let Fn
  combinations be caught without writing anything — but reading it means
  seeing every keystroke, so it is not proposed for RIGStats.
- Also seen: `04 81 02 01 43` at start (an unnamed status, 0x43 = the
  battery %), and on the mouse's channel (`mi_02&col03`) a `03 FF AA` error
  reply about once a minute after a `12 00` — from the service, to be looked
  at separately.

**By cable (`1C24`, 2026-10-18, same guided test with `-ProductId 1c24
-Report 0`):** the command channel is `mi_01` (page `FF00`, report `0x00`,
65-byte reports) and the event comes on `mi_02&col03` (page `FFC0`, report
**`0x03`**): Fn + 2 → `03 70 00 00 04`, Fn + 3 → `08`, Fn + 1 → `02` — the
same bytes after the report id as through the receiver. `12 00` answered
`00 12 00 00 00 25 00 01 00 06 00 01 02` (bytes 5–8 differ from the
receiver's — Gear Link reads the keyboard's own firmware version there by
cable and the dongle's wirelessly; profiles and the current one at the
same places). `12 01`: `42 02 01 01 14` — 66 %, 3 min, lighting off,
**charging = 1** on the cable, 20 %. By cable the keyboard also exposes a
second Windows keyboard collection (`mi_03`, page 1) that can't be opened;
listening on it crashed `KeyboardFollower` once (a dictionary written from
two threads) and a switch went unfollowed — fixed by reading only vendor
collections (`FFxx`), under a lock. A host switch raises no event (#458).

## Open questions

Settled in #461: the low-battery command (`51 3A`; `51 37` is a mouse
frame), the macro frames (per model, G-Helper's unconfirmed), dedicated
macro keys (none), the ZywOo Edition (the same setters), and that a
host-made switch raises no event (#458). The number of profiles needs no
table: the keyboard reports it (`12 00` byte 9).

- **The G-Helper-only families** (Claymore II, Strix Flare / II, Strix
  Scope RX / TKL, TUF K1 / K3 / TX98, the older Falchion): G-Helper's
  `AsusKeyboard` base sends them the same frames as the Azoth family —
  profile `51 00`, idle `51 38`, binding `51 20` — plus the low-battery
  `51 37` that Gear Link uses for mice only. One source, nothing verified:
  offer them the profile switch and idle time (both sources agree on the
  frames for the newer models) marked "From G-Helper, not yet verified",
  never the low-battery level. An ASUS Armoury Crate device package for one
  of them (read as documentation, nothing installed) would be the third
  source if a user asks for it.
- Whether the Omni receiver's dropped-frame behaviour (#458) also happens
  by cable (the cable test only listened; no switch was sent).
