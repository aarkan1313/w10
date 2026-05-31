# WorldGen10 - Slice 2A Geography Engine Design Spec

**Date:** 2026-05-31
**Milestone:** Worldgen core rebuild, Phase 5 / Slice 2A.
**Status:** owner-approved direction, prototype / owner review pending.
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

**Current correction after v5 review:** the first geography-engine implementation did not fully satisfy this
principle. It still built regime heights from the same shared global noise fields and alpha-blended them, so
it behaved like one field with spatially-varying contrast. That is a useful yellow result, but it is
plateauing. Slice 2A now pulls a **7B-lite offline proof** forward: build a coarse world-anchored skeleton
first, route flow on that skeleton, derive regimes from the skeleton, carve height from accumulated flow, and
add noise last as regime material.

### 4.1 7B-Lite Skeleton-First Prototype

The next prototype tests whether routed structure is the missing mid-scale process:

```text
coarse world grid, fixed in world coordinates
  -> uplift / ridge skeleton
  -> coarse flow routing and accumulation
  -> distance-to-crest, distance-to-channel, slope breaks, drainage density
  -> derived regimes:
       range core = high uplift + near crest
       foothill   = mid distance from crest / slope apron
       basin      = low routed elevation / far from crest
       fan        = channel exit + slope break + basin edge
       badlands   = high drainage density on erodible plateau/basin edge
  -> height:
       base uplift/fill
       causal incision from accumulated flow
       local per-regime material/detail
```

This remains offline Python only. If the 7B-lite sheet is a clear improvement, the runtime implication is a
real subsystem: fixed flow windows, stitching, storage/reproducibility, and CPU/facts/collision semantics. Do
not port by flattening the skeleton back into ad-hoc local noise.

**Skeleton v2 implementation note:** after v1 owner review landed as Yellow+ / keep, v2 stays inside this
offline proof and focuses on the 45 km read. It replaces single-neighbor D8 routing with coarse multiple-flow
accumulation, keeps primary channels and tributaries as separate routed fields, damps incision on basin/fan
floors, and lets scenarios alter process widths/weights/smoothing instead of just contrast. This is not a
runtime design yet and remains subject to owner image review.

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
- The skeleton-first proof also fails at 45 km; that means the 85% target needs a deeper terrain-process
  rethink, not another blended-noise tuning pass.

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
3. **7B-lite skeleton proof.** Build a coarse world-anchored uplift/ridge skeleton, route flow, derive
   regimes, carve channels, and render the same sheets against v5.
4. **Artifact pass.** Remove grid/raster/line artifacts from the skeleton and flow fields. If D8 routing is
   used, it must be on a coarse world grid and smoothed/vectorized before sampling by the final render grid.
5. **Metrics pass.** Add focused metrics for relief/slope/curvature/spacing and line artifacts.
6. **Owner review.** Open the sheet in Windows. Owner decides green/yellow/red.

Current prototype state: v2 has completed the artifact/process pass in offline Python and generated
`geography_skeleton_v2_*` contact/debug sheets. Owner selected `SYN rough highlands` as the current best
Skeleton v2 panel. The port gate remains closed until this keeper is turned into an explicitly accepted stack
with a parity/facts/render story. The next prototype slice is a narrow rough-highlands focus pass: render the
keeper plus process-neighbor variants in top-down and oblique scene-read views, then record the owner verdict.
That focus pass is now rendered as `geography_skeleton_rough_focus_*`. The active review harness is the Godot
generated-world switcher `rough_world_review.tscn`, which displays one larger generated 90 km world at a time
from the same camera view. It has no-shadow review lighting plus a flat-light toggle, deterministic export
contract tests, and skeleton-rough metric reports. It is a review harness only; the port gate remains closed.

## 11. Port Gate

Nothing from Slice 2A goes to Rust/GLSL until:

- owner accepts a specific offline image set,
- the accepted algorithm is documented in ROADMAP/STATUS,
- focused Python tests pass,
- and any coarse skeleton/drainage state needed by the algorithm has a parity/facts/render story.

### 11.1 Required 7B-Lite Port Story If The Keeper Uses Routed Structure

If the accepted keeper depends on the 7B-lite skeleton, the runtime port must preserve the skeleton as a
first-class deterministic field. Do not flatten it into local ad-hoc noise.

Minimum design before Rust/GLSL work:

- **World anchoring:** coarse skeleton windows are keyed by absolute world coordinates and seed, never by
  camera position or page identity.
- **Window seams:** neighboring coarse windows share enough apron/overlap that uplift, discharge,
  distance-to-channel, and derived regimes are continuous when sampled by fine pages.
- **Authoritative facts seam:** CPU/facts/collision can query the same skeleton facts used by render pages:
  uplift, discharge/accumulation proxy, channel distance, crest distance, and regime weights.
- **Python reference parity:** the accepted Python prototype becomes the reference for deterministic sample
  fixtures. Rust must match reference values at fixed world points before GPU work starts.
- **GPU sampling story:** page generation samples skeleton facts by world coordinate, then applies local
  material/detail. No render page owns or mutates global drainage state.
- **Cache/storage story:** if flow windows are too expensive to recompute per page, cache them behind a
  deterministic key. Cache misses may be async later, but the generated result must be independent of load
  order.
- **Validation gates:** determinism, cross-window seam continuity, Python-vs-Rust sample parity, CPU-vs-GPU
  parity, visible==collision parity, and the existing no-black/perf gates.

This is the minimum bar for pulling Phase 7B forward. It is intentionally a subsystem, not a shader tweak.

**Current non-visual spike:** `tools/dem_pack/geography_skeleton_windows.py` proves the first subsystem piece
offline: fixed world-anchored windows with aprons, routed accumulation inside the extended window, cropped
authoritative core facts, and bounded adjacent-window seams for uplift, routed surface, discharge,
tributaries, channel axis, and saturated distance facts. The report writer
`tools/dem_pack/analyze_geography_skeleton_windows.py` emits
`D:\tmp\wg10_geography_engine\geography_skeleton_window_seams.{csv,md}`. This is only a Python feasibility
gate; it does not start the Rust/GLSL port and does not remove the owner visual acceptance gate.

Standalone runtime design spec:
`docs/superpowers/specs/2026-05-31-worldgen-phase7b-drainage-skeleton-design.md`.
