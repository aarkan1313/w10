# WorldGen10 — Biome Composition Layer Design

**Date:** 2026-06-01
**Milestone:** Phase 5. Builds the missing layer between the grammar and the height formulas: a deterministic,
seam-exact **biome composition layer** that turns "where am I + what biome(s) the grammar places here" into a
single height by **blending biome PARAMETERS and generating once** (param-blend), not by averaging finished
terrains (field-blend).
**Status:** design-ready; offline Python render-first; implementation gated by owner approval of THIS spec. No
Rust/GLSL port here (that is Slice 3, which this unblocks).
**Parents / context:**
- `docs/superpowers/specs/2026-05-30-worldgen-core-design.md` — already specified `grammar.blend_biome_params(x,z)`
  + a single `generate`; this layer BUILDS that named mechanism (it was specced but never actually built — the
  biome synths drifted into bespoke whole-world generators instead).
- `docs/superpowers/specs/2026-05-30-worldgen10-north-star-vision.md` — "the grammar blends per-biome PARAMS,
  one generator makes infinite seamless terrain" is the vision this realizes.
- `keeper_v2.py` (`compose_windowed_height_v2`) — the owner-accepted (2026-06-01) seam-safe base engine that
  becomes the single shared parameterized generator.
- The 13 biome synths (`mountain_synthesis.py`, `desert_synthesis.py`, … ) — the owner-accepted-setup biome
  characters that become the PARAM PRESETS.
- `tools/dem_pack/terrain_edits/` — the accepted edit framework that composes ON TOP of this base (unchanged).
- ROADMAP Phase 5 Slice 2/2A/3; memory `worldgen10-keeper-formula-fork` (v2 accepted), `worldgen10-north-star-vision`.

---

## 1. Purpose

WorldGen10 has, today, a pile of good per-biome generators (13 `*_synthesis.py`) and an owner-accepted base
height stack (`keeper_v2`, the rough-highlands biome) — but **nothing composes them into one world.** No code
imports the biome synths except their own test/render/export tooling; each is a standalone whole-window
generator. The "all-biome transition" review scene (`biome_transition_world_review.tscn`) glues 12 pre-baked
biome tiles into a static 4×3 atlas with a field-crossfade — a look reference, not a runtime architecture.

This is a real separation-of-concerns gap: there is no single "give me the height at (x,z) for whatever biome
the grammar says is here, blended seam-exactly with its neighbors" entry point. This spec defines that layer.

The immediate deliverable is a deterministic, seam-exact biome composition layer where biomes plug into one
dispatch+blend interface and boundaries are a principled cross-recipe blend. The structural deliverable is the
`blend_biome_params` + biome-`generate` seam the roadmap's Slice 3 already names as its Rust port target —
built and owner-accepted offline, so the port has a frozen, validated thing to port.

> **★ REVISED 2026-06-01 (probe finding → Fork B). Supersedes the "one shared generator / pure param-blend"
> framing in §3 below; read this first.** A render-first probe (`tools/dem_pack/probe_v2_as_mountain.py`,
> `D:/tmp/wg10_biome_compose/probe_v2_as_mountain.png`) tested whether the owner-accepted `keeper_v2` engine can
> express a mountain by tuning `KeeperV2Params`. **It cannot:** cranking relief/ridge/peak knobs just makes a
> TALLER version of v2's rounded rolling hills — it never grows the oriented ridge-lines / connected ranges that
> `mountain_synthesis` produces (v2's regime model has no oriented-ridge vocabulary; this matches memory
> `worldgen10-too-flat-decomposition`). So **"biomes = pure presets of ONE v2 engine" (clean Fork A) is DEAD** —
> it would lose accepted biome looks. A parallel chat's `biome_transition_world` scene was confirmed to be a
> static FIELD-BLEND review atlas (compose-big-then-slice), NOT a runtime architecture — no competing design.
> **DECISION (owner-approved): Fork B.** Each biome KEEPS its own composition recipe (mountain keeps oriented
> ridges, etc.); the unification is (1) make every recipe SEAM-SAFE (replace per-window `zscore`/`norm01` with
> data-independent `affine_remap`, the v2 lesson), and (2) plug them into ONE dispatch+blend interface that
> crossfades at boundaries with a PRINCIPLED cross-recipe blend (NOT mushy field-averaging — the mechanism is the
> one genuinely-unsolved piece). Slice 1 is therefore a render-first BLEND PROBE on two accepted recipes
> (mountain + grassland) to prove the blend is tractable BEFORE converting all 13. `BiomeParams` becomes
> {recipe selector + that recipe's knobs}, still blendable where recipes share primitives. The rest of this spec
> stands EXCEPT: §3.4 "one engine, presets" → per-recipe dispatch; §3.5 "param-blend" → principled cross-recipe
> blend (param-blend where recipes share primitives, mask handoff otherwise); §7 slice plan gains the slice-1
> blend probe. §4 (synths as the accepted source) is REINFORCED by the probe.
>
> **★★ BLEND PROBE RESULT + DECISION 2026-06-01 (`tools/dem_pack/probe_biome_blend.py`,
> `D:/tmp/wg10_biome_compose/probe_biome_blend.png`).** Rendered a mountain↔grassland transition with 3 blend
> mechanisms. **All three read as believable transitions — NOT the feared mushy ghost** — because the synths
> SHARE warp/fbm primitives (the blend morphs between RELATED terrains, not unrelated ones; this narrows the
> earlier "field-blend is mushy" worry to CLASHING pairs). Findings: (1) plain field-blend is an acceptable
> simple fallback for primitive-sharing recipes; (2) "height-favored" — bias the blend weight toward the
> locally-higher-relief recipe inside the band so mountain structure stays CRISP instead of ghost-flattening —
> read best. **DECISION (by the four pillars): blend mode is a TUNABLE `blend_mode` knob; PRIMARY = `height_favored`
> (pillar 3 quality: only one that protects structure through the band; pillar 1: tunable; pillar 4: builds the
> mechanism that actually works rather than assuming the easy case generalizes), FALLBACK = `field` (pillar 2:
> cheap lerp). Height-favored's extra cost (a band-local blurred-relief proxy) is bounded and off the hot
> interior.** HONEST CAVEAT: mountain↔grassland is a GENTLE pair; the real stress test is a structurally CLASHING
> pair (mountain↔desert-dunes, where dune-train directionality could fight ridge orientation). Per pillar 4, the
> plan's FIRST slice re-runs this probe on a clash pair before any bulk conversion — if height_favored needs
> adjustment there, we learn it at biome 2, not 13.

## 2. Non-Goals

- **No Rust/GLSL port.** Offline Python, render-first (all of Phase 5). The port is Slice 3, unblocked by this.
- **No new world facts / no grammar rewrite.** The grammar (WHERE biomes go) is kept; this layer consumes its
  biome weights. Extending the grammar to emit smooth per-(x,z) biome weights is in scope only as much as the
  blend needs (a deterministic, apron-reachable weight field) — not a full biome-placement redesign.
- **No final biome tuning.** The synth presets are SETUP-GRADE (deep tuning explicitly deferred per the biome
  promotion docs). We lock the `BiomeParams` INTERFACE and the composition mechanism, NOT the preset values.
- **No materials/textures.** Grey height-shaded review only (Phase 6 is materials). The look judgment here is
  about HEIGHT structure and transitions, with the honest caveat that some transition quality only becomes
  visible once textured (§8).
- **No field-blend.** Averaging finished terrains is explicitly rejected (§3.5) as the architecture; the
  existing field-blend scene is kept only as the "baseline to beat" in the §7 step-4 bake-off.
- **No magic numbers.** Every biome character value lives in a named `BiomeParams` preset (pillar 1).

## 3. Architecture

A new layer slots between the grammar and the height engine. The 13 biome synths stop being whole-world
generators and become **parameter presets** fed to ONE shared generator; blending two biomes = blending their
param dicts, then generating ONE terrain that morphs continuously across the boundary.

```
world (x,z) + seed
      │
      ▼
  GRAMMAR ──→ biome weights at (x,z)        e.g. {mountain: 0.7, grassland: 0.3}
      │            (deterministic f(world pos), smooth/apron-reachable band)
      ▼
  BIOME REGISTRY ──→ each biome = a BiomeParams preset (extracted from its synth)
      │
      ▼
  blend_biome_params ──→ one blended BiomeParams = Σ weightᵢ · paramsᵢ
      │
      ▼
  generate(params) ──→ height   (the v2 / geography_engine engine, parameterized)
      │
      ▼
  TERRAIN-EDIT FRAMEWORK (already built) ──→ base + Σ edit deltas
      │
      ▼
  Wg10Facts.get_height (M4 seam, unchanged)
```

### 3.1 `BiomeParams` — the data contract
A flat, blendable dataclass of named structural knobs the shared generator consumes (e.g. `warp_amount`,
`warp_freq`, `ridge_strength`, `regime_weights`, `incision_depth`, `relief_scale`, smoothing sigmas, …; ~15–25
fields). **The single home for biome character — no magic numbers anywhere else.** Contract: a weighted sum of
two `BiomeParams` is a valid `BiomeParams` (blend = arithmetic on the dataclass). Non-linearly-blendable knobs
are either reparameterized to blend cleanly or documented as pick-dominant (§3.6).

### 3.2 Biome registry — the presets
`biome_params(name) -> BiomeParams`. The 13 synths' hand-tuned constants extracted into named presets. "Mountain"
becomes DATA, not code. Tunable (pillar 1); setup-grade values, re-tunable later without touching other units.

### 3.3 `blend_biome_params(x, z, seed) -> BiomeParams` — the composition
Asks the grammar for biome weights at `(x,z)`, looks up each weighted biome's preset, returns the weighted blend.
**Deterministic `f(world position)`** — same world coord → same blend regardless of which window asked. This is
what makes it apron-safe and seam-exact. It is the function the core-spec named and Slice 3 will port.

### 3.4 `generate(wx, wz, seed, params: BiomeParams) -> height` — the one shared engine
The owner-accepted `keeper_v2` / `geography_engine` engine, refactored to take `BiomeParams` instead of hardcoded
constants. ONE generator, ONE code path, fed different params. The blend happens on PARAMS (cheap, ~20 floats),
then ONE generate call — not two terrains averaged. This is the thing that already works; we parameterize it,
we do not rewrite it.

### 3.5 Param-blend, not field-blend (the decisive quality call)
Field-blend (compute each biome's full height in the overlap, then crossfade the two fields — what the existing
review scene does, `_compose_world`: `h_m += weight * h_part_m`) produces the "transitions feel averaged or
pasted" failure the ROADMAP explicitly lists as a yellow/red flag: a mountain crossfaded with a plain reads as a
half-height mountain ghost, not a foothill. Param-blend morphs ONE terrain — ridges lose amplitude and gain
plain-smoothness as you cross — which reads as a real transition AND costs one generate, not two. Param-blend
wins pillars 1 (it IS the north-star mechanism), 2 (single generate, no double-compute in the band), and 3
(natural morph). It is the committed architecture.

### 3.6 Separation of concerns (why this is the modular win)
Each unit answers cleanly "what does it do / how do I use it / what does it depend on", and is independently
testable: `blend_biome_params` tests with fake presets and never calls the generator; `generate` tests with one
fixed preset and never touches the grammar; a biome's preset swaps without touching any other unit. The names
map 1:1 onto what the roadmap already committed to (`blend_biome_params`, `generate`).

## 4. Param source: synths supersede DEM-distillation (validated)

The core-spec (2026-05-30) said biome params come from DISTILLING real DEMs (Slice 2). What actually got built +
reviewed is the 13 hand-authored synths. The divergence is recorded here because it was VALIDATED against the
pillars, not assumed:
- **Quality (pillar 3, decisive):** the DEM-distillation LOOK was NEVER owner-accepted (ROADMAP line 346, verbatim:
  "Tooling is BUILT + kept, but the LOOK is NOT accepted"). The synths ARE owner-accepted (≥4 "promoted as an
  owner-accepted setup biome": glacial/karst/volcanic/desert, plus mountain/grassland and others reviewed). Building
  on the accepted source is the only choice consistent with "look-quality is owner-judged" (DESIGN §7.3).
- **Adaptable (pillar 1):** the synths already share `generate(wx,wz,seed,style,feature_span_m)` and several expose
  a style/params dataclass — they are already most of the way to being presets; the interface fits.
- **No shortcuts (pillar 4):** we change the roadmap based on what was actually validated, not on theory.

**Decision:** the accepted biome SYNTHS are the param-preset source. DEM-distillation is KEPT as a
superseded-but-available refinement that can feed the SAME `BiomeParams` interface later IF its look is ever
accepted — it is not deleted, and it is not the source now. The two doc edits in §10 record this.

## 5. Tunability (pillar 1, first-class)

Every biome is a `BiomeParams` preset — swap a preset, dial a knob, no code change. The grammar's biome-weight
field is itself tunable (placement + transition-band width). "Whole world is one biome" falls out for free (the
grammar returns one biome everywhere). The owner's spatial model is "tunable, but usually biomes are regions
within one continuous world" — so cross-biome blending is the default path, single-biome a config of it.

## 6. Verification

- **Parameterization is inert:** `generate(BiomeParams.from_v2())` reproduces current `keeper_v2` output bit-for-bit
  (the refactor changed nothing — pure parameterization). Hard gate before anything else.
- **Determinism:** same `(x,z,seed)` → same blended params and same height.
- **Seam-exactness (hard gate):** adjacent-window border delta 0.0 for the blended-param height (carve-then-slice
  for review scenes; the pure `f(world pos)` discipline for independent windows). The blend band fits inside the
  apron (same constraint keeper_v2's gaussian blur respects).
- **Preset fidelity:** each biome's preset alone reproduces its synth's character within a stated tolerance.
- **Blend naturalness (OWNER eye, the acceptance authority):** rendered boundary reads as a continuous transition,
  not an averaged ghost or a hard seam. First proven at one boundary (§7 step 4) before bulk conversion.
- **Tunability:** changing a named knob measurably changes the result; no magic numbers in the height path.
- **Non-repetition / no-artifact:** no straight scaffolding, cells, chunks, or repeated stamps across a blend.

## 7. Slice plan

Offline Python; TDD; render-first. Sequenced so the architecture is proven at a real boundary BEFORE all 13
biomes are committed to it.
1. **`BiomeParams` + shared `generate(params)`.** Refactor keeper_v2/geography_engine to consume `BiomeParams`.
   Gate: identical to current v2 when fed v2's values (inert parameterization).
2. **`blend_biome_params` + grammar weights.** Grammar → biome weights `f(x,z)` → weighted param blend. Gates:
   determinism, seam-exactness (border delta 0.0), blend band ≤ apron.
3. **Convert the first 3 biomes to presets** (mountain, grassland, rough-highlands/v2). Extract synth constants
   into `BiomeParams`. Discover + handle non-blendable knobs here. Gate: each preset reproduces its synth.
4. **First boundary render + OWNER FLY — GO/NO-GO.** Render mountain→grassland both ways (field-blend baseline
   vs param-blend) side by side; owner judges. Bless the param-blend seam before converting the other 10. Wrong
   → fix the seam at biome 3, not 13.
5. **Convert the remaining 10 biomes to presets.** Mechanical once the pattern is blessed. Gate per biome:
   preset reproduces synth character.
6. **Full multi-biome compose + render/fly.** All 13 in one grammar-driven continuous world (replaces the static
   4×3 atlas). Owner accepts the whole-world read.
7. **Doc reconciliation + the two roadmap/core-spec edits (§10).** Update STATUS/ROADMAP; freeze the
   owner-accepted `generate` + `blend_biome_params` as the Slice 3 port target.

## 8. Boundary / honest risk

- Design-direction until the owner accepts the flown blend. Passed unit gates ≠ acceptance (DESIGN §7.3).
- **Blendability isn't free.** Some synth knobs may not blend linearly (log-space frequencies, categorical
  styles). Each is reparameterized to blend cleanly or documented as pick-dominant. Discovered during step-3
  extraction — not assumed to be all linear.
- **Param-blend may not VISIBLY beat field-blend in grey.** Owner chose to spec param-blend on principle; the
  step-4 fly is where it earns it. If it doesn't read better untextured, that is an ACCEPTED known — the
  architecture is still right (north-star path, perf-better) and the payoff may only show once materials land
  (Phase 6). A "looks the same in grey" fly is NOT read as failure of the architecture.
- **Setup-grade presets.** Extracted from setup-grade synths; values WILL need re-tuning. We lock the interface,
  not the values (pillar 1).
- **Grammar weight field.** The blend needs a smooth, apron-reachable, deterministic biome-weight field. If the
  current grammar emits hard per-region assignment, producing that smooth field is part of step 2 (kept minimal —
  not a placement redesign).
- Downstream unblocked on acceptance, not done here: the Rust/GLSL port (Slice 3), materials (Phase 6), true
  hydrology (Phase 7B).

## 9. KEPT, proven, don't rebuild

The grammar (WHERE biomes go), the `keeper_v2` engine (becomes the parameterized `generate`), the terrain-edit
framework (composes on top, unchanged), the M4 Facts seam, the render/streaming pipeline, the seam-safe windowed
discipline. This layer is additive between grammar and height.

## 10. Roadmap / core-spec edits this spec carries (applied on acceptance)

1. **Core-spec `2026-05-30-worldgen-core-design.md`:** note the biome-param SOURCE is the accepted synths,
   superseding the "distill from DEMs" plan (kept as superseded-but-available, same `BiomeParams` interface).
2. **ROADMAP Phase 5:** record that biome params come from synths (not DEM-distillation, whose look was never
   accepted), and that THIS biome composition layer is what Slice 3 ports (`generate` + `blend_biome_params`,
   names the roadmap already uses). Slice 2 (DEM distillation) marked superseded-as-source, tooling kept.
