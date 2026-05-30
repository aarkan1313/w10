# WorldGen10 Structure Audit Extract

Source files reviewed:
- `STRUCTURE_RESEARCH_RAW.md`
- `STRUCTURE_DECISION_MEMO.md`

Current-code spot checks:
- `wg-10/rust/src/height.rs`
- `tools/dem_pack/worldgen_proto.py`
- `tools/dem_pack/biome_distill.py`
- `wg-10/worldgen_terrain/shaders/height_page.glsl`
- `wg-10/worldgen_terrain/shaders/ring_displace.gdshader`
- `wg-10/rust/src/terrain_view.rs`

## Bottom Line

There is good material here, but the memo should not be treated as a final spec. The best extraction is:

1. Do a render-first prototype upgrade in `worldgen_proto.py`: multifractal weighting, stronger recursive warp, and ridge-coupled valleys.
2. Replace the dead/off-target biome metrics with cheap geomorphometric metrics that actually vary: hypsometry, slope moments, curvature signs, VRM, and windowed relief ratio.
3. Treat gradient-based erosion as a second-stage experiment, not a free edit. It requires analytic gradient noise and a Rust/GLSL parity gate.
4. Keep "true connected drainage" separate from the local generator path. The only credible route is a world-anchored coarse drainage field, and that is a new subsystem, not a reuse of the current clipmap.

The practical split:
- Short-term: make the local height function stop reading as uniform noise.
- Medium-term: add drainage-shaped branching via slope-aligned filters or Worley/cellular graph structure.
- Long-term: if real connected rivers matter, build a deterministic coarse flow field.

## Confirmed Code Reality

The memo is right that there are two terrain stories in the tree:

- `tools/dem_pack/worldgen_proto.py` is the warped-noise prototype: single domain warp, macro fBm, ridged ridges, and independent ridged valley subtraction.
- `wg-10/rust/src/height.rs` is still a tiled-kernel runtime path. It uses `sample_kernel`, `rem_euclid`, and repeats each family kernel by footprint. The file itself notes visible footprint creases are expected for naive tiling.

That means "just edit the octave loop" only applies to the Python prototype today. Anything accepted from the prototype still needs a Rust/GLSL port and parity gate.

Also confirmed:
- `biome_distill.py` intentionally ignores `dominant_wavelength_m` and `ridge_linearity`.
- `anisotropy` is still live: it drives `warp_amount`.
- `height_page.glsl` / `ring_displace.gdshader` currently use cubic smoothstep-style noise, while the Python prototype uses quintic fade. Analytic-gradient work requires unifying this.
- Current clipmap levels do not carry structural information from coarse to fine; coarse height is used for LOD morph/blending, not drainage.

## Good Stuff To Keep

### 1. Multifractal weighting

Adopt this first in the prototype. The strongest low-risk idea is Musgrave-style hybrid/ridged multifractal carry:

```text
weight *= signal
```

This is not real drainage, but it directly attacks the "same roughness everywhere" tell. It is scalar, local, cheap, and parity-friendly.

Use it to make:
- valley floors smoother,
- peaks and ridges rougher,
- biome roughness less uniform.

### 2. Recursive warp, but stop using dead-ish anisotropy as the only driver

The raw research makes a good point: current warp is likely in the "jitter" range, not the "structural bending" range. The prototype computes warp from:

```text
0.35 * anisotropy * 8000m
```

If anisotropy clusters near 0.30, that is about 840m against an 8km macro wavelength, roughly 0.1x wavelength. That is too weak to create large-scale flow/fold coherence.

Action:
- test 0.3x to 1.0x macro wavelength,
- use two-level recursive warp,
- do not trust `anisotropy` until its per-family variance is rechecked.

### 3. Couple valleys to ridges/uplift

The current prototype subtracts an independent ridged field:

```text
h -= valley_depth * ridged_fbm(...)
```

That stacks two unrelated noise phases. The memo is right that this is a major "not a landscape" cue.

Action:
- carve between crests, not from an unrelated field,
- gate valleys by low ridge signal and/or uplift context,
- use the same warped domain and macro organization so ridges and valleys are phase-related.

### 4. Replace the metric set, but be honest about what metrics can do

Keep the cheap offline metric upgrade:

- hypsometric integral plus curve moments,
- slope distribution moments,
- curvature-sign distribution,
- VRM / roughness-at-constant-slope,
- windowed local relief ratio, e.g. 2km vs 10km.

These can make biomes differ in a defensible way. They do not create drainage topology. Defer flow-routed metrics like drainage density and slope-area exponent until there is a network/drainage primitive for them to tune.

### 5. Splines-of-noise are a good param schema candidate

Minecraft-style splines are attractive because they are cheap and parity-clean. They give regime separation: shelf, coast, plateau, cliff, mountain, lowland, etc. That fixes macro monotony better than more scalar knobs.

But the raw adversarial notes are right: "per-biome spline curves will vary" is still a hypothesis. Verify the curve fits on the actual 12 families before making splines the new schema.

### 6. Runevision-style slope-aligned gully filter is the best local "branching-shaped" experiment

This is the most interesting thing in the raw research for beating ridged valleys while staying local. But integrate it correctly:

- it is a filter over an existing height field, not a standalone generator,
- it needs base height plus analytic gradient,
- octave order is sequential and parity-sensitive,
- it produces drainage-shaped branching, not globally connected hydrology.

Correct dependency order:

1. build analytic value+gradient noise,
2. parity-test Rust/GLSL/Python gradient channels,
3. apply the slope-aligned gully filter in the prototype,
4. only then consider a runtime port.

### 7. Worley/cellular flow edges are worth a small prototype

Worley/Voronoi edges are not true rivers, but they are connected local graph structure, which is more than ridged fBm gives. They are local, fixed-loop, and parity-friendly.

Best use:
- mountain belts / plate boundaries,
- karst, volcanic, badlands,
- cheap "watershed-like" partitioning.

Ceiling:
- Voronoi topology has loops/cells and 120-degree junctions,
- it is not downhill-monotone,
- it will not produce Hortonian river hierarchy.

Still worth prototyping because it adds actual connected structure under the local/parity constraints.

### 8. Coarse drainage field is the only credible true-drainage path

The raw research is consistent on this: true drainage depends on upstream area, so it is global. A pure local `f(x,z)` cannot know basin area.

The only real path is:

```text
world-anchored coarse flow/discharge field -> fine local incision/detail
```

This is not currently implemented. The current clipmap is not a structural hierarchy. A real version needs:

- fixed seed/world-anchored flow windows,
- deterministic flow routing and stitching,
- stored or reproducible discharge/distance-to-channel,
- fine-page sampling,
- a CPU mirror or explicit facts/collision story.

Treat this as a later milestone and de-risk it offline first.

## Traps To Avoid

- Do not promise real hydrological connectivity from local noise, derivative damping, or slope-aligned filters.
- Do not spec thermal/talus relaxation as "just local." Fixed-iteration relaxation is a radius-N page stencil with apron/seam and CPU mirror requirements.
- Do not put learned CNN/diffusion terrain on the parity-critical runtime path. It is bake/research-tier only unless the project accepts a separate collision/facts approximation.
- Do not treat baked drainage curves as automatically infinite-safe. Tiling river curves can repeat at trunk scale, which is the same failure mode as tiled DEM kernels at a larger scale.
- Do not pursue partial 3D density as a near-term slice. It is real structure, but it is a terrain/render/collision architecture change.

## Recommended First Slice

Prototype only, render-first:

1. Add hybrid/ridged multifractal weighting to `worldgen_proto.py`.
2. Increase warp into the 0.3x-1.0x wavelength range and add a second recursive warp level.
3. Couple valley carving to ridge/uplift phase instead of subtracting an independent ridged field.
4. Render A/B comparisons:
   - current prototype,
   - new prototype,
   - real DEM hillshade,
   - at least mountain, badlands, and glacial.
5. Owner eye-test before Rust/GLSL work.

Parallel audit task:

1. Measure actual per-family variance of current `anisotropy`.
2. If it is dead/clustered, remove it as the primary warp driver.
3. Add cheap replacement metrics: HI, slope moments, curvature stats, VRM, and windowed relief ratio.

Second slice, only if first slice improves the read:

1. Build value+gradient noise in Python/Rust/GLSL with matching fade and rotation constants.
2. Add a gradient parity gate.
3. Prototype Runevision-style slope-aligned gully filtering.
4. Optionally prototype Worley/cellular flow-edge carving as a separate branch.

## Decision Summary

Use the memo's Path A, but amend it with the raw audit corrections:

- "multifractal + recursive warp + coupled valleys" is a good first prototype;
- derivative erosion is not free and needs gradient parity;
- spline-of-noise is promising but must be empirically verified;
- Worley/cellular edges deserve a small phase/connectivity prototype;
- real drainage means a world-anchored coarse flow field, not local noise and not the current clipmap.

