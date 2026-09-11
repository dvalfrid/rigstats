# GPU Engine fixtures — real-hardware corpus

This folder is a versioned corpus of **real PDH `\GPU Engine(*)` instance names
and DXGI adapter lists** captured from contributors' machines. Each fixture is
the exact input `gpu_process::parse_instance` / `aggregate` consume in
production (`gpu_processes.rs`, the "GPU Apps" panel — issue #182), so the
corpus lets us:

- catch instance-name shapes we can't parse on hardware/drivers we've never
  seen (this corpus exists *because* of exactly that: a live AMD driver
  emitted `"Compute 0"` — space-delimited — where the docs and our first
  implementation only handled `"Compute_0"`, underscore-delimited, and a
  fuller capture turned up half a dozen more real shapes: `Video Decode 1`,
  `Video JPEG 0`, `Video Codec Engine`, `High Priority 3D`, `Timer 0`, ...),
- grow coverage across GPU vendors (NVIDIA / AMD / Intel), driver versions,
  and Windows builds with **zero new test code** — the fixture test
  auto-discovers every folder here,
- exercise multi-GPU LUID attribution (`adapter_luid_map`) with real DXGI
  adapter lists instead of only synthetic ones.

This is the same corpus pattern as `sensor-sidecar.Tests/fixtures/` (LHM
sensor trees), applied to a different data source: PDH GPU Engine counters
and DXGI, read directly by the Rust side (`gpu_process.rs`) — this data never
goes through the sensor sidecar. See that folder's README for the sibling
corpus if you're looking for LHM sensor data instead.

## Where the data comes from

**Status → Collect Diagnostics…** writes `gpu-engine.txt` into the diagnostics
ZIP (`rigstats-diag-<ts>.zip`) — the exact output of
`gpu_process::dump_diagnostics()`. That file's format is *already* what a
fixture folder wants, so contributing one is just: export, copy the one file
in, done.

## What we check in (and what we do NOT)

The diagnostics ZIP contains many files with personal data (`environment.txt`
has the username/computer name, `settings.json` may too). **Do not commit the
ZIP.**

Commit only these two files per machine:

| File | Required | Notes |
| --- | --- | --- |
| `gpu-engine.txt` | ✅ yes | The `parse_instance`/`aggregate` input + a DXGI adapter list. Contains only PIDs (ephemeral numbers), LUIDs, engine-type strings, and adapter model names — no usernames, no process names, no file paths. No scrubbing needed. |
| `meta.json`      | ✅ yes | Provenance (see schema below). |

## Naming convention

One folder per **distinct GPU/engine-set combination**, named as a descriptive
kebab slug:

```
<board>-<cpu>-<gpu>[-<discriminator>]
```

**Reuse the same slug as the machine's `sensor-sidecar.Tests/fixtures/` entry
when one exists** — same machine, same identity, easier to cross-reference.
Otherwise follow that corpus's README naming rules directly (they're
identical here): pick a cause-naming `<discriminator>` (`adl<ver>`/`drv<ver>`,
`bios<ver>`, a GPU boot/runtime state label, or a collection date) over a bare
`-2`/`-3`.

`_sample-synthetic/` is a hand-written example of the format (not real
hardware) — keep it as living documentation.

## Dedup rule — when to add a sibling vs. when to skip

Same rule as the sidecar corpus, applied to the engine-type *set* instead of
the sensor tree shape:

- **Same engine-type strings, same adapter shape** (only PIDs/LUIDs/values
  differ — those are always volatile) → **do not add a duplicate.** Bump
  `confirmed_by` in the existing `meta.json` instead.
- **New engine-type string(s) appear**, or the DXGI adapter list has a new
  shape (extra fields populated, different `Flags`, a `phys_N` numbering we
  haven't seen) → **this is the valuable case.** Keep both as siblings with a
  cause-naming discriminator, and fill `variant_of` + `differs`.

## meta.json schema

```json
{
  "slug": "asus-b650m-ryzen7-9800x3d-rx9070xt",
  "synthetic": false,
  "contributor": "github-handle or anonymous",
  "collected_at_unix": 1789152381,
  "rigstats_version": "1.38.0",
  "os": "Windows 11 Home (build 26100)",
  "gpu_driver": null,
  "adapters": ["AMD Radeon RX 9070 XT (dGPU)", "AMD Radeon(TM) Graphics (iGPU)"],
  "gpu_state": "dual-active",
  "notes": "idle-ish desktop; Media Player actively decoding video during capture",

  "confirmed_by": 1,
  "variant_of": null,
  "differs": null
}
```

| Field | Meaning |
| --- | --- |
| `gpu_driver` | Driver version if known (from Status → drivers card or `hardware.json` in the same diagnostics ZIP), else `null`. |
| `adapters` | Human-readable adapter list, for skimming the index without opening every `gpu-engine.txt`. |
| `gpu_state` | Boot/runtime adapter state, mirroring the sidecar corpus's field: e.g. `single-gpu`, `dual-active`, `igpu-only`, `dgpu-only`, `egpu-attached`. High-value when it differs — exercises multi-adapter LUID attribution with a real DXGI list. |
| `confirmed_by` | How many machines produced the same engine-type set (bump instead of adding a duplicate; default 1). |
| `variant_of` / `differs` | Same meaning as the sidecar corpus: sibling relationship + one line on what changed. |

## How the test uses these

`gpu_process`'s fixture test (`#[cfg(test)]`, bottom of `gpu_process.rs`)
enumerates every subfolder containing a `gpu-engine.txt`, extracts the
`=== GPU Engine ===` section's instance names, and asserts **every one parses
via `parse_instance`**.

**If a fixture fails this test, that is the intended signal** — a real
machine produced an instance-name shape `parse_instance`/`compact_engine`
doesn't handle yet. Fix the parser first (a `fix(gpu)` commit extending
`compact_engine`'s match table or `parse_instance`'s shape-matching, with a
unit test reproducing the new shape), confirm the fixture test passes, *then*
commit the fixture alongside the fix — same order as the sidecar corpus's
"logic fix is a separate commit, review before committing" rule.

### To contribute a machine

1. **Status → Collect Diagnostics…** in RIGStats.
2. Open the ZIP, copy out `gpu-engine.txt`.
3. Create `fixtures/gpu-engine/<slug>/`, drop the file in, add `meta.json`.
4. `cargo test --manifest-path src-egui/Cargo.toml -q gpu_process` — your
   machine is now a permanent regression fixture. If it fails, see above.
