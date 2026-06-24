---
name: sensor-fixture
description: Add a real-hardware sensor fixture to the sidecar test corpus. Trigger when the user says "ny sensor-fixture", "add this sensor file", or provides a sensor-tree.txt / diagnostics folder path.
---

When the user gives you a `sensor-tree.txt` (or a diagnostics ZIP / its folder path), add it to the real-hardware corpus. Follow this procedure exactly — do not re-derive it.

## Steps

1. **Read the full file.** Prefer the **diagnostics folder** (or ZIP) path — then `hardware.json` + `manifest.json` sit beside `sensor-tree.txt` and you can fill `meta.json` fully. If only `sensor-tree.txt` is given, look for `hardware.json`/`manifest.json` in the same directory. A pasted snippet is often truncated — prefer the path. The dump format and rules live in `sensor-sidecar.Tests/fixtures/README.md`.

2. **Pick the slug** `<board>-<cpu>-<gpu>[-<discriminator>]` (lowercase kebab), from the `HW` names in the tree and/or `hardware.json`. If that `<board>-<cpu>-<gpu>` prefix already exists, this is a potential sibling — see step 7 before settling on the name; choose a cause-naming discriminator (`adl<ver>`/`bios<ver>`/state label like `igpu-only`/date) over a bare `-2`.

3. **Create `sensor-sidecar.Tests/fixtures/<slug>/`** and copy in `sensor-tree.txt` (required) and `hardware.json` (if present). Write `meta.json` (schema in the fixtures README; `collected_at_unix` + `rigstats_version` from `manifest.json`; fill `gpu_driver`/`bios`/`gpu_state`/`variant_of`/`differs` when it's a sibling).

4. **PII check:** only `sensor-tree.txt` + `hardware.json` are safe (hardware model names only). **Never commit the full ZIP** — `environment.txt`/`event-log.txt`/`settings.json` carry user identity. Glance through before committing.

5. **Run tests:**
   ```powershell
   dotnet test sensor-sidecar.Tests/sensor-sidecar.Tests.csproj -c Release
   ```
   The auto-discovering `FixtureTests` theory now covers the new machine — no code change needed.

6. **Generate the golden snapshot:**
   ```powershell
   $env:RIGSTATS_WRITE_EXPECTED = "1"
   dotnet test sensor-sidecar.Tests/sensor-sidecar.Tests.csproj -c Release
   Remove-Item Env:\RIGSTATS_WRITE_EXPECTED
   ```
   Copy each generated `expected.json` from `sensor-sidecar.Tests/bin/Release/net10.0-windows/fixtures/<slug>/` back into the source `fixtures/<slug>/`. Re-run without the flag to confirm it compares equal.

7. **Dedup check** (only if the `<board>-<cpu>-<gpu>` prefix already existed): compare the new golden to the existing sibling(s).
   - **Shape identical** (same sensor set/names; only values differ) → discard this fixture, bump `confirmed_by` in the existing `meta.json`.
   - **Shape differs** (sensor appears/disappears, name changes, field flips `null`↔populated) → keep as sibling with cause-naming discriminator; fill `variant_of` + `differs`. See the README "Dedup rule" + "Three scenarios".

8. **Review the golden `expected.json` for silent gaps** — sensors that came out `null`/missing although the hardware clearly exposes them (e.g. a `d3d_vdec` stays null because the real sensor is named `D3D Video Decode 1`). Report findings; a logic fix is a separate `fix(...)` commit + issue and requires regenerating affected goldens.

9. **Commit:**
   ```
   test(sidecar): add fixture <slug>
   ```
   Type `test` — does not surface in the changelog, which is correct. No issue required for a pure fixture add. A `confirmed_by` bump with no new folder: `test(sidecar): confirm fixture <slug> on second machine`.
