# Mountain World-Layer Runtime Contract - 2026-06-04

Latest owner-report audit: see
`docs/plans/WG10_IMPLEMENTATION_SPEC_AUDIT_AND_VALIDATION_PLAN_2026-06-04.md`
for the current implementation/spec audit, the shared fly-camera sync fix, and
the next progression-scene validation plan.

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

The raw procedural mountain page path uses the same seed, relief family, and
source window, but it still runs the seam-safe page recipe. That recipe has
fixed affine constants, world-anchored kernels, flow-level gating, and no
pass-network or whole-field conditioning fact. It is correct for page-stable
runtime synthesis, but it is not the same accepted mountain-network world layer.
In the reviewed `MOUNTAIN/network_ref` mode, that raw path is currently bypassed
by the reference-backed bridge described below so the owner-visible mountain
network look is stable while the procedural world-layer producer is designed.

Latest bridge status: live `MOUNTAIN/network_ref` now binds the accepted
mountain payload beside the live producer as a fact/material/height reference.
The contract report can see pass-network routes, route carving, page-stable
conditioning, corridor facts, and material hints from that reference, and the
renderer can use the bound reference for both height and material page
presentation. The current renderer now treats those material hints as restrained
terrain tints instead of bright categorical overlays, improving readability
without changing the reference height/fact contract. This is still a
reference-backed visual recovery bridge, not final procedural synthesis: the report uses
`contract_kind=single_mountain_world_layer_reference_bridge`,
`height_source=bound_world_layer_reference_payload`, and
`procedural_world_layer_height=false`.

Latest ownership split: bound world-layer references now use an explicit
`BoundWorldLayerReference` wrapper. This keeps "accepted static baseline as the
active producer" separate from "accepted world-layer facts bound beside another
active producer" in the page pool state, while preserving the same loaded
`StaticHeightRuntime` payload underneath. The wrapper also caches the
display-to-source transform used by `MOUNTAIN/network_ref`, so GDScript does not
duplicate scene-scale constants.

Latest owner-view status: the fly harness now reconfigures the actual
`Wg10TerrainView` whenever the active producer is rebuilt after a preset or
relief change. The runtime snapshot records `view.config_report()`, and the
smoke gate proves the view itself, not only the producer intent, switches
between `network_ref` (`relief_scale=1.0`, `relief_ref=1700`) and raw
`close_debug` (`relief_scale=0.25`, `relief_ref=425`) before restoring the
accepted network bridge. This makes the visual reports trustworthy, but it does
not change the contract outcome: raw `close_debug` is still
`single_seam_safe_mountain_page_recipe` and still lacks pass-network,
conditioning, and material facts.

Latest review-state status: mode/preset rebuilds now also reset presentation
diagnostics to normal material review state. The gate proves morph heatmap and
cull-disabled experiments do not leak across modes 1/2/3. This makes manual
visual comparison cleaner, while preserving the contract boundary above.

Latest material-presentation status: the accepted material facts are now carried
through the runtime bridge as a renderer-facing RGBA fact page instead of a
temporary one-channel class code. Channels are R=low-pass/corridor, G=floor,
B=rock, and A=snow. The shader blends those channels as separate terrain hints.
The fact texture is intentionally lower resolution than height (`page_px / 2`)
because these are low-frequency presentation masks and the owner fly still uses
synchronous page misses.

Latest owner-review presentation status: the review scene now opens with
procedural display detail disabled, with `N` as the explicit opt-in toggle. The
clipmap page transition fade is disabled for owner review: the streamer/pin path
keeps pages resident before display, and the old parent-to-fine settle window
read as terrain lagging/popping during flight. This keeps modes 1/2/3 on the
accepted reference presentation path before any optional close-surface dressing
is judged.

Latest owner-motion status: the live clipmap display loop now uses toroidal
3x3 page slots keyed by absolute page origin. Already-visible pages keep their
mesh/material slot when the camera crosses a page boundary, so the renderer no
longer rebinds an entire visible 3x3 level at once. The new progression motion
gate first exposed the all-mode failure (`repage_frame_max=18`, `repage=72`,
zero hide/show/full), then passed after the fix (`repage_frame_max=8`,
`repage=26`, zero hide/show/full across REFERENCE, MOUNTAIN/network,
MOUNTAIN/close-debug, and WORLD preview). This addresses the renderer-side
pop class. The progression visual repage gate now also holds the render camera
fixed across L0/L1/L2 page-boundary crosses and compares terrain-mask pixels;
latest 12-pair proof reports worst mean/p95/p99 RGB delta
`0.000831/0.002614/0.020915`. This does not promote raw
`MOUNTAIN/close_debug` or full WORLD compose to accepted terrain.
The manual owner-stress gate now also fails CPU p99/max or GPU p99 above
`16.7 ms` across REFERENCE, MOUNTAIN, and WORLD with morph off/on, so a
one-frame synchronous hitch is no longer permitted just because the broader p99
path stays green.

Latest visual-acceptance status: the runtime fly presentation now uses the same
warm accepted-review sky/ambient framing as `mountain_network_chunks_review.tscn`,
and the ring shader lighting is tuned toward that old static scene rather than
the colder diagnostic palette. `review_runtime_visual` now measures terrain
color distance as well as silhouette: latest static-vs-runtime REFERENCE focus
comparison is `static_frac=0.789`, `runtime_frac=0.776`, `iou=0.984`, and
`mean_color_delta=0.076` against a `0.130` budget. This closes the previous gate
hole where the runtime bridge could match the accepted footprint but still look
wrong.

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
   source/display mapping, runtime-page sampler
   (`source_origin_for_display`, `sample_world_page`, `sample_payload_page`),
   and a runtime-cacheable world-layer tile boundary
   (`build_runtime_world_layer_tile`, `serialize_runtime_world_layer_tile`,
   `build_runtime_world_layer_payload`, `sample_world_layer_tile_page`). The
   committed exporter `tools/dem_pack/export_godot_mountain_world_layer_tiles.py`
   writes the ignored runtime artifact
   `wg-10/worldgen_terrain/generated/review/mountain_world_layer_tiles.json`.
   This is the contract the later Rust/GPU page producer should consume or
   mirror.
   Current Rust bridge seam: `StaticHeightRuntime` can now load the exported
   runtime tile payload shape directly, preserving height scale, corridor,
   pass-network, conditioning, low-pass/floor/rock/snow material facts, and the
   explicit display/source origin, span, and ratio fields.
   Current owner-fly bridge: REFERENCE, MOUNTAIN/network, and WORLD preview now
   bind that runtime tile artifact. The live `MOUNTAIN/network_ref` source
   transform is derived from the bound tile mapping at bind time, not from
   duplicated GDScript scene-scale constants. This is still a reference-backed
   bridge, not final procedural world-layer synthesis.
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
   Follow-up live-MOUNTAIN bridge: `MOUNTAIN/network_ref` now binds the accepted
   mountain world-layer payload as a separate height/fact/material reference.
   Its contract report exposes bound pass-network, carving, conditioning,
   corridor, and material-hint facts, and its page reports expose those facts
   over runtime pages. This remains a bridge only: height is reference-backed,
   not generated by a procedural world-layer producer.

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
  MOUNTAIN/network, MOUNTAIN/close, WORLD/material, and WORLD/routes. It also
  compares the REFERENCE and reference-backed MOUNTAIN/network captures at the
  reviewed frame and along a sprint-speed page-boundary path, and fails on
  drift. It now also compares runtime REFERENCE against the old static
  mountain-network focus view with both silhouette and terrain-color budgets
  (`mean_color_delta=0.076` latest).
- `review_runtime_modes` proves the current owner WORLD mode is only a bounded
  diagnostic preview: one active biome per page keeps streaming within budget
  (latest WORLD `cpu_p99=9.952 ms`, `cpu_max=13.508 ms`, render p99 `0.749 ms`).
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
  `accepted_static_reference_visual_baseline`, classifies reviewed
  `MOUNTAIN/network_ref` as
  `single_mountain_world_layer_reference_bridge`, and does not let any current
  mode claim `satisfies_mountain_world_layer_contract=true`.
- `review_runtime` now also proves live `MOUNTAIN/network_ref` binds the
  accepted mountain world-layer reference facts: bound source scope
  `coherent_full_field_carved_with_pass_network_sliced_for_review`, nonzero
  pass-network route/carve facts, page-stable conditioning facts, corridor
  facts, material-hint facts, and nonzero static material page tiles. It also
  proves the recovered height bridge is explicit:
  `height_consumes_world_layer_facts=true`,
  `height_source=bound_world_layer_reference_payload`,
  `procedural_world_layer_height=false`, and
  `satisfies_mountain_world_layer_contract=false`.
- `review_runtime` now compares the default `REFERENCE` center-page fact report
  against the live `MOUNTAIN/network_ref` bound center-page report. The guard
  checks level, origin, world span, sample count, corridor coverage, and
  low/floor/rock/snow material hint means, so the bridge cannot drift to a
  different page-fact sample while still passing only screenshot-level checks.
- Latest bridge proof after the no-settle owner-fly fix: `cargo fmt -p
  wg10_terrain -- --check` passes, `cargo test -p wg10_terrain --lib` = 231/0,
  `tools\build_rust.ps1` builds, `fast` = 8/8, `m3` = 10/10,
  `review_runtime` = 2/2, `review_runtime_visual` = 2/2,
  `review_runtime_modes` = 2/2, and `review_runtime_stress` = 1/1. The latest
  mode gate reports zero hide/show/full events in REFERENCE, MOUNTAIN, and
  WORLD; scripted motion CPU p99/max is REFERENCE 10.046/10.116 ms, MOUNTAIN
  9.921/10.546 ms, and WORLD 9.952/13.508 ms, with `acquired_max=1` and zero
  full events in all three. Latest render p99 is REFERENCE 0.748 ms, MOUNTAIN
  0.748 ms, and WORLD 0.749 ms. The latest visual capture shows
  MOUNTAIN/network and WORLD preview matching the REFERENCE view at the reviewed
  frame and along the sprint path.
- The renderer page transition fade is disabled for owner review. This targets
  owner-visible REPAGE lag/settle without changing page data, reference facts,
  or WORLD composition.
- Latest bridge-drift proof in `review_runtime_visual`: 57,600 sampled pixels at
  stride 4, mean RGB delta `0.000000`, p95 RGB delta `0.000000`, budgets
  `0.002500` / `0.020000`.
- Latest path bridge proof in `review_runtime_visual`: REFERENCE and
  MOUNTAIN/network were compared along an 8000 m/s page-boundary path at frames
  80/160/240. Mean/p95 RGB deltas are `0.000000/0.000000` for all three
  sampled frames.
- `python -m pytest tools\dem_pack\test_mountain_world_layer_contract.py -q -s`
  proves the tracked world-layer builder contract. With the generated review
  payload present, it also records the current seam-safe live-producer gap:
  mean absolute normalized delta `1.211743`, p95 `2.276974`, and correlation
  `-0.048456` over the same mapped page.
- The same pytest now proves the accepted runtime-page sampler owns the
  source/display mapping used by the live preset (`display 0,0 -> source
  207000,176000`) and samples height/floor/rock fields from the accepted
  world-layer payload without test-local duplicate sampling code.
- The same pytest now proves the runtime-cacheable world-layer tile payload is
  JSON-ready and preserves the accepted page contract after a JSON round trip:
  tile sampling matches stitched world-layer page sampling for height, corridor,
  and all material hint fields to `1.0e-12`, while exposing pass-network,
  conditioning, material-hint, and source/display mapping facts. Current focused
  proof: `8 passed`.
- The same pytest contract now proves the accepted builder emits non-vacuous,
  bounded material hint fields (`low_pass_hint`, `floor_hint`, `rock_hint`,
  `snow_hint`) on chunks and aprons, with the stitched low-pass/floor hints
  covering the accepted corridor mask.
- `cargo test -p wg10_terrain --lib page_pool::static_reference::payload -- --nocapture`
  now proves the Rust static-reference loader accepts the exported runtime tile
  schema and preserves height, corridor, pass-network, conditioning, material
  facts, and source/display mapping fields; focused result: 8 passed / 0
  failed.
- `review_runtime` now gates the real runtime-tile source/display mapping for
  REFERENCE and `MOUNTAIN/network_ref`: display origin `-38400,-38400`, display
  span `76800`, source origin `72000,41000`, source span `270000`, and
  `source_scene_ratio=3.515625`.
- `fast` now proves the producer helper keeps its source transform identity
  before runtime binding, while `review_runtime` proves the bound pool transform
  is derived from the accepted runtime tile mapping (`scale=3.515625`, offsets
  `207000,176000`). The fly harness no longer owns those scene-scale constants.
- `cargo test -p wg10_terrain --lib` now passes 233 / 0 after the loader split.
- `review_runtime` = 2/2, `review_runtime_modes` = 2/2,
  `review_runtime_visual` = 2/2, and `review_runtime_stress` = 1/1 after
  rebinding the owner fly to `mountain_world_layer_tiles.json`. The latest
  visual gate proves REFERENCE vs MOUNTAIN/network and REFERENCE vs WORLD
  preview remain mean/p95 RGB delta `0.000000/0.000000`, and the old static
  chunks scene vs runtime REFERENCE comparison passes at mask IoU `0.986`.
- The current `review_runtime_stress` budget is strict: CPU p99/max and GPU p99
  must stay at or below `16.7 ms` in all six REFERENCE/MOUNTAIN/WORLD morph
  off/on manual-stress cases, with zero hide/show/full events.
- Current live `MOUNTAIN/network_ref` does not yet satisfy this contract because
  pass-network and page-stable conditioning facts do not exist in the live
  producer. The contract report now makes that explicit by requiring its
  `blocking_gap` to name the missing pass-network work.
- `review_runtime` now also gates the real view configuration after owner
  preset changes. It proves the raw `MOUNTAIN/close_debug` candidate is using
  the intended close/debug relief view and source transform, then proves the
  accepted `MOUNTAIN/network_ref` bridge is restored to the reference view.
- `review_runtime` now also gates review-state reset: after deliberately
  enabling the morph heatmap and disabling culling, a mode switch restores
  normal material mode, culling, detail-off baseline, and default morph state.
- `m3`, `review_runtime_visual`, and `review_runtime_modes` now prove the RGBA
  accepted-material fact presentation remains render-safe, stays within the
  owner-fly frame budget, and keeps the accepted reference bridge comparisons
  within visual budget.
