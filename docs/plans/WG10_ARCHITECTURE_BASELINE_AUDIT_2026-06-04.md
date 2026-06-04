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
   - Producer: `Wg10PagePool.configure_biome(...)`
   - Shaders: `recipe_primitives.glsl` + `biome_page.glsl` + `biome_mountain.glsl`
   - Renderer: same clipmap renderer as legacy.
   - Current scene: `wg-10/worldgen_terrain/harness/mountain_fly_review.tscn`
   - Current behavior: all visible terrain is the mountain producer; grammar/region/palette multi-biome selection is not yet the runtime driver.

This explains the owner report:

- The old network-chunk scene looked better because it used the accepted mountain
  world artifact: broad 90 km feature structure plus explicit connected pass
  carving and review dressing.
- The current live BIOME fly is proving a page producer and renderer, not the full
  accepted world/grammar architecture. It currently hardcodes a single mountain
  fragment and has been tuned toward close-up page readability.
- The pop-in and morph issues are renderer/LOD problems. The "ground looks bad /
  all the same" issue is content architecture: the live path is not yet consuming
  the accepted biome composition/grammar layer.

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
- `cargo test -p wg10_terrain --lib`: 217 passed / 0 failed.
- `python tools\gate.py --suite review_static`: 1/1 passed. This suite is now
  scoped to `mountain_network_chunks_review.tscn`, the accepted baseline. An
  earlier attempt that also included `mountain_world_chunks_review_check.gd`
  failed because that generic check is stale against its current
  `biome_compose_world_v2_scale_contract` payload; do not use it as the accepted
  network-baseline proof.
- `python tools\gate.py --suite m3`: 9/9 passed after the page-fade renderer
  change. `m3_accept` p99 = 5.81 ms against the 6.0 ms budget.
- `python tools\gate.py --suite biome_fly`: 4/4 passed. Production 576 macro
  maxd = 2.3156e-5 <= 5e-4, full 576 maxd = 0.001471 <= 0.002, cross-level
  macro ratio = 0.066665 <= 0.08, biome fly GPU p99 = 0.103 ms.
- Direct smoke launch of `mountain_fly_review.tscn`: passed after rebuilding the
  loaded DLL. The scene now starts on the accepted `network_ref` scale
  (`feature_span_m=90000`) and exposes `P` for the old close-up debug scale
  (`feature_span_m=3500`), so the manual review scale is visible in the HUD.

## Why The Current Live BIOME View Does Not Match The Accepted Network Scene

### 1. It is not using the same world layer

`mountain_network_chunks_review.tscn` loads an offline payload whose source scope
is a coherent full-field mountain synthesis with connected pass-network carving.
The mesh review is not a streaming page runtime. It is a baked inspection artifact
that carries exactly the authored structure the owner liked.

`mountain_fly_review.tscn` calls `configure_biome(...)` with a single mountain
fragment. It does not yet ask the grammar for active biome weights or compose
multiple accepted biome recipes into one world.

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
does not solve the missing grammar/compose layer; it removes a review-harness
ambiguity so owner fly feedback names the actual preset being reviewed.

### 3. The current BIOME runtime is all mountain everywhere

The BIOME path currently answers "can the mountain GPU producer stream live
pages?" It does not answer "does the world select and blend biomes like the
accepted composition plan?" That is why the owner sees sameness.

The next content architecture step is not more mountain shader tuning. It is the
Slice 4 Part B integration already called out in `STATUS.md`: port the grammar
region/palette/family selection so each page picks active biomes and composes
them through the proven compose layer.

### 4. There are two independent visual failure classes

Renderer/streaming failures:

- fine pages appear only when resident, so forward movement can show snap-in;
- geomorph can still expose cross-level mismatch when parent/child surfaces are
  not true low-pass relatives;
- page hiding/showing, culling, and repage churn are renderer concerns.

Content/world failures:

- all terrain reads like one biome because live runtime is single-fragment mountain;
- accepted pass-network carving is not part of the live page world layer yet;
- static review scale and live page scale are not reconciled.

These need separate fixes and separate gates.

## What Is Actually Proven

Proven or strongly supported:

- The static mountain 9x9/network review artifact exists and captures an accepted visual direction.
- The Rust biome producer path has extensive parity coverage.
- Scale-invariant producer work is implemented through `flow_max_level` and cross-level gate wiring.
- The renderer has a dedicated M3 gate family, but current page-fade verification is blocked by the open editor.

Not proven yet:

- The current live BIOME fly matches the accepted mountain-network artifact.
- The current live BIOME fly uses grammar-driven multi-biome composition.
- The new renderer page fade passes windowed M3/biome runtime gates.
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
   - current runtime content limitation: all-mountain, no grammar compose;
   - current renderer evidence: `m3` 9/9 passed after page fade; owner re-fly
     is still required because gates do not prove the visual read.
2. Add or update a smoke check for `mountain_network_chunks_review.tscn`.
   Existing `mountain_world_chunks_review_check.gd` checks the non-network
   scene; the network scene deserves its own check because it is now the owner
   baseline.
3. Editor-closed gate results are now recorded above:
   - `python tools/gate.py --suite review_static` -> 1/1 pass;
   - `python tools/gate.py --suite m3` -> 9/9 pass;
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

### Phase 3 - Restore The Accepted Mountain Path In The Live Runtime

Goal: make the live fly able to reproduce the accepted mountain-network read
before tuning other biomes.

Steps:

1. Add a runtime preset named after the baseline:
   - `mountain_network_reference`
   - carries `feature_span_m=90000.0`, relief/reference scale, and pass-network
     assumptions from the static artifact.
2. Keep the current close-up diagnostic preset separate:
   - `mountain_close_live_debug`
   - carries `feature_span_m=3500.0`.
3. Add harness toggle or separate scene that makes the selected preset explicit
   in HUD/log output.
4. Compare screenshots/fly notes against `mountain_network_chunks_review.tscn`.

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
python tools\gate.py --suite m3
python tools\gate.py --suite biome_fly
python tools\gate.py --suite review_static
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
