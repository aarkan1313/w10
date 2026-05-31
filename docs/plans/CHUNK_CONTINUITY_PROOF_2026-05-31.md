# WorldGen10 Chunk Continuity Proof - 2026-05-31

Purpose: record exactly what the AFK chunk-continuity proof does and does not prove.
This is a review artifact for the current rough-highlands keeper, not a production
runtime architecture.

## Current Verdict

Status: **bounded proof built; owner visual acceptance pending**.

The proof is good enough to review chunk-to-chunk terrain continuity in Godot:
adjacent 25.6 km chunks are different terrain, share exact border heights, and
carry low-corridor masks across seams well enough for a first visual pass.

It is **not** yet proof of arbitrary infinite generation in all directions. The
current rough-highlands generator still has window/span-local steps, so the safe
bounded proof exports one authoritative 3x3 world-coordinate super-window and
splits it into chunks. That is the correct review artifact, but the future
runtime needs world-window authority and apron semantics before independent
streamed windows can replace it.

## Artifacts

- Exporter: `tools/dem_pack/export_godot_rough_world_chunks.py`
- Godot payload: `wg-10/worldgen_terrain/generated/review/rough_world_chunks_3x3.json`
- Review scene: `wg-10/worldgen_terrain/harness/rough_world_chunks_review.tscn`
- Tests: `tools/dem_pack/test_rough_world_chunks.py`
- External report: `D:\tmp\wg10_geography_engine\rough_world_chunks_3x3_seams.{csv,md}`

## What It Proves

- Deterministic bounded export from seed + world origin + chunk span + generator version.
- Two visible seed examples: `133` and `211`, switchable in-scene with `T`.
- A 3x3 layout of adjacent 25.6 km chunks.
- Adjacent chunks are not repeated copies. Reported mean absolute center/east chunk deltas:
  - seed 133: `0.9658`
  - seed 211: `0.5257`
- Different seeds produce materially different worlds. Center chunk seed-pair mean absolute delta:
  - `133 -> 211`: `0.6383`
- Shared chunk-border height deltas are exact in the generated payload:
  - max abs height delta: `0.000000`
- Corridor continuity is non-vacuous and mostly continuous:
  - minimum low-corridor seam match fraction: `0.951`
- The review scene uses one-sample aprons for chunk-edge normals, reducing visual shading seams at shared edges.

## What It Does Not Prove

- It does not prove the final Rust/GLSL runtime.
- It does not prove arbitrary chunk windows can be generated independently today.
- It does not prove full hydrology, gameplay navmesh quality, or route desirability.
- It does not prove owner visual acceptance; the owner still needs to fly the scene.
- It does not prove long-distance travel pacing. A 76.8 km-wide 3x3 proof is enough for seam review, not for travel-loop acceptance.

## Why The Boundary Exists

The current rough-highlands path is seeded and world-coordinate aware, but it still
uses window/span-local operations in the review generator path. Examples include
coarse skeleton span selection, local normalization, and final review conditioning.
If each 25.6 km chunk were generated independently through that path, edges would
not be a reliable proof. The current exporter avoids that by generating an
authoritative 3x3 world first, then splitting it into chunks.

For a true infinite-in-all-directions implementation, the generator must be
defined over deterministic world windows with aprons and a clear authority rule:
same seed + same world coordinate + same generator version must produce the same
height/facts regardless of which page/window requested the sample.

## Infinite / Player-Travel Review

If the owner accepts the bounded 3x3 visual read, the next review should answer:

1. **Authority model:** which window owns a sample, how wide is the apron, and how are facts cropped?
2. **Sampling contract:** can height, corridor facts, and material descriptors be sampled consistently by render, collision, and AI?
3. **Travel pacing:** at expected player speeds, how much terrain is visible before repetition or boredom appears?
4. **Streaming shape:** what is the smallest viable window/cache system for 25.6 km playable chunks without loading a whole 3x3 super-window every time?
5. **Continuity gates:** exact height seams, bounded normal/material seams, corridor continuation, deterministic seed variation, and no repeated stamps.
6. **Owner visual gate:** fly across boundaries at terrain view, corridor overlay, and oblique/overview scales.

Do not start a Rust/GLSL runtime port until those answers are accepted for the keeper.

## Verification

Last focused verification for the proof:

```text
python -m pytest tools\dem_pack\test_rough_world_chunks.py tools\dem_pack\test_rough_world_traversability.py tools\dem_pack\test_geography_skeleton.py tools\dem_pack\test_geography_skeleton_windows.py -q
26 passed
```

Godot import evidence:

```text
Godot --headless --import --path wg-10
exit 0, no GDScript parse errors
```

Known caveat: a separate headless scene-run attempt crashed Godot after a
`user://logs` write failure. Do not use that as terrain evidence. The visible
Windows review scene is the intended owner gate.
