# WG10 Implementation Spec Audit And Validation Plan - 2026-06-04

Purpose: give the next session one clear source for where WG10 stands, what is
accepted, what is only diagnostic, and how to fix the current owner-visible
"slow, laggy, weird in modes 1/2/3" report without mixing architectures again.

## Current Checkpoint

Branch: `slice4-gpu-page-integration`.

Latest committed backup before this audit pass:

- `a9e76f7 refactor(slice4): split world layer reports`
- tag `backup-slice4-world-layer-reports-20260604-a9e76f7`

Latest uncommitted fix in this pass:

- `wg-10/worldgen_terrain/harness/fly_camera.gd` now implements
  `sync_mouse_from_rotation()`.
- `mountain_fly_review.gd` already tried to call that method after review
  camera reframing, but the shared fly camera did not expose it. That left
  mouse-look yaw/pitch stale after another script called `look_at`, so the first
  manual mouse move could snap the view back to old internal rotation state.
- `mountain_fly_snapshot.gd` exposes
  `camera_has_sync_mouse_from_rotation`.
- `mountain_fly_review_smoke_check.gd` gates that the camera sync method exists.

Latest proofs after the camera fix:

- `python tools/gate.py --suite review_runtime` = 2/2.
- `python tools/gate.py --suite review_runtime_modes` = 2/2.
- `python tools/gate.py --suite review_runtime_stress` = 1/1.

Mode-gate numbers after restoring the stable one-page/frame budget:

- REFERENCE: CPU p99/max `9.824/9.968 ms`, GPU p99 `0.746 ms`.
- MOUNTAIN/network_ref: CPU p99/max `9.597/9.907 ms`, GPU p99 `0.747 ms`.
- WORLD/network_ref preview: CPU p99/max `9.758/13.061 ms`, GPU p99 `0.748 ms`.
- All three: `hide=0`, `show=0`, `full_events=0`, `acquired_max=1`.

Manual stress after the camera fix:

- Six cases across REFERENCE, MOUNTAIN, WORLD and morph off/on passed.
- All six had `hide=0`, `show=0`, `full_events=0`.
- REFERENCE/MOUNTAIN and REFERENCE/WORLD final images remained identical in the
  reference-backed bridge captures.
- One residual single-frame spike remains worth hardening: MOUNTAIN morph-on
  reported `cpu_max=22.349 ms` with p99 still below 10 ms. Current gates permit
  that; a future owner-spike gate should make the intended budget explicit.

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
| Streaming stability | Current bridge gates zero hide/show/full events in modes 1/2/3. | Partial | REPAGE visual delta and single-frame owner spikes are not strict enough yet. |
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

## What The Current Gates Prove

`review_runtime` proves:

- The owner fly scene instantiates and switches modes.
- Mode taxonomy and contract reports are explicit.
- Camera sync support exists after this pass.
- The sprint churn check has no hide/show events.

`review_runtime_modes` proves:

- Keys 1/2/3 are stable on scripted motion.
- No visible tiles hide/show, no pool-full events, and render p99 stays low.
- It does not prove manual mouse free-fly, editor-window stalls, or visual
  quality beyond nondegenerate terrain.

`review_runtime_stress` proves:

- A hand-style speed-pulse path passes for modes 1/2/3 with morph off/on.
- Reference-backed MOUNTAIN and WORLD preview captures match REFERENCE.
- It does not yet fail on abrupt REPAGE visual changes when tiles remain visible.
- It does not yet fail on single-frame spikes below the current max budget.

`review_runtime_visual` proves:

- Runtime REFERENCE and MOUNTAIN/network bridge match in captured frames.
- Runtime REFERENCE stays tied to the old accepted static network scene.
- It does not prove raw procedural mountain content is acceptable.

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

3. Add a stricter owner-spike gate.
   - Current stress allows a one-frame `22.349 ms` MOUNTAIN morph-on spike.
   - Decide an owner review max budget and fail it explicitly.
   - Record frame, mode, morph state, acquired pages, and repage count.

4. Add a visual REPAGE-delta gate.
   - Current gates count REPAGE as acceptable if tiles never hide.
   - The owner complaint is perceptual: content popping while moving forward.
   - Capture pre/post repage frames, compare image delta in terrain mask, and
     fail if the terrain change is too abrupt.

5. Keep modes 1/2/3 honest.
   - Mode 1 is the accepted baseline.
   - Modes 2/3 matching mode 1 is intentional until procedural content is built.
   - Do not make mode 2 or mode 3 "look different" by tuning random color/relief
     knobs. That would hide the contract gap.

6. Build the progression review scene before adding more content.
   - A new scene should add one roadmap feature at a time and gate each step.
   - This prevents another round of "multiple architectures in one scene" drift.

## Progression Scene Plan

Recommended files:

- `wg-10/worldgen_terrain/harness/wg10_progression_review.tscn`
- `wg-10/worldgen_terrain/harness/wg10_progression_review.gd`
- `wg-10/worldgen_terrain/tests/wg10_progression_review_check.gd`

Principle: one shared renderer/streamer path, one feature added per step, and a
snapshot/gate for each step before promotion.

Steps:

1. Baseline render shell.
   - Flat/simple static height.
   - Same streamer, rings, camera, profiler.
   - Gate: nonblank terrain, no hide/show/full events, stable p99.

2. Accepted static REFERENCE.
   - Load `mountain_world_layer_tiles.json`.
   - Gate against `mountain_network_chunks_review.tscn` focus view.

3. Source/display mapping overlay.
   - Show display window and sampled source window.
   - Gate the mapping report, no hidden scale constants.

4. MOUNTAIN/network_ref bridge.
   - Same accepted payload through the single-producer lane.
   - Gate byte/visual equivalence to REFERENCE.

5. Material facts.
   - Toggle low-pass/corridor, floor, rock, snow channels one at a time.
   - Gate nonvacuous visual deltas and no perf regression.

6. Pass-network facts.
   - Add corridor overlay and route reports.
   - Gate nonzero route/carve coverage.

7. Raw MOUNTAIN close-debug candidate.
   - Show it as a candidate only.
   - Gate and document measured visual/numeric gap versus REFERENCE.

8. Procedural/generated mountain world-layer candidate.
   - Consume generated tile/fact data through the runtime contract.
   - Goal is to reduce the measured gap from step 7.

9. WORLD route/weight diagnostic.
   - Keep active biome preview bounded and explicit.
   - Gate route/weight reports separately from owner terrain acceptance.

10. Full WORLD compose.
    - Only after async/cache or a cheaper preview contract exists.
    - Explicit perf gate; synchronous full compose stays prohibited.

11. Facts/collision.
    - Collision must read the same terrain facts that the visual layer presents.
    - Gate visible/queryable parity.

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
```

## Next Definition Of Done

The next checkpoint is not "biomes look cool." It is:

- The progression scene exists.
- Steps 1-4 replay the current accepted bridge without ambiguity.
- A stricter spike gate and a REPAGE visual-delta gate exist.
- Raw procedural mountain remains visibly/numerically compared against the
  accepted baseline instead of being promoted by feel.
- The docs continue to state which modes are accepted, bridge, prototype, or
  diagnostic.
