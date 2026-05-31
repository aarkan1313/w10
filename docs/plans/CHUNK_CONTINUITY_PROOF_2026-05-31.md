# WorldGen10 Chunk Continuity Proof - 2026-05-31

Purpose: record exactly what the AFK chunk-continuity proof does and does not prove.
This is a review artifact for the current rough-highlands keeper, not a production
runtime architecture.

## Current Verdict

Status: **offline independent-window proof built; owner visually accepted seams
for the bounded review scene**.

Owner seam verdict (2026-05-31): "from what i can see seams are good visually."
Treat this as seam-continuity acceptance for the 3x3 review scene, not full
terrain/gameplay acceptance and not production/runtime acceptance.

Owner first 5x5 travel-scene read (2026-05-31): "seems good", but possibly too
flat / not enough elevation; current judgement is confounded by missing biome
and texture dressing. Treat this as a yellow keep signal for continuity/travel
scale, not final terrain acceptance.

The proof is good enough to review chunk-to-chunk terrain continuity in Godot:
adjacent 25.6 km chunks are different terrain, share exact border heights, and
carry structural route/corridor masks across seams well enough for a first visual pass.

It is **not** yet proof of arbitrary infinite generation in all directions. The
current review path is still offline Python, and the Godot scene loads a static
JSON artifact. The latest proof now generates each 25.6 km chunk from its own
deterministic world-coordinate skeleton window with a 25.6 km apron, then crops
the authoritative core. That is a better shape than the previous 3x3
super-window split, but the future runtime still needs a real window/cache
authority, fact sampling contract, and Rust/GLSL port before streamed chunks can
replace the static review payload.

## Artifacts

- Exporter: `tools/dem_pack/export_godot_rough_world_chunks.py`
- Godot payload: `wg-10/worldgen_terrain/generated/review/rough_world_chunks_3x3.json`
- Review scene: `wg-10/worldgen_terrain/harness/rough_world_chunks_review.tscn`
- Godot scene smoke check: `wg-10/worldgen_terrain/tests/rough_world_chunks_review_check.gd`
- Contact-sheet renderer: `tools/dem_pack/render_rough_world_chunks_review.py`
- Tests: `tools/dem_pack/test_rough_world_chunks.py`
- External report: `D:\tmp\wg10_geography_engine\rough_world_chunks_3x3_seams.{csv,md}`
- Virtual-travel report: `D:\tmp\wg10_geography_engine\rough_world_chunks_virtual_travel.{csv,md}`
- Visual seam report: `D:\tmp\wg10_geography_engine\rough_world_chunks_visual_seams.{csv,md}`
- Static review sheet: `D:\tmp\wg10_geography_engine\rough_world_chunks_review_contact.png`
- Wider travel-review exporter: `tools/dem_pack/export_godot_rough_world_travel_review.py`
- Wider travel-review payload: `wg-10/worldgen_terrain/generated/review/rough_world_chunks_travel_5x5.json`
- Wider travel-review scene: `wg-10/worldgen_terrain/harness/rough_world_travel_review.tscn`
- Wider travel-review smoke check: `wg-10/worldgen_terrain/tests/rough_world_travel_review_check.gd`
- Wider travel-review report: `D:\tmp\wg10_geography_engine\rough_world_chunks_travel_5x5.{csv,md}`
- Wider travel-review contact sheet: `D:\tmp\wg10_geography_engine\rough_world_chunks_travel_5x5_contact.png`

## What It Proves

- Deterministic bounded export from seed + world origin + chunk span + generator version.
- Chunks are tagged `source=independent_window`: each chunk is built from its own
  world-coordinate skeleton window, not by slicing one stretched height export.
- Two visible seed examples: `133` and `211`, switchable in-scene with `T`.
- A 3x3 layout of adjacent 25.6 km chunks.
- Review scene has default-off seam inspection aids: `B` toggles cyan seam
  guide lines, and `N` jumps the fly camera to the next shared border so the
  owner can inspect boundaries deliberately, then turn guides off for a natural read.
- Godot scene smoke check instantiates the actual review scene and verifies
  runtime payload/mesh construction: 9 chunk meshes, 12 seam guides, 2 seed
  worlds, default-off seam guides, and `N`/next-seam focus enabling guides.
- Static contact sheet renders both seeds in terrain, terrain+seam-guide,
  corridor-mask, and slope-band views from the same Godot payload. This helps
  quickly scan for repeated stamps and obvious border artifacts before flying.
- Owner visual review of the opened Godot scene did not find visible seam
  problems in the bounded 3x3 proof.
- Adjacent chunks are not repeated copies. Reported mean absolute center/east chunk deltas:
  - seed 133: `0.2247`
  - seed 211: `0.3854`
- Different seeds produce materially different worlds. Center chunk seed-pair mean absolute delta:
  - `133 -> 211`: `0.3959`
- Shared chunk-border height deltas are exact in the generated payload:
  - max abs height delta: `0.000000`
- Corridor continuity is non-vacuous and mostly continuous by connected-component seam check:
  - minimum structural-corridor seam match fraction: `0.917`
- The review scene uses one-sample aprons for chunk-edge normals, reducing visual shading seams at shared edges.
- The corridor overlay now prefers the exported routed/route mask when present,
  instead of only the old low-height/passable-slope heuristic.
- Offline visual seam audit mirrors the Godot review mesh's edge height, normal,
  slope, default terrain-color, and corridor-edge math. Current 3x3 report:
  height delta `0.0000 m`, normal max angle `0.0000 deg`, slope max delta
  `0.000000`, terrain color max delta `0.000000`, corridor mismatches `0`
  across all shared edges for both seeds.
- Same world coordinate is independent of request origin: focused tests build the
  same chunk through two different origin/chunk-index requests and assert the
  height, apron, and corridor arrays match.
- A wider 5x5 virtual-travel stress probe now builds independently generated
  chunks over 128 km for both seeds. It is not rendered in Godot, but it checks
  whether the independent-window contract holds beyond the review scene:
  - seed 133: 40 seams, height max `0.000000`, corridor min `0.971`, adjacent median delta `0.3481`, max adjacent corr `0.3411`
  - seed 211: 40 seams, height max `0.000000`, corridor min `1.000`, adjacent median delta `0.3708`, max adjacent corr `0.3892`
- A separate 5x5 Godot travel-review scene now renders the wider 128 km area for
  owner terrain/travel judgement. It uses lower per-chunk mesh density (`65x65`
  vertices) to keep the 25-chunk scene flyable as a review artifact.
  - chunks: `5x5`, world span: `128.0 km`, chunk_n: `65`
  - shared seam rows: `80`
  - height max abs delta: `0.000000`
  - corridor min match fraction: `0.905`
  - normal max angle: `0.0000 deg`
  - corridor edge mismatches: `0`
  - adjacent pair median delta: `0.3591`
  - adjacent max correlation: `0.4874`

## What It Does Not Prove

- It does not prove the final Rust/GLSL runtime.
- It does not prove arbitrary infinite travel, cache eviction, or authority-window
  handoff across many windows.
- It does not prove full hydrology, gameplay navmesh quality, or route desirability.
- It does not prove full owner terrain acceptance; only seam visibility in the
  bounded 3x3 review scene has been accepted.
- It does not prove long-distance travel pacing. A 76.8 km-wide 3x3 proof is enough for seam review, not for travel-loop acceptance.
- The 5x5 virtual-travel report and 5x5 Godot travel-review scene support the
  infinite-world direction and owner travel-scale review, but they do not prove
  live streaming, cache eviction, player-speed pacing, or final visual
  desirability during travel.
- The contact sheet is top-down/static. It does not prove in-motion feel,
  oblique readability, or player-scale travel pacing.
- It does not prove the legacy `geography_skeleton.compose_height` path is safe
  when run as isolated 25.6 km windows; that diagnostic still fails and is kept
  in the report on purpose.

## Why The Boundary Exists

The legacy rough-highlands path is seeded and world-coordinate aware, but it still
uses window/span-local operations in the earlier review generator path. Examples
include coarse skeleton span selection, local normalization, and final review
conditioning. If each 25.6 km chunk is generated independently through that path,
edges are not reliable.

The current exporter avoids that specific failure by using the Phase 7B-lite
world-window facts (`geography_skeleton_windows.py`) and a fixed
world-coordinate height/corridor composition with no per-window normalization.
The routed skeleton supplies broad uplift/channel facts; deterministic
world-coordinate route texture is also carved into height and exported as the
structural corridor mask so visible routes are a seed+coordinate function.

This is now quantified in the report's independent-window diagnostic. Running
the legacy keeper as separate adjacent 25.6 km windows produces nonzero seam
deltas:

- seed 133, x-neighbor: conditioned max delta `0.6614`, mean delta `0.2275`
- seed 133, z-neighbor: conditioned max delta `1.4417`, mean delta `1.0738`

Those numbers are not acceptable as a chunk seam. They are the reason the current
proof should be read as a bounded super-window review artifact, not as the final
infinite-window contract.

For a true infinite-in-all-directions implementation, the generator must be
defined over deterministic world windows with aprons and a clear authority rule:
same seed + same world coordinate + same generator version must produce the same
height/facts regardless of which page/window requested the sample.

## Infinite / Player-Travel Review

With the bounded 3x3 seam read accepted, the next review should answer:

1. **Authority model:** which window owns a sample, how wide is the apron, and how are facts cropped?
2. **Sampling contract:** can height, corridor facts, and material descriptors be sampled consistently by render, collision, and AI?
3. **Travel pacing:** at expected player speeds, how much terrain is visible before repetition or boredom appears?
4. **Streaming shape:** what is the smallest viable window/cache system for 25.6 km playable chunks without loading a whole 3x3 super-window every time?
5. **Continuity gates:** exact height seams, bounded normal/material seams, corridor continuation, deterministic seed variation, and no repeated stamps.
6. **Owner visual gate:** fly across boundaries at terrain view, corridor overlay, and oblique/overview scales.
   Use `B`/`N` to deliberately locate seams, then disable guides to judge whether the terrain itself reveals them.

Do not start a Rust/GLSL runtime port until those answers are accepted for the keeper.

The current immediate review artifact for this decision is
`wg-10/worldgen_terrain/harness/rough_world_travel_review.tscn`. Use it to judge
whether the rough-highlands keeper holds together across 128 km of adjacent
chunks, whether the routes/corridors feel legible enough, and whether the
terrain has enough variation without exposing chunk structure.

## Verification

Last focused verification for the proof:

```text
python -m pytest tools\dem_pack\test_rough_world_chunks.py tools\dem_pack\test_rough_world_traversability.py tools\dem_pack\test_geography_skeleton.py tools\dem_pack\test_geography_skeleton_windows.py -q
31 passed
```

Latest focused verification for the wider 5x5 travel-review artifact:

```text
python -m pytest tools\dem_pack\test_rough_world_chunks.py tools\dem_pack\test_rough_highlands_keeper_contract.py tools\dem_pack\test_geography_skeleton_windows.py -q
22 passed
```

The focused chunk test file includes independent-window source, seam,
seed-variation, corridor-edge, and legacy diagnostic assertions; run by itself it reports:

```text
python -m pytest tools\dem_pack\test_rough_world_chunks.py -q
9 passed
```

Godot import evidence:

```text
Godot --headless --import --path wg-10
exit 0, no GDScript parse errors
```

Godot review-scene smoke evidence:

```text
Godot --headless --path wg-10 --script res://worldgen_terrain/tests/rough_world_chunks_review_check.gd
[wg10-rough-chunks-review] status=pass chunks=9 seam_guides=12 seeds=2
```

```text
Godot --headless --path wg-10 --script res://worldgen_terrain/tests/rough_world_travel_review_check.gd
[wg10-rough-travel-review] status=pass chunks=25 seam_guides=40 seeds=2
```

Known caveat: a separate headless scene-run attempt crashed Godot after a
`user://logs` write failure. Do not use that as terrain evidence. The visible
Windows review scene is the intended owner gate.
