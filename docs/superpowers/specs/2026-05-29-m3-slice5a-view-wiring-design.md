# WorldGen10 M3 — Slice 5a: Rings↔Scheduler Wiring + Carry-Forward Fixes Design

**Date:** 2026-05-29
**Status:** Approved (brainstorming) — pending writing-plans
**Layer:** Render pipeline (M3) — slice 5a: wire pool + streamer + rings into one live frame-loop driver, and fix the two slice-4 audit carry-forwards that break under camera motion
**Builds on:** M3 slice 2 (Wg10PagePool single RID owner), slice 3 (Wg10Streamer velocity-aware coverage), slice 4 (Wg10ClipmapRings + L↔L+1 geomorph)
**Followed by:** M3 slice 5b (WASD/mouse fly camera + diagnostics/fps/p99 overlay + the renderer p99<6ms acceptance gate at ~1000 m/s + manual fly — finishes M3)

---

## 0. Framing

Slices 2–4 built the three pieces of the render pipeline independently: the bounded page
pool (single RID owner), the velocity-aware stream-ahead scheduler, and the concentric
clipmap rings with the L↔L+1 geomorph. They are **not yet connected** — slice 4's gate
bound pages directly for a *static* capture at `recenter(0,0)`, and slice 3's scheduler ran
over abstract page keys with no meshes.

This slice connects them into one live frame-loop driver, **and** fixes the two latent bugs
the slice-4 audit found — both invisible at the world origin (all slice 4 tested) but
breaking the moment a camera moves the rings:

1. **Per-level page span** (audit Issue 3): `acquire_page` ignores `level` for coverage, so
   a level-L page is computed at `world_span` instead of `world_span·2^L` — positionally
   wrong for every level above 0.
2. **Geomorph coarse_origin** (audit Issue 2): the shader assumes the coarse page is
   centered at the world origin, so the morphed seam reopens proportional to the camera's
   displacement from the origin.

This is the first slice that renders **seamless terrain at non-zero camera positions** —
the correctness-under-motion proof slice 4 structurally could not provide. It does NOT add
interactive flight or a perf gate; those are slice 5b and they finish M3.

---

## 1. Scope

**In scope (5a):**
- **Fix #1 — per-level page span:** `Wg10PagePool::acquire_page` derives
  `span_L = world_span · 2^level` for the compute dispatch (was a flat `world_span`).
  Signature unchanged.
- **Fix #2 — geomorph coarse_origin:** `ring_displace.gdshader` gains a `coarse_origin`
  uniform; the coarse sample becomes `(world.xz − coarse_origin)/coarse_span`.
  `Wg10ClipmapRings::bind_page` gains a `coarse_origin` parameter.
- **`Wg10TerrainView`** (godot Node3D): owns pool + streamer + rings; `update(cam_x, cam_z,
  vel_x, vel_z)` ticks the live §5.4 loop — advance the streamer, then per level acquire the
  resident page (coarser fallback on Full) + its coarser neighbor, `bind_page` (with
  coarse_origin), and `recenter`. Owns no RIDs.
- **`m3_view_check.gd`** (`m3` suite, WINDOWED): scripted moving sweep across page
  boundaries; asserts seamless + never-black at several non-zero camera positions, plus the
  per-level-span CPU check. Saves PNGs.

**Out of scope (deferred to slice 5b — finishes M3):**
- WASD/mouse free-fly camera + movement controller (DESIGN §6.4).
- Diagnostics / profiling / UI overlay (live fps + p99 + stats).
- The renderer **p99 < 6 ms acceptance gate at ~1000 m/s** (DESIGN §7.3) and the manual fly.

**Why this split:** one reviewable concern per slice. 5a = "the wired live loop renders
seamless never-black terrain while the camera MOVES, and the two carry-forward fixes are
proven by a moving gate." 5b = interactive input + the perf-acceptance gate, which is only
meaningful once a real fly camera drives the loop.

---

## 2. `Wg10TerrainView` — the live-loop coordinator

A thin godot `#[class(base=Node3D)]` — the single drop-in terrain node DESIGN §6.2
envisions ("one node + one config"). It *owns* `Gd` handles to the pool, streamer, and rings
and wires them each frame. It contains **no scheduling math** (the streamer's job) and **no
mesh/RID logic** (the rings'/pool's job). Owns no page RIDs.

### 2.1 API

```
configure(pool: Wg10PagePool, streamer: Wg10Streamer, rings: Wg10ClipmapRings,
          num_levels: int, base_span: float, height_scale: float,
          morph_region: float, relief_ref: float)
update(camera_x: float, camera_z: float, vel_x: float, vel_z: float) -> void
stats() -> Dictionary    # passthrough/merge of streamer + pool stats (minimal here; 5b overlay consumes it)
```

The caller configures the pool/streamer/rings first (each its own `configure`); the view
just stores the handles + tunables.

### 2.2 `update` — one frame of the §5.4 loop

```
1. streamer.update(camera_x, camera_z, vel_x, vel_z)
   # bounded stream-ahead + eviction bookkeeping (slice 3): coarsest-first acquires,
   # releases departing pages. The scheduler keeps the blanket resident under motion.

2. for L in 0 .. num_levels:
     span_L   = base_span * 2^L
     # level L's page key: the coverage origin at radius_pages=0 — the page containing
     # the camera, floor-quantized to span_L (matches scheduler + RingLayout).
     ox_L = floor(camera_x / span_L) * span_L
     oz_L = floor(camera_z / span_L) * span_L
     tex_L = pool.acquire_page(L, ox_L, oz_L)            # cache hit on resident page; re-protects it

     # coarser neighbor for the morph (level L+1); the coarsest level morphs to itself.
     if L < num_levels - 1:
         span_C = base_span * 2^(L+1)
         ox_C = floor(camera_x / span_C) * span_C
         oz_C = floor(camera_z / span_C) * span_C
         coarse_tex = pool.acquire_page(L+1, ox_C, oz_C)
     else:
         span_C = span_L ; ox_C = ox_L ; oz_C = oz_L ; coarse_tex = tex_L

     # never-black fallback: if this level's page isn't resident (Full/null), display the
     # coarser page in BOTH slots (correct-but-coarse). If the coarser is also null, walk
     # up until one is resident (the streamer keeps the coarsest blanket resident, so the
     # walk terminates — slice-3 never-black guarantee).
     if tex_L == null:
         tex_L = coarse_tex   # (and span_L/ox_L/oz_L stay this level's; morph degenerates to flat coarse)

     morph_L = (L < num_levels - 1) ? morph_region : 0.0   # coarsest level: no outer morph
     rings.bind_page(L, tex_L, coarse_tex, span_L, span_C, height_scale, morph_L, relief_ref,
                     /*coarse_origin=*/ Vector2(ox_C, oz_C))

3. rings.recenter(camera_x, camera_z)
```

### 2.3 Why the view acquires, not just the streamer

The streamer's acquires drive **stream-ahead** — fetch pages *before* they're sampled,
bounded per frame, coarsest-first. The view's acquires **retrieve the texture to display
now**: on a resident page they are cache hits (the pool returns the existing `Texture2Drd`,
no recompute) and they re-protect the page currently on screen. Both paths go through the
one pool/policy, so LRU/protected bookkeeping stays correct:
- pages currently displayed → re-protected by the view's acquire (never evicted while shown);
- pages streamed-ahead but not yet shown → evictable last.

No new pool API. The moving gate (§5) watches `created`/`recomputed` stay bounded to confirm
the view's acquires are cache hits, not churn.

### 2.4 File

`wg-10/rust/src/terrain_view.rs` — `Wg10TerrainView`. Holds `Gd<Wg10PagePool>`,
`Gd<Wg10Streamer>`, `Gd<Wg10ClipmapRings>` + tunables. Owns no RIDs, no meshes, no scheduling
math — pure orchestration. Under the 600-line cap.

---

## 3. Fix #1 — per-level page span in the pool

**Bug (audit Issue 3):** `Wg10PagePool::acquire_page(level, ox, oz)` passes a flat
`self.world_span` to `compute_into_texture` regardless of `level`. A level-1 ring spans
`2·world_span` but its page is computed over `world_span` → the terrain is stretched to half
scale, positionally wrong. Correct only at level 0.

**Fix:** at the dispatch site in `acquire_page` (the `Allocate` and `AllocateEvicting` arms),
the span local becomes:
```rust
let span_l = self.world_span * 2f64.powi(level as i32);
```
and `span_l` is passed to `compute_into_texture` in place of the flat `self.world_span`
(the existing `ws` local). Everything else (key origins in world metres, page_px, seed)
unchanged.

- **Signature unchanged** — `acquire_page` already takes `level`; it just wasn't using it
  for span.
- **One shared definition:** `span_L = base_span·2^L` is identical to the scheduler's
  `SchedulePolicy::level_span` and the rings' `RingLayout::level_span` — now honored
  end-to-end (scheduler picks keys, rings size bands, pool computes coverage — all agree).
- **No regression:** slice-1/2/3 gates and `m3_pool_check` acquire only at level 0
  (`2^0 = 1`), so their dispatch span is byte-identical → those gates unchanged.
- **Formula correctness:** the page hash→grammar→height formula is evaluated over the page's
  world footprint; a wider span samples the same deterministic field over a larger area —
  exactly what a coarser clipmap level shows. M2 CPU/GPU parity is unaffected (same formula,
  wider coordinate extent).

**Gate check:** the 5a gate asserts (CPU-side) that level-1's page covers 2× level-0's world
span — e.g. by confirming the view computes `span_1 == 2·span_0` and the rendered level-1
band shows correspondingly lower-frequency relief than level-0 over the same screen area.

---

## 4. Fix #2 — geomorph `coarse_origin` (seam under motion)

**Bug (audit Issue 2, conf 90):** `ring_displace.gdshader` computes
`uv_coarse = world.xz / coarse_span + 0.5`, assuming the coarse page is centered at the world
origin. After a non-zero recenter, level L+1 is centered on the (quantized) camera, so
`uv_coarse` is wrong by `coarse_center / coarse_span`; the morph samples the wrong coarse
texel and the seam reopens ∝ camera displacement. Invisible at `recenter(0,0)`.

**Fix:**
- Add `uniform vec2 coarse_origin;` to the shader (the world-space **corner** of the coarse
  page this level morphs toward — i.e. level L+1's quantized page origin `(ox_C, oz_C)`).
- Change the coarse sample to:
  ```glsl
  vec2 uv_coarse = (world.xz - coarse_origin) / coarse_span;   // page spans [origin, origin+span] -> [0,1]
  ```
  (no `+0.5`: the page now spans `[coarse_origin, coarse_origin + coarse_span]`, so subtract
  the corner and divide by span).
- `Wg10ClipmapRings::bind_page` gains a `coarse_origin` parameter (a `Vector2`, or two f64s),
  set into the material as the `coarse_origin` uniform. `Wg10TerrainView` passes level L+1's
  quantized page origin (§2.2).
- **Default `coarse_origin = vec2(0.0)`** → reproduces the old origin-centered behavior; but
  note the *formula* also changed (`+0.5` dropped). To keep slice-4's static gate
  byte-compatible, slice-4's `m3_rings_check` must pass `coarse_origin` equal to the coarse
  page's corner (which at the origin, for a page centered on origin spanning
  `[-span/2, +span/2]`, is `−coarse_span/2`). **This is a behavior change to the coarse-UV
  convention** — §4.1 spells out the fine/coarse asymmetry so callers set it right.

### 4.1 Fine/coarse UV conventions (stated explicitly so it isn't mistaken for a bug)

- **Fine sample** (`uv_fine = VERTEX.xz/world_span + 0.5`): UNCHANGED. The fine page is
  centered on the level's own mesh instance, which `recenter` keeps under the camera; local
  XZ in `[-span/2, +span/2]` maps to `[0,1]`. Correct as-is.
- **Coarse sample** (`uv_coarse = (world.xz − coarse_origin)/coarse_span`): references a
  *different* level's page via WORLD coords, so it needs that page's world corner
  (`coarse_origin`) and the page-spans-`[corner, corner+span]` convention. The view supplies
  `coarse_origin = (ox_C, oz_C)` = level L+1's quantized origin.

For this to close the seam, the coarse PAGE's world placement must match: a page acquired at
key origin `(ox_C, oz_C)` with span `span_C` must cover world `[ox_C, ox_C+span_C]`. Fix #1
makes the pool compute the page over exactly that footprint (span `span_C` at that origin),
so the conventions align: the coarse texel the morph reads = the texel level L+1 renders at
the same world point. **Fix #1 and Fix #2 are interdependent — both are needed for the seam
to close under motion**, which is why they ship together in 5a.

### 4.2 Seam-closure under motion (the gate's claim)

At level-0's outer edge, world point `P`: level 0 computes `t=1`, samples the coarse page at
`(P − coarse_origin)/coarse_span`. Level 1 renders `P` as an interior vertex sampling its own
page; since level-1's page origin = `coarse_origin` and covers `[coarse_origin,
coarse_origin+coarse_span]`, level-1's sample of `P` is the same texel. Both → same coarse
texel → seam closed at ANY camera position. The moving-sweep gate verifies this at several
non-zero positions including a boundary crossing.

---

## 5. Gate: `m3_view_check.gd` (`m3` suite, WINDOWED)

Needs the global RenderingDevice (windowed); SKIP code 2 headless. Assembles a configured
pool + streamer + rings + `Wg10TerrainView`, then drives `view.update(...)` over a **scripted
moving sweep**: several camera positions stepping +x across page boundaries — e.g.
`0, 0.25·base_span, 0.5·base_span (a level-0 boundary crossing), base_span, 3000.0, …`. At
each sampled position, render a top-down **orthographic** capture **centered on the current
camera** to a SubViewport, capture, and assert:

1. **No holes** — `nonblack_frac` ≈ 1.0 over the framed terrain (black bg; a gap reads black).
2. **Real relief** — distinct quantized colors ≥ threshold.
3. **Seam continuity** — sample across the level-0/1 boundary (at its camera-relative pixel
   offset, since the frame follows the camera); no black gap AND no hard color jump. This is
   the `coarse_origin` fix — it must hold at NON-ZERO positions, which is the whole point.
4. **Never-black** — at that position, every covered level is resident or has a resident
   coarser fallback (queried via pool/streamer stats / resident_keys).
5. **Per-level span (CPU)** — assert level-1's page covers 2× level-0's world span (Fix #1).

Saves a PNG per sampled position (`m3_view_<i>.png`) for eyeball confirmation. **Non-vacuous:**
at least one sampled position is a genuine boundary crossing at non-zero camera, where the
pre-fix seam bug would manifest as a crack/jump (so the assertion can actually fail if the
fix regresses). Also assert `pool.stats().created`/`recomputed` stay bounded across the sweep
(the view's acquires are cache hits, not churn).

Wire `m3_view_check.gd` as the **5th** entry in the `m3` suite. `fast`/`gpu` unchanged.

---

## 6. Files

**New:**
- `wg-10/rust/src/terrain_view.rs` — `Wg10TerrainView` (godot Node3D): the live-loop
  coordinator (§2). Owns no RIDs.
- `wg-10/worldgen_terrain/tests/m3_view_check.gd` — the §5 moving-sweep gate.

**Modified:**
- `wg-10/rust/src/page_pool.rs` — per-level span at the dispatch site (Fix #1, §3).
- `wg-10/worldgen_terrain/shaders/ring_displace.gdshader` — `coarse_origin` uniform + the
  coarse-UV formula change (Fix #2, §4).
- `wg-10/rust/src/clipmap_rings.rs` — `bind_page` gains a `coarse_origin` parameter, set into
  the material.
- `wg-10/worldgen_terrain/tests/m3_rings_check.gd` — pass `coarse_origin` (= the level's
  coarse-page corner) so slice-4's static gate stays correct under the new coarse-UV
  convention.
- `wg-10/rust/src/lib.rs` — `mod terrain_view;`.
- `tools/gate.py` — add `m3_view_check.gd` to the `m3` suite (→ 5 checks).

**Soft cap:** all new files under DESIGN §7's ~600-line cap.

---

## 7. Definition of done

- `Wg10TerrainView` drives the live loop (streamer.update → per-level acquire+bind+morph →
  recenter); both carry-forward fixes land.
- `m3_view_check` passes: seamless (no holes, seam + morph continuity) and never-black at
  several NON-ZERO camera positions incl. a boundary crossing; per-level span verified; pool
  churn bounded. PNGs saved and **eyeballed** for seamless relief that follows the camera.
- `m3` suite = **5** checks `fail=0` (windowed); `fast`/`gpu` unchanged; cargo green.
  Slice-1/2/3/4 gates still pass (regression — the shader `coarse_origin` change + the
  slice-4 gate update must keep slice-4's static capture correct).
- STATUS + ROADMAP updated: 5a done; the two carry-forwards closed; M3's remaining close-out
  is 5b (fly-cam + overlay + p99<6ms acceptance gate + manual fly). Honest baseline: 5a
  proves correctness under SCRIPTED motion, NOT interactive flight or the perf target.
- Each task committed separately (TDD shape where Rust logic exists). Per DESIGN §7.3 the
  perf+visual+manual acceptance gate is the **M3 milestone** gate (slice 5b), not 5a's done.

---

## 8. Risks & mitigations

- **Shader `coarse_origin` change breaks slice-4's static gate.** The coarse-UV convention
  changed (`+0.5` dropped, corner-relative). Mitigation: update `m3_rings_check.gd` to pass
  the correct `coarse_origin` (the coarse page's world corner), and re-run the FULL m3 suite
  — slice 4 must still pass byte-equivalent (its camera is at origin; the corner is a known
  constant there).
- **Fix #1 and Fix #2 are interdependent.** The seam only closes if the coarse page's world
  footprint (Fix #1: span `span_C` at origin `(ox_C,oz_C)`) matches what the morph samples
  (Fix #2: `(world−coarse_origin)/coarse_span`). They ship together; the moving gate proves
  the combination, not each in isolation.
- **View double-acquire churns the pool.** The view's per-level acquires are cache-hit reuses
  of resident pages (re-protect, no recompute). The gate asserts `created`/`recomputed` stay
  bounded across the sweep; if they grow per frame, the pool capacity or the
  streamer/view interaction is wrong.
- **Never-black fallback walk.** If a level's page AND its immediate coarser neighbor are both
  not resident, the view must walk further up. The streamer keeps the coarsest ring resident
  (slice-3 guarantee), so the walk terminates; the gate's never-black assertion catches any
  gap.
- **Capacity must hold all displayed levels + the streamer's working set.** The gate sizes
  pool capacity to `num_levels` displayed (re-protected) + headroom for stream-ahead, mirroring
  slice-3's sizing. Too small → the view's re-protect + streamer's acquires hit Full and a
  level falls back unnecessarily; the never-black check still passes (coarser shows) but the
  per-level-span/seam checks would reveal over-coarsening.
