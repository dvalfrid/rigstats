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

One folder per **distinct sensor tree**, named as a descriptive kebab slug so
coverage gaps are visible at a glance:

```
<board>-<cpu>-<gpu>[-<discriminator>]
```

Examples: `asus-x670e-ryzen9-7950x-rtx4090`, `msi-b550-ryzen5-5600-rx6700xt`,
`framework-13-intel-i7-1360p-iris-xe`.

The optional `<discriminator>` distinguishes **siblings that share the same
hardware identity but produce a genuinely different tree** (see the dedup rule
below). Pick it to say *why* they differ, in this priority order:

| Cause of the difference | Discriminator | Example |
| --- | --- | --- |
| GPU driver / ADL version changed sensor set | `adl<ver>` / `drv<ver>` | `…-rx9070xt-adl25.6` |
| BIOS / Super-I/O firmware changed | `bios<ver>` | `…-b650m-bios2.18` |
| **Boot / runtime GPU state** (MUX, hybrid, iGPU on/off, eGPU, device disabled) | a state label | `-igpu-only`, `-dgpu-only`, `-hybrid`, `-egpu`, `-mux-discrete` |
| nothing else fits | collection date | `-2026-06` |

Use a bare numeric `-2`, `-3` **only** as a last-resort tiebreak when none of the
above explains the difference. A discriminator that names the cause is always
preferred over a number.

`_sample-synthetic/` is a hand-written example of the format (not real hardware);
keep it as living documentation.

## Dedup rule — when to add a sibling vs. when to skip

The corpus is keyed on **distinct sensor trees**, not on machines. Before adding
a fixture whose `<board>-<cpu>-<gpu>` prefix already exists, compare the freshly
generated golden against the existing sibling(s):

- **Golden is identical** (or differs only in volatile sensor *values*, not in
  the set of sensors / their names) → **do not add a duplicate.** Instead bump
  `confirmed_by` in the existing `meta.json`. We gain nothing from a second copy
  but pay its test time.
- **Golden differs in the *shape*** — a sensor appears/disappears, a name
  changes, or a field flips `null`↔populated → **this is the valuable case.**
  Keep both as siblings with a cause-naming discriminator, and fill
  `variant_of` + `differs` in the new `meta.json` so the relationship is explicit.

A driver/BIOS update that fills a previously-`null` field then surfaces as a
**deliberate, reviewed golden diff** instead of a silent change — exactly the
regression signal we want.

### Three scenarios this covers

1. **Same hardware type, different owners, identical tree** → redundant; bump
   `confirmed_by`, don't add a folder.
2. **Same machine over time, driver/BIOS changed the tree** → sibling with
   `adl…`/`bios…`/date discriminator.
3. **Same machine, different boot/runtime GPU state** (one or more GPUs
   enabled/disabled depending on how it booted — MUX switch, hybrid graphics,
   iGPU toggled in BIOS, eGPU plugged/unplugged) → one sibling **per state**,
   each with a state-label discriminator. These are high value: they exercise
   `select_gpu_idx` / multi-adapter handling with real trees.

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
  "anonymized": true,

  "confirmed_by": 1,
  "gpu_driver": "32.0.21001.6017",
  "bios": "2.18",
  "gpu_state": "all-active",
  "variant_of": null,
  "differs": null
}
```

`collected_at_unix` and `rigstats_version` come straight from `manifest.json`
inside the ZIP. `gpu_driver` / `bios` come from `hardware.json` when present.

Fields supporting the dedup rule (all optional, fill what's relevant):

| Field | Meaning |
| --- | --- |
| `confirmed_by` | How many machines produced this same tree (bump instead of adding a duplicate; default 1). |
| `gpu_driver` / `bios` | The versions in effect — the usual reason a sibling's tree changed. |
| `gpu_state` | Boot/runtime adapter state for scenario 3: e.g. `all-active`, `igpu-only`, `dgpu-only`, `hybrid`, `egpu-attached`. |
| `variant_of` | Slug of the sibling this one branched from (`null` for the first/canonical tree). |
| `differs` | One line on what changed vs. `variant_of` (e.g. `"d3d_vdec now populated (ADL 25.6)"`, `"dGPU absent — booted on iGPU"`). |

## How the tests use these

`FixtureTests` enumerates every subfolder containing a `sensor-tree.txt`, rebuilds
the LHM tree via `SensorTreeLoader`, runs `SensorReader.Extract`, and asserts
structural invariants (GPU count matches, fans > 0, temps ≥ 5 °C, voltages > 0.1 V,
plausible CPU temp). Adding a folder = adding coverage automatically.

### Golden snapshots (`expected.json`)

A fixture may also ship an `expected.json` — the full extracted `SensorPayload`
(snake_case, indented). When present, the test asserts the extraction matches it
**exactly**, so any change in `SensorReader` output is caught precisely. Generate
or refresh it after deliberately changing extraction logic:

```bash
RIGSTATS_WRITE_EXPECTED=1 dotnet test sensor-sidecar.Tests/sensor-sidecar.Tests.csproj
# (writes expected.json into each fixture's output copy; copy it back into the
#  source fixtures/<slug>/ folder and review the diff before committing)
```

On Windows PowerShell: `$env:RIGSTATS_WRITE_EXPECTED="1"; dotnet test …`.

### To contribute a machine

1. *Collect Diagnostics…* in RIGStats (Status window).
2. Open the ZIP, copy out `sensor-tree.txt` (and optionally `hardware.json`).
3. Create `fixtures/<slug>/`, drop the files in, add `meta.json`.
4. `dotnet test sensor-sidecar.Tests/sensor-sidecar.Tests.csproj` — your machine is
   now a permanent regression fixture.
