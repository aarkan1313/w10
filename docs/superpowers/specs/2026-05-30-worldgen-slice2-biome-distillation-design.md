# WorldGen10 — Worldgen Slice 2: Biome Distillation (offline) design spec

**Date:** 2026-05-30
**Milestone:** Worldgen-core rebuild, **Slice 2** (offline Python). Distills the 115 real DEMs (12 families)
into per-biome STRUCTURAL parameter-sets that drive the warped-noise `generate` from Slice 1.
**Status:** design approved (brainstorm), pre-plan.
**Predecessor:** Slice 1 (`worldgen_proto.generate`, owner-accepted "pretty good, a little noisy"). The
spectral kernel-DNA approach was REFUTED by eye (spectrum = roughness, discards phase = structure) — this
slice deliberately measures STRUCTURAL metrics that drive structure-generating machinery, NOT a spectrum.

> Parent docs: `docs/superpowers/specs/2026-05-30-worldgen-core-design.md` (§4 biome schema + distillation,
> §8 Slice 2), `…/2026-05-30-worldgen10-north-star-vision.md`. Living docs (DESIGN/ROADMAP/STATUS) + HANDOFF
> stay current and point here. Memory: `worldgen10-north-star-vision`, `worldgen10-wg9-height-recipe`.
> Loose ends: `docs/plans/LOOSE_ENDS_LEDGER.md` — B1/B2/B3 close before Slice 3 (the first runtime build),
> NOT before this slice. This slice is entirely offline and cannot touch the running engine.

---

## 1. Purpose & the bar

Slice 1 proved the warped-noise generator reads as contiguous structured terrain from HAND-TUNED params.
Slice 2 replaces the hand-tuning with params **distilled from the real DEMs**, so each of the 12 biome
families gets a structural identity derived from real-world terrain. The bar (owner):

- **Per-family character match** — the synthesized terrain reads as the SAME KIND of terrain as that
  family's real DEMs (ridge spacing, valley density, roughness, relief feel) — **NOT the same place.**
  The synth is an infinite field; the DEM is one fixed tile; they must NOT match pixel-for-pixel (that
  would mean copying/tiling the DEM — against the whole no-tiling architecture). Character, not copy.
- **Honest, debuggable, fast-to-first-image** — interpretable metrics, simple documented transforms,
  render-first; refine by eye. The v1 metric set is a sensible starting point, **NOT claimed final**
  (worldgen-core spec §4 says exactly this).

**Look-quality is owner-judged.** Gates prove invariants (determinism, bounds, metric-measures-what-it-
claims, non-repetition); they CANNOT judge "looks like that biome." Every look decision ends in an owner eye
verdict on rendered images. (The S1/spectral discipline.)

## 2. The 12 families (real, from the approved map)

`tools/dem_pack/kernel_family_map.approved.json` maps 115 kernels → 12 families (counts):

```
coast 13 · badlands 12 · grassland 11 · karst 11 · glacial 10 · mountain 10 · rainforest 10 ·
desert 9 · volcanic 9 · temperate 7 · tundra 7 · wetland 6        (= 115 kernels, 12 families)
```

Real DEMs live in WG9 (READ-ONLY): `D:/workflows/worldgen9/factory/kernels/<id>/normalized_height.npy`
(z-score normalized, mean 0 / std 1) + `<id>/kernel.json` metadata (`height_range_m`,
`approx_sample_spacing_m`, `sample_px`). Each family has multiple member kernels.

## 3. Architecture & data flow

A new offline Python module `tools/dem_pack/biome_distill.py` (pure measurement + mapping functions +
one orchestrator). Pure/IO split mirrors the existing codebase (`dem_pack_lib.py` = pure, testable;
`build_pack.py` = I/O). Nothing touches the engine, Rust, GLSL, or render — entirely `tools/dem_pack/`.

```
For each family (12):
  for each member kernel:
    load normalized_height.npy (z-score) + kernel.json meta            # I/O (orchestrator)
    drop if max|z| > MAX_ABS_ZSCORE (=12)  — reuse build_pack's spike guard (clean set, no fake height)
    un-normalize to METRES (§5) ; spacing_m = approx_sample_spacing_m  # real-world units
    measure STRUCTURAL METRICS (§4) in real units                      # pure
  aggregate metrics: MEDIAN of each metric across the family's kernels  # pure (robust to one outlier)
  map metrics -> generator knobs via documented transforms (§4)         # pure
  -> BiomeParams for that family
write all 12 param-sets into the pack:  families[*].biome_params {...}   # additive; kernel pixels KEPT for now
```

**Median aggregation** (owner-chosen): one param-set per family = the "typical member" identity, robust to a
single weird kernel. No intra-family variability machinery in v1 (YAGNI; the generator doesn't consume it).

## 4. The metric set + metric→knob mapping (v1)

A small, robust, interpretable set — each metric drives exactly one knob. **Structural descriptors driving
structure-GENERATING machinery (ridged noise, warp, carving)** — never statistics-driving-plain-noise (the
refuted spectral path). The generator (`worldgen_proto.generate`) consumes: `relief_m, octave_amps[6],
ridge_strength, valley_depth, warp_amount, base_freq, ridge_freq, valley_freq, warp_freq` (+ schema
`slope_bias`). All transform constants are named config at the top of `biome_distill.py` (no magic numbers).

**Metric SOURCE (data-driven decision — survey of all 12 families' `kernel.json`):** WG9's pipeline already
pre-computed some structural metrics in `kernel.json`. A survey showed which are trustworthy and which are
not, and the choice is made by the pillars + AAA output (NOT by least-code):
- **TRUST the vetted metadata** where it cleanly separates families: `height_range_m` (relief — mountain
  4361 m vs grassland 903 m vs wetland 507 m), `mean_slope_deg`/`slope_p50/p95_deg` (slope — 10.5° vs 1.6°
  vs 0.4°), and `height_std_m`/`curvature_abs_mean`/`roughness_residual_std_m` as roughness cross-checks.
- **DO NOT trust `ridge_density`/`valley_density`** — they are a CONSTANT 0.100 for EVERY kernel in EVERY
  family (WG9's ridge/valley detector is degenerate). Trusting them would make `ridge_strength`/`valley_depth`
  identical across all 12 biomes → the "everything looks the same" collapse the pillars forbid. So the two
  knobs that make a biome read as itself are **computed from the raw DEM here.**
- **`anisotropy_score` is weak** (0.19–0.36, barely separates) → used only as a hint; the warp driver is the
  DEM-computed coherence.
This hybrid is strictly better for the pillars: it neither re-derives the vetted slope/relief (no waste) nor
inherits WG9's dead ridge/valley detector (no biome-collapse). Computed metrics are fixture-monotonicity
gated; trusted-metadata metrics are range/finite asserted.

| Metric (real units) | Source / Method | → Knob | Transform (v1) |
|---|---|---|---|
| **relief_real_m** | **metadata** `height_range_m` (vetted, clean separation) | `relief_m` | direct |
| **amp_profile[6]** | **computed** — bandpass DEM into 6 octave bands (difference-of-Gaussian-blurs); each band's std. **Amplitude only — never phase-as-structure** (the spectral lesson) | `octave_amps[6]` | normalize so `amps[0]=1.0`; rest = relative band stds |
| **dominant_wavelength_m** | **computed** — peak band of the amp profile → its centre wavelength in metres (pixels × spacing_m) | `base_freq` | `1 / dominant_wavelength_m` |
| (derived) | fixed ratios of base_freq (coherent freqs; S1 convention) | `ridge_freq`, `valley_freq` | ridge ≈ 2× base, valley ≈ 1.2× base (config) |
| (derived) | warp lower-freq than features (S1 convention) | `warp_freq` | `1 / (k · dominant_wavelength_m)`, k≈2.7 (config) |
| **ridge_linearity** (0..1) | **computed** (WG9 ridge_density is dead-constant 0.100) — on upper-elevation mask: structure-tensor eigenvalue ratio (λ₁≫λ₂ ⇒ linear/ridgey; λ₁≈λ₂ ⇒ blobby) | `ridge_strength` | clamped linear map → [0, ~1] |
| **incision_depth** (m) | **computed** (WG9 valley_density is dead-constant 0.100) — curvature-gated local relief: depth of valleys below surrounding ridgelines, in real metres | `valley_depth` | normalized by relief → [0, ~1] |
| **anisotropy/flow** (0..1) | **computed** (metadata anisotropy_score too weak, 0.19–0.36) — dominant-orientation coherence of the gradient field | `warp_amount` | fraction of dominant_wavelength_m, scaled by flow score (warp in metres, ∝ feature size) |
| **slope_bias** | **metadata** `mean_slope_deg` (vetted, clean separation 10.5°/1.6°/0.4°) | `slope_bias` | direct — **STORED in schema but NOT yet consumed** by the current `generate()`; documented, consumed in a later generator rev (no silent dead field, no silent generator change) |

**Honest caveat (spec §4 of the parent):** which metrics best capture "looks like mountains" is the tuning
part. v1 is a sensible starting set; renders + the owner's eye drive refinement; the schema is extensible.

## 5. Real-scale handling (the z-score trap — the one correctness landmine)

WG9 DEMs are z-score normalized (mean 0, std 1); real elevation range is `kernel.json.height_range_m`; real
horizontal spacing is `approx_sample_spacing_m`. So before measuring:

- **Vertical:** convert z-score → metres so incision/relief are in REAL metres (else they're meaningless
  across families with different relief). (z-score × a per-kernel vertical scale derived from
  `height_range_m`; exact conversion stated in the plan, applied identically to all kernels.)
- **Horizontal:** every wavelength/spacing measured in pixels × `approx_sample_spacing_m` → real metres, so
  `dominant_wavelength_m` and all derived freqs are real-world.
- **Spike guard:** reuse `build_pack.py`'s `MAX_ABS_ZSCORE = 12` drop, so distillation sees the SAME clean
  kernel set the pack ships — corrupt spike pixels never poison a metric.

## 6. Pack storage (additive, validated)

The pack gains per-FAMILY structural params. Important structural fact: the CURRENT pack's `families` dict is
keyed per-KERNEL (`families[<kernel_id>] = {kernel, relief_m, footprint_m}`, per `dem_pack_lib.
build_pack_dict`), and the family label lives in the palette/family map — there is no per-family entry today.
Slice 2 distills ONE param-set per FAMILY (12 of them), so it adds a NEW top-level table keyed by family
name, leaving the existing per-kernel `families` dict untouched:

```
biome_params: {                    # NEW top-level table, keyed by FAMILY name (12 entries)
  "mountain":  { relief_m, octave_amps[6], ridge_strength, valley_depth, warp_amount,
                 base_freq, ridge_freq, valley_freq, warp_freq, slope_bias },
  "grassland": { ... },
  ... (all 12 families)
}
# existing per-kernel `families` dict + kernels/*.npy: UNTOUCHED, KEPT (atlas removal is Slice 4, runtime)
```

The exact key/shape is finalized in the plan, but the principle is fixed: **additive, a separate per-family
table, existing per-kernel entries and pixels untouched** (no existing pack consumer breaks; the runtime
loader reads this table by family in Slice 3+). **Validated on write/load:** reject NaN/degenerate/out-of-
range params with a descriptive error naming the family (pillar 4 — no silent default). **Parity-readiness
constraint (forward-looking, pillar 4):** distilled values must be finite, f32-representable, and within each
knob's documented domain (e.g. `ridge_strength`/`valley_depth` ∈ [0, ceiling], freqs > 0), so the Slice 3/4
GLSL mirror can represent them exactly and the parity contract is not silently violated downstream. Clamp +
descriptive-error on violation; do NOT silently coerce. Distillation keeps the source DEMs to regenerate.

## 7. Verification

Two-layer + render-images-first (look is the point; gates can be blind to it — the spectral lesson).

**Automated gates** (`tools/dem_pack/test_biome_distill.py`, pure/headless, joins the existing pytest suite):
- **Determinism + finite** — same kernels → same metrics → same params; all finite.
- **Metric-measures-what-it-claims (fixture monotonicity)** — a hand-built LINEAR-ridge synthetic array
  scores higher `ridge_linearity` than a flat array; a deeply-carved fixture scores higher `incision_depth`
  than a smooth one; a directional fixture scores higher anisotropy. (The "assert on a known fixture"
  discipline — proves the metric isn't measuring noise.)
- **Produced params pass the generator's closed-form bounds** (the S1 bound test, reused).
- **Non-repetition autocorrelation gate** (reuse S1's) on a synth field generated from REAL distilled
  params — proves real params still don't tile.

**Render-image verification (the real signal)** — `tools/dem_pack/render_biomes.py` writes, per family,
a **real-vs-synth side-by-side hillshade** at MATCHED metres/pixel, captioned with that family's distilled
metrics, to `D:\tmp\`. (Owner render choice: side-by-side real vs synth.)

**Owner-flown/eyeballed acceptance (the bar)** — the owner judges per-family CHARACTER MATCH (same kind of
terrain), not pixel copy. Recorded verbatim in STATUS.

**Honest baseline** — gates prove determinism/bounds/metric-validity/non-repetition; they do NOT prove
"looks like that biome." STATUS says so.

## 8. The four pillars
1. **Adaptable** — every transform constant is named config; metric set extensible; params are the biome DNA
   the grammar blends. The framework spine.
2. **Performance** — distillation is OFFLINE (once per pack); runtime cost unchanged (params are tiny floats,
   no atlas). N/A to the live budget.
3. **Quality** — structural metrics → structure machinery (ridges/valleys/warp), the thing the spectrum
   lacked; deterministic; bounded; validated-on-load; non-repetition gated.
4. **No shortcuts** — params DISTILLED from real DEMs in REAL units (honest, real-world-informed), validated/
   rejected; structural-metrics-drive-structure (not the dead spectrum); render-images-first so we never
   claim a look a gate can't see (the spectral lesson); `slope_bias` stored-not-consumed is documented, not
   silently dead or silently wired.

## 9. Slice plan (prove on 3, then fan to 12 — fail cheap like S1)

1. **Build** `biome_distill.py` (metrics §4 + mapping + real-scale §5) + `test_biome_distill.py` (TDD:
   fixture monotonicity, determinism, bounds, non-repetition). Gate green.
2. **Prove on 3 contrasting families** — `mountain`, `grassland` (plains-like), `badlands`. Distill + render
   real-vs-synth for those 3. **Owner eye verdict.** If the mapping reads wrong, refine the transforms HERE —
   cheap, before 12. (Retires the metric→knob risk on 3, the S1 discipline.)
3. **Fan to all 12** — distill every family; render the full real-vs-synth contact sheet (coast, karst,
   glacial, rainforest, desert, volcanic, temperate, tundra, wetland + the first 3). **Owner eye verdict
   per family.**
4. **Write** params into the pack (`families[*].biome_params`), validate-on-load (descriptive errors),
   non-repetition gate on a distilled-param field.
5. **Update living docs** (STATUS/ROADMAP) + commit the spec/plan. Precondition reminder for the READER:
   B1/B2/B3 + (already-done) doc drift close before Slice 3, NOT now.

**Order rationale:** the scariest unknown (does measured structure → a good-looking synth?) is retired on 3
families in OFFLINE images before spending effort on 12 — the S1/spectral discipline (fail cheap, offline,
owner-judged, before any runtime).

## 10. Risks & mitigations
- **Metric→knob mapping produces ugly/uncharacteristic synth** → proven on 3 families in offline renders
  before 12; refine transforms cheaply; owner eye is the judge (render-first).
- **z-score/real-scale mishandled → meaningless cross-family metrics** → §5 makes real-unit conversion
  explicit + identical across kernels; fixture tests assert metric behavior on known arrays.
- **Distilled metrics don't capture biome identity** → render each distilled family (S2 step 3), refine by
  eye; schema extensible; the v1 set is explicitly not claimed final.
- **Real params accidentally tile** → non-repetition autocorrelation gate on a distilled-param field.
- **Pack storage breaks existing consumers** → additive only (kernels/pixels KEPT until Slice 4); the plan
  picks a per-family param table that doesn't disturb the current per-kernel/palette structure; validate-on-
  load.
- **slope_bias confusion (stored but unused)** → documented explicitly as stored-for-later; not wired into
  `generate()` this slice (that's a generator change = different slice).

## 11. Definition of done
- `biome_distill.py` produces per-family param-sets from the real DEMs in real units; `test_biome_distill.py`
  green (determinism, fixture monotonicity, bounds, non-repetition) in the dem_pack pytest suite.
- Real-vs-synth renders for all 12 families written; **owner eye verdict per family recorded verbatim**
  (character match accepted, or refine-and-re-render noted).
- Pack carries validated `biome_params` for all 12 families (kernels/pixels still present; atlas removal is
  Slice 4) — every value finite, f32-representable, within its knob domain (parity-readiness for Slice 3/4).
- STATUS/ROADMAP updated; spec + plan committed. No Rust/GLSL/engine/render touched.
