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
   - Producers: `Wg10PagePool.configure_biome(...)` for the default single-mountain
     fly mode, `Wg10PagePool.configure_biome_world(...)` for the WORLD composition A/B mode.
   - Shaders: `recipe_primitives.glsl` + `biome_page.glsl` + the 11 compiled
     `biome_<name>.glsl` fragments.
   - Renderer: same clipmap renderer as legacy.
   - Current scene: `wg-10/worldgen_terrain/harness/mountain_fly_review.tscn`
   - Current behavior: MOUNTAIN mode starts first so the mountain review scene
     reviews mountain content. WORLD mode remains available through `B`; it generates
     a texel-corner runtime-biome weight field per page, dispatches each active
     biome context, and folds those core height fields through the GPU compose
     passes before writing the page texture.

This explains the owner report:

- The old network-chunk scene looked better because it used the accepted mountain
  world artifact: broad 90 km feature structure plus explicit connected pass
  carving and review dressing.
- The current live BIOME fly is proving page producer composition and renderer
  behavior, not full owner visual acceptance. The mountain review now starts on
  single-mountain content again, while WORLD composition stays as an explicit A/B.
  WORLD can compose active runtime-biome weights, but materials/content/facts and
  the owner fly review are still open.
- The pop-in and morph issues have two pieces: renderer scheduling controls
  hide/show and geomorph timing, while the route diagnostics explain why the old
  whole-page selector was structurally wrong at coarse LODs. The renderer now
  separates visible display coverage from led prefetch coverage and has an
  automated zero-hide sprint gate. The live path now consumes grammar weights
  through compose; any remaining "ground looks bad" report needs a fresh owner
  fly against this composed runtime.

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

- `cargo build -p wg10_terrain` into the Godot-loaded target: passed.
- `cargo test -p wg10_terrain --lib`: 220 passed / 0 failed.
- `python tools\gate.py --suite review_static`: 1/1 passed. This suite is now
  scoped to `mountain_network_chunks_review.tscn`, the accepted baseline. An
  earlier attempt that also included `mountain_world_chunks_review_check.gd`
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
  `m3_accept` p99 = 5.64 ms against the 6.0 ms budget;
  `ring_material_tint_check.gd` proves
  `biome_debug_color` and WORLD `biome_material_mix=0.34` are bound through the
  actual `Wg10ClipmapRings` material API; `m5_detail_check` remained
  non-vacuous (`diff=0.0124`), bounded, and edge-safe.
- `python tools\gate.py --suite biome_fly`: 4/4 passed. Production 576 macro
  maxd = 2.3156e-5 <= 5e-4, full 576 maxd = 0.001471 <= 0.002, cross-level
  macro ratio = 0.066665 <= 0.08, biome fly GPU p99 = 0.106 ms.
- `python tools\gate.py --suite fast`: 8/8 passed after extracting
  `mountain_fly_producers.gd` and `mountain_fly_runtime_config.gd`. The producer
  check locks the live review helper's default MOUNTAIN/network preset,
  close-debug preset, relief clamps, and B-cycle order. The runtime config check
  locks the shared renderer defaults: 5 levels, 8192 m base span, 196608 m loaded
  edge, morph/detail default off, and the review sky color.
- Direct smoke launch of `mountain_fly_review.tscn`: passed after rebuilding the
  loaded DLL. The scene now starts in `MOUNTAIN` mode on the accepted
  `network_ref` scale (`feature_span_m=90000`) and exposes `P` for the old
  close-up debug scale (`feature_span_m=3500`), so the manual review scale is
  visible in the HUD. Smoke log: `mode=MOUNTAIN runtime=single biome_path=true
  preset=network_ref feature_span_m=90000 relief_m=1000`.
- `python tools\gate.py --suite review_runtime`: 2/2 passed. This windowed
  gate instantiates the actual `mountain_fly_review.tscn` owner scene, waits for
  startup, then verifies `MOUNTAIN/network_ref`, runtime=`single`,
  biome_path=`true`, and real page startup (`created=45`, `resident=45`). It also
  runs `mountain_fly_visibility_churn_check.gd`, a sprint-speed motion gate over
  360 frames: `stream_events=24`, `resident=69`, `repage=72`, `hide=0`,
  `show=0`, `hidden_frames=0`, `max_hidden=0`. This specifically gates the
  GDScript/Rust call signature, default scene wiring, and forward-motion
  hide/show pop-in.
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
- `biome_fly_capture.gd` now writes four visual artifacts:
  `D:/tmp/wg10_biome_compose/biome_mountain_network_fly_capture.png`,
  `D:/tmp/wg10_biome_compose/biome_mountain_close_fly_capture.png`,
  `D:/tmp/wg10_biome_compose/biome_world_fly_capture.png`, and
  `D:/tmp/wg10_biome_compose/biome_world_fly_capture_routes.png`. The mountain
  network capture gives a separate visual proof for the scene's default producer;
  the close-debug capture shows why that 3.5 km scale should remain diagnostic
  only; the route capture proves the renderer receives page route labels; the
  normal WORLD material capture now receives a restrained route-color tint, so
  per-biome regions no longer collapse to the same mountain palette. Final
  per-pixel biome materials/content remain open.
- `python tools\gate.py --suite review_runtime_visual`: 1/1 passed. This
  regenerates the four live runtime PNG artifacts above and now routes producer
  mode/preset configuration through `mountain_fly_producers.gd` and renderer
  setup through `mountain_fly_runtime_config.gd`, so the owner scene and runtime
  visual capture share producer constants, `configure_biome*` call shape,
  clipmap/view constants, morph/detail defaults, fog, and loaded edge.

Follow-up live visual rerun:

- `ring_displace.gdshader` now colors terrain from the same displayed height used
  for `VERTEX.y`, after `relief_scale`. This fixes a presentation bug where the
  palette saw unscaled page metres while the geometry was drawn at the scaled
  height.
- The corrected final capture still does not match
  `mountain_network_chunks_review.tscn`. MOUNTAIN/network remains a raw live page
  recipe over the current sampled region; the accepted baseline is a conditioned
  270 km source field with connected pass-network carving, sliced into the
  review scene.
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

`mountain_fly_review.tscn` now defaults to MOUNTAIN mode and calls
`configure_biome(...)` with the single mountain fragment, so the mountain review
scene no longer starts by showing arbitrary WORLD grammar content. The separate
WORLD mode remains available through `B`; it calls `configure_biome_world(...)`,
asks the grammar for active runtime-biome weights, and composes the active GPU
biome recipes into the streamed page texture.

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
   - sample the same source window (`world_origin_x_m=72000`,
     `world_origin_z_m=41000`);
   - apply a seam-safe equivalent of the full-field conditioning contract;
   - decide whether the connected pass network is a runtime fact, a coarse
     baked fact, or a static-reference-only acceptance target.
4. Add harness toggle or separate scene that makes the selected preset explicit
   in HUD/log output.
5. Compare screenshots/fly notes against `mountain_network_chunks_review.tscn`.

Exit: when the live mountain runtime looks wrong, we know whether it is because
the producer cannot express the accepted terrain or because we selected a
different preset.

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
