# WorldGen10 - Slice 2A Geography Engine Design Spec

**Date:** 2026-05-31
**Milestone:** Worldgen core rebuild, Phase 5 / Slice 2A.
**Status:** owner-approved direction, pre-plan / prototype.
**Parent:** `docs/plans/ROADMAP.md` Phase 5, `STRUCTURE_AUDIT_EXTRACT.md`, and
`docs/superpowers/specs/2026-05-30-worldgen-core-design.md`.

---

## 1. Purpose

The current prototype family can make terrain texture, but it does not reliably make geography. Batch 1
structure variants looked basically the same. The broad matrix found a least-bad basin/range/flow/fine-detail
cell, but it was still not good enough. The landform-regime probe made the right conceptual move, but visible
line/scaffold artifacts are unacceptable.

Slice 2A is therefore not another noise-combo search. It is an offline geography-engine prototype whose job is
to prove, in rendered contact sheets, that WG10 can plausibly reach an 85%-class terrain read before any
Rust/GLSL port.

The 85% target means: at normal game and fly-camera distances, terrain reads as plausible real geography with
coherent basins, ranges, ridges, drainage-shaped corridors, and local variation that follows landform history.
It does not mean expert-grade DEM indistinguishability under GIS inspection.

## 2. Non-Goals

- No Rust, GLSL, Godot, facts, or render-pipeline changes in this slice.
- No runtime port until owner accepts an offline image set.
- No claim of true globally connected hydrology from local noise.
- No acceptance based on metrics alone. Metrics guide tuning; owner image review decides the look.
- No "least bad" continuation. If the best sheet still reads as procedural scaffolding or nice noise, stop and
  redesign.

## 3. Inputs And References

Use the current offline Python tooling under `tools/dem_pack/`.

Use WG9/real DEM kernels as side-by-side references, read-only:

```text
D:\workflows\worldgen9\factory\kernels\<kernel_id>\normalized_height.npy
D:\workflows\worldgen9\factory\kernels\<kernel_id>\kernel.json
```

Reference rows must include at least:

- mountain range terrain,
- basin/range or foothill terrain,
- badlands/incised terrain,
- one smoother family such as grassland/plain/karst/plateau.

The existing generated reference sheet at `D:\tmp\wg10_reference_kernels.png` is useful for review, but the
prototype renderer should be able to build fresh reference rows without depending on that scratch file.

## 4. Core Design

The generator is a composition of landform regimes over an irregular structural frame.

```text
world position
  -> coarse structural frame
       -> uplift/range skeleton
       -> basin/plateau/lowland masks
       -> drainage corridors over the coarse field where feasible
  -> regime assignment and soft weights
       basin floor, alluvial fan, foothill, range core, plateau,
       badlands/incised, plain/grassland, optional glacial/karst
  -> per-regime shaping
       elevation envelope, roughness, ridge style, incision, fill, apron, detail
  -> artifact suppression
       remove straight segments/cell edges from final height, soften regime joins
  -> final height
```

The important shift is that noise becomes material inside regimes, not the organizing principle. A basin is
not average mountain plus average plain. A range core, foothill, fan, and basin floor have different process
rules and blend spatially.

## 5. Structural Frame Requirements

The coarse frame must create recognizable geography:

- Ranges have irregular orientation, curvature, width changes, and discontinuities.
- Basins are lower, broader, smoother, and receive fill.
- Foothills transition between range cores and basins.
- Drainage corridors are shaped by the coarse frame and should prefer downhill/coherent paths where possible.
- Local detail follows the coarse frame. It must not be independent sandpaper.

Visible straight procedural lines, Voronoi cell edges, obvious masks, repeated stamps, or ruler-like fault
segments are hard failures.

## 6. Regime Requirements

Each regime must have a different job:

- **Range core:** high relief, asymmetric ridge texture, local roughness tied to uplift/ridge masks.
- **Foothill:** lower relief, smoother ridge remnants, transition apron from range to basin.
- **Alluvial fan:** fan/apron shapes at range exits, smoother than range, not flat noise.
- **Basin floor:** low relief, broad fill, subtle channels or dry washes, no uniform blank plane.
- **Plateau:** elevated broad surface with edge incision and sparse internal roughness.
- **Badlands/incised:** dense fine drainage texture tied to a valley/corridor field.
- **Plain/grassland:** low relief, broad undulation, sparse drainage, no mountain-like ridges.
- **Optional glacial/karst:** only if a cheap offline variant helps; not required for first acceptance.

Regime weights must be soft enough to avoid pasted regions, but not so averaged that every location becomes
the same median terrain.

## 7. Contact Sheet Requirements

Every review sheet must include real DEM references beside synth output.

Minimum views:

- 200 km regional view,
- 40 km landform view,
- close crop for near detail.

Minimum output:

- hillshade sheet for human review,
- optional debug sheet showing regime weights, uplift/range mask, drainage/corridor mask, and artifact checks.

Do not show only debug masks as proof. The final hillshade is the acceptance image.

## 8. Acceptance Signals

**Green / continue toward port:**

- At least one generated patch reads to the owner as real geography, not nice noise.
- Basin/range/valley/ridge logic is recognizable without explaining the algorithm.
- No visible straight scaffolds, cell borders, chunks, or repeated stamps.
- The same candidate works at 200 km, 40 km, and close crop.
- Real DEM references are close enough that the gap feels like fidelity/tuning, not wrong algorithm class.

**Yellow / iterate offline:**

- One scale works but another breaks.
- Ridges exist but drainage still reads decorative.
- Regimes are distinct but joins feel pasted.
- Detail improves the read but is not yet causally tied to the coarse structure.

**Red / realign before port:**

- Generated outputs all look basically the same.
- The best result is only "least bad."
- Straight lines, mask edges, cell edges, or repeated motifs remain visible.
- The explanation depends on "with tuning this might work."
- A coarse routed drainage/skeleton field appears necessary. In that case, pull ROADMAP Phase 7B forward
  before Rust/GLSL work.

## 9. Objective Metrics

Metrics are secondary checks after visual review. Compute where cheap:

- local relief ratio at multiple windows,
- slope distribution moments,
- curvature-sign balance,
- ridge/valley spacing,
- patch/regime area proportions,
- channel/corridor spacing when a network exists,
- line-artifact score for unnaturally straight long features,
- non-repetition/autocorrelation checks at old page/kernel scales.

Metrics should be reported in text beside the generated sheets when useful, but they do not override owner
rejection.

## 10. Prototype Slices

1. **Spec and renderer cleanup.** Make this spec, keep ROADMAP/STATUS aligned, and keep old matrix/regime
   experiments as local evidence until intentionally committed or discarded.
2. **Geography v0.** Build a clean offline generator path with regime maps, irregular range/basin skeleton,
   coarse drainage/corridors, per-regime shaping, and DEM-reference contact sheets.
3. **Artifact pass.** Add a debug sheet and remove visible straight-line/cell/mask artifacts. If this cannot
   be done locally, stop and design the coarse drainage/skeleton data model.
4. **Metrics pass.** Add focused metrics for relief/slope/curvature/spacing and line artifacts.
5. **Owner review.** Open the sheet in Windows. Owner decides green/yellow/red.

## 11. Port Gate

Nothing from Slice 2A goes to Rust/GLSL until:

- owner accepts a specific offline image set,
- the accepted algorithm is documented in ROADMAP/STATUS,
- focused Python tests pass,
- and any coarse skeleton/drainage state needed by the algorithm has a parity/facts/render story.

