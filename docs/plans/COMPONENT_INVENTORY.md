# WorldGen10 — Component Inventory & Prove-One-At-A-Time Plan

Created 2026-05-29 after the M3 render layer turned into "a mess" (stacked slices 4→8 without
proving the surface was continuous live). **Method: enumerate every part, then activate + PROVE
each one in dependency order, on-screen, before trusting the next.** The owner flies/approves
each step — automated gates assist but are NOT the authority (a green gate has twice masked a
defect the live fly caught).

> This is a temporary working doc to drive the reset. When the render layer is proven end to
> end, fold the outcome into STATUS/ROADMAP and delete this (the 3-living-docs rule).

## FIXED — frustum-cull of GPU-displaced tiles (was the rotation-vanish AND creep-blink)
Owner reported tiles vanishing on rotation, and (later, reproducible) a chunk blinking in/out when
creeping slowly toward a boundary and stopping. SAME root cause: flat (y=0) tile meshes + GPU
vertex displacement → Godot frustum-culls on the flat AABB → a tile whose displaced terrain is
on-screen gets culled when its flat box leaves the frustum. Data-residency probes showed 0
never-black holes, which correctly pointed at RENDERING, not residency. FIX (done):
`Wg10ClipmapRings::configure` sets a custom AABB per tile (full XZ footprint + ±8000 m Y for
worst-case z-score DEM displacement). Probe: terrain persists 100% across a rotation sweep; m3 6/6
green (accept p99=1.87 ms — even cheaper). Lesson: GPU-displaced meshes ALWAYS need a custom AABB.

## CONFIRMED ROOT CAUSE (step 2, owner-verified 2026-05-29)
**The page texture samplers defaulted to REPEAT wrap.** A tile edge vertex at `uv=1.0` wrapped
to sample the page's OPPOSITE edge → a wrong height exactly at every tile boundary → the visible
seams / "overlapping offset sheets". Heights and mesh placement were always correct (CPU model
matched to 0.00025 m); the bug was purely the GPU sampler wrap mode. FIX: declare the page
samplers `filter_linear, repeat_disable` (clamp-to-edge) in `ring_displace.gdshader`. Owner
confirmed two pages now join as one continuous landmass. This is why the slice-8 "seam=0" data
gate missed it — it compared texel DATA (correct), not the wrapped GPU SAMPLE (wrong). This bug
was corrupting EVERY tile boundary in the full clipmap.

## The "lines / blue squares" are EXTREME DEM DATA, not a render bug (diagnosed 2026-05-29)
The owner's step-6 fly showed hard-edged blue flat patches / lines. Traced rigorously:
- NOT the morph (identical with morph off), NOT a flat/failed page (pages have real relief), NOT
  a coarse seam (abutting coarse pages' shared edge matches to 0.0013 m).
- ROOT: the `dem_v1` gate pack's height field is pathologically spiky — sampling it across the
  view showed deep-low (`#`) directly adjacent to high (`^`) everywhere, with ~450 m height jumps
  over 500 m (near-vertical cliffs). The deep BLUE is just real LOW elevation in the height->color
  debug map. The hard "square" edge is a real near-vertical DEM feature, amplified because the
  COARSE mesh (64 m/texel) can't represent a cliff smoothly -> renders it as a flat facet + hard
  edge. (Cf. memory `worldgen-dem-kernel-normalization`: DEM kernels are z-score normalized;
  height legitimately goes very negative/positive.)
- CONCLUSION: the render pipeline is CORRECT here; the artifact is CONTENT (the placeholder DEM
  pack is extreme). Fixes live in the data/material layers, NOT the clipmap: a saner height pack
  / clamping relief, real materials+normals (M6) that hide facet-scale steps, and erosion (M7)
  that smooths cliffs. Do NOT "fix" this in the render layer.

## Mechanism vs policy (owner clarified, 2026-05-29) — what's fixed vs what's tunable
The owner asked: "long term we'll have adjustable size/detail/chunk-count — what's best?" The
answer is a clean split:
- **MECHANISM (architecture, pillar-fixed, NOT a preference):** a coarser clipmap level is ALWAYS
  resident under the finer one, so any unready fine tile falls back to coarse-but-correct terrain
  — never a hole, never a wink. This IS never-black (Pillar 3, never-collapse); it's the
  structural defense against WG9's black-slab death. Step 5 builds it unconditionally.
- **POLICY (config, Pillar 1 "no magic numbers", tuned later vs real assets):** page span,
  number of levels, resident capacity (how many chunks stay loaded), lead_seconds (pre-fetch
  distance), max_per_frame. These are ALREADY config args to `configure` — we just haven't picked
  good defaults yet (no real content). The "adjustable size/detail/chunk-count" the owner wants is
  exactly these, and the architecture already supports them being knobs.
So: build the never-black fallback now (pillars decide it); the size/detail/capacity stay the
adjustable config they already are.

## Scale / look is DELIBERATELY a stand-in at this stage (owner asked, 2026-05-29)
What looks "off-scale" now is test scaffolding, resolved later on the roadmap — NOT a bug to fix
in the render reset:
- `HEIGHT_SCALE=0.35`, `BASE_SPAN=8192` (8.2 km/finest page), `GRID_RES=64` (128 m triangles) are
  proving-ground constants chosen to see shape, not final values. All are CONFIG (pillar 1: no
  magic numbers) — tuned against real assets once M5/M6 exist.
- Blue/yellow gradient is debug coloring (height→color); real materials/normals come in M6.
- The blobby/coarse look = missing high-frequency detail (M5) + no textures (M6) + un-eroded
  large forms (M7). The `dem_v1` pack is a gate subset placeholder.
We are proving STRUCTURE (continuity, streaming, LOD) now; scale + look are filled in by M5–M7.

## The two suspected gaps (hypotheses to confirm, not facts yet)

- **G1 — levels overdraw (not hollow rings).** `clipmap_rings::configure` builds EVERY level as
  a FULL `grid_res` grid (`band_mesh(level=0)` for all 3 levels) → level 0,1,2 each draw a solid
  3×3 stacked by render_priority. The clipmap intent is HOLLOW rings (coarse fills only the
  annulus the finer level doesn't cover). Full overlap → "overlapping offset sheets" look.
- **G2 — fallback / per-tile height mismatch → grid cracks.** Adjacent tiles at different heights
  (one fine, one coarse-fallback, or page-edge mismatch) render a wall at every tile boundary in
  a perfect grid. Even with page-data seam=0, the *rendered* surface cracks.

## Components, in dependency order (leaf → top)

Legend: ✅ proven & trusted · 🟡 has a gate but NOT proven on-screen · ❓ suspected gap · ⬜ not built

### Tier 0 — deterministic CPU bedrock (LEAF, pure, no godot)
- [✅] **hash** (`hash.rs`) — FNV-1a stable_hash/hash_grid/value_noise/fbm. Gate: `hash_parity_check` (bit-exact vs WG9 fixture). Trusted.
- [✅] **grammar** (`grammar.rs`, `Wg10Grammar`) — palette/family rolls. Gate: `grammar_check` + `determinism_check`. Trusted.
- [✅] **height** (`height.rs`, `Wg10Height`) — `height(x,z)` CPU field. Gate: `height_check`. Trusted.
- [✅] **pack** (`pack.rs`) — terrain-pack v1 load/validate. Gate: `dem_pack_check`. Trusted.

### Tier 1 — GPU formula parity
- [✅] **gpu_compute** (`gpu_compute.rs`, `Wg10GpuCompute`) — `height_field.glsl`, CPU/GPU parity. Gates: `gpu_parity_check`, `gpu_parity_dem_check` (families EXACT, height within eps). Trusted.

### Tier 2 — page production & ownership
- [🟡] **page_compute** (`page_compute.rs`, `Wg10PageCompute`) — `height_page.glsl` → R32F page; cached context. Indirectly exercised by pool gate. GAP: no direct "one page renders correct, continuous relief" on-screen proof since slice 1.
- [✅] **page_policy** (`page_policy.rs`, pure) — LRU + protected + budget + zero-churn eviction. Gate: `page_policy_tests` (11 unit). Trusted (pure).
- [🟡] **page_pool** (`page_pool.rs`, `Wg10PagePool`) — single RID owner, acquire/release/get_resident. Gate: `m3_pool_check`. Trusted-ish; texel-corner gen (slice 8) needs a fresh single-page eyeball.

### Tier 3 — scheduling
- [✅] **schedule_policy** (`schedule_policy.rs`, pure) — coverage/coarser_fallback/plan_frame, coarsest-first, never-black, single led ring + `coverage_center`. Gate: `schedule_policy_tests`. Trusted (pure).
- [🟡] **streamer** (`streamer.rs`, `Wg10Streamer`) — frame-loop driver. Gate: `m3_stream_check` (never-black under motion). Trusted-ish.

### Tier 4 — geometry
- [✅] **ring_geometry** (`ring_geometry.rs`, pure) — `band_mesh` full grid / hollow annulus. Gate: `ring_geometry_tests`. Pure & trusted — BUT clipmap_rings only ever calls it with `level=0` (full grid), so the HOLLOW-ring path is built+tested but UNUSED. (Root of G1.)

### Tier 5 — render presentation (THE MESS — to rebuild & prove step by step)
- [❓] **clipmap_rings** (`clipmap_rings.rs`, `Wg10ClipmapRings`) — N×9 tiles, placement+sampling, render_priority. Gate `m3_view_check` passes but the LIVE fly shows grid cracks + overdraw (G1, G2).
- [❓] **terrain_view** (`terrain_view.rs`, `Wg10TerrainView`) — live coordinator; fallback + led-center logic. Two green-gate-but-wrong fixes already.
- [🟡] **ring_displace.gdshader** — world-UV fine sample + neighborhood-center geomorph + texel-corner. Math proven in isolation; live result is the crack mess.

## Prove-one-at-a-time sequence (each step: a scene + ON-SCREEN proof the owner approves)

A single growable scene (`proving_ground.tscn`) with a fly camera + HUD; each step flips on one
more component and the owner confirms before the next.

1. **[DONE ✅ owner-confirmed] One page, flat.** Single continuous tile, real relief, no cracks.
2. **[DONE ✅ owner-confirmed] Two adjacent pages.** FOUND the root bug: page sampler defaulted to
   REPEAT → edge vertices wrapped to the opposite page edge → seam. Fixed with `filter_linear,
   repeat_disable` (clamp-to-edge). This was corrupting EVERY tile boundary.
3. **[DONE ✅ owner-confirmed] Static 3×3 of one level.** One continuous surface, no internal grid.
4. **[DONE ✅ owner-confirmed] Streamer drives the 3×3.** FOUND the lead bug: `lead_frames` × m/s
   velocity, unclamped → up to 64 km lead at sprint → ring flew off the camera. Fixed: `lead_seconds`
   + clamp in `coverage_center` (camera always in-ring); view reads the clamped centre from the
   streamer. Probed: 0 churn, camera-in-ring 99.9%.
5. **[DONE ✅ owner-confirmed] Second level = never-black coarse blanket.** Built as a FULL coarse
   3×3 drawn UNDER the fine 3×3 (NOT hollow — see note), always resident; an unready fine tile shows
   coarse through. Probed: 0 holes, edge winks all masked by coarse. (Owner: "better".)
6. **[DONE — probe-clean, owner saw residual = DEM data] Geomorph the L0/L1 boundary.** The step-5
   "LOD line" was the morph being OFF (each tile bound its own page as the morph target = nothing to
   blend). Fixed: each fine tile binds the REAL coarse parent page + the fine-neighborhood centre,
   blends fine→coarse over the outer band. The remaining "blue squares/lines" the owner sees were
   traced to EXTREME DEM DATA, not the render (see the diagnosis box up top).
7. **[NEXT] Third level + full speed.** PROVE: p99 < 6 ms AND visually clean at ~1000 m/s — the real
   M3 acceptance, owner-flown.

**HOLLOW-RING decision (re G1):** I did NOT make the coarse level hollow. The proving-ground draws
a FULL coarse 3×3 under the fine 3×3 (finer on top via render_priority). This is correct and
never-black (coarse always covers); the overdraw cost is bounded (a few extra full-3×3 draws) and
measured against the p99 budget at step 7. Hollow rings are a perf optimization, not a correctness
requirement — defer until step 7 shows p99 needs it. (`ring_geometry::band_mesh` hollow path stays
available, currently unused.) G1 "overdraw" was thus a non-issue once the REPEAT-seam (the real
cause of the "overlapping sheets") was fixed; G2 "grid cracks" was the same REPEAT-seam bug.

**FOLD-BACK (after step 7):** the rebuilt logic currently lives in `proving_ground.gd` (per-step
`_build_stepN`/`_drive_stepN`). Once step 7 is proven, port the final drive into the real
`Wg10TerrainView` + `Wg10ClipmapRings`, update the shader sampler hints there, and reconcile the
m3 gates (m3_accept/continuity/view/stream) to the rebuilt path before M3 acceptance.
