# WorldGen10 — Worldgen Core (param-driven warped-noise) design spec

**Date:** 2026-05-30
**Milestone:** FOUNDATIONAL — replaces the height core (`height::height` + `sample_kernel` tiling). The
heart of the north-star vision: a contiguous, structured, infinite, "Google-Maps-explorable" landmass.
**Status:** design approved (brainstorm), pre-plan.
**Predecessor:** the spectral kernel-DNA approach was REFUTED by eye (spectrum=roughness, discards
phase=structure). WG9 was traced as a reference (`worldgen10-wg9-height-recipe`). Owner steered the design:
contiguous structured landmass is priority 1; "like Google Maps explore — no chunks/squares/lines/
repetition"; kernels become a distilled DNA library, not sampled pixels.

> Living docs (DESIGN/ROADMAP/STATUS) + HANDOFF stay current and point here. Memory:
> `worldgen10-north-star-vision`, `worldgen10-wg9-height-recipe`. Loose ends tracked in
> `docs/plans/LOOSE_ENDS_LEDGER.md` (B1/B2/B3 + doc drift close BEFORE the first runtime build, Slice 3).
> 2026-05-31 code-audit addendum: the old DEM-pack `normalized_height.npy × relief_m(height_range_m)`
> contract is invalid for z-score kernels and must not be copied into this rebuild. If a kernel/residual
> layer survives as material/detail, convert z-score to metres with `height_std_m` or rebake to a documented
> range and gate the contract.

---

## 1. Purpose & the bar

WG10 terrain reads as "blobby, placed, chunks/squares/lines, same region repeating" — because
`sample_kernel` reads kernel DEMs as TILING textures and makes the tiled kernel the whole height. The bar
(owner): **a fully contiguous procedural world that looks "like pulling up Google Maps and exploring
around"** — no grid/tiling/repetition artifacts at any zoom, real-world-plausible structure (ridgelines,
valleys, drainage) everywhere, seamless biome transitions. Priority 1 = the contiguous structured landmass
(biome character + materials build on it later).

## 2. The architecture (owner-locked)

> **PARAM-SOURCE UPDATE (2026-06-01):** the per-biome param SOURCE below ("real DEMs distilled into per-biome
> structural DNA") is SUPERSEDED by the owner-accepted hand-authored biome SYNTHS (`*_synthesis.py`). The
> DEM-distillation look was never owner-accepted (ROADMAP Slice 2); the synths were reviewed + promoted
> biome-by-biome. DEM-distillation is kept as a superseded-but-available refinement feeding the SAME
> `BiomeParams` interface. The mechanism below (grammar places + blends params → one generator) is UNCHANGED and
> is built by `docs/superpowers/specs/2026-06-01-worldgen-biome-composition-layer-design.md`.

Parameter-driven procedural worldgen: real DEMs distilled into per-biome structural DNA; the grammar places
+ smoothly blends that DNA; one continuous warped-noise generator turns blended DNA into infinite seamless
terrain. **Kernels are never sampled at runtime — they are the offline DNA library, so they cannot tile.**

```
OFFLINE (tools/, Python, once per pack):
  per biome family: analyze its kernel DEMs → a BIOME PARAMETER SET (structural DNA)
  store param-sets in the pack (NOT pixels)

RUNTIME (height.rs pure Rust + GLSL mirror, parity-gated, per (x,z)):
  params = grammar.blend_biome_params(x,z)     # grammar places + smoothly blends DNA (continuous)
  h      = generate(x, z, params)              # the warped-noise toolkit (§3)
  return h * relief_scale                       # existing knob
```

**Replaced:** `height::height` + `sample_kernel` (tiling pixel reader) → param-driven `generate`; pack
`kernel` pixel refs → `biome_params`. **Kept (proven):** grammar (WHERE biomes go — extended to blend
params), the clipmap render pipeline, facts/collision, relief_scale, the parity-gated noise primitives
(hash/value_noise/fbm).

**Three anti-repetition layers (the "no chunks/squares/lines" bar):** (1) MATH — domain warping kills the
grid/regular/repeat look; (2) KERNELS — never sampled → cannot tile; (3) RENDER SEAMS — clipmap page-edge
lines, handled by the kept render pipeline + its loose ends (B2/tile-edge), separate from this worldgen.

**Game-mode flex (built in, not bolted on):** infinite (native `f(x,z)`) · bounded/Diablo (sample a region
or lock a biome) · island/SotF (mask knob) · spherical planet/SE (feed sphere-surface coords — SAME
generator, swap coordinate domain) · handmade (blend authored params in, same blend mechanism). Everything =
knob-presets + coordinate-domain over one continuous field — which is WHY it flexes across games.

## 3. The warped-noise generator (`generate(x,z,params)`)

Local, deterministic, parity-safe. Stages, each param-driven (pseudocode):

```
generate(p, b):                                  # b = blended BiomeParams
  # 1. DOMAIN WARP — bend space so nothing reads as grid/repeat (the anti-tiling spine).
  w = p + b.warp_amount * vec2(fbm(p*WARP_FREQ+17), fbm(p*WARP_FREQ+43))
  # 2. MACRO LANDMASS — continuous multi-octave fBm, amplitudes from b.octave_amps (contiguous base).
  h = 0; freq = BASE_FREQ
  for i in 0..N: h += b.octave_amps[i] * value_noise(w*freq); freq *= 2
  # 3. RIDGES — ridged noise (1-|noise|) = linear ridgelines, upland-amplified, scaled by b.ridge_strength.
  upland = smoothstep(LOW, HIGH, h)
  h += b.ridge_strength * upland * ridged_fbm(w*RIDGE_FREQ)
  # 4. VALLEYS — inverted ridged noise carves connected drainage, depth = b.valley_depth.
  h -= b.valley_depth * ridged_fbm(w*VALLEY_FREQ)
  # 5. relief
  return h * b.relief_m
```

- **Why it hits "Google-Maps contiguity":** domain warp (stage 1, applied once, inherited downstream)
  destroys the regular/grid/repeat look — features meander like real geography; all stages are
  `f(world pos)` over continuous noise → no tiles/seams at any zoom; ridged noise → linear ridgelines (not
  blobs); inverted-ridged → connected valleys (the structure pure noise lacks).
- **Scale = config knobs (the 1-10m adaptable target):** `BASE_FREQ`, `WARP_FREQ`, `RIDGE_FREQ`,
  `VALLEY_FREQ`, `N` are config — dial ~1km features down to 1-10m near-field detail per game.
  Keep this distinct from runtime clipmap resolution: the current render stack still couples
  `BASE_SPAN`, `PAGE_PX`, 2^L spans, and shader detail frequency. Phase 5 can expose landform/content
  scale as a creative knob, but Slice 5 must still design the per-level runtime scale policy.
- **Parity + perf:** `value_noise` (parity-proven primitive) summed/warped/ridged — deterministic, CPU==GPU,
  cheap (N+few noise evals/sample). No atlas, no pixel sampling — GPU gets simpler/cheaper than today. The
  hardened GPU-time gate measures it.

## 4. Biome parameter schema + offline distillation

```
BiomeParams {
  relief_m,                 # characteristic elevation/range target for this generator, not the old z-score scalar
  octave_amps[N],           # relative amplitude per octave (roughness AMPLITUDE only)
  ridge_strength,           # mountainous/ridged 0..1
  valley_depth,             # drainage incision
  warp_amount,              # flowing/meandering vs blocky
  slope_bias,               # gentle vs steep
  # extensible: more structural descriptors as the look demands
}
```

Distillation (offline, Python): per biome family, measure STRUCTURAL metrics from its real DEMs — NOT a
power spectrum (the dead end): ridge_strength ← connected-linear-ridge vs scattered-bump stats;
valley_depth ← drainage-incision stats; octave_amps ← multi-scale amplitude (spectrum-LIKE but only the
roughness amplitude, PAIRED with the structural params the spectrum lacked — that's the fix); warp_amount/
slope_bias ← anisotropy/slope-distribution stats; relief_m ← chosen generator relief target. If this is derived
from DEM z-score kernels, use `height_std_m` or a documented rebake; do not reuse the old
`normalized_height.npy × height_range_m` contract.

**Pillar-4 principle:** structural descriptors driving structure-GENERATING machinery (ridged noise, warp,
carving) — not statistics driving plain noise (the refuted spectral path). **Honest:** which metrics best
capture "looks like mountains" is the tuning part — start with a sensible set, render, refine by eye; the
schema is extensible. v1 metric set NOT claimed final.

Storage: pack family entry → `{biome_params:{...}}`, ~15 floats vs 256K pixels (atlas removed). Validated on
load (reject NaN/degenerate, descriptive errors). Distillation tool keeps source DEMs to regenerate.

## 5. Grammar param-blending (seamless transitions)

The grammar ALREADY returns smoothly-blended continuous `family_weights(x,z)`. Repoint them from kernels to
params:

```
blend_biome_params(x,z,pack):
  weights = grammar.family_weights(x,z,pack)     # UNCHANGED — smooth, continuous, corner-blended
  b = BiomeParams::zero()
  for (family, w) in weights:
    bp = pack.biome_params(family)
    b += w * bp        # weighted vector sum of every param (linear OR log per param — schema marks which)
  return b
```

**Why seamless (the key):** `family_weights` is already continuous, so blended params change SMOOTHLY across
space; at a mountain→plains border, ridge_strength/valley_depth fade smoothly → the generator's STRUCTURE
smoothly morphs (mountains flatten into plains) with NO seam — because it's one continuous field with
smoothly-changing knobs, not two stitched terrains. Grammar stays the WHERE-authority (provinces/regions/
palette rolls) unchanged; we only change what the weights multiply. Per-param linear-vs-log blend marked in
the schema (amplitudes linear; frequencies log) so transitions stay natural.

## 6. The four pillars
1. **Adaptable** — STRONGEST fit: everything is params/knobs (biome param-sets, warp/ridge/valley/scale
   knobs, blend widths); game modes = presets + coordinate-domain. The framework spine.
2. **Performance** — runtime = blend a few param vectors (cheap, grammar already does it) + N+few noise
   evals; offline distillation; no atlas/pixels/global-sim; hardened GPU-time gate guards it.
3. **Quality** — one continuous warped-noise field + smoothly-blended params → seamless/contiguous by
   construction; deterministic + CPU/GPU parity (value_noise primitive); bounded; structure (ridges/valleys)
   the spectrum lacked.
4. **No shortcuts** — params DISTILLED from real DEMs (honest, real-world-informed), validated/rejected;
   structural-metrics-drive-structure (not the dead spectrum); render-images-first so we don't claim a look
   a gate can't see (the spectral lesson).

## 7. Verification

Two-layer + render-images-first (look is the point; gates can be blind to it — the spectral lesson).

**Automated gates:** contiguity/seam (abutting regions + biome borders bit-continuous) · determinism ·
CPU/GPU parity (re-baselined `gpu_parity_check` to `generate`, CPU==GPU epsilon) · bounded (closed-form
ceiling) · **NEW non-repetition gate** (sample a large area, assert LOW auto-correlation at the old tiling
periods → proves the grid/chunks/repeat are gone — guards the owner's specific complaint) · facts/collision
parity (visible==collision, relief_scale holds) · hardened GPU-time perf (after the B3 fix).

**Render-image verification (dev-time, the real signal):** every slice renders hillshaded top-down +
perspective images so we SEE contiguity/structure/repetition BEFORE claiming it works. First-class practice.

**Owner-flown acceptance (the bar):** fly it — contiguous, structured, explorable ("Google Maps"), distinct
seamlessly-transitioning biomes, no chunks/squares/lines/repetition. Only the owner judges this.

**Honest baseline:** gates prove contiguity/parity/perf/no-tiling-metric; they do NOT prove "looks like real
geography" — that's the render images + the fly. STATUS says so.

## 8. Slice plan (prove-one-at-a-time, render-images-first)

- **Slice 1 — Generator prototype (OFFLINE Python, RENDER-FIRST).** Build `generate` (warp+macro+ridges+
  valleys), hand-tuned params for ~3 biomes; render hillshaded images over a large area. **Owner judges:
  contiguous structured terrain? Google-Maps-ish?** *Retires the core look-risk OFFLINE before any Rust/GLSL
  — fail cheap like spectral did.*
- **Slice 2 — Biome distillation (OFFLINE Python).** kernel→biome-param distillation (structural metrics);
  param-sets for all 12 families from real DEMs; render each biome from its DISTILLED params. Owner judges
  per-biome fidelity; non-repetition gate.
- **Slice 3 — Rust generator core.** Port `generate` + `blend_biome_params` to `height.rs` (replace
  `sample_kernel`/`height`); loader reads `biome_params`. Gates: determinism/bounded/seam/non-repetition
  (fast, headless). Render-parity vs the Python prototype. **(Precondition: B1/B2/B3 + doc pass closed first —
  this is the first runtime BUILD.)**
- **Slice 4 — GPU parity + integrate.** Mirror `generate` in GLSL; remove the kernel atlas; re-baseline
  `gpu_parity_check`; wire into render + facts (relief_scale, visible==collision); hardened perf gate.
- **Slice 5 — Scale tune + grammar-blend live + fly.** Tune scale knobs toward 1-10m adaptable; confirm
  seamless biome transitions under a real fly; "Google Maps contiguity" acceptance fly; audit vs pillars;
  update living docs.

**Order rationale:** the scariest unknown (does warped-noise structure LOOK contiguous + real?) is proven in
OFFLINE images in S1-S2 before any runtime rebuild (the spectral discipline). Then core→parity→integrate→
tune, each gated + image-checked. **Precondition:** close the ledger's B1/B3/B2 + doc drift before S3.

## 9. Risks & mitigations
- **Warped-noise structure still looks like noise, not real geography** → proven in OFFLINE renders S1-S2
  before runtime; if it fails, refine the toolkit (more structure machinery) or the metrics, cheap; owner
  eye is the judge (render-first).
- **Parity break** → it's value_noise math (parity-proven primitive); re-baseline the gate; GLSL mirrors Rust.
- **Perf regression** → expected cheaper (atlas gone); hardened GPU-time gate (post-B3-fix) measures.
- **Repetition not actually killed** → the non-repetition auto-correlation gate + render images guard it.
- **Biome transitions seam** → params blend via the existing continuous family_weights (smooth by
  construction); seam gate at borders.
- **Distilled metrics don't capture biome identity** → render each distilled biome (S2), refine by eye;
  schema extensible.

## 10. Definition of done
- Non-repetition gate green (no grid/chunks); contiguity/seam/determinism/bounded green; CPU/GPU parity
  re-baselined green; facts-parity green; hardened GPU-time within budget; fast/cargo green.
- Owner fly: "contiguous structured explorable landmass, Google-Maps-ish, distinct seamless biomes, no
  chunks/squares/lines/repetition."
- Kernel atlas removed; pack stores biome_params; scale dialed toward the 1-10m adaptable target.
- DESIGN/ROADMAP/STATUS + HANDOFF updated; spec committed (this file).
