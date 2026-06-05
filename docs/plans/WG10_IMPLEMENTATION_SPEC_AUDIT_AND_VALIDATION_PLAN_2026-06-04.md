# WG10 Implementation Spec Audit And Validation Plan - 2026-06-04

Purpose: give the next session one clear source for where WG10 stands, what is
accepted, what is only diagnostic, and how to fix the current owner-visible
"slow, laggy, weird in modes 1/2/3" report without mixing architectures again.

## Current Checkpoint

Branch: `slice4-gpu-page-integration`.

Current scoped backup checkpoint:

- `6860e6a fix(slice4): reduce material fact hitch`
- tag `backup-slice4-material-hitch-20260604-6860e6a`

Earlier split checkpoint kept for archaeology:

- `a9e76f7 refactor(slice4): split world layer reports`
- tag `backup-slice4-world-layer-reports-20260604-a9e76f7`

Latest camera fix checkpoint:

- `wg-10/worldgen_terrain/harness/fly_camera.gd` now implements
  `sync_mouse_from_rotation()`.
- `mountain_fly_review.gd` already tried to call that method after review
  camera reframing, but the shared fly camera did not expose it. That left
  mouse-look yaw/pitch stale after another script called `look_at`, so the first
  manual mouse move could snap the view back to old internal rotation state.
- `mountain_fly_snapshot.gd` exposes
  `camera_has_sync_mouse_from_rotation`.
- `mountain_fly_review_smoke_check.gd` gates that the camera sync method exists.

Implemented follow-up checkpoint:

- `wg-10/worldgen_terrain/harness/wg10_progression_review.tscn`
- `wg-10/worldgen_terrain/harness/wg10_progression_review.gd`
- `wg-10/worldgen_terrain/tests/wg10_progression_review_check.gd`
- `wg-10/worldgen_terrain/tests/wg10_progression_motion_check.gd`
- `wg-10/worldgen_terrain/tests/wg10_progression_repage_visual_check.gd`
- `python tools/gate.py --suite review_progression` = 3/3.

This new scene is the next-chat validation harness. It exposes four current
steps with explicit status and expected contract:

1. `reference_baseline`: accepted REFERENCE runtime baseline.
2. `mountain_network_bridge`: reference-backed MOUNTAIN bridge.
3. `mountain_close_debug_candidate`: raw live mountain prototype.
4. `world_reference_preview`: bounded WORLD diagnostic over accepted reference
   height/materials.

It also records and gates a machine-readable feature manifest. The first two
review features are implemented: each current step emits a
`source_display_report` and `material_fact_report`, and the scene renders
source/display plus material-fact mini-overlays that the smoke gate proves are
visible and nondegenerate. The remaining planned features are pass-network
facts, procedural mountain world-layer production, and facts/collision parity;
each carries its label, added contract, proving gate, acceptance rule, and
promotion blocker.

Latest strict-hitch fix checkpoint:

- Accepted/reference-backed material fact pages now stream at `page_px / 4`
  instead of `page_px / 2`. Height pages stay full resolution; only the
  low-frequency RGBA fact masks are cheaper.
- This fixed the fresh `review_runtime_stress` failure where REFERENCE
  morph-off hit `cpu_max=22.436 ms` against the `16.7 ms` strict budget.
- Rebuilt proof after the fix: `review_runtime_stress` = 1/1,
  `review_runtime_modes` = 2/2, `review_runtime_visual` = 2/2,
  `review_progression` = 3/3, and targeted Rust
  `cargo test static_reference` = 10/10.

Latest motion fix checkpoint:

- Root cause for the owner-visible "pops while moving in 1/2/3" class was the
  shared live clipmap rebinding too many visible tiles on page-boundary crosses.
- Before the fix, the new progression motion gate failed all four steps with
  `repage_frame_max=18`, `repage=72`, while still reporting `hide=0`,
  `show=0`, and `full_events=0`. That explains why the older gates were green
  while the owner still saw popping: the gates allowed visible REPAGE churn.
- `terrain_view.rs` now maps displayed pages to toroidal 3x3 slots by absolute
  page origin. Existing pages keep their mesh/material slot; only newly-entering
  rows/columns rebind.
- After the fix, `review_progression` passes with `repage_frame_max=8`,
  `repage=26`, `hide=0`, `show=0`, and `full_events=0` for all four steps.
- Pixel-level repage proof now exists in
  `wg10_progression_repage_visual_check.gd`: it holds the render camera fixed,
  crosses L0/L1/L2 page boundaries in all four progression steps, and compares
  terrain-mask before/after images. Latest proof checked 12 pairs with worst
  mean/p95/p99 RGB delta `0.000831/0.002614/0.020915`.

Latest proofs after the camera and clipmap fixes:

- `python tools/gate.py --suite review_runtime` = 2/2.
- `python tools/gate.py --suite review_runtime_modes` = 2/2.
- `python tools/gate.py --suite review_runtime_visual` = 2/2.
- `python tools/gate.py --suite review_progression` = 3/3.
- `python tools/gate.py --suite review_runtime_stress` = 1/1 with strict
  CPU p99/max and GPU p99 budgets at `16.7 ms`.

Mode-gate numbers after the toroidal slot fix:

- REFERENCE: CPU p99/max `12.293/13.652 ms`, GPU p99 `0.811 ms`.
- MOUNTAIN/network_ref: CPU p99/max `12.918/13.812 ms`, GPU p99 `0.792 ms`.
- WORLD/network_ref preview: CPU p99/max `12.143/20.185 ms`, GPU p99 `0.784 ms`.
- All three: `hide=0`, `show=0`, `full_events=0`, `acquired_max=1`,
  `repage=26`.

Manual stress after the camera fix:

- Six cases across REFERENCE, MOUNTAIN, WORLD and morph off/on pass under the
  stricter owner-spike budget.
- All six had `hide=0`, `show=0`, `full_events=0`.
- REFERENCE/MOUNTAIN and REFERENCE/WORLD final images remained identical in the
  reference-backed bridge captures.
- The manual stress gate now fails any measured CPU p99 or CPU max over
  `16.7 ms`, making one-frame owner-visible hitches explicit failures instead
  of tolerated tail events. Latest fresh visible morph-on tail peaked at
  REFERENCE `cpu_max=10.779 ms`; all six cases passed the stricter max budget.

## Mode Truth

Mode `1` / `REFERENCE`:

- Accepted owner-visible mountain-network baseline.
- Runtime producer: `configure_static_reference(...)`.
- Source: `mountain_world_layer_tiles.json`, generated from the accepted static
  mountain world-layer artifact.
- This is the target look for the current recovery loop.

Mode `2` / `MOUNTAIN/network_ref`:

- Runtime producer is still `configure_biome(...)`, but reviewed network preset
  binds the accepted world-layer reference for height/material/facts.
- Contract report:
  `single_mountain_world_layer_reference_bridge`,
  `height_source=bound_world_layer_reference_payload`,
  `procedural_world_layer_height=false`.
- This is an accepted visual bridge, not final procedural mountain synthesis.

Mode `2` / `MOUNTAIN/close_debug`:

- Raw live seam-safe mountain page recipe.
- Prototype only.
- Expected to look worse than the old network-chunk scene because it lacks the
  accepted pass-network, route carving, and whole-field conditioning contract.

Mode `3` / `WORLD`:

- Grammar-routed WORLD diagnostics remain live.
- Owner-facing normal preview binds the accepted mountain reference height and
  material facts, so it matches REFERENCE by design.
- Full multi-biome WORLD height composition is not accepted in the owner fly.
  A previous direct full-compose attempt caused roughly 1.9 s page-build
  hitches, so full WORLD must be async/cached or replaced by a cheaper preview.

Mode `4` / `LEGACY`:

- Old DEM/kernel atlas regression path.
- Useful for renderer A/B checks, not a target look.

## Texture Scope

Final terrain textures have not been started and are not part of the current
acceptance bar. The live review shader only provides:

- a height/slope mountain palette;
- low-resolution RGBA material facts for low-pass/corridor, floor, rock, and
  snow readability;
- morph and route debug modes.

Do not treat the current palette as final art. For this checkpoint the visual
bar is geometry, streaming stability, accepted reference readability, and
contract evidence. Texture/art production starts only after pass-network facts,
procedural mountain world-layer generation, and facts/collision parity are
proved through the progression scene.

## Requirement Audit Against The Mountain World-Layer Contract

| Requirement | Current evidence | Status | Gap |
| --- | --- | --- | --- |
| Source/display mapping | Runtime tile payload exposes display/source mapping and the bridge uses it. | Partial | Proven for reference-backed bridge, not final procedural producer. |
| Mountain macro field | Accepted payload gives the owner-liked 90 km mountain structure. | Partial | Raw live mountain recipe does not reproduce this contract. |
| Pass-network routes | Accepted payload has pass-network facts and route carving. | Partial | Procedural live producer does not own these facts yet. |
| Route carving | Accepted payload carves connected passes before runtime sampling. | Partial | Live seam-safe page recipe has no page-stable connected corridor fact. |
| Conditioning | Accepted payload includes whole-field conditioning stats. | Partial | Live producer lacks equivalent page-stable conditioning. |
| Material/fact hints | Runtime material pages preserve low-pass/corridor, floor, rock, snow channels. | Partial | Works for accepted reference payload; final procedural material facts remain open. |
| Facts/collision | Base facts API exists. | Open | Mountain world-layer facts are not yet the collision authority for final procedural content. |
| Streaming stability | Current bridge gates zero hide/show/full events in modes 1/2/3; progression motion bounds same-frame repage bursts to 8; progression visual repage checks 12 fixed-camera boundary pairs with worst mean/p95/p99 RGB delta `0.000831/0.002614/0.020915`; manual stress now fails CPU p99/max or GPU p99 over `16.7 ms`. | Partial | Scripted/manual stress is covered; unscripted editor free-fly remains human confirmation. |
| Owner visual acceptance | Static network scene and runtime REFERENCE bridge match by silhouette/color gates. | Partial | Procedural MOUNTAIN and full WORLD are not accepted. |

## Architecture Audit

The broad "everything is over 1000 lines" concern is no longer the active
runtime failure mode after the latest splits. The important source ownership is:

- `page_pool/config_api.rs`: Godot-facing producer configuration.
- `page_pool/world_runtime.rs`: cached WORLD runtime state and teardown.
- `page_pool/world_layer_bindings.rs`: binding accepted references beside live
  producers.
- `page_pool/world_layer_contract.rs`: contract taxonomy and blocking-gap report.
- `page_pool/static_reports.rs`: sampled static/reference reports.
- `terrain_view.rs`: shared streaming-to-rings display loop.
- `clipmap_rings.rs`: mesh/material presentation and page binding.
- `mountain_fly_producers.gd`: mode/preset producer selection.
- `mountain_fly_runtime_config.gd`: review renderer constants.
- `mountain_fly_snapshot.gd`: owner-facing diagnostic report shape.

The current risk is not one giant file. The risk is that accepted baseline,
reference bridge, raw procedural prototype, WORLD diagnostics, and legacy atlas
are still co-present in one fly scene. The next work should make that taxonomy
impossible to confuse.

## Current Source-Size Evidence

Fresh line-count audit on the active terrain/runtime/tooling paths found no
Rust, GDScript, GLSL, or Python source file over 1000 lines.

| Path | Lines | Meaning |
| --- | ---: | --- |
| `tools/dem_pack/fixtures/recipe_noise_fixtures.json` | 3646 | Fixture data, not source. |
| `docs/plans/STATUS.md` | 2019 | Living history document. |
| `tools/dem_pack/proposed_family_tags.json` | 1280 | Data/config. |
| `tools/dem_pack/export_godot_rough_world_chunks.py` | 745 | Largest active Python source. |
| `wg-10/worldgen_terrain/harness/mountain_world_chunks_review.gd` | 695 | Largest accepted static-review harness. |
| `wg-10/worldgen_terrain/harness/wg10_progression_review.gd` | 687 | Current progression harness. |
| `wg-10/worldgen_terrain/shaders/biome_page.glsl` | 612 | Largest active shader. |
| `wg-10/rust/src/recipes_karst.rs` | 566 | Largest active Rust source in the audit set. |
| `wg-10/rust/src/biome_page_compute/local_compose.rs` | 565 | Largest shared biome compose Rust module. |

Conclusion: the refactor priority is not "split every 1000-line file." It is
to keep accepted/reference/prototype/diagnostic lanes separate, and to split the
progression harness once feature overlays make it materially harder to review.

## What The Current Gates Prove

`review_runtime` proves:

- The owner fly scene instantiates and switches modes.
- Mode taxonomy and contract reports are explicit.
- Camera sync support exists after this pass.
- The sprint churn check has no hide/show events.

`review_runtime_modes` proves:

- Keys 1/2/3 are stable on scripted motion.
- No visible tiles hide/show, no pool-full events, and render p99 stays low.
- After the toroidal slot fix, visible repage totals in the scripted path are
  down to 26 for each mode.
- It does not prove manual mouse free-fly, editor-window stalls, or visual
  quality beyond nondegenerate terrain.

`review_runtime_stress` proves:

- A hand-style speed-pulse path passes for modes 1/2/3 with morph off/on.
- Reference-backed MOUNTAIN and WORLD preview captures match REFERENCE.
- The progression suite now owns the fixed-camera pixel-delta repage proof; this
  stress gate still focuses on hand-style path perf, hide/show/full events, and
  bridge image matching.
- It now fails CPU p99/max or GPU p99 above `16.7 ms`, so one-frame synchronous
  owner hitches are budgeted directly.

`review_runtime_visual` proves:

- Runtime REFERENCE and MOUNTAIN/network bridge match in captured frames.
- Runtime REFERENCE stays tied to the old accepted static network scene.
- It does not prove raw procedural mountain content is acceptable.

`review_progression` proves:

- The recovery progression scene exposes the current four-step ladder with
  explicit status and contract kinds.
- The scene exposes a gated progression manifest: current steps, future steps,
  proving suites, promotion rule, and per-step `source_display_report` plus
  `material_fact_report`.
- The source/display and material-fact overlay features are implemented and
  visible in normal scene mode, while probe mode hides those overlays so
  terrain visual-delta gates remain focused on the render path.
- The same scene survives scripted page-boundary motion with zero hide/show/full
  events and bounded repage bursts.
- The same scene passes fixed-camera pixel-delta checks at L0/L1/L2
  page-boundary crosses across all four current steps.
- It does not prove future planned features until each step is added and gated.

## Fix Plan For "Slow, Laggy, Weird In 1/2/3"

1. Keep the camera-sync fix.
   - This fixes the concrete stale yaw/pitch bug after review reframing.
   - It affects all harnesses that call `sync_mouse_from_rotation()`.

2. Do not raise the default page budget casually.
   - Testing `MAX_PER_FRAME=2` passed, but produced page-acquire p99 around
     `17.5-17.9 ms`.
   - The stable owner default remains `MAX_PER_FRAME=1`.
   - If we need more throughput, make it mode-specific or async/cache-backed,
     not a global synchronous spike.

3. Keep the progression motion gate.
   - Implemented as `wg10_progression_motion_check.gd`.
   - It caught the all-mode repage burst that older hide/show gates missed.
   - The toroidal slot fix reduced `repage_frame_max` from 18 to 8.

4. Keep the visual REPAGE-delta gate.
   - Implemented as `wg10_progression_repage_visual_check.gd`.
   - It holds the camera fixed, crosses known page boundaries, and compares
     terrain-mask images so normal camera motion cannot hide a renderer pop.
   - Latest worst mean/p95/p99 RGB delta is
     `0.000831/0.002614/0.020915`.

5. Keep the strict owner-spike gate.
   - Implemented in `mountain_fly_manual_stress_check.gd`.
   - The stress gate fails CPU p99/max or GPU p99 above `16.7 ms`.
   - It records frame, mode, morph state, acquired pages, repage count, and
     evidence captures for the bridge comparisons.
   - Latest concrete failure/fix: REFERENCE morph-off hit `cpu_max=22.436 ms`
     before accepted material fact pages were reduced to `page_px / 4`; after
     the rebuild, the strict stress suite passes all six cases.

6. Keep modes 1/2/3 honest.
   - Mode 1 is the accepted baseline.
   - Modes 2/3 matching mode 1 is intentional until procedural content is built.
   - Do not make mode 2 or mode 3 "look different" by tuning random color/relief
     knobs. That would hide the contract gap.

7. Build from the progression review scene before adding more content.
   - A new scene should add one roadmap feature at a time and gate each step.
   - This prevents another round of "multiple architectures in one scene" drift.

## Progression Scene Plan

Implemented files:

- `wg-10/worldgen_terrain/harness/wg10_progression_review.tscn`
- `wg-10/worldgen_terrain/harness/wg10_progression_review.gd`
- `wg-10/worldgen_terrain/tests/wg10_progression_review_check.gd`

Principle: one shared renderer/streamer path, one feature added per step, and a
snapshot/gate for each step before promotion.

Current active ladder:

1. `reference_baseline`: accepted REFERENCE runtime baseline from
   `mountain_world_layer_tiles.json`.
2. `mountain_network_bridge`: MOUNTAIN single-producer lane with the accepted
   world-layer reference bound beside it.
3. `mountain_close_debug_candidate`: raw live mountain recipe, explicitly
   prototype-only and measured against REFERENCE.
4. `world_reference_preview`: WORLD route/weight diagnostics over the accepted
   reference height/material preview.

Implemented cross-cutting review features:

1. Source/display mapping report and overlay.
   - Gated for all four active steps.
   - Smoke checks cover `source_display_report`, overlay visibility, mapping
     kind, positive source/display spans, and nonzero overlay rectangles.
2. Material fact report and overlay.
   - Gated for all four active steps.
   - Accepted/bridge/preview steps expose low-pass/corridor, floor, rock, and
     snow fact channels; the raw close-debug step is expected to report the
     missing-material-facts gap.

Next feature queue:

1. Pass-network facts.
   - Add corridor/route report and overlay to the active ladder.
   - Gate nonzero route/carve coverage for REFERENCE and MOUNTAIN/network.
   - Gate the raw close-debug step as an explicit missing-gap report, not as a
     failure hidden by the reference preview.
2. Procedural/generated mountain world-layer candidate.
   - Consume generated/cached tile/fact data through the same runtime contract.
   - Promote only if numeric/visual gap to REFERENCE improves while
     `review_progression`, `review_runtime_visual`, and `review_runtime_stress`
     remain green.
3. WORLD compose.
   - Keep owner preview bounded until async/cache or a cheaper preview contract
     exists.
   - Synchronous full compose stays prohibited in the owner fly.
4. Facts/collision parity.
   - Collision/query authority must read the same facts presented by the visual
     layer.
   - Gate visible/queryable parity over sampled pages.

## Do Not Do

- Do not call raw `MOUNTAIN/close_debug` accepted.
- Do not call full `WORLD` accepted.
- Do not sync full WORLD compose in the owner fly.
- Do not tune mode 2/3 colors just to make them look different.
- Do not remove the accepted REFERENCE bridge while procedural content is still
  unproven.
- Do not clean the noisy worktree with broad reset/checkout commands.

## Pickup Commands

From `D:\workflows\worldgen10`:

```powershell
git status --short
cargo fmt -p wg10_terrain -- --check
cargo test -p wg10_terrain --lib page_pool -- --nocapture
cargo test -p wg10_terrain --lib
powershell -ExecutionPolicy Bypass -File tools\build_rust.ps1
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite review_runtime
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite review_runtime_modes
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite review_runtime_stress
$env:GODOT_BIN='C:\Godot\v4.6.2\Godot_v4.6.2-stable_mono_win64\Godot_v4.6.2-stable_mono_win64_console.exe'; python tools\gate.py --suite review_progression
```

## Next Definition Of Done

The next checkpoint is not "biomes look cool." It is:

- The progression scene exists.
- Steps 1-4 replay the current accepted bridge without ambiguity.
- The progression scene has static, motion, and fixed-camera visual repage
  gates.
- The progression scene carries a machine-readable future-step manifest and
  source/display plus material-fact reports/overlays for the active steps.
- The next implemented feature is pass-network facts, with explicit route/carve
  reports and overlays plus a gated missing-gap report for raw close-debug.
- The strict owner-spike gate remains green under modes 1/2/3 with morph off/on.
- Raw procedural mountain remains visibly/numerically compared against the
  accepted baseline instead of being promoted by feel.
- The docs continue to state which modes are accepted, bridge, prototype, or
  diagnostic.
