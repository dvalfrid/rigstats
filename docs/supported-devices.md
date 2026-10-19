# Supported devices

<!-- Generated from the sensor sidecar's model tables by LightingCatalog — do not edit by hand.
     Regenerate: $env:RIGSTATS_UPDATE_SUPPORTED_DEVICES=1; dotnet test sensor-sidecar.Tests --filter SupportedDevices -->

RIGStats drives these lighting devices natively — no Armoury Crate, OpenRGB or other software needed. **Verified on hardware**: seen working on real hardware. **From OpenRGB, not yet verified**: same protocol as a verified device, listed from OpenRGB's device list (read as documentation) — it should work; **From Gear Link, not yet verified**: from ASUS Gear Link's own device definitions, same commands as a verified device — likewise; [report it](https://github.com/dvalfrid/rigstats/issues/new?template=device.yml) with your diagnostics ZIP if it doesn't, or to confirm it does.

## Lighting

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
| ROG Azoth (2.4 GHz) | Keyboard | 1A85 | Verified on hardware | Runs its own effects |
| ROG Azoth X | Keyboard | 1C24 | Verified on hardware | Runs its own effects |
| ROG Falchion Ace HFX | Keyboard | 1B7E | Verified on hardware | Runs its own effects |
| ASUS TX75 Analog Gaming Keyboard | Keyboard | 1D68, 1D6A | From Gear Link, not yet verified | Runs its own effects |
| ASUS TX75 Core Gaming Keyboard | Keyboard | 1DF3, 1DF4 | From Gear Link, not yet verified | Runs its own effects |
| ProArt Keyboard KD300 | Keyboard | 1D45, 1D46, 1D47 | From Gear Link, not yet verified | Runs its own effects |
| ROG Azoth 96 HE / Lite | Keyboard | 1C10, 1C11, 1C12 | From Gear Link, not yet verified | Runs its own effects |
| ROG Azoth Extreme | Keyboard | 1B3F, 1B40, 1B42 | From Gear Link, not yet verified | Runs its own effects |
| ROG Azoth Extreme Edition 20 | Keyboard | 1DA2, 1DA3, 1DA4 | From Gear Link, not yet verified | Runs its own effects |
| ROG Azoth Extreme Special Edition | Keyboard | 1CEF, 1CF0, 1CF1 | From Gear Link, not yet verified | Runs its own effects |
| ROG Azoth X | Keyboard | 1C25 | From Gear Link, not yet verified | Runs its own effects |
| ROG Falcata | Keyboard | 1C2F, 1C31 | From Gear Link, not yet verified | Runs its own effects |
| ROG Falchion Ace 75 HE | Keyboard | 1C7D | From Gear Link, not yet verified | Runs its own effects |
| ROG Falchion Ace HFX | Keyboard | 1B7F | From Gear Link, not yet verified | Runs its own effects |
| ROG Falchion Ace HFX ZywOo Edition | Keyboard | 1D1D | From Gear Link, not yet verified | Runs its own effects |
| ROG Strix Morph 96 Wireless | Keyboard | 1CE8, 1CE9, 1CEA | From Gear Link, not yet verified | Runs its own effects |
| ROG Strix Morph 96 Wireless (PBZ) | Keyboard | 1E5B, 1E5C, 1E5D | From Gear Link, not yet verified | Runs its own effects |
| ROG Strix Morph 96 X Wireless | Keyboard | 1E0A, 1E0B, 1E0C | From Gear Link, not yet verified | Runs its own effects |
| ROG Strix Scope II 96 Wireless | Keyboard | 1AAE, 1AAF, 1AB0 | From Gear Link, not yet verified | Runs its own effects |
| ROG Strix Scope II RX | Keyboard | 1AB5, 1AB6 | From Gear Link, not yet verified | Runs its own effects |
| TUF Gaming K4 Magnetic | Keyboard | 1DF5, 1DF6 | From Gear Link, not yet verified | Runs its own effects |
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
| ROG Delta II | Headset | 1AFA | Verified on hardware | Through its 2.4 GHz dongle; found when switched on; every write read back |
| ROG Delta II (KJP) | Headset | 1D41 | From Gear Link, not yet verified | Through its 2.4 GHz dongle; found when switched on; every write read back |
| ROG Delta II (PBZ) | Headset | 1E6B | From Gear Link, not yet verified | Through its 2.4 GHz dongle; found when switched on; every write read back |
| ROG Pelta | Headset | 1B82, 1B84 | From Gear Link, not yet verified | Through its 2.4 GHz dongle; found when switched on; every write read back |
| Any Windows Dynamic Lighting (HID LampArray) device, any brand | Keyboard, mouse, other | — | Verified on hardware (ROG Harpe Ace on the Omni receiver) | Skipped while Windows Dynamic Lighting controls it; keyboards above use their own protocol instead. ASUS devices: set the device's Cross-device Lighting Toggle to "Aura Sync & Windows Dynamic Lighting" in Gear Link or Armoury Crate, otherwise it ignores every lighting change |
| ROG Harpe Ace Aim Lab Edition | Mouse | 1A94 | Verified on hardware | Through the ROG Omni receiver; its Cross-device Lighting Toggle can be switched from the Lighting tab |
| ROG Harpe Ace Mini | Mouse | 1B65 | Verified on hardware | Through the ROG Omni receiver; its Cross-device Lighting Toggle can be switched from the Lighting tab |
| ROG Harpe Ace Extreme | Mouse | 1B69 | From Gear Link, not yet verified | Through the ROG Omni receiver; its Cross-device Lighting Toggle can be switched from the Lighting tab |
| ROG Keris II Ace | Mouse | 1B18 | From Gear Link, not yet verified | Through the ROG Omni receiver; its Cross-device Lighting Toggle can be switched from the Lighting tab |
| Other ROG mice on the ROG Omni receiver (ProArt Mouse MD301, ROG Gladius IV Ace, ROG Gladius IV Ace Max, ROG Harpe II Ace, ROG Harpe II Ace (PBZ), ROG Harpe II Ace Mini, ROG Harpe II Ace Mini Demon1 Edition, ROG Harpe II Extreme Edition 20, ROG Keris II Origin, ROG Keris II Origin (KJP), ROG Spatha X 65K) | Mouse | 1C0E, 1C6B, 1CD3, 1D4E, 1D7C, 1DBF, 1DE0, 1E32, 1E4B, 1E4F, 1E52 | From Gear Link, not yet verified | Named in the Lighting tab; lit through the receiver's Dynamic Lighting (no toggle needed) |
| Philips Hue lights, through a Hue Bridge (square, v2) | Room lights | — (network) | Verified on hardware | Paired from the Control Center; only the rooms and zones you choose follow the rig. Breathing and spectrum cycle fade slowly (the bridge takes about one command a second) |

Not supported: RGB on memory modules and graphics cards (reached over SMBus/I²C, where a wrong write can damage the hardware). A device that isn't listed: the diagnostics export's `lighting-devices.json` lists every HID device on the machine, plus read-only replies from ASUS receivers and the mice paired to them — attach it to a [device report](https://github.com/dvalfrid/rigstats/issues/new?template=device.yml).

## Device settings

The Control Center's **Headset**, **Keyboard** and **Mouse** pages change these devices' own settings, kept in the device. Mice: DPI levels, polling rate, button response, lift-off, angle snapping and tuning, acceleration, button bindings and energy — as far as the model has them — and which onboard profile each Control Center profile uses. Keyboards: which onboard profile each Control Center profile uses (Fn + that number on the keyboard switches the profile back), and on wireless models when the keyboard sleeps, what its lighting does when the battery is low and at which level. **From G-Helper, not yet verified**: the model and its settings as G-Helper documents them, read as documentation; the commands are the ones checked on the verified mouse and against ASUS Gear Link.

| Device | Type | USB id | Status | Notes |
|---|---|---|---|---|
| ROG Delta II | Headset | 1AFA | Verified on hardware | Sidetone on/off and 0–20; noise reduction on/off and Low / Medium / High; equalizer on/off and 8 bands (125 Hz – 16 kHz), 8 presets (G-Helper's tables), one per Control Center profile if wanted, through its 2.4 GHz dongle |
| ROG Delta II (KJP) | Headset | 1D41 | From Gear Link, not yet verified | Sidetone on/off and 0–20; noise reduction on/off and Low / Medium / High; equalizer on/off and 8 bands (125 Hz – 16 kHz), 8 presets (G-Helper's tables), one per Control Center profile if wanted — equalizer not yet verified, through its 2.4 GHz dongle |
| ROG Delta II (PBZ) | Headset | 1E6B | From Gear Link, not yet verified | Sidetone on/off and 0–20; noise reduction on/off and Low / Medium / High; equalizer on/off and 8 bands (125 Hz – 16 kHz), 8 presets (G-Helper's tables), one per Control Center profile if wanted — equalizer not yet verified, through its 2.4 GHz dongle |
| ROG Pelta | Headset | 1B82, 1B84 | From Gear Link, not yet verified | Sidetone on/off and 0–20; noise reduction on/off; equalizer on/off and 10 bands (32 Hz – 16 kHz), 14 presets (G-Helper's tables), one per Control Center profile if wanted — equalizer not yet verified, through its 2.4 GHz dongle |
| ROG Azoth (2.4 GHz) | Keyboard | 1A85 | Verified on hardware | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Azoth X | Keyboard | 1C24, 1C25 | Verified on hardware | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ASUS TX75 Analog Gaming Keyboard | Keyboard | 1D68, 1D6A | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ASUS TX75 Core Gaming Keyboard | Keyboard | 1DF3, 1DF4 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ProArt Keyboard KD300 | Keyboard | 1D45, 1D46, 1D47 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Azoth | Keyboard | 1A83 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Azoth 96 HE / Lite | Keyboard | 1C10, 1C11, 1C12 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Azoth Extreme | Keyboard | 1B3F, 1B40, 1B42 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Azoth Extreme Edition 20 | Keyboard | 1DA2, 1DA3, 1DA4 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Azoth Extreme Special Edition | Keyboard | 1CEF, 1CF0, 1CF1 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Falcata | Keyboard | 1C2F, 1C31 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Falchion | Keyboard | 193C | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Falchion (wireless) | Keyboard | 193E | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Falchion Ace 75 HE | Keyboard | 1C7D | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Falchion Ace HFX | Keyboard | 1B7E, 1B7F | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Falchion Ace HFX ZywOo Edition | Keyboard | 1D1D | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Flare | Keyboard | 1875 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Flare CoD Black Ops 4 Edition | Keyboard | 18AF | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Flare II | Keyboard | 19FE | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Flare II Animate | Keyboard | 19FC | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Flare PNK LTD | Keyboard | 18CF | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Morph 96 Wireless | Keyboard | 1CE8, 1CE9, 1CEA | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Strix Morph 96 Wireless (PBZ) | Keyboard | 1E5B, 1E5C, 1E5D | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Strix Morph 96 X Wireless | Keyboard | 1E0A, 1E0B, 1E0C | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Strix Scope | Keyboard | 18F8 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Scope II | Keyboard | 1AB3 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Scope II 96 RX Wireless | Keyboard | 1B78 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Scope II 96 Wireless | Keyboard | 1AAE, 1AAF, 1AB0 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back; sleep, power saving and the low-battery level |
| ROG Strix Scope II RX | Keyboard | 1AB5, 1AB6 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Scope NX Wireless Deluxe | Keyboard | 19F6 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Scope NX Wireless Deluxe (2.4 GHz) | Keyboard | 19F8 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Scope RX | Keyboard | 1951 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Strix Scope RX EVA-02 Edition | Keyboard | 1B12 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| TUF Gaming K1 | Keyboard | 1945 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| TUF Gaming K3 | Keyboard | 194B | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| TUF Gaming K3 Gen II | Keyboard | 1B30 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| TUF Gaming K3 Gen II Miku Edition | Keyboard | 1C5E | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| TUF Gaming K4 Magnetic | Keyboard | 1DF5, 1DF6 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| TUF Gaming K5 | Keyboard | 1899 | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| TUF Gaming K7 | Keyboard | 18AA | From Gear Link, not yet verified | Its onboard profile per Control Center profile, and Fn + 1…6 on the keyboard switches the profile back |
| ROG Harpe Ace Aim Lab Edition | Mouse | 1A92, 1A94 | Verified on hardware (ROG Omni receiver) | Cable, own 2.4 GHz dongle, ROG Omni receiver |
| ROG Harpe Ace Mini | Mouse | 1B63, 1B65 | Verified on hardware (ROG Omni receiver) | Cable, ROG Omni receiver |
| ASUS SmartO Mouse MD200 Silent Plus | Mouse | 1A24 | From G-Helper, not yet verified | Own 2.4 GHz dongle |
| ASUS TUF Gaming M3 Gen II | Mouse | 1A9B | From G-Helper, not yet verified | Cable |
| ASUS TUF GAMING Mini Wireless Mouse Hatsune Miku Edition | Mouse | 1C56, 1C57 | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle |
| Gladius II Origin COD | Mouse | 18B1 | From G-Helper, not yet verified | Cable; no button bindings yet (an older model) |
| ROG Chakram | Mouse | 18E3, 18E5 | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle; no button bindings yet (an older model); the DPI level in use is picked with its DPI button |
| ROG Chakram Core | Mouse | 1958 | From G-Helper, not yet verified | Cable; no button bindings yet (an older model); the DPI level in use is picked with its DPI button |
| ROG Chakram X | Mouse | 1A18, 1A1A | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle; no button bindings yet (its own button layout) |
| ROG Gladius II | Mouse | 1845 | From G-Helper, not yet verified | Cable; no button bindings yet (an older model) |
| ROG Gladius II Core | Mouse | 18DD | From G-Helper, not yet verified | Cable; no button bindings yet (an older model) |
| ROG Gladius II Origin | Mouse | 1877 | From G-Helper, not yet verified | Cable; no button bindings yet (an older model) |
| ROG Gladius II Origin PNK LTD | Mouse | 18CD | From G-Helper, not yet verified | Cable; no button bindings yet (an older model) |
| ROG Gladius II Wireless | Mouse | 18A0 | From G-Helper, not yet verified | Own 2.4 GHz dongle; no button bindings yet (an older model) |
| ROG Gladius III | Mouse | 197B, 197D, 197F | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle |
| ROG Gladius III Wireless AimPoint | Mouse | 1A70, 1A72 | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle, ROG Omni receiver |
| ROG Gladius III Wireless AimPoint EVA-02 Edition | Mouse | 1B0A, 1B0C | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle |
| ROG Harpe Ace Extreme | Mouse | 1B67, 1B68, 1B69 | From G-Helper, not yet verified | Cable, ROG Omni receiver |
| ROG Harpe II Ace | Mouse | 1AD0, 1C69 | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle |
| ROG Keris | Mouse | 195C | From G-Helper, not yet verified | Cable; no button bindings yet (an older model); the DPI level in use is picked with its DPI button |
| ROG Keris II Ace | Mouse | 1B16, 1B18, 1B1A | From G-Helper, not yet verified | Cable, ROG Omni receiver |
| ROG Keris II Origin | Mouse | 1C0C, 1C0E | From G-Helper, not yet verified | Cable, ROG Omni receiver |
| ROG Keris II Origin (KJP) | Mouse | 1D4C, 1D4E | From G-Helper, not yet verified | Cable, ROG Omni receiver |
| ROG Keris Wireless | Mouse | 195E, 1960 | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle; no button bindings yet (an older model); the DPI level in use is picked with its DPI button |
| ROG Keris Wireless AimPoint | Mouse | 1A66, 1A68, 1A6A | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle, ROG Omni receiver |
| ROG Keris Wireless EVA Edition | Mouse | 1A57, 1A59 | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle; no button bindings yet (an older model); the DPI level in use is picked with its DPI button |
| ROG Pugio | Mouse | 1846 | From G-Helper, not yet verified | Cable; no button bindings yet (an older model) |
| ROG Pugio II | Mouse | 1906, 1908 | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle; no button bindings yet (an older model); the DPI level in use is picked with its DPI button |
| ROG Spatha X | Mouse | 1977, 1979 | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle; no button bindings yet (its own button layout) |
| ROG Strix Carry | Mouse | 18B4 | From G-Helper, not yet verified | Own 2.4 GHz dongle; no button bindings yet (an older model); the DPI level in use is picked with its DPI button |
| ROG Strix Evolve | Mouse | 185B | From G-Helper, not yet verified | Cable |
| ROG Strix Impact | Mouse | 1847 | From G-Helper, not yet verified | Cable; no button bindings yet (an older model) |
| ROG Strix Impact II | Mouse | 18E1 | From G-Helper, not yet verified | Cable; no button bindings yet (an older model) |
| ROG Strix Impact II Electro Punk | Mouse | 1956 | From G-Helper, not yet verified | Cable; no button bindings yet (an older model) |
| ROG Strix Impact II Moonlight White | Mouse | 19D2 | From G-Helper, not yet verified | Cable; no button bindings yet (an older model) |
| ROG Strix Impact II Wireless | Mouse | 1947, 1949 | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle; no button bindings yet (an older model); the DPI level in use is picked with its DPI button |
| ROG Strix Impact III | Mouse | 1A88 | From G-Helper, not yet verified | Cable |
| ROG Strix Impact III Wireless | Mouse | 1AD7 | From G-Helper, not yet verified | ROG Omni receiver |
| TUF Gaming M3 | Mouse | 1910 | From G-Helper, not yet verified | Cable |
| TUF Gaming M4 Air | Mouse | 1A03 | From G-Helper, not yet verified | Cable |
| TUF Gaming M4 Wireless | Mouse | 19F4 | From G-Helper, not yet verified | Own 2.4 GHz dongle |
| TUF Gaming M5 | Mouse | 1898 | From G-Helper, not yet verified | Cable; no button bindings yet (an older model) |
| TX Gaming Mouse | Mouse | 1A8D | From G-Helper, not yet verified | Own 2.4 GHz dongle |
| TX Gaming Mouse Mini | Mouse | 1AF3, 1AF5 | From G-Helper, not yet verified | Cable, own 2.4 GHz dongle |

The older models (Gladius II, Strix Impact I / II, Pugio I / II, Chakram / Core, Keris, Strix Carry, TUF Gaming M5) use their own variants of the commands, each as G-Helper documents it (#432). To confirm a model, or if one doesn't work: open its page once, then collect diagnostics — the service log (`rigstats-sensor.log`) has a `Mouse: … read …` or `Headset: … read …` line with what the device answered — and send the ZIP with a [device report](https://github.com/dvalfrid/rigstats/issues/new?template=device.yml).

## Battery

The PERIPHERALS panel and the tray icon's hover card show the battery of these devices, read every 20 s. **From Solaar, not yet verified**: built from Solaar's implementation of Logitech's protocol, read as documentation.

| Device | Type | USB id | Status | Notes |
|---|---|---|---|---|
| ROG Delta II | Headset | 1AFA | Verified on hardware | Battery and charging, through its 2.4 GHz dongle |
| ROG Delta II (KJP) | Headset | 1D41 | From Gear Link, not yet verified | Battery and charging, through its 2.4 GHz dongle |
| ROG Delta II (PBZ) | Headset | 1E6B | From Gear Link, not yet verified | Battery and charging, through its 2.4 GHz dongle |
| ROG Pelta | Headset | 1B82, 1B84 | From Gear Link, not yet verified | Battery and charging, through its 2.4 GHz dongle |
| ROG Azoth | Keyboard | 1A83 | Verified on hardware | Battery and charging |
| ROG Azoth (2.4 GHz) | Keyboard | 1A85 | Verified on hardware | Battery and charging |
| ROG Azoth X | Keyboard | 1C24, 1C25 | Verified on hardware | Battery and charging |
| ASUS TX75 Analog Gaming Keyboard | Keyboard | 1D68, 1D6A | From Gear Link, not yet verified | Battery and charging |
| ProArt Keyboard KD300 | Keyboard | 1D45, 1D46, 1D47 | From Gear Link, not yet verified | Battery and charging |
| ROG Azoth 96 HE / Lite | Keyboard | 1C10, 1C11, 1C12 | From Gear Link, not yet verified | Battery and charging |
| ROG Azoth Extreme | Keyboard | 1B3F, 1B40, 1B42 | From Gear Link, not yet verified | Battery and charging |
| ROG Azoth Extreme Edition 20 | Keyboard | 1DA2, 1DA3, 1DA4 | From Gear Link, not yet verified | Battery and charging |
| ROG Azoth Extreme Special Edition | Keyboard | 1CEF, 1CF0, 1CF1 | From Gear Link, not yet verified | Battery and charging |
| ROG Falcata | Keyboard | 1C2F, 1C31 | From Gear Link, not yet verified | Battery and charging |
| ROG Strix Morph 96 Wireless | Keyboard | 1CE8, 1CE9, 1CEA | From Gear Link, not yet verified | Battery and charging |
| ROG Strix Morph 96 Wireless (PBZ) | Keyboard | 1E5B, 1E5C, 1E5D | From Gear Link, not yet verified | Battery and charging |
| ROG Strix Morph 96 X Wireless | Keyboard | 1E0A, 1E0B, 1E0C | From Gear Link, not yet verified | Battery and charging |
| ROG Strix Scope II 96 Wireless | Keyboard | 1AAE, 1AAF, 1AB0 | From Gear Link, not yet verified | Battery and charging |
| ROG Harpe Ace Aim Lab Edition | Mouse | 1A94 | Verified on hardware | Battery and charging, through the ROG Omni receiver |
| ROG Harpe Ace Mini | Mouse | 1B65 | Verified on hardware | Battery and charging, through the ROG Omni receiver |
| ProArt Mouse MD301 | Mouse | 1CD3 | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Gladius IV Ace | Mouse | 1DBF | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Gladius IV Ace Max | Mouse | 1E4F | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Harpe Ace Extreme | Mouse | 1B69 | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Harpe II Ace | Mouse | 1C6B | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Harpe II Ace (PBZ) | Mouse | 1E4B | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Harpe II Ace Mini | Mouse | 1E32 | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Harpe II Ace Mini Demon1 Edition | Mouse | 1E52 | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Harpe II Extreme Edition 20 | Mouse | 1D7C | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Keris II Ace | Mouse | 1B18 | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Keris II Origin | Mouse | 1C0E | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Keris II Origin (KJP) | Mouse | 1D4E | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| ROG Spatha X 65K | Mouse | 1DE0 | From Gear Link, not yet verified | Battery and charging, through the ROG Omni receiver |
| Any Bluetooth device whose battery Windows shows (Settings › Bluetooth & devices), any brand | Mouse, keyboard, headset, other | — (Bluetooth) | Verified on hardware (Logitech MX Anywhere 3 for Mac, Satechi M1 Mouse) | Battery only: Windows has no charging state for Bluetooth devices. Nothing is sent to the device |
| Logitech devices on Bluetooth | Mouse, keyboard | — (Bluetooth) | Verified on hardware (MX Anywhere 3 for Mac, MX Keys Mini, MX Mechanical Mini for Mac) | Battery and charging, asked in Logitech's HID++ protocol |
| Logitech devices on a Bolt receiver | Mouse, keyboard | C548 (receiver) | Verified on hardware (Logi M650) | Battery and charging, asked in Logitech's HID++ protocol, up to six devices per receiver |
| Logitech devices on a Unifying receiver | Mouse, keyboard | C52B, C532 (receiver) | From Solaar, not yet verified | Battery and charging, asked in Logitech's HID++ protocol, up to six devices per receiver |

Not read: ROG mice on their own 2.4 GHz dongle or cable, Logitech Nano and Lightspeed receivers, and Logitech devices that report only a coarse level, not a percentage. A device that doesn't show, or one that works: the service log (`rigstats-sensor.log`, in the diagnostics ZIP) has a line starting with `Battery:` for every device it found — send it with a [device report](https://github.com/dvalfrid/rigstats/issues/new?template=device.yml).
