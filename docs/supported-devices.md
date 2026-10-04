# Supported lighting devices

<!-- Generated from the sensor sidecar's model tables by LightingCatalog — do not edit by hand.
     Regenerate: $env:RIGSTATS_UPDATE_SUPPORTED_DEVICES=1; dotnet test sensor-sidecar.Tests --filter SupportedDevices -->

RIGStats drives these devices natively — no Armoury Crate, OpenRGB or other software needed. **Verified on hardware**: seen working on real hardware. **From OpenRGB, not yet verified**: same protocol as a verified device, listed from OpenRGB's device list (read as documentation) — it should work; [open an issue](https://github.com/dvalfrid/rigstats/issues) with your diagnostics ZIP if it doesn't, or to confirm it does.

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
| Any Windows Dynamic Lighting (HID LampArray) device, any brand | Keyboard, mouse, other | — | Verified on hardware (ROG Harpe Ace on the Omni receiver) | Skipped while Windows Dynamic Lighting controls it; keyboards above use their own protocol instead |
| Philips Hue lights, through a Hue Bridge (square, v2) | Room lights | — (network) | Verified on hardware | Paired from the Control Center; only the rooms and zones you choose follow the rig. Breathing and spectrum cycle fade slowly (the bridge takes about one command a second) |

Not supported: RGB on memory modules and graphics cards (reached over SMBus/I²C, where a wrong write can damage the hardware). A device that isn't listed: the diagnostics export's `lighting-devices.json` lists every HID device on the machine — attach it to an issue.
