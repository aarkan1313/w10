# WorldGen10 — Component Inventory & Prove-One-At-A-Time Plan

Created 2026-05-29 after the M3 render layer turned into "a mess" (stacked slices 4→8 without
proving the surface was continuous live). **Method: enumerate every part, then activate + PROVE
each one in dependency order, on-screen, before trusting the next.** The owner flies/approves
each step — automated gates assist but are NOT the authority (a green gate has twice masked a
defect the live fly caught).

> This is a temporary working doc to drive the reset. When the render layer is proven end to
> end, fold the outcome into STATUS/ROADMAP and delete this (the 3-living-docs rule).

## CONFIRMED ROOT CAUSE (step 2, owner-verified 2026-05-29)
**The page texture samplers defaulted to REPEAT wrap.** A tile edge vertex at `uv=1.0` wrapped
to sample the page's OPPOSITE edge → a wrong height exactly at every tile boundary → the visible
seams / "overlapping offset sheets". Heights and mesh placement were always correct (CPU model
matched to 0.00025 m); the bug was purely the GPU sampler wrap mode. FIX: declare the page
samplers `filter_linear, repeat_disable` (clamp-to-edge) in `ring_displace.gdshader`. Owner
confirmed two pages now join as one continuous landmass. This is why the slice-8 "seam=0" data
gate missed it — it compared texel DATA (correct), not the wrapped GPU SAMPLE (wrong). This bug
was corrupting EVERY tile boundary in the full clipmap.

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

1. **One page, flat.** Pool.acquire_page ONE level-0 page → one MeshInstance (full grid, span_l)
   with ring_displace, morph OFF, no streamer. Fly over it. PROVE: single continuous tile, real
   relief, stable, no internal cracks. (Proves page_compute + pool + shader fine-sample alone.)
2. **Two adjacent pages.** Add the +X neighbor page+tile. PROVE: the shared edge is continuous in
   the rendered surface under a moving camera (this is the seam that the data-gate said =0; prove
   it LIVE). If it cracks here, the bug is tile placement/UV, isolated to 2 tiles.
3. **A 3×3 of one level.** PROVE: 9 tiles, one surface, no internal grid. (Isolates G2.)
4. **Streamer drives the 3×3.** Add streamer + get_resident + fallback. Fly. PROVE: no churn (HUD
   recomputed stable), no flicker, fallback (if any) is not a wrong-height tile.
5. **Second level as a HOLLOW ring.** Add level 1 as an annulus that fills ONLY outside level 0
   (fix G1 — use band_mesh hollow path). PROVE: no overdraw, no double-surface, seam at the L0/L1
   boundary continuous.
6. **Geomorph at the L0/L1 boundary.** Turn morph ON only at that real boundary. PROVE: smooth
   LOD transition, no lattice.
7. **Third level + full speed.** PROVE: p99 < 6ms AND visually clean at ~1000 m/s (the real M3
   acceptance — owner-flown).

Each step that reveals a gap gets fixed and re-proven before moving on. Steps 5–6 will likely
require real changes to clipmap_rings (hollow rings) — that's the expected G1 fix surfacing.
