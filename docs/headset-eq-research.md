# Headset equalizer — research (#483)

What ASUS headsets' equalizer is, which commands set it, and what RIGStats can
build on. Sources were read as text and never run. Nothing in this document
has been sent to a headset yet.

## Sources

| Source | What it gave | How |
|---|---|---|
| **ASUS Gear Link** (gearlink.asus.com, ASUS's WebHID app) | All 8 headset manifests and modules (`/view/<pid>/`): each model's declarative command schema (`equalizerStatus`, `equalizerConfig`, …), its `deviceProps` (`legacy`, `hardwareEffect`) and its `render.equalizer` caps (presets, bands, gain range) | Downloaded to a scratch folder outside the repo with `scratch/tools/gl-modules.mjs <dir> 04`; schema and caps pulled out with `gl-hs-schema.mjs` and `gl-hs-render.mjs` |
| **G-Helper** (`AsusHeadset.cs`, `hsmodels/`) | The same frames, the gain scale per model, preset gain tables, and a firmware quirk | Read as documentation, no code copied |

## Which headsets have an equalizer in the headset

Gear Link marks an equalizer that runs in the headset with
`deviceProps.hardwareEffect.equalizer`. Its other audio effects — reverb,
virtual surround, the parametric EQ, the "audio profiles" — run in ASUS's
own Windows audio service (`window.service`, `SetAudioEffects`), not in the
headset, so RIGStats leaves them out.

| Model | USB id | In RIGStats | Hardware EQ | Schema | Bands shown | Gain scale (flat) |
|---|---|---|---|---|---|---|
| ROG Delta II | 1AFA | yes (verified) | yes | legacy | 8: 125 Hz – 16 kHz | 0–100 (50) |
| ROG Delta II KJP | 1D41 | yes | yes | legacy | 8 | 0–100 (50) |
| ROG Delta II PBZ | 1E6B | yes | yes | legacy | 8 | 0–100 (50) |
| ROG Pelta | 1B82, 1B84 | yes | yes | legacy | 10: 32 Hz – 16 kHz | 0–24 (12) |
| ROG Cetra Open Wireless | 1CAC | no | yes (+ phantom bass) | newer | 10 | 0–24 |
| ROG Gjallar | 1CB0 | no | yes | newer | 10 | 0–24 |
| ROG Pelta Core | 1D62 | no | **no** (`hasHWAudioEffects: false`) | — | — | — |

The five models RIGStats already supports are exactly the five with the
legacy schema, so the EQ can be built for all of them with one set of
frames.

## Commands (legacy schema)

Gear Link's schema for 1AFA, 1D41, 1E6B, 1B82 and 1B84 is identical for the
equalizer, and G-Helper's `AsusHeadset` sends the same bytes. Frames are
`[report id, command, key, 00, 00, data…]`, as for sidetone (#428).

| Gear Link name | Frame | Meaning |
|---|---|---|
| `equalizerStatus` get | `12 21` | reply byte 5 = on (1) / off, bytes 6–15 = the 10 band gains |
| `equalizerStatus` set | `41 03 00 00 <0/1>` | EQ on / off |
| `equalizerConfig` set | `41 06 00 00 <band> <gain>` | one band (index 0–9) |
| `equalizerConfigs` set | `41 04 00 00 <10 gains>` | all bands at once — Gear Link uses it only to reset |

**Bands.** The reply always carries 10 gains. On the Delta II family Gear
Link shows only bands 2–9 (125, 250, 500 Hz, 1, 2, 4, 8, 16 kHz); bands 0
and 1 (32 and 64 Hz) are left at flat, as G-Helper's presets for these
models also do. The Pelta shows all 10.

**Gain.** Not in dB on the wire. Delta II family 0–100 with 50 flat
(G-Helper's default and its test reply for `12 21`; Gear Link's Delta II
caps give no EQ range — its 0–100 sliders there belong to the reverb, so
the live read must confirm this); Pelta
0–24 with 12 flat (G-Helper's `Pelta.MaxEqualizerGain`, Gear Link's
`slider: {start: 0, end: 24}`). Neither source labels the steps in dB, so
RIGStats shouldn't either.

**Quirk (G-Helper):** the firmware ignores an all-bands table sent right
after the EQ is switched on — G-Helper pauses 100 ms and then sends every
band on its own as well. Writing changed bands one by one with `41 06`
(as Gear Link does for a slider) avoids it.

## Presets

The headset keeps only the current gains (and on/off). There is no preset
command in the legacy schema: Gear Link's preset list comes with its audio
service, and its gain tables are not in the modules. G-Helper carries
tables of its own:

- Delta II family (0–100): Default, Classic, Hip hop, Jazz, Metal, Rock,
  Techno, Vocal.
- Pelta (0–24): the same eight plus Communication, Immersive, Movie, Music,
  CS2, FPS.

A preset in RIGStats would write those gains band by band. They are
G-Helper's numbers, not ASUS's — to be labelled as such.

## Newer schema (Cetra Open Wireless, Gjallar) — not built

These keep EQ settings per *audio profile* inside the headset:
`equalizerStatus` key 36 `[audioProfileId, status]`, `equalizerConfig`
key 37 `[audioProfileId, equalizerProfileId, bandCount, (index, gain)…]`,
`equalizerProfile` key 45, `audioProfileConfig` key 46, phantom bass key 39
(Cetra Open). The command byte these keys go with is set elsewhere in the
newer protocol and wasn't settled here, and RIGStats doesn't support
either headset yet (no battery, lighting or settings). Left for when one of
them is added.

## What RIGStats can build (proposal)

1. **Equalizer group on the Headset page** for the five supported models:
   on/off, one slider per band the model shows (8 or 10, labelled in Hz /
   kHz), "Flat" to reset, and the presets as a dropdown (G-Helper's tables,
   marked "not yet verified"). Read with `12 21` when the page opens,
   changed bands written with `41 06` (`41 03` for on/off), read back.
   Kept in the headset, like sidetone.
2. **Open question for the owner — EQ per Control Center profile?** Today
   headset settings are kept in the headset for every profile. An EQ preset
   is the kind of setting the "does it change with what I'm doing?" rule
   puts in a profile (FPS while gaming, Music otherwise). Option: a profile
   part "Headset EQ preset: leave unchanged / <preset>", like the mouse's
   onboard profile, with the sliders staying device-kept.
3. **Live check first** on the owner's Delta II: read `12 21` (the answer's
   status and 10 gains), then one band up and back, then a preset.
