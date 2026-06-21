# Sensor fixtures — real-hardware corpus

This folder is a versioned corpus of **real LibreHardwareMonitor sensor layouts**
captured from contributors' machines. Each fixture is the exact input that
`SensorReader.Extract` consumes in production, so the corpus lets us:

- catch sensors we silently drop on hardware we've never seen (the #110 goal),
- validate the filtering thresholds (`< 5 °C`, `> 0.1 V`, storage prefixes, …)
  against real chips,
- grow coverage across boards / GPUs / Super I/O chips / RAM types with **zero new
  test code** — `FixtureTests` auto-discovers every folder here.

## What we check in (and what we do NOT)

The diagnostics ZIP (`rigstats-diag-<ts>.zip`, from Status → *Collect Diagnostics…*)
contains many files. **Do not commit the ZIP** — several of its files contain
personal data (`environment.txt` has the username/computer name, `event-log.txt`
and `settings.json` may too).

Commit only these two text files per machine:

| File | Required | Notes |
| --- | --- | --- |
| `sensor-tree.txt` | ✅ yes | The `Extract` input. Contains only hardware model names (public info), no user identity. |
| `hardware.json`   | optional | WMI ground truth (GPU/VRAM/RAM/board) — handy to cross-check expected output. |
| `meta.json`       | ✅ yes | Provenance (see schema below). |

`sensor-tree.txt` needs no scrubbing in practice (it has no usernames/paths), but
**glance through it before committing** and remove anything you consider private.

## Naming convention

One folder per machine, named as a descriptive kebab slug so coverage gaps are
visible at a glance:

```
<board>-<cpu>-<gpu>
```

Examples: `asus-x670e-ryzen9-7950x-rtx4090`, `msi-b550-ryzen5-5600-rx6700xt`,
`framework-13-intel-i7-1360p-iris-xe`. On collision append `-2`, `-3`, …

`_sample-synthetic/` is a hand-written example of the format (not real hardware);
keep it as living documentation.

## meta.json schema

```json
{
  "slug": "asus-x670e-ryzen9-7950x-rtx4090",
  "synthetic": false,
  "contributor": "github-handle or anonymous",
  "collected_at_unix": 1750000000,
  "rigstats_version": "1.33.0",
  "os": "Windows 11 24H2 (build 26100)",
  "notes": "idle snapshot",
  "anonymized": true
}
```

`collected_at_unix` and `rigstats_version` come straight from `manifest.json`
inside the ZIP.

## How the tests use these

`FixtureTests` enumerates every subfolder containing a `sensor-tree.txt`, rebuilds
the LHM tree via `SensorTreeLoader`, runs `SensorReader.Extract`, and asserts
structural invariants (GPU count matches, fans > 0, temps ≥ 5 °C, voltages > 0.1 V,
plausible CPU temp). Adding a folder = adding coverage automatically.

### To contribute a machine

1. *Collect Diagnostics…* in RIGStats (Status window).
2. Open the ZIP, copy out `sensor-tree.txt` (and optionally `hardware.json`).
3. Create `fixtures/<slug>/`, drop the files in, add `meta.json`.
4. `dotnet test sensor-sidecar.Tests/sensor-sidecar.Tests.csproj` — your machine is
   now a permanent regression fixture.
