# WG10 Architecture Baseline Audit - 2026-06-04

Purpose: reconcile the owner-accepted mountain review artifact with the current
Slice 4 live GPU runtime. This is a current-state audit, not a new feature spec.

## Executive Read

The project currently has multiple terrain architectures alive at once:

1. **Accepted static review architecture**
   - Scene: `wg-10/worldgen_terrain/harness/mountain_network_chunks_review.tscn`
   - Script: `wg-10/worldgen_terrain/harness/mountain_world_chunks_review.gd`
   - Data: `wg-10/worldgen_terrain/generated/review/mountain_network_chunks.json`
   - Shape: offline-generated 9x9 chunk meshes, vertex colors, baked collision, connected pass-network carving.
   - Important scale facts in the payload: `feature_span_m=90000.0`, `source_chunk_span_m=30000.0`, `chunk_span_m=8533.333...`, `height_scale_m=1700.0`.

2. **Legacy live streaming architecture**
   - Producer: `Wg10PagePool.configure(...)`
   - Shader: `height_page.glsl`
   - Data source: `packs/dem_v1` kernel atlas.
   - Renderer: `Wg10Streamer` -> `Wg10TerrainView` -> `Wg10ClipmapRings` -> `ring_displace.gdshader`.
   - Status: old runtime path, still useful as a renderer regression and A/B path, but not the intended final terrain content path.

3. **Current biome live streaming architecture**
   - Producers: `Wg10PagePool.configure_biome(...)` for the explicit
     single-mountain fly mode, `Wg10PagePool.configure_biome_world(...)`
     for the WORLD composition A/B mode.
   - Shaders: `recipe_primitives.glsl` + `biome_page.glsl` + the 11 compiled
     `biome_<name>.glsl` fragments.
   - Renderer: same clipmap renderer as legacy.
   - Current scene: `wg-10/worldgen_terrain/harness/mountain_fly_review.tscn`
   - Current behavior: MOUNTAIN mode remains available through key `2` and the
     accepted owner-review `B` cycle.
     In the accepted `network_ref` preset it is currently a reference-backed
     visual bridge, not raw procedural height. The raw seam-safe live recipe
     remains visible through the close-debug preset and as the next procedural
     world-layer target. WORLD mode remains available through direct key `3`; it
     generates a texel-corner runtime-biome weight field per page, dispatches
     each active biome context, and folds those core height fields through the
     GPU compose passes before writing the page texture.

4. **Runtime static-reference bridge**
   - Producer: `Wg10PagePool.configure_static_reference(...)`.
   - Data source: `mountain_network_chunks.json`, stitched into the accepted
     1153x1153 height field and sampled into runtime R32F page textures.
   - Renderer: same clipmap renderer as the live runtime.
   - Status: the first mode in `mountain_fly_review.tscn` and an explicit
     capture target in `biome_fly_capture.gd`. This proves the renderer can
     display the accepted mountain-network world layer, but it is not the final
     live biome producer.

5. **Reference-backed MOUNTAIN visual bridge**
   - Producer state: `configure_biome(...)` plus
     `bind_mountain_world_layer_reference(...)`.
   - Data source: the same accepted `mountain_network_chunks.json` payload used
     by REFERENCE.
   - Status: `MOUNTAIN/network_ref` reports runtime=`single` and
     biome_path=`true`, but its contract kind is
     `single_mountain_world_layer_reference_bridge` with
     `height_source=bound_world_layer_reference_payload` and
     `procedural_world_layer_height=false`. This recovers the owner-visible
     mountain-network look in the live review mode while keeping the final
     procedural producer gap explicit.

This explains the owner report:

- The old network-chunk scene looked better because it used the accepted mountain
  world artifact: broad 90 km feature structure plus explicit connected pass
  carving and review dressing.
- The new `REFERENCE` mode now streams that accepted height payload through the
  runtime page pool and clipmap renderer, so the live renderer can be compared
  against the same mountain world layer without pretending the biome recipe has
  learned the pass-network/conditioning contract.
- The current live BIOME fly is proving page producer composition and renderer
  behavior. The mountain review now starts on accepted `REFERENCE`; reviewed
  `MOUNTAIN/network_ref` matches that baseline through the reference-backed
  bridge; WORLD composition stays as an explicit diagnostic mode.
  WORLD can compose active runtime-biome weights, but materials/content/facts and
  the owner fly review are still open.
- The pop-in and morph issues have two pieces: renderer scheduling controls
  hide/show and geomorph timing, while the route diagnostics explain why the old
  whole-page selector was structurally wrong at coarse LODs. The renderer now
  separates visible display coverage from led prefetch coverage and has an
  automated zero-hide sprint gate. The live path now consumes grammar weights
  through compose; any remaining "ground looks bad" report needs a fresh owner
  fly against this composed runtime.

Latest renderer-presentation follow-up: height repage already faded from parent
to newly resident fine page, but static material pages and WORLD route tint were
still switching instantly. `ring_displace.gdshader` now multiplies those
presentation mixes by `page_fade`, so material/tint changes fade over the same
short window as height. This addresses visual pop during REPAGE without changing
producer data, page ownership, scheduling, or facts.

Latest accepted-reference bridge proof: `review_runtime_visual` now captures the
old static review scene and the runtime REFERENCE bridge under matching focus
framing, then compares terrain masks instead of colors. Latest result:
static terrain_frac `0.789`, runtime terrain_frac `0.778`, mask IoU `0.987`.
This proves the live REFERENCE bridge preserves the owner-liked mountain
footprint/framing. Remaining owner-visible quality concerns should target
runtime material/mesh presentation and final procedural world-layer content,
not another source-window or camera-scale reset.

Latest harness separation follow-up: the owner-fly runtime snapshot/report
builder has moved out of `mountain_fly_review.gd` into
`mountain_fly_snapshot.gd`. The scene still exposes the same snapshot method for
tests, but diagnostic report shape is now separate from scene input,
reconfigure, camera, and rendering assembly. This reduces the mixed-architecture
pressure in the live review harness before the next mode-specific visual/perf
fix.

Latest renderer-presentation follow-up: the live bridge keeps the same accepted
height/material fact payload, but the shader now presents static material pages
as softer hints rather than hard class colors and uses gentler manual lighting.
This narrows the visible gap between modes 1/2 and the old chunk scene without
changing producer data or promoting WORLD. The remaining WORLD artifacts are
still composition/diagnostic-path issues, not mountain-reference bridge issues.

## 2026-06-04 Deep-Dive Addendum

The latest source-window fix made the raw live mountain recipe sample the same
270 km source window that the accepted static payload came from, but that did
not make the recipe match the accepted mountain-network scene. The current
evidence says this is expected. The reviewed `MOUNTAIN/network_ref` mode now
uses the accepted payload as a reference-backed height/material/fact bridge
while the procedural world-layer producer remains open.

The accepted payload generator is:

1. `tools/dem_pack/export_godot_mountain_network_chunks.py`
2. `mountain.generate(...)` over one 270 km field with `apron_px=0`, which uses
   the old full-field diagnostic branch: window-level `zscore` / `norm01`,
   non-scale-anchored Gaussian widths, and rotation around the field midpoint.
3. `mountain_pass_network.carve_pass_network(...)`, which runs a coarse
   least-cost route network and carves ramps into the one raw field.
4. `_condition(...)`, which applies whole-field percentile normalization,
   a small Gaussian, and `tanh`.
5. Only then does the exporter slice 9x9 chunks for review.

The live `configure_biome(...)` path is intentionally different:

1. It uses the seam-safe page branch of `mountain.generate(...)` mirrored in
   Rust/GLSL.
2. It uses fixed affine constants instead of per-window `zscore` / `norm01`.
3. It anchors blur widths for cross-level invariance and gates flow by level.
4. It has no coarse route-network fact and no whole-field conditioning pass.

So the current visual mismatch is not a remaining command/configuration error.
It is a content producer contract gap. The aligned fix is not more relief-scale
tuning; it is a named mountain world-layer producer/fact design that can provide
connected pass routes and a page-stable conditioning contract to the live
runtime, or an explicit decision that the static `REFERENCE` payload remains the
temporary accepted baseline while the live seam-safe recipe is judged as a
separate prototype.

The runtime now exposes that distinction directly through
`Wg10PagePool.mountain_world_layer_contract_report()`. `review_runtime` gates
that `REFERENCE` reports the accepted static visual baseline facts, reviewed
`MOUNTAIN/network_ref` reports
`single_mountain_world_layer_reference_bridge` with reference-backed
height/material/facts, and no active mode claims full procedural
mountain-world-layer contract satisfaction yet.

The accepted Python world-layer now also owns the runtime-page sampling seam:
`source_origin_for_display`, `sample_world_page`, and `sample_payload_page`.
The contract test proves the same source mapping used by the live preset
(`display 0,0 -> source 207000,176000`) and samples accepted height/material
fields from the stitched world layer. This resolves the first Phase 3 shape
choice: start with a generated/coherent world tile plus facts sampled into
runtime pages, then port that seam into Rust/GPU instead of inventing another
page-local mountain recipe.

Current source-size check also changes the refactor framing. No tracked Rust,
GDScript, GLSL, or Python source file is over 1000 lines after the split. The
remaining large tracked files are mostly docs/history, generated fixtures, or
old review harnesses. The runtime GPU producer hotspot was
`wg-10/rust/src/biome_page_compute/runtime_context.rs`, which mixed cached
context construction, single-biome dispatch, and WORLD composition dispatch.
That file is now split by ownership:

- `runtime_context.rs` builds/frees cached GPU resources.
- `runtime_dispatch.rs` dispatches one biome page into a caller-owned texture or
  core buffer.
- `runtime_compose.rs` folds multiple biome page cores through the WORLD compose
  path.

The refactor risk is now producer ownership and mode taxonomy, not a single
giant terrain source file.

Follow-up page-pool split: WORLD-only active-limit and route/weight diagnostic
reports now live in `wg-10/rust/src/page_pool/world_reports.rs`. Generic
`state_api.rs` no longer owns WORLD preview reporting; it carries source
transform, resident page lookup, display pins, and generic pool stats. The
public Godot method names did not change, and the split is proven by Rust lib
tests plus `biome_world` and `review_runtime`.

Follow-up WORLD producer split: WORLD route and weight-field adaptation now
lives in `wg-10/rust/src/page_pool/world_producer.rs`. `producer.rs` still owns
active producer classification and dispatch, but no longer owns WORLD
page-center selection, page/probe weights, or per-texel weight-field helpers.
This keeps the diagnostic reports, pure route math, and producer dispatch as
separate seams while preserving the same Godot API and gate behavior.

Follow-up WORLD preview contract guard: `mountain_fly_review.gd` now exposes the
live pool's center-page WORLD route report and sampled per-texel weight-field
report through `debug_runtime_snapshot()`. `mountain_fly_review_smoke_check.gd`
uses that snapshot after switching to WORLD and proves the actual preview field
is still the bounded diagnostic contract: a 17x17 sample, normalized weights,
`active_biomes=1`, and `max_texel_active_count=1`. This is deliberately a guard
against accidentally presenting full multi-biome WORLD compose as accepted
owner terrain; it does not promote mode 3.

## Current Checkpoint

Branch: `slice4-gpu-page-integration`

Backup made before this audit checkpoint:

- `backup-slice4-pre-architecture-audit-20260604-e653a36`

Checkpoint commits made during this audit:

- `5ccb2cb fix(slice4): fade newly resident terrain pages`
- `840f11d docs(slice4): record architecture baseline split`

That commit only changes the shared clipmap renderer:

- `wg-10/rust/src/clipmap_rings.rs`
- `wg-10/worldgen_terrain/shaders/ring_displace.gdshader`

It adds a short visual fade when a newly resident fine page appears, starting
from the parent/coarse height and settling to the normal LOD/morph height. This
targets the forward "hidden until resident, then snap on" pop separately from
geomorph blending.

Validation:

- `powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1`: passed.
- `cargo test -p wg10_terrain --lib`: 225 passed / 0 failed.
- `python tools\gate.py --suite review_static`: 1/1 passed. This suite now runs
  windowed because the accepted static review scene builds render/collision
  resources and the headless command shape was unreliable in the local Godot
  runtime. It is scoped to `mountain_network_chunks_review.tscn`, the accepted
  baseline.
  An earlier attempt that also included `mountain_world_chunks_review_check.gd`
  failed because that generic check is stale against its current
  `biome_compose_world_v2_scale_contract` payload; do not use it as the accepted
  network-baseline proof.
- Direct static baseline capture passed with the Godot console binary:
  `worldgen_terrain/tests/mountain_network_visual_capture.gd` wrote
  `D:/tmp/wg10_biome_compose/mountain_network_static_focus_capture.png` and
  `D:/tmp/wg10_biome_compose/mountain_network_static_overview_capture.png`
  from `mountain_network_chunks_review.tscn` (`chunks=9`,
  `feature_span_m=90000`, `1280x720`). These captures preserve the owner-liked
  offline artifact for direct comparison against the live runtime. This is now
  wired as `python tools\gate.py --suite review_static_visual`.
- `python tools\gate.py --suite m3`: 10/10 passed after the lit-material,
  route-debug, WORLD route-tint, and display/prefetch scheduler changes.
  Latest rerun after the reference bridge: `m3_accept` p99 = 5.25 ms against
  the 6.0 ms budget;
  `ring_material_tint_check.gd` proves
  `biome_debug_color` and WORLD `biome_material_mix=0.34` are bound through the
  actual `Wg10ClipmapRings` material API; `m5_detail_check` remained
  non-vacuous (`diff=0.0124`), bounded, and edge-safe.
- `python tools\gate.py --suite biome_fly`: 4/4 passed. Production 576 macro
  maxd = 2.3156e-5 <= 5e-4, full 576 maxd = 0.001471 <= 0.002, cross-level
  macro ratio = 0.066665 <= 0.08, biome fly GPU p99 = 0.104 ms.
- `python tools\gate.py --suite fast`: 8/8 passed after extracting
  `mountain_fly_producers.gd` and `mountain_fly_runtime_config.gd`. The producer
  check locks the review helper's accepted REFERENCE startup, explicit
  MOUNTAIN/network candidate preset, close-debug preset, relief clamps, and
  accepted-only B-cycle order. The runtime config check
  locks the shared renderer defaults: 5 levels, 8192 m base span, 196608 m loaded
  edge, morph/detail default off, and the review sky color.
- Direct smoke launch of `mountain_fly_review.tscn`: passed after rebuilding the
  loaded DLL. The scene now starts in `REFERENCE` mode so owner review opens on
  the accepted mountain-network payload through the live page pool and renderer.
  `MOUNTAIN/network_ref` remains the explicit live candidate through `2`/`B`; it
  uses `runtime_seed=177`, `relief_m=1700`, a MOUNTAIN/network-only view relief
  scale of `1.0`, and the accepted source-window transform
  (`source_scale=3.515625`, source center `207000,176000`), matching the accepted
  mountain-network seed/relief/source family without changing the global renderer
  relief scale. `P` still exposes the old close-up debug scale
  (`feature_span_m=3500`) while in MOUNTAIN mode.
- `python tools\gate.py --suite review_runtime`: 2/2 passed. This windowed
  gate instantiates the actual `mountain_fly_review.tscn` owner scene, waits for
  startup, then verifies the accepted `REFERENCE` contract, including static
  payload source scope, pass-network facts, material hint facts, page-sampled
  corridor/material hints, view relief scale=`1.0`, identity source transform,
  and real page startup. It also switches to `MOUNTAIN/network_ref` and verifies
  runtime=`single`, biome_path=`true`, seed=`177`, relief_m=`1700`, view relief
  scale=`1.0`, source scale=`3.515625`, and source offset=`207000,176000`. It
  also
  runs `mountain_fly_visibility_churn_check.gd`, a sprint-speed motion gate over
  360 frames: `stream_events=24`, `resident=69`, `repage=72`, `hide=0`,
  `show=0`, `hidden_frames=0`, `max_hidden=0`. This specifically gates the
  GDScript/Rust call signature, accepted default scene wiring, explicit live
  candidate wiring, and forward-motion hide/show pop-in.
- `python tools\gate.py --suite biome_world`: 1/1 passed when run outside the
  filesystem sandbox. The gate configures WORLD mode, builds the 11 cached
  runtime contexts plus the compose context, acquires one composed page, reads
  back a non-degenerate texture, and prints runtime=`world`, `biome_path=true`,
  nonzero=65536, min=-1633.198242, max=842.125427.
- Follow-up `biome_world` route diagnostics now separate the pop-in mechanism:
  `route_inpage corner_mixed=0` in the sampled level-0 window, but
  `lod_route_by_parent L1=0/289 L2=63/289 L3=120/289`, and complete parent
  child scans report `parents=243 mixed=153 child_mismatch=2472/6804
  max_child_routes=6`. This means page-center routing is stable at level 1 in
  the sample but breaks structurally at coarser levels; a single parent biome is
  not a faithful low-detail representation of its children.
- The runtime compose bridge is now wired into page production:
  `page_pool/world_route.rs` generates the texel-corner per-page runtime-biome
  weight field from grammar, and `compute_biome_world_page_composed(...)` copies
  GPU recipe core buffers into a cached compose context before cropping the
  composed height to the page texture. `biome_world` reports
  `route_weight_field samples=289 active_biomes=2 max_sum_delta=0.000000` for
  the sampled live page.
- `Wg10PagePool.configure_static_reference(...)` now loads the accepted
  `mountain_network_chunks.json` payload, stitches the 9x9 chunk heights into a
  single accepted height field, and uploads sampled page textures with
  `RenderingDevice.texture_update`. Page textures now include
  `CAN_UPDATE_BIT`, and the mode reports runtime=`static_reference`.
- `biome_fly_capture.gd` now writes five visual artifacts:
  `D:/tmp/wg10_biome_compose/biome_mountain_reference_fly_capture.png`,
  `D:/tmp/wg10_biome_compose/biome_mountain_network_fly_capture.png`,
  `D:/tmp/wg10_biome_compose/biome_mountain_close_fly_capture.png`,
  `D:/tmp/wg10_biome_compose/biome_world_fly_capture.png`, and
  `D:/tmp/wg10_biome_compose/biome_world_fly_capture_routes.png`. The mountain
  reference capture proves the runtime renderer can display the accepted
  mountain-network height layer. The mountain network capture gives a separate
  visual proof for the explicit live MOUNTAIN candidate;
  the close-debug capture shows why that 3.5 km scale should remain diagnostic
  only; the route capture proves the renderer receives page route labels; the
  normal WORLD material capture now receives a restrained route-color tint, so
  per-biome regions no longer collapse to the same mountain palette. Final
  per-pixel biome materials/content remain open.
- `python tools\gate.py --suite review_runtime_visual`: 1/1 passed. This
  regenerates the five live runtime PNG artifacts above and now routes producer
  mode/preset configuration through `mountain_fly_producers.gd` and renderer
  setup through `mountain_fly_runtime_config.gd`, so the owner scene and runtime
  visual capture share producer constants, `configure_biome*` call shape,
  clipmap/view constants, morph/detail defaults, fog, and loaded edge.

Follow-up live visual rerun (historical, superseded for reviewed
`MOUNTAIN/network_ref` by
`7e0fb98 fix(slice4): recover mountain network visual bridge`):

- `ring_displace.gdshader` now colors terrain from the same displayed height used
  for `VERTEX.y`, after `relief_scale`. This fixes a presentation bug where the
  palette saw unscaled page metres while the geometry was drawn at the scaled
  height.
- The corrected final capture still does not match
  `mountain_network_chunks_review.tscn`. The current MOUNTAIN/network capture now
  has dense mountain-scale relief after the seed/relief calibration and source-window
  transform; it synthesizes from the accepted 270 km source window while rendering
  over the 76.8 km review footprint. It remains a raw live page recipe, while the
  accepted baseline is a conditioned 270 km source field with connected
  pass-network carving, sliced into the review scene.
- The new REFERENCE capture restores the accepted mountain massifs through the
  same runtime page pool and clipmap renderer. That isolates the explicit live
  MOUNTAIN candidate mismatch to the content/world-layer producer and
  material/dressing layer; the renderer can show the accepted shape when fed the
  accepted payload.
- Tried and rejected presentation-only relief changes (`RELIEF_SCALE=0.5` and
  `1.0`): they can make silhouettes stronger, but they break close-debug/WORLD
  captures by pushing cameras into terrain or producing foreground spikes. The
  remaining mountain mismatch is content/world-layer architecture, not a safe
  scalar-tuning fix.

## Why The Current Live BIOME View Does Not Match The Accepted Network Scene

### 1. It is not using the same world layer

`mountain_network_chunks_review.tscn` loads an offline payload whose source scope
is a coherent full-field mountain synthesis with connected pass-network carving.
The mesh review is not a streaming page runtime. It is a baked inspection artifact
that carries exactly the authored structure the owner liked.

`mountain_fly_review.tscn` now defaults to REFERENCE mode and calls
`configure_static_reference(...)`, so the mountain review scene starts on the
accepted mountain-network payload through the same page pool and clipmap
renderer. MOUNTAIN mode remains available through `2`/`B`; it calls
`configure_biome(...)` with the single mountain fragment as the explicit live
candidate, so live recipe review is still one keypress away without confusing it
with the accepted baseline. The separate WORLD mode remains available through
direct key `3`; it calls `configure_biome_world(...)`, asks the grammar for active
runtime-biome weights, and composes the active GPU biome recipes into the
streamed page texture.

The `B` cycle now stays on `REFERENCE` <-> `MOUNTAIN/network_ref` so the owner
comparison lane stays separate from WORLD/LEGACY diagnostics. `REFERENCE` is
intentionally named as a reference bridge: it helps separate renderer review from
live content-producer work, but it does not mean the live mountain recipe has
reproduced the accepted pass-network/conditioning process.

The accepted network exporter (`tools/dem_pack/export_godot_mountain_network_chunks.py`)
does one more thing the live page recipe does not: it carves a connected pass
network into one large raw field, then applies percentile/tanh conditioning to
that whole field before slicing it. The live page recipe is seam-safe and
scale-proven, but it does not currently carry that full-field conditioning or
pass-network fact.

### 2. The scale context changed

The accepted network payload records `feature_span_m=90000.0`.

The current live fly harness previously used:

```gdscript
const FEATURE_SPAN_M := 3500.0
```

That may make individual pages read as mountains under close fly review, but it
is not the same mountain artifact. It compresses the mountain vocabulary into a
different content scale. Treat it as a diagnostic/live-page tuning knob, not as
proof that runtime mountain scale matches the accepted static baseline.

The harness now defaults to the accepted `network_ref` scale (`feature_span_m=90000`)
and exposes the old 3.5 km behavior as the `close_debug` preset behind `P`. This
does not solve remaining visual/material/facts acceptance; it removes a
review-harness ambiguity so owner fly feedback names the actual preset being reviewed.

### 3. The current BIOME runtime is no longer all mountain, but acceptance is still open

The BIOME path now answers "can the live runtime compose active grammar-driven
biome weights from the ported GPU producers?" It does not yet answer "does the
owner accept the visual result in motion with the current materials, streaming,
and facts story?" WORLD mode is now composed instead of page-center selected.
Missing badlands-native runtime support and per-biome material/content review
remain expected limitations.

The next content architecture step is not more mountain shader tuning. It is a
fresh owner fly and then targeted work on whatever remains visible: materials,
streaming/pops, badlands support, facts/collision, or Slice 4c runtime flip.

### 4. There are two independent visual failure classes

Renderer/streaming failures:

- fine pages appear only when resident, so forward movement can show snap-in;
- geomorph can still expose cross-level mismatch when parent/child surfaces are
  not true low-pass relatives;
- page hiding/showing, culling, and repage churn are renderer concerns.
- route diagnostics still show why the old whole-page selector was incompatible
  at coarse levels; the producer now composes weight fields, and hide/show pop is
  gated at zero hides, so remaining visual artifacts need owner fly review rather
  than another selector-only diagnosis.

Content/world failures:

- WORLD mode now composes per-page runtime-biome weight fields;
- coarser WORLD pages still need owner review because route diagnostics show
  the old selector would disagree with many children;
- accepted pass-network carving is not part of the live page world layer yet;
- static review scale and live page scale are not reconciled.

These need separate fixes and separate gates.

## What Is Actually Proven

Proven or strongly supported:

- The static mountain 9x9/network review artifact exists and captures an accepted visual direction.
- The Rust biome producer path has extensive parity coverage.
- Scale-invariant producer work is implemented through `flow_max_level` and cross-level gate wiring.
- The renderer has a dedicated M3 gate family, and page-fade verification passed
  in the current editor-closed run.
- The first WORLD runtime compose gate (`biome_world`) passes and proves
  `configure_biome_world` can build recipe contexts plus the compose context,
  acquire a composed page, and write a non-degenerate texture.
- The REFERENCE runtime bridge passes visual capture and proves the same
  clipmap renderer can show the accepted mountain-network height payload.

Not proven yet:

- The current live BIOME fly matches the accepted mountain-network artifact.
- The current live BIOME fly is visually accepted as a per-pixel grammar-driven
  multi-biome composition.
- Slice 4c runtime flip and atlas removal are safe.
- Visible biome terrain and facts/collision are aligned.

## Refactor Boundary Plan

Do not start by splitting files just because they are large. The largest raw
source files are now mostly review/export or recipe-local. The destabilizing
problem is architecture overlap: producer selection, world composition, renderer
streaming, and review harnesses are being reasoned about as one problem.

### Phase 0 - Lock Baselines

Goal: make it impossible to confuse "accepted visual baseline" with "current
runtime path."

1. Add a small baseline table to `STATUS.md`:
   - accepted visual baseline: `mountain_network_chunks_review.tscn`;
   - current live runtime: `mountain_fly_review.tscn`;
   - current runtime content limitation: composed WORLD pages still need owner
     visual acceptance, per-biome materials, and facts/collision alignment;
   - current renderer evidence: `m3` 10/10 passed after page fade and WORLD
     route-tint material binding; owner re-fly is still required because gates
     do not prove the visual read.
2. Add or update a smoke check for `mountain_network_chunks_review.tscn`.
   Existing `mountain_world_chunks_review_check.gd` checks the non-network
   scene; the network scene deserves its own check because it is now the owner
   baseline.
3. Editor-closed gate results are now recorded above:
   - `python tools/gate.py --suite review_static` -> 1/1 pass;
   - `python tools/gate.py --suite review_static_visual` -> 1/1 pass;
   - `python tools/gate.py --suite m3` -> 10/10 pass;
   - `python tools/gate.py --suite biome_fly` -> 4/4 pass.

Exit: we know which view is the baseline, which path is live, and which gates
are current.

### Phase 1 - Separate Review Artifacts From Runtime Harnesses

Goal: stop review-only scenes from looking like runtime architectures.

Suggested layout:

```text
wg-10/worldgen_terrain/harness/review_static/
  mountain_network_chunks_review.tscn
  mountain_world_chunks_review.tscn
  glacial_world_chunks_review.tscn

wg-10/worldgen_terrain/harness/runtime/
  mountain_fly_review.tscn
  m3_review.tscn
```

This can be done with compatibility scene paths or docs first if moving scenes is
too disruptive. The key is naming: static generated chunks are not live runtime.

Exit: a developer can tell from the path whether a scene is static/offline review
or streaming runtime.

### Phase 2 - Split Live Runtime Into Four Owners

Target ownership:

1. **World/biome selection**
   - grammar region/palette/family selection;
   - active biome IDs and weights per page/sample;
   - no texture RIDs, no renderer state.

2. **Biome page producer**
   - recipe dispatch, compose math, flow policy, shader ABI;
   - writes a page texture when asked;
   - does not own page residency or tile visibility.

3. **Page pool/streaming**
   - page texture ownership, LRU, protect/release, producer routing;
   - no recipe constants, no grammar decisions beyond producer inputs.

4. **Renderer**
   - clipmap rings, morph, page fade, culling, material uniforms;
   - no page compute and no biome selection.

Exit: fixing pop-in only touches renderer; fixing sameness only touches
world/biome selection and producer inputs.

First implemented step: `mountain_fly_review.gd` no longer owns producer
constants, scale presets, relief state, or `configure_biome*` calls. Those live
in `mountain_fly_producers.gd`; the scene still owns renderer setup, input, HUD,
route diagnostics, and page-stream state. This does not complete Phase 2, but it
removes one mixed concern from the live owner review scene and gives the mode
state its own fast gate. The follow-up `review_runtime` gate now instantiates the
actual owner scene so this separation is proven on the path the user flies.
`biome_fly_capture.gd` also consumes `mountain_fly_producers.gd`, so live visual
evidence no longer duplicates producer config behind the scene.

Second implemented step: runtime renderer constants now live in
`mountain_fly_runtime_config.gd`. `mountain_fly_review.gd` and
`biome_fly_capture.gd` both consume it for clipmap levels/span, streamer lead,
view relief scale/ref, morph/detail defaults, shader globals, fog/loaded edge,
and ring shader path. `review_runtime` and `review_runtime_visual` prove the
owner scene and visual evidence still start after this split.

Third implemented step: the live review producer now owns explicit world and
mountain-reference seed constants through `runtime_seed()` (renamed away from the
GDScript built-in `seed()`), the MOUNTAIN/network candidate relief is `1700m`,
and only the MOUNTAIN/network review preset overrides the renderer's default view
relief scale to `1.0`. `mountain_fly_review.gd` exposes
`debug_runtime_snapshot()` so the smoke gate validates the owner scene through a
stable debug surface instead of reaching into private fields.

Fourth implemented step: `Wg10PagePool` now has an identity-default live-biome
source transform seam. The MOUNTAIN/network review preset applies the accepted
source/display mapping (`source_scale=3.515625`, center offset `207000,176000`)
after `configure_biome(...)`, so the live recipe samples the same source window
scale as the accepted network payload while the renderer, streamer, and page keys
remain in display coordinates. This is a source-coordinate/world-layer fix; it
does not implement the accepted full-field conditioning or pass-network carving.

### Phase 3 - Restore The Accepted Mountain Path In The Live Runtime

Goal: make the live fly able to reproduce the accepted mountain-network read
before tuning other biomes.

Steps:

1. Add a runtime preset named after the baseline:
   - `mountain_network_reference`
   - carries `feature_span_m=90000.0`, the static payload origin/conditioning
     contract, and pass-network assumptions from the accepted artifact.
2. Keep the current close-up diagnostic preset separate:
   - `mountain_close_live_debug`
   - carries `feature_span_m=3500.0`.
3. Port or mirror the accepted world layer, not just its scalar defaults:
   - same source-window sampling is now implemented for MOUNTAIN/network through
     the page-pool source transform;
   - remaining work is a seam-safe equivalent of the full-field conditioning
     contract;
   - decide whether the connected pass network is a runtime fact, a coarse baked
     fact, or a static-reference-only acceptance target.
4. Add harness toggle or separate scene that makes the selected preset explicit
   in HUD/log output.
5. Compare screenshots/fly notes against `mountain_network_chunks_review.tscn`.

Exit: when the live mountain runtime looks wrong, we know whether it is because
the producer cannot express the accepted terrain or because we selected a
different preset.

Implemented bridge: `REFERENCE` mode now streams the accepted generated payload
through `Wg10PagePool` and the clipmap renderer. Implemented live-source fix:
MOUNTAIN/network now samples the same accepted source window scale. Remaining
Phase 3 work is not a renderer bridge or scalar/source-coordinate fix; it is
deciding how much of the accepted full-field conditioning/pass-network process
should become a live runtime fact versus a separate authored/static acceptance
target.

### Phase 4 - Implement Slice 4 Part B Before More Visual Tuning

Goal: fix the "all the same" problem by implementing the intended world layer.

Work:

1. Runtime grammar selection emits active biome IDs and weights.
2. Page producer consumes those IDs/weights.
3. Compose layer uses the already proven GPU compose passes.
4. Gate verifies the BIOME runtime is not a single hardcoded mountain fragment.

Exit: a live page can be all mountain for the mountain-reference harness, but
the general BIOME runtime can produce mixed/transition worlds by design.

### Phase 5 - Then Resume Mechanical Refactor

Only after baseline/gate clarity:

- continue splitting `page_pool` by producer boundary;
- continue shader ABI manifest/generation;
- keep `biome_page_compute` as a facade while modules settle;
- avoid algorithm changes while moving files.

## Immediate Next Commands

For future verification after renderer/runtime edits:

```powershell
$env:CARGO_TARGET_DIR='D:\workflows\worldgen10\wg-10\rust\target'
cargo build -p wg10_terrain

$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'
python tools\gate.py --suite review_static
python tools\gate.py --suite review_static_visual
python tools\gate.py --suite review_runtime
python tools\gate.py --suite review_runtime_visual
python tools\gate.py --suite m3
python tools\gate.py --suite biome_fly
& $env:GODOT_BIN --path D:\workflows\worldgen10\wg-10 res://worldgen_terrain/harness/mountain_fly_review.tscn --quit-after 2
```

Then re-fly:

```text
res://worldgen_terrain/harness/mountain_fly_review.tscn
res://worldgen_terrain/harness/mountain_network_chunks_review.tscn
```

The comparison question should be explicit:

- Did page fade reduce forward snap-in?
- With morph off/on, what remains?
- Does the live mountain runtime use the same scale preset as the network baseline?
- Is the visual complaint renderer pop, content sameness, or both?

## Decision

Use `mountain_network_chunks_review.tscn` as the current owner visual baseline
for mountain quality. Do not treat `mountain_fly_review.tscn` as a failed
version of that baseline until the runtime is configured to reproduce the same
world/scale assumptions. The live fly is currently a producer/renderer proving
scene; the network chunks scene is the accepted content reference.
