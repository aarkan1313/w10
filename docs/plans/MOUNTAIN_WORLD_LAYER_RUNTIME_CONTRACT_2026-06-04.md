# Mountain World-Layer Runtime Contract - 2026-06-04

Purpose: turn the accepted `mountain_network_chunks_review.tscn` visual into a
live-runtime producer target without confusing it with the current seam-safe
single-page mountain recipe.

## Current Truth

The accepted baseline is not the live page recipe with different constants. It
is a separate world-layer artifact:

1. Build one coherent 270 km mountain field with the old full-field diagnostic
   branch (`apron_px=0`).
2. Route a sparse connected pass network across the field.
3. Carve ramps into the raw field.
4. Apply whole-field percentile/tanh conditioning.
5. Slice the conditioned field into the 9x9 review payload.

The tracked construction source for this contract is now
`tools/dem_pack/mountain_world_layer.py`. The JSON exporter is deliberately a
thin writer around that module, so tests and future Rust/GPU ports can depend
on the world-layer contract instead of scraping an exporter implementation.

The live `MOUNTAIN/network_ref` path now uses the same seed, relief family, and
source window, but it still runs the seam-safe page recipe. That recipe has
fixed affine constants, world-anchored kernels, flow-level gating, and no
pass-network or whole-field conditioning fact. It is correct for page-stable
runtime synthesis, but it is not the same accepted mountain-network world layer.

## Contract

A runtime mountain world layer must provide these facts independently of any
single rendered page:

| Fact | Owner | Required property |
|---|---|---|
| Source/display mapping | producer config | One explicit mapping from display metres to source metres; no hidden scene-scale constants. |
| Mountain macro field | world-layer producer | Deterministic by seed and source coordinate; stable across page and LOD boundaries. |
| Pass-network routes | route/fact layer | Sparse connected routes through a large mountain window, not per-page disconnected valleys. |
| Route carving | mountain layer | Carve is applied before final conditioning and is stable where pages overlap. |
| Conditioning | mountain layer | Page-stable normalization/shape contract; no per-page zscore/norm drift. |
| Material/dressing hints | render material layer | Low/pass corridor, snow/high, rock/slope, and floor hints exposed separately from height. |
| Facts/collision story | follow-up facts layer | Either explicitly static/reference-backed or generated from the same world-layer facts. |

## Non-Goals

- Do not tune global relief/view scale to fake the accepted network look.
- Do not add per-page `zscore` / `norm01` to the live clipmap path; that recreates
  the old seam/LOD drift problem.
- Do not treat static `REFERENCE` as final live synthesis. It is the accepted
  visual baseline and renderer bridge until the live world-layer producer exists.
- Do not call WORLD composition accepted just because its route tint is visible.
- Do not move multi-biome WORLD height composition into the owner fly stream as a
  synchronous "visual fix". Top-2/full compose currently causes ~1.9 s page-build
  hitches in `review_runtime_modes`; it needs backgrounding, caching, or a cheaper
  preview contract before it can be owner-review default.

## Implementation Path

1. **Lock the taxonomy.**
   `review_runtime` must prove all four owner-scene architectures reconfigure:
   `MOUNTAIN`, `REFERENCE`, `WORLD`, and `LEGACY`.

2. **Add a mountain-layer fixture/probe.**
   Build a small numeric artifact that compares:
   - accepted conditioned network payload samples,
   - live seam-safe mountain samples over the same source/display mapping,
   - and, later, candidate mountain world-layer output.
   The first version is allowed to prove "these are different"; the promotion
   version must prove the candidate closes the gap.
   Current probe: `tools/dem_pack/test_mountain_world_layer_contract.py`.
   It proves the tracked `mountain_world_layer.py` builder contract. When the
   ignored generated review payload is present locally, it also samples the
   accepted network payload and the live seam-safe page over the same mapped
   display/source window. Current measured gap:
   `mean_abs=1.211743`, `p95_abs=2.276974`, `peak_abs=3.200543`,
   `corr=-0.048456`, `ref_ptp=1.584039`, `live_ptp=4.914207`.

3. **Choose the live world-layer shape.**
   Chosen first implementation seam:
   - CPU-authored/generated route and conditioning facts cached per large world
     tile, sampled into runtime page coordinates.
   `tools/dem_pack/mountain_world_layer.py` now owns the accepted
   source/display mapping and runtime-page sampler
   (`source_origin_for_display`, `sample_world_page`, `sample_payload_page`).
   This is the contract the later Rust/GPU page producer should consume or
   mirror.
   Remaining viable porting options:
   - GPU/CPU hybrid where the page producer consumes precomputed route/conditioning
     facts but still emits the page texture on the RenderingDevice.
   - Static payload only as a temporary accepted baseline, not as procedural final.

4. **Thread facts into rendering.**
   Height alone is not enough. The accepted scene reads better because corridors,
   floors, slopes, and snow/rock zones are visible. Add stable material hint
   channels or a documented temporary equivalent.
   First runtime-facing step: `Wg10PagePool.static_reference_report()` now
   exposes the accepted payload's generator version, source scope, height scale,
   feature span, corridor presence/fraction, and pass-network route summary.
   `Wg10PagePool.static_reference_page_report(...)` samples corridor coverage
   over a runtime page, and the REFERENCE renderer uses that page-level fact for
   a restrained corridor tint/material mix. The accepted Python world-layer
   builder now also emits page-stable material hint arrays per chunk:
   `low_pass_hint`, `floor_hint`, `rock_hint`, and `snow_hint`, derived over the
   coherent conditioned field before slicing. These are contract fields for the
   runtime port; final per-pixel materials remain follow-up work. The Rust
   static-reference loader now validates those four arrays as a complete
   all-or-none payload contract, exposes whole-payload and page-sampled hint
   coverage, and lets REFERENCE rendering pick material color/mix from those
   page-level hints.
   The same static-reference report now exposes the accepted full-field
   conditioning stats (`source_ptp`, `p05/p50/p95`, and conditioned range) so
   future live producers can be compared against the normalization/shape
   contract instead of only against screenshots.
   Follow-up runtime-facing step: `Wg10PagePool.mountain_world_layer_contract_report()`
   now exposes a compact contract/fact summary for every active producer. It
   distinguishes the accepted static-reference visual baseline from the explicit
   live `MOUNTAIN` candidate and from WORLD/LEGACY, and it keeps the current
   blocking gaps machine-visible instead of burying them in screenshots.

5. **Gate in layers.**
   Required gates before owner acceptance:
   - static reference still renders accepted payload,
   - live mode taxonomy reconfigures cleanly,
   - candidate mountain layer has bounded seam/LOD deltas,
   - candidate layer moves numeric/visual metrics toward REFERENCE,
   - owner fly of `mountain_fly_review.tscn` accepts the result.
   The review scene now opens in `REFERENCE` so the first viewport is the
   accepted mountain-network baseline. `MOUNTAIN` remains the explicit live
   candidate mode for checking whether the procedural producer has closed this
   contract gap.

## Open Decisions

- Where should pass-network facts live: generated payload, Rust world-tile cache,
  or a future terrain-fact database?
- What is the smallest conditioning contract that preserves the accepted look
  without per-page normalization?
- Resolved for the current review loop: `REFERENCE` is the first manual-review
  mode until the live world layer closes the visual gap; `MOUNTAIN` remains
  available as the explicit live candidate.
- Which material hints are required for first acceptance versus later dressing?

## Current Proofs

- `review_runtime` proves the owner scene starts in accepted `REFERENCE`, can
  jump to live `MOUNTAIN`, `WORLD`, `LEGACY`, and back to `REFERENCE`, and still
  passes the sprint-speed zero-hide churn gate.
- `review_runtime_visual` writes separate visual evidence for REFERENCE,
  MOUNTAIN/network, MOUNTAIN/close, WORLD/material, and WORLD/routes.
- `review_runtime_modes` proves the current owner WORLD mode is only a bounded
  diagnostic preview: one active biome per page keeps streaming within budget
  (latest WORLD `cpu_p99=7.686 ms`, `cpu_max=10.050 ms`, render p99 `0.505 ms`).
  A direct top-2/full WORLD height-compose attempt failed with ~1900-1950 ms
  update hitches, so full WORLD composition remains a background/cache task.
- `REFERENCE` proves the renderer can display the accepted mountain-network
  geometry when fed the accepted payload.
- `review_runtime` now also proves the REFERENCE bridge loaded the accepted
  mountain-world facts, not just anonymous height data: source scope
  `coherent_full_field_carved_with_pass_network_sliced_for_review`, pass-network
  generator, nonzero routes, nonzero carved fraction, and corridor facts.
- `review_runtime` now also proves a page-level REFERENCE corridor report exists
  for the runtime renderer (`samples_px=17`, `has_corridor=true`), and the
  renderer consumes that page report for static-reference corridor tinting.
- `review_runtime` now proves the REFERENCE bridge loaded material hint facts
  too (`has_material_hints=true`, nonzero floor/rock coverage, and page-level
  floor/rock means), and the renderer consumes those page-level hints before
  falling back to corridor-only tinting.
- `review_runtime` now proves the REFERENCE bridge loaded whole-field
  conditioning facts too (`has_conditioning_stats=true`, positive source and
  conditioned spans, and ordered `p05/p95` percentiles).
- `review_runtime` now also proves `mountain_world_layer_contract_report()`
  classifies startup `REFERENCE` as
  `accepted_static_reference_visual_baseline`, classifies live `MOUNTAIN` as
  `single_seam_safe_mountain_page_recipe`, and does not let any current mode
  claim `satisfies_mountain_world_layer_contract=true`.
- `python -m pytest tools\dem_pack\test_mountain_world_layer_contract.py -q -s`
  proves the tracked world-layer builder contract. With the generated review
  payload present, it also records the current seam-safe live-producer gap:
  mean absolute normalized delta `1.211743`, p95 `2.276974`, and correlation
  `-0.048456` over the same mapped page.
- The same pytest now proves the accepted runtime-page sampler owns the
  source/display mapping used by the live preset (`display 0,0 -> source
  207000,176000`) and samples height/floor/rock fields from the accepted
  world-layer payload without test-local duplicate sampling code.
- The same pytest contract now proves the accepted builder emits non-vacuous,
  bounded material hint fields (`low_pass_hint`, `floor_hint`, `rock_hint`,
  `snow_hint`) on chunks and aprons, with the stitched low-pass/floor hints
  covering the accepted corridor mask.
- Current live `MOUNTAIN/network_ref` does not yet satisfy this contract because
  pass-network and page-stable conditioning facts do not exist in the live
  producer. The contract report now makes that explicit by requiring its
  `blocking_gap` to name the missing pass-network work.
