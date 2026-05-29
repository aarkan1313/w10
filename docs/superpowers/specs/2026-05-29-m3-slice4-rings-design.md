# WorldGen10 M3 — Slice 4: Clipmap Rings Design

**Date:** 2026-05-29
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** Render pipeline (M3) — slice 4: concentric clipmap ring meshes that turn the scheduler's page coverage into seamless visible terrain
**Builds on:** M3 slice 1 (Wg10PageCompute → R32F Texture2DRD → `ring_displace` vertex displace), slice 2 (Wg10PagePool single RID owner), slice 3 (SchedulePolicy + Wg10Streamer: velocity-aware coverage, coarsest-first never-black)
**Followed by:** M3 slice 5+ (fly-test harness: WASD/mouse camera + diagnostics overlay + the renderer p99<6ms acceptance gate at ~1000 m/s)

---

## 0. Framing

DESIGN §5.1 specifies the renderer as a **fixed set of N concentric square clipmap
rings centered on the camera** — ring 0 finest, each outer ring doubling spacing and
area, persistent meshes created once and never rebuilt, movement = recenter (translate)
not rebuild. §5.4's frame loop ends with "rings render: sample resident page, else
coarser fallback; shader morphs L↔L+1."

Slices 1–3 built everything *behind* the rings: the page producer, the bounded
single-owner pool, and the velocity-aware scheduler that keeps the pool fed under
motion and never goes black. But coverage so far is **abstract page keys** — nothing
is drawn beyond the slice-1/2 single-page render gates. This slice builds the rings:
the geometry that consumes the scheduler's coverage and renders it as seamless terrain.

It is a **visual + geometry-correctness** slice. The headline quality claim is
**no holes and no cracks** between ring levels — the thing a naive multi-resolution
mesh gets wrong (T-junction gaps where densities differ). The fix is the L↔L+1
geomorph.

---

## 1. Scope

**In scope (slice 4):**
- **`Wg10ClipmapRings`** (godot) — owns N persistent ring meshes (one grid per level),
  their materials, recenter-on-move, and per-frame page rebinding. Owns NO RIDs (the
  pool stays sole owner; the rings only *sample* pool textures).
- **Ring geometry** — level 0 a filled grid square; levels 1..N-1 hollow square
  "ring bands" at 2× spacing, holes exactly covered by the level inside. Persistent,
  created once.
- **Page binding** — one band = one page (band world-span = one page span at that
  level), bound to a single `Texture2DRD` per level, reusing the slice-1 sample path.
- **Recenter** — quantized translate + page rebind each frame; vertex buffers untouched.
- **Geomorph (L↔L+1)** — in each level's outer transition region the shader blends
  its own height toward the next-coarser level's height so the seam matches exactly
  (no crack, no pop).
- **`m3_rings_check.gd`** (`m3` suite, WINDOWED) — assembles the rings fed by the
  pool + streamer, renders top-down, and asserts no holes / real relief / seam
  continuity / morph-math / recenter-doesn't-rebuild. Saves a PNG for eyeball check.

**Out of scope (explicitly deferred to the next M3 slice — the fly-test harness):**
- **WASD/mouse fly camera + free-fly/ground-follow rig** (DESIGN §6.4). This slice
  drives a *scripted* camera move in the gate, not interactive flight.
- **Diagnostics / profiling / UI overlay** (live fps + stats).
- **The renderer p99 < 6 ms acceptance gate at ~1000 m/s** (DESIGN §7.3) and the
  manual fly confirmation. Those are only meaningful once a real fly-cam drives the
  rings; bolting them on here would mix geometry correctness with interactive-harness
  and perf concerns in one spec (the failure slice-3's spec warned against).

**Why this split:** one reviewable concern per slice. This slice = "the rings render
seamless terrain and recenter cheaply under a scripted move." The perf acceptance gate
needs interactive flight to be meaningful and is the next slice's definition of done.

---

## 2. Ring geometry & page binding

### 2.1 Mesh structure — concentric hollow ring-bands, one grid per level

A fixed `num_levels` set of persistent grid meshes, **created once, never rebuilt**:

- **Level 0:** a full filled `grid_res × grid_res` grid square, side = one page span
  (`base_span`), centered on the camera.
- **Levels 1..N-1:** each a **hollow square ring band** (a square annulus) at 2× the
  previous cell spacing. Level L's band outer side = `base_span · 2^L`; its inner hole
  side = `base_span · 2^(L-1)` — exactly the outer extent of level L-1. So the hole in
  each band is precisely filled by the level inside it: **coverage is gapless by
  construction.**

Each band has the same `grid_res` cell count along its perimeter, so vertex count per
level is fixed → **total render cost is constant regardless of view distance or speed**
(DESIGN §5.1's guarantee — the whole reason 1000 m/s is tractable). View distance
scales by adding *levels*, not by enlarging meshes.

### 2.2 Page binding — one band = one page

Each level's band world-span equals **one page** at that level. So the band binds to a
single pooled `Texture2DRD` via a uniform and samples it by UV — the **slice-1
`ring_displace` path verbatim** (one texture, one mesh, `uv = vertex.xz/span + 0.5`).

Consequence: the scheduler run at **`radius_pages = 0`** yields exactly **one page per
level** (the page centered on the velocity-led camera position). Scheduler coverage and
ring binding then line up one-to-one with zero glue code. (The scheduler still supports
`radius_pages ≥ 1`; this slice uses 0 so one band maps to one page. A larger near-detail
radius via more pages/level is a later enhancement, not needed for the unified clipmap.)

### 2.3 Wiring to scheduler + pool (the rings are consumers, not owners)

Each frame, for each level the rings determine the page key the scheduler is keeping
resident (the coverage page at that level for the current camera position), then ask
the pool for that page's `Texture2DRD`:
- if the level's own page is **resident**, bind it as the level's `height_tex`;
- if it is **not yet resident** (streaming behind a fast camera), bind the best
  resident **coarser** page (the `coarser_fallback` page the streamer already computes)
  so the band still shows correct-but-coarser terrain — the never-black path made
  visible.

The rings call **no** `texture_create`/`free_rid` — they read pool textures only. The
pool remains the single RID owner (slice-2 invariant, preserved).

### 2.4 New owner: `Wg10ClipmapRings`

`#[class(base=Node3D)]` (so it can parent the per-level `MeshInstance3D`s in the scene).
Responsibilities:
- `configure(pool, streamer, num_levels, grid_res, base_span, morph_region, height_scale)`
  — build the N persistent meshes + materials, attach as children.
- `recenter(camera_x, camera_z)` — quantized translate of each level + rebind page
  uniforms (see §3.1). No mesh rebuild.
- `stats() -> Dictionary` — per-level resident-vs-fallback state + total vertex count,
  for the gate and the future overlay.

Mesh-band generation (`ArrayMesh` build for the filled grid + the hollow bands) is its
own concern. If `clipmap_rings.rs` approaches ~half the 600-line cap, split a
`ring_mesh.rs` builder; otherwise keep it inline.

---

## 3. Recenter & the geomorph

### 3.1 Recenter — movement = translate, never rebuild

Each frame the rings snap to the camera, **quantized to each level's cell spacing** so
vertices stay locked to their world grid (no "swimming"/shimmer as the camera moves
sub-cell). Recenter is:
1. set each level's `MeshInstance3D` transform origin to the camera position quantized
   to that level's cell size;
2. rebind that level's page uniform(s) to the now-current resident (or fallback) page.

Vertex buffers are never touched. The gate asserts mesh vertex counts are identical
before and after a camera move (translated, not rebuilt) — DESIGN §5.1's "persistent
mesh, never rebuilt" made checkable.

### 3.2 Geomorph (L↔L+1) — the crack fix

Where a finer band's outer edge meets the coarser band around it, the two grids have
different vertex densities. Untreated, that is a T-junction → cracks/gaps. The fix lives
in `ring_displace.gdshader`'s `vertex()`:

- In each level's **outer transition region** — a band of configurable width
  `morph_region` (a fraction of the level span, e.g. 0.15) just inside the level's outer
  edge — compute a morph factor `t ∈ [0,1]` rising from 0 (inner side of the region) to
  1 (the shared outer boundary), from the vertex's distance toward that outer edge.
- Sample this level's own page height `h_fine` (its `height_tex`), and the
  **next-coarser** level's page height `h_coarse` at the same world position (a second
  uniform `coarse_height_tex` + the coarse level's world placement to compute the
  coarse-sample UV).
- `displaced_y = mix(h_fine, h_coarse, t) * height_scale`.

At the shared boundary `t = 1`, so the finer edge height **exactly equals** the coarser
surface there → no gap and no pop; the height eases in across the transition rather than
snapping. Both adjacent levels agree on the seam value, which is what makes the assembly
seamless.

**Edge cases:**
- Level 0's center has no finer level inside it — no inner morph needed; only its outer
  transition (toward level 1) morphs.
- The outermost level (`num_levels-1`) has no coarser level outside it — its outer edge
  is the view horizon; `t` clamps to its own height there (`coarse_height_tex` = its own
  page, so `mix` is a no-op) — no seam to hide.

### 3.3 Shader backward-compatibility

`ring_displace.gdshader` gains optional uniforms (`coarse_height_tex`, level
world-placement, `morph_region`). When a caller binds only the base uniforms (slice-1/2
checks), the morph region is empty / `t=0` everywhere, so the displaced height is exactly
`h_fine * height_scale` — **byte-identical to the current behavior**. The slice-1 and
slice-2 render gates must still pass unchanged; this slice re-runs the full `m3` suite,
not just the new check, to prove it.

### 3.4 Config — no magic numbers

All tunables come from `configure(...)` args (a config dictionary/resource at the
GDScript call site), matching slice-3's `ScheduleConfig` discipline:
`num_levels`, `grid_res`, `base_span` (= the page span, shared with the scheduler),
`morph_region` (transition width fraction), `height_scale`. No scattered constants in
the ring logic or shader.

---

## 4. Gate: `m3_rings_check.gd` (`m3` suite, WINDOWED)

Needs the global RenderingDevice (windowed), like the other m3 checks; returns SKIP
code 2 on a headless/no-GPU box. It assembles a real `Wg10ClipmapRings` fed by a
configured `Wg10PagePool` + `Wg10Streamer` (run at `radius_pages = 0`), renders to a
SubViewport, and asserts:

1. **No holes** — top-down **orthographic** capture of the assembled rings; assert
   `nonblack_frac` ≈ 1.0 across the covered area (a hole/gap renders as the background
   color). Same non-vacuity bar as slice 1 (a flat/empty frame fails).
2. **Real relief** — distinct quantized colors ≥ threshold (the rings show actual DEM
   terrain, not a flat plane).
3. **Seam continuity (the headline claim)** — sample pixel rows/columns crossing a level
   boundary; assert no sudden discontinuity at the seam (a crack/T-junction shows as a
   hard gap or abrupt height/color jump). This is what proves the geomorph, not merely
   that pixels are present.
4. **Morph math (CPU-side companion)** — independently of the pixels, assert that at a
   level boundary the morphed finer-edge height equals the coarser-surface height within
   epsilon (sample both page textures the way the shader does, compute the `t=1` mix,
   compare). Catches a wrong blend even where the pixel check is coarse.
5. **Recenter doesn't rebuild** — record each level's mesh vertex count; move the camera
   a non-trivial distance; `recenter`; assert vertex counts unchanged AND the rings
   still render (nonblack holds at the new position).

Plus: **save `m3_rings.png`** for eyeball confirmation (as `m3_slice1.png` was
inspected). Wire `m3_rings_check.gd` as the **4th** entry in the `m3` suite. `fast`/`gpu`
suites unchanged.

**Non-vacuity:** the seam-continuity and morph-math checks must run against an actual
level boundary with real differing-density grids — the gate is configured with
`num_levels ≥ 2` so a boundary exists, and asserts the boundary region is non-empty.

---

## 5. Files

**New:**
- `wg-10/rust/src/clipmap_rings.rs` — `Wg10ClipmapRings` (godot Node3D): owns the N
  mesh instances + materials, `configure` / `recenter` / `update_pages` / `stats`.
  Owns no RIDs.
- `wg-10/rust/src/ring_mesh.rs` *(only if `clipmap_rings.rs` nears the cap)* — helper
  building the filled level-0 grid + hollow ring-band `ArrayMesh`es.
- `wg-10/worldgen_terrain/tests/m3_rings_check.gd` — the §4 windowed gate.

**Modified:**
- `wg-10/worldgen_terrain/shaders/ring_displace.gdshader` — add the geomorph
  (`coarse_height_tex` + level placement + `morph_region` + `mix`), backward-compatible
  no-morph default (§3.3).
- `wg-10/rust/src/lib.rs` — `mod clipmap_rings;` (+ `mod ring_mesh;` if split).
- `tools/gate.py` — add `m3_rings_check.gd` to the `m3` suite (→ 4 checks).

**Soft cap:** all new files stay under DESIGN §7's ~600-line cap; split `ring_mesh.rs`
out if needed.

---

## 6. Definition of done

- `Wg10ClipmapRings` builds N persistent ring meshes, binds each to its
  scheduler-resident page (coarser fallback when not resident), recenters by translate
  (no rebuild), and the geomorph blends level seams.
- `m3_rings_check.gd` passes: no holes, real relief, seam continuity, morph-math within
  epsilon, recenter-doesn't-rebuild. `m3_rings.png` saved and **eyeballed** for real
  seamless DEM relief.
- `m3` suite = **4** checks `fail=0` (windowed); `fast`/`gpu` unchanged; cargo green.
  Slice-1/2 render gates still pass (shader backward-compatible).
- STATUS + ROADMAP updated: rings done; next = fly-cam harness + diagnostics overlay +
  the p99<6ms acceptance gate (+ manual fly). Honest baseline: this slice proves
  geometry-seamless + recenter-cheap under a SCRIPTED move, NOT the perf target or
  interactive flight.
- Each task committed separately (TDD shape). Per DESIGN §7.3 the perf+visual+manual
  acceptance gate is the **M3 milestone** gate (next slice), not this slice's done.

---

## 7. Risks & mitigations

- **Geomorph seam still cracks.** The headline risk. Mitigated by the CPU morph-math
  assertion (gate #4) catching a wrong blend independently of the pixel seam-continuity
  check (#3) — both must pass, so a bug has to fool two different checks.
- **Shader change breaks slice-1/2 render gates.** Keep the new uniforms optional with
  no-morph defaults (§3.3); re-run the FULL `m3` suite, not just the new check.
- **Ring↔scheduler radius mismatch.** This slice runs the scheduler at `radius_pages=0`
  (one page/level) to match one-band-one-page; stated explicitly so rings and scheduler
  agree. (`radius_pages≥1` multi-page bands are a later enhancement.)
- **Mesh-band winding / UV bugs leave gaps.** The top-down ortho capture + `nonblack≈1.0`
  assertion (#1) catches gaps from bad winding or wrong UVs; the distinct-color check (#2)
  catches an all-flat/garbage sample.
- **Recenter "swimming" (sub-cell shimmer).** Quantizing the recenter translate to each
  level's cell spacing (§3.1) locks vertices to the world grid; a future fly-cam slice
  will confirm visually, but the quantization is the structural fix.
