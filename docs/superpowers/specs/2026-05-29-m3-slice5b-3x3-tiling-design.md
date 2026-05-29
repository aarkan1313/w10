# WorldGen10 M3 — Slice 5b: 3×3 Ring Tiling + Rings↔Streamer Live Wiring Design

**Date:** 2026-05-29
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** Render pipeline (M3) — slice 5b: each clipmap level renders a 3×3 page neighborhood that surrounds the camera, wired to the streamer in a live loop and proven under motion
**Builds on:** slice 4 (Wg10ClipmapRings + L↔L+1 geomorph), slice 5a (per-level page span, geomorph `coarse_origin`, read-only `get_resident_page` — all landed & proven)
**Followed by:** M3 close-out slice (WASD/mouse fly camera + diagnostics/p99 overlay + the renderer p99<6ms acceptance gate at ~1000 m/s + manual fly — finishes M3)

---

## 0. Framing

The slice-5a moving gate proved that **"one band = one page" does not surround the camera**:
a single page `[origin, origin+span]` with `origin = floor(cam/span)*span` *contains* the
camera but not symmetrically, so under motion the camera sits near a page edge and ~75% of a
camera-centered view falls off the page (gate: nonblack → 0.25). A clipmap level must
**surround** the camera at any position.

The fix is the standard clipmap shape: each level renders a **3×3 page neighborhood**
centered on the camera's current page (`radius_pages = 1` — exactly the coverage
`SchedulePolicy::coverage` already emits). This slice rebuilds `Wg10ClipmapRings` from N
single meshes to **N levels × 9 page tiles**, rebuilds the `Wg10TerrainView` live-loop
coordinator to drive them via the read-only `get_resident_page` accessor (the anti-WG9
render-path rule, landed in 5a), and proves the result with a moving-sweep gate that renders
at non-zero camera positions — the thing slice 4's static capture could not test.

The three 5a fixes (per-level page span, geomorph `coarse_origin`, read-only
`get_resident_page`) are reused as-is — they are landed and proven. This slice is the tiling
+ wiring; the fly camera + the p99 acceptance gate are the next (M3-closing) slice.

---

## 1. Scope

**In scope (5b):**
- Rebuild **`Wg10ClipmapRings`**: N levels × 9 tiles (a 3×3 grid of one-page meshes per
  level). Each tile is a full `grid_res × grid_res` grid spanning one page, with its own
  `ShaderMaterial(ring_displace)`. Levels **overlap** (coarse keeps its full 3×3; the finer
  level's 3×3 draws on top; the geomorph blends at the finer's outer edge). Finer draws on
  top via `render_priority` derived from level.
- Per-tile bind + place: `bind_tile(level, dx, dz, …)` places tile `(dx,dz)`'s mesh at its
  page's world corner and binds that page's texture (+ coarser neighbor for the morph).
- Rebuild **`Wg10TerrainView`** for 3×3: each frame `streamer.update`, then per level per
  tile fetch the page via **read-only `get_resident_page`** (coarser fallback on miss —
  never computes on the render path), and `bind_tile`.
- **`m3_view_check.gd`** moving-sweep gate: full coverage + seam continuity + no z-fight in
  the overlap + never-black + view-zero-compute + tile↔page mapping, at several non-zero
  camera positions.
- Retire the slice-4 `m3_rings_check.gd` (its one-page-band geometry is removed; the 3×3
  view gate supersedes it).

**Reused from 5a (landed, proven — NOT rebuilt):** per-level page span in
`Wg10PagePool::acquire_page`; geomorph `coarse_origin` uniform + corner-relative coarse
sample in `ring_displace.gdshader`; read-only `Wg10PagePool::get_resident_page` +
`PagePolicy::slot_of`.

**Out of scope (next slice — finishes M3):** WASD/mouse free-fly camera + movement
controller; diagnostics/profiling/UI overlay (live fps + p99 + stats); the renderer
**p99 < 6 ms acceptance gate at ~1000 m/s** + manual fly.

**Performance honesty (pillar 2):** the overlap means real, **bounded, fixed** overdraw (the
finer 3×3 over part of the coarse center). This slice does NOT claim it is free — it records
the overlap overdraw as an explicit input to the next slice's p99<6ms acceptance gate, and
proves *correctness* (no z-fight) here. Perf is judged with the real fly camera, next slice.

---

## 2. Tile geometry & `Wg10ClipmapRings` rebuild

### 2.1 Geometry: every tile is a full one-page grid

A tile is a full `grid_res × grid_res` centered grid spanning one page (`span_L =
base_span·2^L`) — the same mesh the slice-1 path displaces. **No hollow bands.** The old
`ring_geometry::band_mesh` hollow-annulus path is no longer used by the rings (it was for
one-page-per-level); the rings build a full grid per tile (which is `band_mesh`'s `level==0`
no-hole case — `ring_geometry` itself is unchanged and still tested; the rings just stop
calling the hollow path). The `grid_res % 4` guard still applies.

A level is **9 tiles** at fixed relative offsets `(dx,dz) ∈ {-1,0,+1}²`. `Wg10ClipmapRings`
holds `num_levels × 9` `MeshInstance3D`s, each with its own `ShaderMaterial`. All persistent,
created once in `configure`, never rebuilt.

**Per-tile meshes (no sharing).** Each tile gets its own `ArrayMesh` (27 small meshes at 3
levels). We deliberately do NOT share one mesh resource across a level's 9 tiles — the
micro-optimization risks a correctness assumption (per-instance transform/material vs shared
geometry) for no measured gain (no shortcuts). 27 small meshes is cheap.

### 2.2 Levels overlap; finer draws on top

The hollow-out math doesn't land on tile boundaries (a finer level's 3×3 spans
`3·span_{L-1} = 1.5·span_L`, a fractional number of coarse pages), so we do NOT hollow the
coarse center. Instead:
- The coarse level keeps its **full 3×3**.
- The finer level's 3×3 **overlaps** the coarse center and **draws on top**.
- The geomorph (5a) blends the finer level toward the coarse surface at the finer's **outer
  edge**, so they agree where they meet.

Coverage is gapless by construction (the coarse level covers everything; the finer refines
the center). Cost is fixed (27 tiles), with bounded overlap overdraw (§1 performance note).

**Draw order — `render_priority`.** Finer levels must win in the overlap region. Each tile's
`ShaderMaterial.render_priority` is derived from level (config-driven, not a magic literal):
e.g. `priority = num_levels - 1 - level` so level 0 (finest) has the highest priority and
renders last/on-top, the coarsest renders first. (Depth-test alone cannot resolve this — the
finer and coarse sample the same height field at the seam, so they are coplanar in the
overlap → pure z-fight; explicit `render_priority` is required.)

### 2.3 `configure` + accessors

```
configure(num_levels, base_span, grid_res, shader_path)
```
Build-once guard + `grid_res % 4` guard (as today). For each `level`, for each `(dx,dz)`:
build a full-grid tile mesh at span_L, create MeshInstance3D + ShaderMaterial (with
`render_priority` by level), parent it, store indexed by `(level, dx, dz)`.

Accessors (for the gate):
- `level_count() -> int`, `tile_count() -> int` (= num_levels·9),
- `total_vertex_count() -> int` (the recenter-no-rebuild check),
- `bound_page_key(level, dx, dz) -> Vector2i` (the last page origin bound to that tile — the
  CPU tile↔page mapping check).

### 2.4 `bind_tile` — place + bind one tile

Replaces the old per-level `bind_page` + `recenter`. The view computes the tile↔page math
(§3) and calls:
```
bind_tile(level, dx, dz,
          height_tex, coarse_tex,
          span_l, coarse_span,
          height_scale, morph_region, relief_ref,
          page_origin_x, page_origin_z,        # this tile's page world corner
          coarse_origin_x, coarse_origin_z)    # the coarser page's world corner (for the morph)
```
It (a) sets the tile mesh instance's transform origin to `page_origin + span_l/2` (the
centered mesh then covers world `[page_origin, page_origin+span_l]`), and (b) sets the
material uniforms (`height_tex`, `coarse_height_tex`, `world_span`, `coarse_span`,
`height_scale`, `morph_region`, `relief_ref`, `coarse_origin`) — the existing slice-4/5a
uniform set, now per tile. It records `page_origin` for `bound_page_key`.

**File:** `clipmap_rings.rs` rewritten (one file, under the 600-line cap — 27 instances is
more bookkeeping, not more per-tile logic).

---

## 3. `Wg10TerrainView` rebuild for 3×3 + the live loop

`Wg10TerrainView` (Node3D) drives the rings. `update(camera_x, camera_z, vel_x, vel_z)`:

1. `streamer.update(cam, vel)` — bounded stream-ahead; sole page producer; coarsest-first;
   keeps the blanket resident (slice 3).
2. For each `level` in `0..num_levels`:
   - `span_L = base_span·2^level`; center page origin
     `center = (floor(cam_x/span_L)·span_L, floor(cam_z/span_L)·span_L)` — the **shared
     page-key convention** (= `SchedulePolicy::page_origin`).
   - coarser level span `span_C = base_span·2^(level+1)` (for the coarsest level,
     `span_C = span_L`, coarse = self → morph disabled).
   - For each tile `(dx,dz) ∈ {-1,0,+1}²`:
     - page origin `po = (center_x + dx·span_L, center_z + dz·span_L)`.
     - `tex = pool.get_resident_page(level, po_x, po_z)` — **read-only, never computes.**
     - coarser neighbor: the level-(L+1) page containing this tile's centre
       `tc = po + span_L/2`; `co = (floor(tc_x/span_C)·span_C, floor(tc_z/span_C)·span_C)`;
       `coarse_tex = pool.get_resident_page(level+1 (or level at coarsest), co_x, co_z)`.
     - **never-black fallback:** if `tex` is null → use `coarse_tex` as the height tex with
       morph 0 (the streamer keeps the coarse blanket resident). If BOTH null → leave the
       tile bound to its previous page (stale-but-bounded; the gate's never-black assertion
       catches a true coverage gap). Never trigger a compute.
     - `morph = (level < num_levels-1 && tex is the fine page) ? morph_region : 0.0`.
     - `rings.bind_tile(level, dx, dz, height_tex, coarse_tex, span_L, span_C, height_scale,
       morph, relief_ref, po_x, po_z, co_x, co_z)`.

There is **no separate `recenter`** — placing each tile per frame *is* the recenter; tiles
are persistent (transforms + uniforms change, geometry never rebuilt).

**Key convergence (what 5a lacked):** the view queries exactly the 3×3 page keys per level
that `SchedulePolicy::coverage(radius_pages=1)` emits, using the same `floor(cam/span)·span`
origin. So the streamer makes resident exactly the pages the view looks up → lookups hit,
fallback only fills genuine not-yet-streamed gaps. Scheduler, pool, rings, and view share ONE
key convention.

**Interface:** rings expose `bind_tile(level, dx, dz, …)`; the view owns the tile↔page math;
the rings own the mesh instances + apply transform/uniforms. The view owns no RIDs, no meshes,
no scheduling math.

---

## 4. Gate: `m3_view_check.gd` (`m3` suite, WINDOWED)

Drives the rebuilt `Wg10TerrainView` over a scripted +x sweep across page boundaries. At each
non-zero position: render top-down **orthographic centered on the camera**, framed to ~1 page
(so the 3×3 fills the view), capture, and assert:

1. **Full coverage (headline fix):** `nonblack_frac ≈ 1.0`. The 3×3 surrounds the camera —
   5a failed here at 0.25. This is THE assertion that proves the slice.
2. **Real relief:** distinct quantized colors ≥ threshold.
3. **Seam continuity:** across a level boundary (geomorph `coarse_origin` holds off-origin);
   no black gap, no hard color jump.
4. **No z-fight in the overlap band:** sample the finer/coarse overlap region; assert the
   finer surface wins cleanly (no stale-coarse pixels bleeding through) AND is **stable across
   two settled captures** at the same position (no per-frame flicker). render_priority working.
5. **Never-black:** pool residency non-empty + budget (`resident ≤ capacity`).
6. **View triggers zero compute:** hold position static for a few frames; assert
   `created+recomputed` stays flat (the read-only-accessor / anti-WG9 guarantee).
7. **Tile↔page mapping (CPU):** for one sampled position, assert
   `rings.bound_page_key(level, dx, dz) == (center + (dx,dz)·span_L)`.

Saves a PNG per position (`m3_view_<i>.png`) for eyeball confirmation. Non-vacuous: at least
one boundary crossing at non-zero camera (where 5a's bug manifested). Wire `m3_view_check.gd`
into the `m3` suite; retire `m3_rings_check.gd` (§5). `fast`/`gpu` unchanged.

---

## 5. Files

**Rewrite:**
- `wg-10/rust/src/clipmap_rings.rs` — N×9 tiles, `bind_tile`, `render_priority` by level,
  per-tile meshes, `level_count`/`tile_count`/`total_vertex_count`/`bound_page_key`.

**Recreate:**
- `wg-10/rust/src/terrain_view.rs` — the 3×3 live loop (§3). `mod terrain_view;` re-added to
  `lib.rs`.

**Create:**
- `wg-10/worldgen_terrain/tests/m3_view_check.gd` — the §4 moving-sweep gate.

**Modify:**
- `tools/gate.py` — `m3` suite: add `m3_view_check.gd`, remove `m3_rings_check.gd`.

**Remove:**
- `wg-10/worldgen_terrain/tests/m3_rings_check.gd` (+ its `.uid`) — its one-page-band geometry
  is gone; the 3×3 view gate supersedes its static single-page check. (Slice-1/2 still guard
  the core single-page sample path + the pool.)

**Unchanged (reused from 5a):** `page_pool.rs` (per-level span + `get_resident_page`),
`page_policy.rs` (`slot_of`), `ring_displace.gdshader` (`coarse_origin`).

**Soft cap:** rewritten files under the ~600-line cap.

---

## 6. Definition of done

- `Wg10ClipmapRings` renders N×9 tiles surrounding the camera; finer-on-top via
  `render_priority`; per-tile bind + place; persistent (never rebuilt).
- `Wg10TerrainView` drives them read-only (never computes); falls back to coarser on a miss;
  shares the `floor(cam/span)·span` key with the scheduler.
- `m3_view_check` passes at several NON-ZERO positions incl. a boundary crossing: full
  coverage (~1.0), real relief, seam continuity, no z-fight (stable overlap), never-black,
  zero view-compute, tile↔page mapping. PNGs eyeballed (terrain surrounds the camera + follows
  it across boundaries).
- `m3` suite green (5 checks: slice1, pool, stream, view, + the retiring of rings leaves the
  count at the implementer's tally — verify actual `checks=N fail=0`); `fast`/`gpu` unchanged;
  cargo green.
- STATUS + ROADMAP updated: 3×3 tiling + wiring DONE; the overlap overdraw recorded as an
  explicit input to the next slice's p99<6ms gate; next = fly-cam + overlay + p99 gate +
  manual fly (finishes M3). Honest baseline: 5b proves coverage-surrounds-camera + seamless +
  never-black under SCRIPTED motion; perf (p99) + interactive flight are the next slice.
- Each task committed separately.

---

## 7. Risks & mitigations

- **z-fight / flicker in the overlap band.** render_priority by level (explicit order, not
  depth-test). The gate's assertion #4 proves no stale-coarse bleed AND frame-stability — if
  it flickers, that's a real finding to fix (e.g. a small height bias or priority gap), NOT to
  weaken.
- **27 tiles × per-frame rebind cost.** Fixed and bounded, but real — recorded as an explicit
  input to the next slice's p99<6ms acceptance gate, not claimed free here. (If profiling
  later shows it matters, the toroidal-rebind optimization — only rebind the edge row/col on a
  boundary crossing — is the known lever; deferred until measured.)
- **Both-null fallback shows a stale tile.** Bounded (the previous frame's page); the
  never-black assertion catches a true gap. The streamer keeps the coarse blanket resident
  (slice-3 guarantee), so the coarse fallback almost always has a page; both-null is a cold
  start / extreme-teleport edge, transient.
- **Retiring m3_rings_check loses the static single-page regression.** Acceptable: the
  one-page-band geometry it tested is removed; the new moving gate tests the replacement more
  thoroughly (coverage + seam + no-z-fight under motion). Slice-1 (single-page render) and
  slice-2 (pool) still guard the core sample path + RID ownership.
- **Tile↔page key mismatch with the scheduler.** Both use `floor(cam/span)·span` + the 3×3
  offsets; the gate's mapping assertion (#7) + the coverage~1 assertion (#1, would drop if the
  view queried keys the streamer didn't make resident) catch a divergence.
