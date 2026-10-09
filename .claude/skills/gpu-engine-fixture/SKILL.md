---
name: gpu-engine-fixture
description: Add a real-hardware GPU Engine fixture to the gpu_process.rs test corpus. Trigger when the user says "ny gpu-engine-fixture", "add this gpu-engine.txt", or provides a gpu-engine.txt / diagnostics ZIP path for the GPU Apps panel.
---

When the user gives you a `gpu-engine.txt` (or a diagnostics ZIP / its folder path), add it to the real-hardware corpus at `src-egui/fixtures/gpu-engine/`. Follow this procedure exactly — do not re-derive it. Sibling to `/sensor-fixture` (same pattern, different data source — GPU Engine PDH counters + DXGI instead of LHM sensor trees); the rules live in `src-egui/fixtures/gpu-engine/README.md`.

## Steps

1. **Read the full file.** Prefer the **diagnostics folder** (or ZIP) path so `manifest.json` sits beside `gpu-engine.txt` and you can fill `meta.json`'s `rigstats_version`/`collected_at_unix` from it. If only `gpu-engine.txt` is given, ask for the rest only if you need it. A pasted snippet is often truncated — prefer the path.

2. **Pick the slug** `<board>-<cpu>-<gpu>[-<discriminator>]` (lowercase kebab). **Reuse the sensor-sidecar corpus's slug for the same machine if one exists** (`sensor-sidecar.Tests/fixtures/`) — check there first. If the `<board>-<cpu>-<gpu>` prefix already exists in `fixtures/gpu-engine/`, this is a potential sibling — see step 6 before settling on the name; choose a cause-naming discriminator over a bare `-2`.

3. **Create `src-egui/fixtures/gpu-engine/<slug>/`** and copy in `gpu-engine.txt` (required, unmodified). Write `meta.json` (schema in the fixtures README).

4. **PII check:** `gpu-engine.txt` contains only PIDs, LUIDs, engine-type strings, and adapter model names — no usernames, no process names, no file paths. It needs no scrubbing (unlike other diagnostics-ZIP files — never commit the full ZIP).

5. **Run the fixture test:**
   ```powershell
   cargo test --manifest-path src-egui/Cargo.toml -q gpu_process::fixture_tests
   ```
   The auto-discovering `all_gpu_engine_fixtures_parse` test now covers the new machine — no code change needed, *unless it fails*.

6. **A failure is the point, not a problem.** If `all_gpu_engine_fixtures_parse` fails, the fixture contains a GPU Engine instance-name shape `parse_instance`/`compact_engine` doesn't handle yet (e.g. a new `engtype_` string, or a delimiter variant). Fix it first:
   - Extend `compact_engine`'s match table (or `parse_instance`'s shape-matching) in `src-egui/src/gpu_process.rs`.
   - Add a unit test in `mod tests` reproducing the new shape directly (don't rely on the fixture alone to cover it).
   - Re-run the fixture test to confirm it now passes.
   - This is a separate `fix(gpu)` commit from the fixture-add commit (see step 8) — logic fix first, fixture second, same order as `/sensor-fixture`.

7. **Dedup check** (only if the `<board>-<cpu>-<gpu>` prefix already existed): diff the new file's distinct `engtype_` strings and DXGI adapter shape against the existing sibling(s).
   - **Same engine-type set, same adapter shape** (only PIDs/LUIDs/values differ) → discard this fixture, bump `confirmed_by` in the existing `meta.json`.
   - **New engine-type string(s) or adapter shape** → keep as sibling with cause-naming discriminator; fill `variant_of` + `differs`.

8. **Commit:**
   ```
   test(gpu): add fixture <slug>
   ```
   Type `test` — does not surface in the changelog, which is correct. If step 6 required a parser fix, that's a separate preceding commit: `fix(gpu): handle <what changed> in GPU Engine instance names`. A `confirmed_by` bump with no new folder: `test(gpu): confirm fixture <slug> on second machine`.
