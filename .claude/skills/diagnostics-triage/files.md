# Diagnostics files — where to look

Full ZIP table: "Diagnostics Export → ZIP contents" in `docs/architecture.md`.

| Question | File (ZIP name / on the owner's machine) | Look for |
|---|---|---|
| Was a lighting device discovered at each start? | `sidecar-log.txt` / `%ProgramData%\…\rigstats-sensor.log` | `Lighting: <model> 0x<pid> …` lines just before `Hardware opened`; `Lighting devices changed: + … - …`; `receiver channel … is not a keyboard (layout reply …)` |
| Did writes fail, and with what reply? | same | `Lighting on <device> failed: … (read back <hex>)`; `Applied profile …` / rollback lines |
| Is the hardware visible to Windows at all? | `lighting-devices.json` | `hid_scan`: VID/PID, usage page, product, `known_as` (null = not supported yet) |
| What sits behind an ASUS receiver? | `lighting-devices.json` → `asus_probes` | `paired_reply` (`01 a0 00 <count> 00` + 4 bytes per device: pid lo, pid hi, report id, type), `paired[]` with `device_info` (version reply) |
| What names/ids did the app get? | `lighting-devices.json` → `devices`; `control-capabilities.json` | `id`, `name`, `blocked`, `problem` |
| Is another program in the way? | `lighting-devices.json` → `conflict`, `windows_dynamic_lighting_on` | Armoury Crate, OpenRGB, Dynamic Lighting |
| Fans / CPU / GPU limits | `sidecar-log.txt`, `control-capabilities.json`, `profiles.json` | verify failures, `drives` mapping, PM table version (unsupported = not verified), safety overrides |
| Missing or wrong sensor values | `sensor-tree.txt`, `hardware.json` | the raw LHM tree → `/sensor-fixture` |
| GPU Apps panel | `gpu-engine.txt` | → `/gpu-engine-fixture` |
| App crashed or misbehaved | `debug.log`, `debug-prev.log`, `event-log.txt` | missing `shutdown: clean` = crash; .NET 1026 / Application Error 1000 |
| Service crashed | `sidecar-log.txt` | copied event-log entries, `crash dump` lines (dumps themselves stay on the machine) |
| Installer / service config | `install.log` / `rigstats-install.log`, `sidecar-service.txt` | PawnIO exit codes, `BINARY_PATH_NAME` (quoted since #277) |

## Case: keyboard on the Omni receiver not found (2026-10-09, #280)

The log showed it last discovered on 2026-10-04; `asus_probes` showed it paired (`0x1C25` on report 02) and answering a version query; its version string had changed that morning (firmware). The live log of the fixed build then showed the layout reply had become `0212120000000000` — all zeros, like the mouse channel — so the layout heuristic took it for a mouse. Fix: identify it from the paired list. Lesson: when a device "disappears" without code changes, look for a changed version string and re-check every heuristic that classifies replies.
