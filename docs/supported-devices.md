# Supported lighting devices

<!-- Generated from the sensor sidecar's model tables by LightingCatalog — do not edit by hand.
     Regenerate: $env:RIGSTATS_UPDATE_SUPPORTED_DEVICES=1; dotnet test sensor-sidecar.Tests --filter SupportedDevices -->

RIGStats drives these devices natively — no Armoury Crate, OpenRGB or other software needed. **Verified on hardware**: seen working on real hardware. **From OpenRGB, not yet verified**: same protocol as a verified device, listed from OpenRGB's device list (read as documentation) — it should work; **From Gear Link, not yet verified**: from ASUS Gear Link's own device definitions, same commands as a verified device — likewise; [report it](https://github.com/dvalfrid/rigstats/issues/new?template=lighting_device.yml) with your diagnostics ZIP if it doesn't, or to confirm it does.

| Device | Type | USB id | Status | Notes |
|---|---|---|---|---|
| ASUS Aura USB motherboard controller (ROG, Strix, TUF, Prime boards, 2019+) | Motherboard | 18F3, 1939, 19AF, 1AA6, 1BED | Verified on hardware (19AF); the others from OpenRGB | Onboard LEDs and every ARGB header, as the controller describes them |
| ASUS Aura USB addressable controller (older boards) | Motherboard | 1867, 1872, 18A3, 18A5 | From OpenRGB, not yet verified | ARGB headers |
| ROG Strix XG27AQDMG | Monitor | 1BA3 | Verified on hardware | Runs its own effects |
| ROG Strix XG27ACDNG | Monitor | 1BC9 | From OpenRGB, not yet verified | Effects drawn by RIGStats |
| ROG Strix XG27UCG | Monitor | 1BB4 | From OpenRGB, not yet verified | Effects drawn by RIGStats |
| ROG Swift PG32UCDM | Monitor | 1B2B | From OpenRGB, not yet verified | Effects drawn by RIGStats |
| ROG Swift PG32UCDMR | Monitor | 1C9B | From OpenRGB, not yet verified | Effects drawn by RIGStats |
| ROG Swift PG32UCDP | Monitor | 1BCA | From OpenRGB, not yet verified | Effects drawn by RIGStats |
| ROG Aura Monitor Light Bar | Light bar | 1AC8 | Verified on hardware | Runs its own effects; Desk lamp: on/off, brightness, colour temperature |
| Keyboards paired to the ROG Omni receiver (e.g. ROG Azoth X) | Keyboard | 1ACE | Verified on hardware | Found on whichever receiver channel the keyboard answers |
| ROG Azoth | Keyboard | 1A83 | Verified on hardware | Runs its own effects |
| ROG Azoth X | Keyboard | 1C24 | Verified on hardware | Runs its own effects |
| ROG Falchion Ace HFX | Keyboard | 1B7E | Verified on hardware | Runs its own effects |
| ROG Azoth (2.4 GHz) | Keyboard | 1A85 | From OpenRGB, not yet verified | Runs its own effects |
| ROG Falchion | Keyboard | 193C | From OpenRGB, not yet verified | Runs its own effects |
| ROG Falchion (wireless) | Keyboard | 193E | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Flare | Keyboard | 1875 | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Flare CoD Black Ops 4 Edition | Keyboard | 18AF | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Flare II | Keyboard | 19FE | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Flare II Animate | Keyboard | 19FC | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Flare PNK LTD | Keyboard | 18CF | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Scope | Keyboard | 18F8 | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Scope II | Keyboard | 1AB3 | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Scope II 96 RX Wireless | Keyboard | 1B78 | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Scope II 96 Wireless | Keyboard | 1AAE | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Scope II RX | Keyboard | 1AB5 | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Scope NX Wireless Deluxe | Keyboard | 19F6 | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Scope NX Wireless Deluxe (2.4 GHz) | Keyboard | 19F8 | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Scope RX | Keyboard | 1951 | From OpenRGB, not yet verified | Runs its own effects |
| ROG Strix Scope RX EVA-02 Edition | Keyboard | 1B12 | From OpenRGB, not yet verified | Runs its own effects |
| TUF Gaming K1 | Keyboard | 1945 | From OpenRGB, not yet verified | Runs its own effects |
| TUF Gaming K3 | Keyboard | 194B | From OpenRGB, not yet verified | Runs its own effects |
| TUF Gaming K3 Gen II | Keyboard | 1B30 | From OpenRGB, not yet verified | Runs its own effects |
| TUF Gaming K3 Gen II Miku Edition | Keyboard | 1C5E | From OpenRGB, not yet verified | Runs its own effects |
| TUF Gaming K5 | Keyboard | 1899 | From OpenRGB, not yet verified | Runs its own effects |
| TUF Gaming K7 | Keyboard | 18AA | From OpenRGB, not yet verified | Runs its own effects |
| ROG Delta II | Headset | 1AFA | Verified on hardware | Through its 2.4 GHz dongle; found when switched on |
| Any Windows Dynamic Lighting (HID LampArray) device, any brand | Keyboard, mouse, other | — | Verified on hardware (ROG Harpe Ace on the Omni receiver) | Skipped while Windows Dynamic Lighting controls it; keyboards above use their own protocol instead. ASUS devices: set the device's Cross-device Lighting Toggle to "Aura Sync & Windows Dynamic Lighting" in Gear Link or Armoury Crate, otherwise it ignores every lighting change |
| ROG Harpe Ace Aim Lab Edition | Mouse | 1A94 | Verified on hardware | Through the ROG Omni receiver; its Cross-device Lighting Toggle can be switched from the Lighting tab |
| ROG Harpe Ace Extreme | Mouse | 1B69 | From Gear Link, not yet verified | Through the ROG Omni receiver; its Cross-device Lighting Toggle can be switched from the Lighting tab |
| ROG Harpe Ace Mini | Mouse | 1B65 | From Gear Link, not yet verified | Through the ROG Omni receiver; its Cross-device Lighting Toggle can be switched from the Lighting tab |
| ROG Keris II Ace | Mouse | 1B18 | From Gear Link, not yet verified | Through the ROG Omni receiver; its Cross-device Lighting Toggle can be switched from the Lighting tab |
| Other ROG mice on the ROG Omni receiver (ProArt Mouse MD301, ROG Gladius IV Ace, ROG Gladius IV Ace Max, ROG Harpe II Ace, ROG Harpe II Ace (PBZ), ROG Harpe II Ace Mini, ROG Harpe II Ace Mini Demon1 Edition, ROG Harpe II Extreme Edition 20, ROG Keris II Origin, ROG Keris II Origin (KJP), ROG Spatha X 65K) | Mouse | 1C0E, 1C6B, 1CD3, 1D4E, 1D7C, 1DBF, 1DE0, 1E32, 1E4B, 1E4F, 1E52 | From Gear Link, not yet verified | Named in the Lighting tab; lit through the receiver's Dynamic Lighting (no toggle needed) |
| Philips Hue lights, through a Hue Bridge (square, v2) | Room lights | — (network) | Verified on hardware | Paired from the Control Center; only the rooms and zones you choose follow the rig. Breathing and spectrum cycle fade slowly (the bridge takes about one command a second) |

Not supported: RGB on memory modules and graphics cards (reached over SMBus/I²C, where a wrong write can damage the hardware). A device that isn't listed: the diagnostics export's `lighting-devices.json` lists every HID device on the machine, plus read-only replies from ASUS receivers and the mice paired to them — attach it to a [lighting device report](https://github.com/dvalfrid/rigstats/issues/new?template=lighting_device.yml).
