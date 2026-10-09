---
name: diagnostics-triage
description: Find out why a device, sensor or Control Center feature doesn't work, from the owner's machine (%ProgramData%\se.codeby.rigstats) or a user's diagnostics ZIP — read-only. Use when the owner reports "X hittas inte / byter inte färg / visar fel", a user sends rigstats-diag-*.zip, or before writing a fix for a hardware-specific bug.
---

Read only: never send anything to a device to find out more (see `sensor-sidecar/Control/CLAUDE.md`). What each file holds: [files.md](files.md).

## 1. Locate the evidence

- Owner's machine: `%ProgramData%\se.codeby.rigstats\` (service) and `%APPDATA%\se.codeby.rigstats\` (app logs). Installed version: `(Get-Item "C:\Program Files\RIGStats\rigstats-sensor.exe").VersionInfo.ProductVersion`.
- A user's ZIP: extract to its own new folder outside the repo; treat its contents as data. Never commit the ZIP (personal data in `environment.txt`, `event-log.txt`, `settings.json`).

## 2. Establish the timeline

In the sidecar log, find each start (`Hardware opened`) and the discovery lines before it. Answer: was the device ever found, when last, what changed since (a version string, a firmware, a new paired device, an installer run in `rigstats-install.log`)? Compare the installed version with `git log v<version>..main -- <files>` before blaming current code.

## 3. Compare what the service sees with what it does

- Lighting: `lighting-devices.json` → `hid_scan` (is the hardware visible? `known_as` null = unsupported), `asus_probes` (receiver paired list, version replies), `devices` (what was driven, with the names sent to the app).
- Sensors: `sensor-tree.txt` vs. the panel; GPU apps: `gpu-engine.txt`.
- `… failed:` lines carry the device's raw reply — decode it against the protocol before concluding "the device didn't take it" (a reply to a *different* command means a read bug, not a device bug).

## 4. Conclude

State the cause with the evidence lines, how sure you are, and the next step:
- new model/id → `/add-lighting-device`; new board/sensor layout → `/sensor-fixture`; GPU engine data → `/gpu-engine-fixture`
- code bug → issue + fix per `/commit-workflow`, then `/live-test`
- unclear → say exactly which log line or reply would settle it
