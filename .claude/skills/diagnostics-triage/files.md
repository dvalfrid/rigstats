# Diagnostics files — where to look

Full ZIP table: "Diagnostics Export → ZIP contents" in `docs/architecture.md`.

| Question | File (ZIP name / on the owner's machine) | Look for |
|---|---|---|
| Was a lighting device discovered at each start? | `sidecar-log.txt` / `%ProgramData%\…\rigstats-sensor.log` | `Lighting: <model> 0x<pid> …` lines just before `Listening on \\.\pipe\rigstats-sensors` (`Hardware opened. Listening on …` up to 1.48); `Lighting devices changed: + … - …`; `receiver channel … is not a keyboard (layout reply …)` |
| Did writes fail, and with what reply? | same | `Lighting on <device> failed: … (read back <hex>)`; `Applied profile …` / rollback lines |
| Is the hardware visible to Windows at all? | `lighting-devices.json` | `hid_scan`: VID/PID, usage page, product, `known_as` (null = not supported yet) |
| What sits behind an ASUS receiver? | `lighting-devices.json` → `asus_probes` | `paired_reply` (`01 a0 00 <count> 00` + 4 bytes per device: pid lo, pid hi, report id, type), `paired[]` with `device_info` (version reply) |
| What names/ids did the app get? | `lighting-devices.json` → `devices`; `control-capabilities.json` | `id`, `name`, `blocked`, `problem` |
| Is another program in the way? | `lighting-devices.json` → `conflict`, `windows_dynamic_lighting_on` | Armoury Crate, OpenRGB, Dynamic Lighting |
| Was a ROG mouse found, and what does it answer? | `sidecar-log.txt`; `lighting-devices.json` | `Mouse: <model> 0x<pid> (<link>)` at discovery; `Mouse: … read <settings> from 120000:<hex> 120400:<hex> …` — the raw replies and what they were read as (#438), to check an unverified model against `MouseProtocol.Parse`; `Mouse: … writes <hex …>` — the frames a change sent (no report id), to check against the model's `MouseFrames` (its family: the row in `MouseCatalog`, #432); `didn't answer <question> … asking again`; `settings not read`. Not found: `hid_scan` → its collection `known_as: asus_mouse_settings` (found) or `null` (not in `MouseCatalog`, or another interface / report id); on the Omni receiver `asus_probes` → `paired` |
| Headset settings (sidetone, noise reduction) | `sidecar-log.txt` | `Headset: <name> 0x<pid> read HeadsetSettings { … } from <hex> <hex> <hex>` (replies to `12 24`, `12 19`, `41 20`); `settings not read`; after a change `Headset: … sidetone on 16, noise reduction …` |
| Which onboard mouse profile a profile uses | `profiles.json` → `mouse.profile`; `sidecar-log.txt` | `Mouse: <model> on onboard profile <n>.` |
| Fans / CPU / GPU limits | `sidecar-log.txt`, `control-capabilities.json`, `profiles.json` | verify failures, `drives` mapping, PM table version (unsupported = not verified), safety overrides |
| Missing or wrong sensor values | `sensor-tree.txt`, `sensor-tree-now.txt`, `hardware.json` | the raw LHM tree at service start / when the ZIP was collected (#557, ZIPs from 1.51 on) — a sensor in the first but not the second went missing while the service ran → `/sensor-fixture` (from `sensor-tree.txt`) |
| A GPU value missing from the panel | `sensor-tree-now.txt`, `dashboard-now.json`, `debug.log`, `driver-updates.txt` | the tree = what the service reads; `service_line.gpu_devices` = what it sent the app, `shown` = what the panel shows from it (ZIPs from 1.51 on, #559); `gpu: showing <card> — temperature missing, …` = when it changed; match that time against the display driver resets / sleep-wake section at the end of `driver-updates.txt` |
| GPU Apps panel | `gpu-engine.txt` | → `/gpu-engine-fixture` |
| App crashed or misbehaved | `debug.log`, `debug-prev.log`, `event-log.txt` | missing `shutdown: clean` = crash; .NET 1026 / Application Error 1000 |
| Sensors gone or a hardware item missing | `sidecar-log.txt` | `The hardware worker ended during … (exit code …) while reading <hardware> (<id>)`, `did not answer … within 8 s`, `… crashed the hardware worker 3 times — it is no longer read` (#374); `[hardware-worker]` lines = the worker's own stack |
| Service crashed | `sidecar-log.txt` | copied event-log entries, `crash dump` lines (dumps themselves stay on the machine) |
| Installer / service config | `install.log` / `rigstats-install.log`, `sidecar-service.txt` | PawnIO exit codes (`50` + `windows_build` < 17763 = Windows older than 1809, #507), `BINARY_PATH_NAME` (quoted since #277) |

## Every ZIP becomes test data

After the triage, add from the ZIP whatever the corpora don't have yet, in the same PR or a `test:` PR of its own. The table of which ZIP file goes into which corpus, and the test that runs it, is in `sensor-sidecar.Tests/fixtures/README.md` ("Everything else a diagnostics ZIP gives the tests"). In short:
- the sensor tree → `/sensor-fixture`, with `hardware.json` and its `expected-wmi.json`
- `gpu-engine.txt` → `/gpu-engine-fixture`
- the log's `Mouse:`/`Headset:`/`Keyboard: … read … from` lines → `fixtures/peripherals/`
- `Aura:` config lines → `fixtures/aura/`
- `hid_scan` → `fixtures/hid-scan/`
- `gpu-power.json` → `fixtures/gpu-power/`
- `displays.json` → `src-egui/fixtures/displays/`

A failure when adding is a finding, not a nuisance: a reading the parser gets wrong, or a device recognised differently than at capture.

## Case: keyboard on the Omni receiver not found (2026-10-09, #280)

The log showed it last discovered on 2026-10-04; `asus_probes` showed it paired (`0x1C25` on report 02) and answering a version query; its version string had changed that morning (firmware). The live log of the fixed build then showed the layout reply had become `0212120000000000` — all zeros, like the mouse channel — so the layout heuristic took it for a mouse. Fix: identify it from the paired list. Lesson: when a device "disappears" without code changes, look for a changed version string and re-check every heuristic that classifies replies.
