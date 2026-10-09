---
name: add-lighting-device
description: Add support for a new RGB/lighting device (or new models of an existing family) to the Control Center's Lighting provider in sensor-sidecar/Control/Lighting/, including regenerating docs/supported-devices.md and the website table. Use when the user wants RIGStats to drive a new keyboard, mouse, headset, monitor, motherboard controller or other lighting device, or adds a model id / Aura fixture.
---

Read `sensor-sidecar/Control/CLAUDE.md` first. You never send anything to a real device — the owner tests live.

## Before writing code: the protocol

Native protocols only; other software is documentation, never copied. Sources in order of cost are in `docs/control-architecture.md` → "Finding a lighting protocol" (OpenRGB source, read-only probes from a user's `lighting-devices.json`, ASUS GearLink, unpacked vendor app). **Only commands that are documented or observed — never sweep unknown command numbers, not even "get"s.**

If the device exposes a HID LampArray, `LampArrayDevice` may already drive it — check before writing a driver.

## New model of an existing family

Add the product id to the family's model table (`AsusMonitorDevice.Models`, `AsusKeyboardDevice`, `AsusHeadsetDevice`, `AuraUsb` …) with `Verified = false` unless it has been seen working on real hardware. Motherboards: add an Aura fixture instead (`sensor-sidecar.Tests/fixtures/aura/README.md`). Then regenerate (below).

## New device driver

1. **Issue + branch** per `/commit-workflow`.
2. `Lighting/<Name>Device.cs` implementing `ILightingDevice` (+ `ILampDevice` for a white lamp): stable `Id` (`"<family>-<vid>-<pid>"`), `Kind`, `Zones`, `Apply`, `Release` (back to the device's own effect), `Blocked` when another controller owns it (Armoury Crate, Windows Dynamic Lighting — yield, don't fight), and `Diagnostics()` with everything a fixture needs.
3. A `static Discover(IReadOnlyList<HidDeviceInfo> hid)` and a call to it in the discovery lambda in `Program.cs` (`new LightingProvider(Hid.Enumerate, hid => { … })`).
4. Add its rows to `LightingCatalog.All()` so the supported-devices list includes it.
5. **Tests** in `sensor-sidecar.Tests/LightingTests.cs`: report bytes for each effect against a fake HID device, read-back verification where the device supports it, discovery by VID/PID/usage page.
6. Notes in `docs/control-architecture.md` → "More lighting devices".

## Regenerate the supported-devices list (always, after a model-table change)

```powershell
$env:RIGSTATS_UPDATE_SUPPORTED_DEVICES=1; dotnet test sensor-sidecar.Tests --filter SupportedDevices; Remove-Item Env:RIGSTATS_UPDATE_SUPPORTED_DEVICES
```

This rewrites `docs/supported-devices.md` and the table in `website/index.html`; commit both. Without it `SupportedDevicesTests` fails.

## Finish

`cargo xtask verify` (stop `rigstats-sensor` first; ask the owner if it runs). Hand over for live testing with `tools\dev-sidecar.ps1 -Live`; flip `Verified = true` only after the owner confirms it works, then regenerate again.
